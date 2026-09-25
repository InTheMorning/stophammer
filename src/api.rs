#![expect(
    clippy::significant_drop_tightening,
    reason = "MutexGuard<Connection> must be held for the full spawn_blocking scope"
)]

//! Axum HTTP router, handlers, and shared application state.
//!
//! Exposes three routes:
//! - `POST /ingest/feed` — crawler submission endpoint; validates via
//!   [`verify::VerifierChain`] and writes atomically via [`db::ingest_transaction`].
//! - `GET /sync/events` — paginated event log for community nodes.
//! - `POST /sync/reconcile` — negentropy-style diff for nodes rejoining after downtime.
//!
//! All blocking database operations are run in [`tokio::task::spawn_blocking`]
//! to avoid stalling the async executor. Join errors are converted to
//! [`ApiError`] with HTTP 500 rather than panicking.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use governor::{Quota, RateLimiter, clock::DefaultClock, state::keyed::DefaultKeyedStateStore};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use utoipa::ToSchema;

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{db, db_pool, event, fetch_guard, ingest, medium, model, query, signing, sync, verify};

// ── FG-02 SSE artist follow — 2026-03-13 ─────────────────────────────────

/// Broadcast channel capacity per artist.
const SSE_CHANNEL_CAPACITY: usize = 256;

/// Maximum number of recent events kept per artist for Last-Event-ID replay.
const SSE_RING_BUFFER_SIZE: usize = 100;

/// Maximum number of unique artist entries in the SSE registry.
/// Prevents unbounded memory growth from attackers creating channels for
/// fabricated artist IDs. 10,000 is generous for any legitimate deployment.
const MAX_SSE_REGISTRY_ARTISTS: usize = 10_000;

/// Maximum number of concurrent SSE connections across the server.
/// Each SSE connection holds a long-lived tokio task polling at 100ms intervals.
/// Without a cap, an attacker can exhaust server resources with persistent connections.
const MAX_SSE_CONNECTIONS: usize = 1_000;

/// CORS preflight cache duration in seconds (1 hour).
///
/// Browsers cache the `Access-Control-Max-Age` header for this long before
/// re-issuing an OPTIONS preflight. One hour reduces preflight traffic while
/// keeping clients reasonably up-to-date with any policy changes.
const CORS_MAX_AGE_SECS: u64 = 3600;

/// Warn when a primary ingest request takes at least as long as the crawler's
/// default ingest timeout, so operators can correlate crawler-side
/// `ingest_error` logs with slow handler completions.
const INGEST_SLOW_WARNING_SECS: u64 = 10;

/// A single SSE frame delivered to subscribers following an artist.
#[derive(Clone, Debug, Serialize)]
pub struct SseFrame {
    /// Event type, e.g. `"track_upserted"`, `"feed_upserted"`.
    pub event_type: String,
    /// The subject entity GUID (`track_guid`, `feed_guid`, etc.).
    pub subject_guid: String,
    /// Full event payload as JSON.
    pub payload: serde_json::Value,
    /// Monotonically increasing sequence number (primary key in `events` table).
    /// Used as the SSE `id:` field for unambiguous `Last-Event-ID` replay.
    pub seq: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AmbiguousTrackGuidCandidate {
    pub feed_guid: String,
    pub href: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AmbiguousTrackGuidBody {
    pub error: String,
    pub code: &'static str,
    pub track_guid: String,
    pub candidates: Vec<AmbiguousTrackGuidCandidate>,
}

#[must_use]
pub fn canonical_track_href(feed_guid: &str, track_guid: &str) -> String {
    format!("/v1/feeds/{feed_guid}/tracks/{track_guid}")
}

#[must_use]
pub fn ambiguous_track_guid_body(
    track_guid: &str,
    candidates: Vec<AmbiguousTrackGuidCandidate>,
) -> AmbiguousTrackGuidBody {
    AmbiguousTrackGuidBody {
        error: format!(
            "track_guid {track_guid} is ambiguous; retry with the canonical feed-scoped route"
        ),
        code: "ambiguous_track_guid",
        track_guid: track_guid.to_string(),
        candidates,
    }
}

#[must_use]
pub fn ambiguous_track_guid_response(
    track_guid: &str,
    candidates: Vec<AmbiguousTrackGuidCandidate>,
) -> Response {
    (
        StatusCode::CONFLICT,
        Json(ambiguous_track_guid_body(track_guid, candidates)),
    )
        .into_response()
}

#[derive(Debug)]
struct SignedEventRow {
    row: db::EventRow,
    seq: i64,
    signed_by: String,
    signature: String,
}

type IngestBlockingOutput = (
    ingest::IngestResponse,
    Vec<event::Event>,
    Vec<(String, SseFrame)>,
);

/// Outcome of the read-only verification phase of `handle_ingest_feed`.
///
/// The reader connection that produces this value is dropped before any of
/// the three outcomes is handled, so a `NoChange` outcome is free to acquire
/// the writer lock for the URL-observation step of ADR 0049 Section 1.
enum ReadPhaseOutcome {
    /// The verifier chain passed; carries the accumulated warnings.
    Verified(Vec<String>),
    /// The content hash matched the last crawl; the feed body is unchanged.
    NoChange,
    /// A verifier refused the submission; carries the rejection reason.
    Rejected(String),
}

/// Registry managing per-artist broadcast channels and ring buffers for SSE.
// CRIT-03 Debug — 2026-03-13
pub struct SseRegistry {
    /// `artist_id` -> broadcast sender for that artist's events.
    senders: std::sync::RwLock<HashMap<String, tokio::sync::broadcast::Sender<SseFrame>>>,
    /// `artist_id` -> ring buffer of recent events for `Last-Event-ID` replay.
    ring_buffers: std::sync::RwLock<HashMap<String, std::collections::VecDeque<SseFrame>>>,
    /// Current number of active SSE connections (for connection cap enforcement).
    active_connections: std::sync::atomic::AtomicUsize,
}

impl std::fmt::Debug for SseRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SseRegistry")
            .field("artist_count", &self.artist_count())
            .field(
                "active_connections",
                &self
                    .active_connections
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl SseRegistry {
    /// Creates a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            senders: std::sync::RwLock::new(HashMap::new()),
            ring_buffers: std::sync::RwLock::new(HashMap::new()),
            active_connections: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Returns the current number of active SSE connections.
    #[must_use]
    pub fn active_connections(&self) -> usize {
        self.active_connections
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Attempts to acquire an SSE connection slot. Returns `true` if the
    /// connection is allowed, `false` if the maximum has been reached.
    // Issue #23 atomic TOCTOU fix — 2026-03-13
    pub fn try_acquire_connection(&self) -> bool {
        self.active_connections
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |current| (current < MAX_SSE_CONNECTIONS).then(|| current + 1),
            )
            .is_ok()
    }

    /// Releases an SSE connection slot when a client disconnects.
    pub fn release_connection(&self) {
        self.active_connections
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Returns the number of unique artist entries in the registry.
    #[must_use]
    pub fn artist_count(&self) -> usize {
        self.senders.read().map_or(0, |g| g.len())
    }

    /// Returns a broadcast receiver for the given artist. Creates the channel
    /// lazily if it does not yet exist. Returns `None` if the registry is full
    /// and the artist is not already tracked.
    pub fn subscribe(&self, artist_id: &str) -> Option<tokio::sync::broadcast::Receiver<SseFrame>> {
        // Try read-lock first (fast path for existing channels).
        {
            if let Ok(guard) = self.senders.read()
                && let Some(tx) = guard.get(artist_id)
            {
                return Some(tx.subscribe());
            }
        }
        // Slow path: create channel under write lock.
        let mut guard = self
            .senders
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Check if another thread created it while we waited for the write lock.
        if let Some(tx) = guard.get(artist_id) {
            return Some(tx.subscribe());
        }
        // Enforce registry size limit before creating a new entry.
        if guard.len() >= MAX_SSE_REGISTRY_ARTISTS {
            return None;
        }
        let (tx, _) = tokio::sync::broadcast::channel(SSE_CHANNEL_CAPACITY);
        let rx = tx.subscribe();
        guard.insert(artist_id.to_string(), tx);
        Some(rx)
    }

    /// Publishes a frame to the broadcast channel for `artist_id` and appends
    /// it to the ring buffer.
    pub fn publish(&self, artist_id: &str, frame: SseFrame) {
        // Try read-lock first (fast path for existing channels).
        let sent = {
            if let Ok(guard) = self.senders.read()
                && let Some(tx) = guard.get(artist_id)
            {
                let _ = tx.send(frame.clone());
                true
            } else {
                false
            }
        };

        // Slow path: create channel if it did not exist and send.
        // Publish always creates channels (these are real events from ingest),
        // but is also bounded by MAX_SSE_REGISTRY_ARTISTS.
        if !sent {
            let mut guard = self
                .senders
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(tx) = guard.get(artist_id) {
                let _ = tx.send(frame.clone());
            } else if guard.len() < MAX_SSE_REGISTRY_ARTISTS {
                let (tx, _) = tokio::sync::broadcast::channel(SSE_CHANNEL_CAPACITY);
                let _ = tx.send(frame.clone());
                guard.insert(artist_id.to_string(), tx);
            } else {
                tracing::warn!(
                    artist_id,
                    "SSE registry full ({MAX_SSE_REGISTRY_ARTISTS} artists); dropping event"
                );
                return;
            }
        }

        // Append to ring buffer.
        let mut rb_guard = self
            .ring_buffers
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Also enforce the same limit on ring buffers.
        if !rb_guard.contains_key(artist_id) && rb_guard.len() >= MAX_SSE_REGISTRY_ARTISTS {
            return;
        }
        let buf = rb_guard
            .entry(artist_id.to_string())
            .or_insert_with(|| std::collections::VecDeque::with_capacity(SSE_RING_BUFFER_SIZE));
        if buf.len() >= SSE_RING_BUFFER_SIZE {
            buf.pop_front();
        }
        buf.push_back(frame);
    }

    /// Returns cloned recent events for replay (bounded by `SSE_RING_BUFFER_SIZE`).
    #[must_use]
    pub fn recent_events(&self, artist_id: &str) -> Vec<SseFrame> {
        let guard = self
            .ring_buffers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .get(artist_id)
            .map(|buf| buf.iter().cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for SseRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Issue-SSE-PUBLISH helpers — 2026-03-14 ────────────────────────────────

/// Extracts the artist ID(s) relevant to an event for SSE channel routing.
///
/// Returns an empty vec for event types that do not map to a specific artist
/// (e.g. `FeedRetired`, `TrackRemoved`) since those payloads only carry GUIDs
/// and the entity may already be deleted. The caller should fall back to the
/// `subject_guid` if needed, but in practice these events are less relevant
/// to live SSE followers.
fn extract_artist_ids(ev: &event::Event) -> Vec<String> {
    match &ev.payload {
        event::EventPayload::ArtistUpserted(p) => {
            vec![p.artist.artist_id.clone()]
        }
        event::EventPayload::ArtistCreditCreated(p) => p
            .artist_credit
            .names
            .iter()
            .map(|n| n.artist_id.clone())
            .collect(),
        // FeedRetired, TrackRemoved, RoutesReplaced, FeedRoutesReplaced:
        // These payloads do not embed artist info. We skip SSE publish for
        // these rather than doing a DB lookup that may fail (entity deleted).
        _ => vec![],
    }
}

fn artist_ids_for_feed(
    conn: &rusqlite::Connection,
    feed_guid: &str,
) -> Result<Vec<String>, db::DbError> {
    let Some(feed) = db::get_feed_by_guid(conn, feed_guid)? else {
        return Ok(vec![]);
    };
    let Some(credit) = db::get_artist_credit(conn, feed.artist_credit_id)? else {
        return Ok(vec![]);
    };
    let mut artist_ids: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for name in credit.names {
        if seen.insert(name.artist_id.clone()) {
            artist_ids.push(name.artist_id);
        }
    }
    Ok(artist_ids)
}

fn live_sse_payload(
    feed_guid: &str,
    live_event: &model::LiveEvent,
    status: &str,
) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "live_item_guid": live_event.live_item_guid,
        "title": live_event.title,
        "content_link": live_event.content_link,
        "status": status,
        "scheduled_start": live_event.scheduled_start,
        "scheduled_end": live_event.scheduled_end,
    })
}

/// Build SSE frames describing live-event start/end transitions for a feed.
///
/// This diffs the old and new live-event snapshots and associates ended live
/// events with their promoted `TrackUpserted` events when possible.
///
/// # Errors
///
/// Returns `DbError` if the helper queries needed to resolve artist channels
/// or promoted-track mappings fail.
pub fn build_live_sse_frames_for_feed(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    old_live_events: &[model::LiveEvent],
    new_live_events: &[model::LiveEvent],
    events: &[event::Event],
) -> Result<Vec<(String, SseFrame)>, db::DbError> {
    let artist_ids = artist_ids_for_feed(conn, feed_guid)?;
    if artist_ids.is_empty() {
        return Ok(vec![]);
    }

    let live_snapshot_seq = events
        .iter()
        .filter_map(|ev| match &ev.payload {
            event::EventPayload::LiveEventsReplaced(p) if p.feed_guid == feed_guid => Some(ev.seq),
            _ => None,
        })
        .max();

    let ended_track_seqs: HashMap<String, i64> = events
        .iter()
        .filter_map(|ev| match &ev.payload {
            event::EventPayload::TrackUpserted(p) if p.track.feed_guid == feed_guid => {
                Some((p.track.track_guid.clone(), ev.seq))
            }
            _ => None,
        })
        .collect();

    let old_by_guid: HashMap<&str, &model::LiveEvent> = old_live_events
        .iter()
        .map(|live_event| (live_event.live_item_guid.as_str(), live_event))
        .collect();
    let new_by_guid: HashMap<&str, &model::LiveEvent> = new_live_events
        .iter()
        .map(|live_event| (live_event.live_item_guid.as_str(), live_event))
        .collect();

    let mut frames: Vec<(String, SseFrame)> = Vec::new();

    if let Some(seq) = live_snapshot_seq {
        for live_event in new_live_events
            .iter()
            .filter(|live_event| live_event.status == "live")
        {
            let was_live = old_by_guid
                .get(live_event.live_item_guid.as_str())
                .is_some_and(|old_live_event| old_live_event.status == "live");
            if was_live {
                continue;
            }
            let frame = SseFrame {
                event_type: "live_event_started".to_string(),
                subject_guid: live_event.live_item_guid.clone(),
                payload: live_sse_payload(feed_guid, live_event, "live"),
                seq,
            };
            for artist_id in &artist_ids {
                frames.push((artist_id.clone(), frame.clone()));
            }
        }
    }

    for old_live_event in old_live_events {
        if new_by_guid.contains_key(old_live_event.live_item_guid.as_str()) {
            continue;
        }
        let Some(&seq) = ended_track_seqs.get(&old_live_event.live_item_guid) else {
            continue;
        };
        let frame = SseFrame {
            event_type: "live_event_ended".to_string(),
            subject_guid: old_live_event.live_item_guid.clone(),
            payload: live_sse_payload(feed_guid, old_live_event, "ended"),
            seq,
        };
        for artist_id in &artist_ids {
            frames.push((artist_id.clone(), frame.clone()));
        }
    }

    Ok(frames)
}

pub fn publish_sse_frames(registry: &SseRegistry, frames: &[(String, SseFrame)]) {
    for (artist_id, frame) in frames {
        registry.publish(artist_id, frame.clone());
    }
}

/// Fire-and-forget SSE publish for a batch of events.
///
/// For each event, extracts the relevant artist ID(s) and publishes an
/// `SseFrame` to each artist's broadcast channel. Errors are logged but
/// never propagated — SSE is best-effort and must not fail the mutation.
// Issue-SSE-PUBLISH — 2026-03-14
pub fn publish_events_to_sse(registry: &SseRegistry, events: &[event::Event]) {
    for ev in events {
        let artist_ids = extract_artist_ids(ev);
        if artist_ids.is_empty() {
            continue;
        }
        let frame = SseFrame {
            event_type: serde_json::to_string(&ev.event_type)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string(),
            subject_guid: ev.subject_guid.clone(),
            payload: serde_json::to_value(&ev.payload).unwrap_or(serde_json::Value::Null),
            seq: ev.seq,
        };
        for artist_id in &artist_ids {
            registry.publish(artist_id, frame.clone());
        }
    }
}

// ── SP-03 rate limiting — 2026-03-13 ─────────────────────────────────────

/// Reads `RATE_LIMIT_RPS` and `RATE_LIMIT_BURST` from the environment, falling
/// back to 50 / 100 respectively.
#[must_use]
pub fn rate_limit_config() -> (u32, u32) {
    let rps: u32 = std::env::var("RATE_LIMIT_RPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    let burst: u32 = std::env::var("RATE_LIMIT_BURST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    (rps, burst)
}

/// Per-IP token-bucket rate limiter keyed by `String` (IP address).
pub type IpRateLimiter = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

/// Builds a keyed (per-IP) governor rate limiter with the given `rps` and `burst`.
///
/// The caller owns the limiter and should apply it in the serving layer
/// (e.g. main.rs) rather than inside `build_router`, so that `tower::ServiceExt::oneshot`
/// tests are not affected.
///
/// # Panics
///
/// Panics if internal `NonZeroU32` fallback constants are zero (impossible in
/// practice — the fallbacks are hard-coded to 50 and 100).
#[must_use]
pub fn build_rate_limiter(rps: u32, burst: u32) -> IpRateLimiter {
    let quota = Quota::per_second(
        NonZeroU32::new(rps).unwrap_or(NonZeroU32::new(50).expect("50 is nonzero")),
    )
    .allow_burst(NonZeroU32::new(burst).unwrap_or(NonZeroU32::new(100).expect("100 is nonzero")));
    RateLimiter::keyed(quota)
}

// ── Availability limits ────────────────────────────────────────────────────

/// Maximum request body size (bytes). Applied globally via `DefaultBodyLimit`.
/// 2 MiB is sufficient for the largest legitimate ingest payload (a feed with
/// ~200 tracks and payment routes). Larger payloads are rejected with 413.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Maximum number of tracks allowed in a single `POST /ingest/feed` request.
/// Prevents a single malicious submission from creating thousands of DB rows.
const MAX_TRACKS_PER_INGEST: usize = 500;

/// Maximum number of `have` event refs in a `POST /sync/reconcile` request.
const MAX_RECONCILE_HAVE: usize = 10_000;

// Finding-5 reconcile pagination — 2026-03-13
/// Maximum number of event refs loaded by `get_event_refs_since` during reconcile.
/// Prevents unbounded memory usage when a node is far behind.
const MAX_RECONCILE_REFS: i64 = 50_000;

/// Maximum number of full events returned in a single reconcile response.
const MAX_RECONCILE_EVENTS: i64 = 10_000;

/// Maximum allowed wall-clock skew for signed sync/register requests.
///
/// Limits replay lifetime for captured registration payloads while tolerating
/// modest clock skew between nodes.
const SYNC_REGISTER_MAX_SKEW_SECS: i64 = 600;

// ── AppState ────────────────────────────────────────────────────────────────

/// Shared application state injected into every Axum handler.
// CRIT-03 Debug derive — 2026-03-13
#[derive(Debug)]
pub struct AppState {
    /// `SQLite` WAL connection pool (writer singleton + reader pool).
    // Issue-WAL-POOL — 2026-03-14
    pub db: db_pool::DbPool,
    /// Ordered chain of verifiers that must all pass before an ingest is accepted.
    pub chain: Arc<verify::VerifierChain>,
    /// Signs event payloads with this node's ed25519 key.
    pub signer: Arc<signing::NodeSigner>,
    /// Hex-encoded ed25519 public key identifying this node in the network.
    pub node_pubkey_hex: String,
    /// Token required in `X-Admin-Token` for admin endpoints.
    pub admin_token: String,
    /// Optional dedicated token for sync endpoints (`X-Sync-Token` header).
    /// When `Some`, only this token is accepted for sync reads and writes.
    /// When `None`, sync endpoints reject requests with 403.
    pub sync_token: Option<String>,
    /// HTTP client used for push fan-out to peer community nodes.
    pub push_client: reqwest::Client,
    /// In-memory cache of active push peers: pubkey → push URL.
    pub push_subscribers: Arc<RwLock<HashMap<String, String>>>,
    /// FG-02 SSE artist follow — 2026-03-13
    /// Registry for SSE per-artist broadcast channels and replay buffers.
    pub sse_registry: Arc<SseRegistry>,
    /// When true, skip SSRF validation of a peer's `node_url` during
    /// `sync/register`. Only intended for test environments where mock
    /// servers use localhost.
    // CRIT-02 feature-gate — 2026-03-13
    #[cfg(feature = "test-util")]
    pub skip_ssrf_validation: bool,
}

fn build_source_contributor_claims(
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
    persons: &[ingest::IngestPerson],
    now: i64,
) -> Vec<model::SourceContributorClaim> {
    persons
        .iter()
        .enumerate()
        .map(|(position, person)| {
            #[expect(
                clippy::cast_possible_wrap,
                reason = "contributor counts are bounded by feed size"
            )]
            let position = position as i64;
            model::SourceContributorClaim {
                id: None,
                feed_guid: feed_guid.to_string(),
                entity_type: entity_type.to_string(),
                entity_id: entity_id.to_string(),
                // Preserve contributor order but normalize positions to a
                // unique per-entity sequence so malformed feeds with repeated
                // incoming positions do not violate staging-table constraints.
                position,
                name: person.name.clone(),
                role: person.role.clone(),
                role_norm: normalize_role(person.role.as_deref()),
                group_name: person.group_name.clone(),
                href: person.href.clone(),
                img: person.img.clone(),
                npub: person.npub.clone(),
                source: "podcast_person".to_string(),
                extraction_path: format!("{entity_type}.podcast:person"),
                observed_at: now,
            }
        })
        .collect()
}

fn normalize_role(role: Option<&str>) -> Option<String> {
    let role = role?.trim();
    if role.is_empty() {
        return None;
    }

    Some(
        role.split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Gives the feed release artist and the name of the source that gave it.
///
/// The order is: a non-empty trimmed `itunes:author`, giving
/// `"itunes_author"`; else a non-empty trimmed `itunes:owner` name that does
/// not name a platform, giving `"itunes_owner"`; else the placeholder
/// `"Unknown Artist"`, giving `"placeholder"`. ADR 0049 §5 owns this rule.
fn derive_release_artist(feed_data: &ingest::IngestFeedData) -> (String, &'static str) {
    if let Some(author_name) = non_empty_trimmed(feed_data.author_name.as_deref()) {
        return (author_name.to_string(), "itunes_author");
    }

    if let Some(owner_name) = non_empty_trimmed(feed_data.owner_name.as_deref())
        && !is_platform_owner_name(owner_name)
    {
        return (owner_name.to_string(), "itunes_owner");
    }

    ("Unknown Artist".to_string(), "placeholder")
}

/// Gives the feed `publisher_text`: the trimmed `itunes:owner` name, or
/// `None`. ADR 0049 §5 owns this rule.
fn derive_publisher_name(feed_data: &ingest::IngestFeedData) -> Option<String> {
    non_empty_trimmed(feed_data.owner_name.as_deref()).map(str::to_string)
}

fn event_type_tag(event_type: &event::EventType) -> Result<String, ApiError> {
    let tag = serde_json::to_string(event_type).map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("failed to serialize event type for fan-out: {e}"),
        www_authenticate: None,
    })?;
    Ok(tag.trim_matches('"').to_string())
}

fn signed_row_to_event(signed: SignedEventRow) -> Result<event::Event, ApiError> {
    let et_str = event_type_tag(&signed.row.event_type)?;
    let tagged = format!(
        r#"{{"type":"{et_str}","data":{}}}"#,
        signed.row.payload_json
    );
    let payload = serde_json::from_str::<event::EventPayload>(&tagged).map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("failed to deserialize event payload for fan-out: {e}"),
        www_authenticate: None,
    })?;
    Ok(event::Event {
        event_id: signed.row.event_id,
        event_type: signed.row.event_type,
        payload,
        subject_guid: signed.row.subject_guid,
        signed_by: signed.signed_by,
        signature: signed.signature,
        seq: signed.seq,
        created_at: signed.row.created_at,
        warnings: signed.row.warnings,
        payload_json: signed.row.payload_json,
    })
}

fn sign_event_row(
    conn: &rusqlite::Connection,
    row: db::EventRow,
    signer: &signing::NodeSigner,
) -> Result<SignedEventRow, ApiError> {
    let (seq, signed_by, signature) = db::insert_event(
        conn,
        &row.event_id,
        &row.event_type,
        &row.payload_json,
        &row.subject_guid,
        signer,
        row.created_at,
        &row.warnings,
    )
    .map_err(ApiError::from)?;
    Ok(SignedEventRow {
        row,
        seq,
        signed_by,
        signature,
    })
}

fn feed_url_observed_event_row(
    observation: &db::FeedUrlObservation,
) -> Result<db::EventRow, ApiError> {
    let payload = event::FeedUrlObservedPayload {
        url: observation.url.clone(),
        feed_guid: observation.feed_guid.clone(),
        observed_at: observation.observed_at,
    };
    let payload_json = serde_json::to_string(&payload).map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("failed to serialize FeedUrlObserved payload: {e}"),
        www_authenticate: None,
    })?;
    Ok(db::EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: event::EventType::FeedUrlObserved,
        payload_json,
        subject_guid: observation.feed_guid.clone(),
        created_at: observation.observed_at,
        warnings: vec![],
    })
}

/// Records which URL gave `feed_guid`, and signs one `FeedUrlObserved` event
/// for each URL whose observation changed (ADR 0049 Section 1).
///
/// Records `canonical_url`, and `source_url` too when it differs from
/// `canonical_url`. Runs in its own transaction, after the main ingest
/// transaction commits.
fn record_feed_url_observations_for_ingest(
    conn: &mut rusqlite::Connection,
    canonical_url: &str,
    source_url: &str,
    feed_guid: &str,
    now: i64,
    signer: &signing::NodeSigner,
) -> Result<Vec<SignedEventRow>, ApiError> {
    let mut urls = vec![canonical_url];
    if source_url != canonical_url {
        urls.push(source_url);
    }

    let tx = conn.transaction().map_err(db::DbError::from)?;
    let mut signed_rows = Vec::new();
    for url in urls {
        if let Some(observation) = db::record_feed_url_observation(&tx, url, feed_guid, now)? {
            signed_rows.push(sign_event_row(
                &tx,
                feed_url_observed_event_row(&observation)?,
                signer,
            )?);
        }
    }
    tx.commit().map_err(db::DbError::from)?;
    Ok(signed_rows)
}

/// Builds the `FeedCopyObserved` event row for one summary of `feed_guid` at
/// `url` (ADR 0058 sections 1 and 1a).
fn feed_copy_observed_event_row(
    feed_guid: &str,
    url: &str,
    first_seen: i64,
    summary: &model::CopySummary,
    digest: &str,
    now: i64,
) -> Result<db::EventRow, ApiError> {
    let payload = event::FeedCopyObservedPayload {
        feed_guid: feed_guid.to_string(),
        url: url.to_string(),
        first_seen,
        title: summary.title.clone(),
        item_guids: summary.item_guids.clone(),
        feed_recipients: summary.feed_recipients.clone(),
        track_recipients: summary.track_recipients.clone(),
        summary_digest: digest.to_string(),
    };
    let payload_json = serde_json::to_string(&payload).map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("failed to serialize FeedCopyObserved payload: {e}"),
        www_authenticate: None,
    })?;
    Ok(db::EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: event::EventType::FeedCopyObserved,
        payload_json,
        subject_guid: feed_guid.to_string(),
        created_at: now,
        warnings: vec![],
    })
}

/// Records the ADR 0058 sections 1 and 1a summary of a mirror body at `url`
/// for `feed_guid`. Signs one `FeedCopyObserved` event for a new or a
/// changed summary.
///
/// A row that exists gets a new `last_seen`. The digest decides the rest:
/// the same digest signs no event, and a changed digest writes the new
/// summary and signs one event, with the row's own `first_seen`. When no row
/// exists and the GUID already holds `MAX_COPIES_PER_GUID` rows, the call
/// writes no row and no event, and adds one to the overflow count instead.
///
/// Runs in its own transaction. Returns the signed event, when the call
/// signs one, or an empty list.
fn record_feed_copy_for_ingest(
    conn: &mut rusqlite::Connection,
    feed_guid: &str,
    url: &str,
    feed_data: &ingest::IngestFeedData,
    now: i64,
    signer: &signing::NodeSigner,
) -> Result<Vec<SignedEventRow>, ApiError> {
    let summary = model::copy_summary(feed_data);
    let digest = model::summary_digest(&summary);

    let tx = conn.transaction().map_err(db::DbError::from)?;
    let existing = db::get_feed_copy(&tx, feed_guid, url)?;

    let signed_row = match existing {
        Some(row) => {
            db::touch_feed_copy_last_seen(&tx, feed_guid, url, now)?;
            if digest == row.summary_digest {
                None
            } else {
                db::upsert_feed_copy_summary(
                    &tx,
                    feed_guid,
                    url,
                    row.first_seen,
                    &summary,
                    &digest,
                )?;
                Some(sign_event_row(
                    &tx,
                    feed_copy_observed_event_row(
                        feed_guid,
                        url,
                        row.first_seen,
                        &summary,
                        &digest,
                        now,
                    )?,
                    signer,
                )?)
            }
        }
        None => {
            if db::count_feed_copies(&tx, feed_guid)? >= db::MAX_COPIES_PER_GUID {
                db::increment_copy_overflow(&tx, feed_guid)?;
                None
            } else {
                db::upsert_feed_copy_summary(&tx, feed_guid, url, now, &summary, &digest)?;
                db::touch_feed_copy_last_seen(&tx, feed_guid, url, now)?;
                Some(sign_event_row(
                    &tx,
                    feed_copy_observed_event_row(feed_guid, url, now, &summary, &digest, now)?,
                    signer,
                )?)
            }
        }
    };

    tx.commit().map_err(db::DbError::from)?;
    Ok(signed_row.into_iter().collect())
}

// ── feed_guid_changes (ADR 0052 sections 4 and 5, task 007) ─────────────────

/// Builds one `FeedGuidChangeObserved` event row (ADR 0052 section 4, task
/// 007). `new_guid` equal to `old_guid` is the delete marker: a return to
/// the GUID the record already holds, or the pending-row cleanup of the
/// automatic transition.
fn guid_change_observed_event_row(
    source_url: &str,
    old_guid: &str,
    new_guid: &str,
    first_seen: i64,
    now: i64,
) -> Result<db::EventRow, ApiError> {
    let payload = event::FeedGuidChangeObservedPayload {
        source_url: source_url.to_string(),
        old_guid: old_guid.to_string(),
        new_guid: new_guid.to_string(),
        first_seen,
    };
    let payload_json = serde_json::to_string(&payload).map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("failed to serialize FeedGuidChangeObserved payload: {e}"),
        www_authenticate: None,
    })?;
    Ok(db::EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: event::EventType::FeedGuidChangeObserved,
        payload_json,
        subject_guid: old_guid.to_string(),
        created_at: now,
        warnings: vec![],
    })
}

/// The outcome of [`record_guid_change_pending_for_ingest`].
enum GuidChangePendingOutcome {
    /// The row already holds a `reject` decision for this same `new_guid`.
    /// Nothing was written.
    Rejected,
    /// The row was written or confirmed. Carries the signed
    /// `FeedGuidChangeObserved` event when the row was new or `new_guid`
    /// changed; empty when the same pending value was seen again.
    Pending(Vec<SignedEventRow>),
}

/// Writes or updates the `feed_guid_changes` pending row of `source_url`
/// (ADR 0052 section 4, task 007 item 2).
///
/// Signs one `FeedGuidChangeObserved` event when the row is new or its
/// `new_guid` changed, and always updates `last_seen`, local to the primary.
/// When the row already holds a `reject` decision for this same `new_guid`,
/// the rejection holds: the call writes nothing and answers
/// [`GuidChangePendingOutcome::Rejected`].
///
/// # Errors
///
/// Returns [`ApiError`] if a database or signing step fails.
fn record_guid_change_pending_for_ingest(
    conn: &mut rusqlite::Connection,
    source_url: &str,
    old_guid: &str,
    new_guid: &str,
    now: i64,
    signer: &signing::NodeSigner,
) -> Result<GuidChangePendingOutcome, ApiError> {
    let existing = db::get_guid_change(conn, source_url).map_err(ApiError::from)?;
    if let Some(row) = &existing
        && row.new_guid == new_guid
        && row.decision.as_deref() == Some("reject")
    {
        return Ok(GuidChangePendingOutcome::Rejected);
    }

    let is_new_or_changed = existing.as_ref().is_none_or(|row| row.new_guid != new_guid);
    let first_seen = if is_new_or_changed {
        now
    } else {
        existing.as_ref().map_or(now, |row| row.first_seen)
    };

    let tx = conn.transaction().map_err(db::DbError::from)?;
    db::upsert_guid_change(&tx, source_url, old_guid, new_guid, first_seen)
        .map_err(ApiError::from)?;
    db::touch_guid_change_last_seen(&tx, source_url, now).map_err(ApiError::from)?;
    let signed_rows = if is_new_or_changed {
        vec![sign_event_row(
            &tx,
            guid_change_observed_event_row(source_url, old_guid, new_guid, first_seen, now)?,
            signer,
        )?]
    } else {
        vec![]
    };
    tx.commit().map_err(db::DbError::from)?;

    Ok(GuidChangePendingOutcome::Pending(signed_rows))
}

/// Deletes the pending row of `source_url`, when one exists, and signs one
/// `FeedGuidChangeObserved` delete-marker event (ADR 0052 section 4, task
/// 007: "a return"). Returns an empty list when there was no row to delete.
///
/// # Errors
///
/// Returns [`ApiError`] if a database or signing step fails.
fn record_guid_change_return_for_ingest(
    conn: &mut rusqlite::Connection,
    source_url: &str,
    guid: &str,
    now: i64,
    signer: &signing::NodeSigner,
) -> Result<Vec<SignedEventRow>, ApiError> {
    if db::get_guid_change(conn, source_url)
        .map_err(ApiError::from)?
        .is_none()
    {
        return Ok(vec![]);
    }

    let tx = conn.transaction().map_err(db::DbError::from)?;
    db::delete_guid_change(&tx, source_url).map_err(ApiError::from)?;
    let delete_marker = sign_event_row(
        &tx,
        guid_change_observed_event_row(source_url, guid, guid, now, now)?,
        signer,
    )?;
    tx.commit().map_err(db::DbError::from)?;
    Ok(vec![delete_marker])
}

/// Runs the ADR 0052 section 5 transition (task 007 items 1 and 3), in one
/// transaction: retires `old_guid` with reason `guid_superseded` and no
/// block, signs `FeedRetired` then `FeedGuidSuperseded`, and deletes the
/// pending row of `source_url` when one exists.
///
/// Does not admit the new record at `new_guid`. `handle_ingest_feed` calls
/// this immediately before step 11 (`db::ingest_transaction`), not from the
/// `classify_submission` match that decides a transition must run — every
/// check that can still reject the submission (the track-count limit, and
/// every step that builds the new record) must run first, so a rejection
/// there never leaves `old_guid` retired with no replacement and no payment
/// route. Calling this immediately before step 11, rather than any earlier,
/// still keeps it strictly before that call: `feeds.feed_url` is UNIQUE, and
/// `db::ingest_transaction` inserts the new record at the `feed_url` value
/// `old_guid`'s row holds until this call retires it.
///
/// The caller does not return early after this call: it falls through to
/// the ordinary admission code that already runs below, because deleting
/// `old_guid` here makes `classify_submission` give `NewFeed` for `new_guid`
/// on its next call, with no new case needed there (task 007's second
/// escalation trigger).
///
/// A block would stop the source URL from being admitted again under its
/// new GUID, and a later reversal back to `old_guid` would then be
/// impossible (task 007's first escalation trigger) — this call never signs
/// one, so neither trigger fires.
///
/// # Errors
///
/// Returns [`ApiError`] if a database or signing step fails.
fn guid_change_transition(
    conn: &mut rusqlite::Connection,
    old_guid: &str,
    new_guid: &str,
    source_url: &str,
    signer: &signing::NodeSigner,
    now: i64,
) -> Result<Vec<SignedEventRow>, ApiError> {
    // Reads happen before the transaction, under the caller's writer lock,
    // mirroring handle_retire_feed.
    let old_feed = db::get_feed_by_guid(conn, old_guid).map_err(ApiError::from)?;
    let old_tracks = db::get_tracks_for_feed(conn, old_guid).map_err(ApiError::from)?;

    let tx = conn.transaction().map_err(db::DbError::from)?;

    for track in &old_tracks {
        let _ = crate::search::delete_from_search_index(
            &tx,
            "track",
            &db::canonical_track_entity_id(&track.feed_guid, &track.track_guid),
            "",
            &track.title,
            track.description.as_deref().unwrap_or(""),
            "",
        );
    }
    if let Some(feed) = &old_feed {
        let _ = crate::search::delete_from_search_index(
            &tx,
            "feed",
            &feed.feed_guid,
            "",
            &feed.title,
            feed.description.as_deref().unwrap_or(""),
            feed.raw_medium.as_deref().unwrap_or(""),
        );
    }

    db::delete_feed_sql(&tx, old_guid).map_err(ApiError::from)?;
    let retired_payload = event::FeedRetiredPayload {
        feed_guid: old_guid.to_string(),
        reason: Some("guid_superseded".to_string()),
    };
    let retired_json = serde_json::to_string(&retired_payload).map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("failed to serialize FeedRetired payload: {e}"),
        www_authenticate: None,
    })?;
    let mut signed_rows = vec![sign_event_row(
        &tx,
        db::EventRow {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: event::EventType::FeedRetired,
            payload_json: retired_json,
            subject_guid: old_guid.to_string(),
            created_at: now,
            warnings: vec![],
        },
        signer,
    )?];

    let superseded_payload = event::FeedGuidSupersededPayload {
        old_guid: old_guid.to_string(),
        new_guid: new_guid.to_string(),
        source_url: source_url.to_string(),
    };
    let superseded_json = serde_json::to_string(&superseded_payload).map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("failed to serialize FeedGuidSuperseded payload: {e}"),
        www_authenticate: None,
    })?;
    db::insert_guid_supersession(&tx, old_guid, new_guid, source_url, now)
        .map_err(ApiError::from)?;
    signed_rows.push(sign_event_row(
        &tx,
        db::EventRow {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: event::EventType::FeedGuidSuperseded,
            payload_json: superseded_json,
            subject_guid: old_guid.to_string(),
            created_at: now,
            warnings: vec![],
        },
        signer,
    )?);

    if db::get_guid_change(&tx, source_url)
        .map_err(ApiError::from)?
        .is_some()
    {
        db::delete_guid_change(&tx, source_url).map_err(ApiError::from)?;
        signed_rows.push(sign_event_row(
            &tx,
            guid_change_observed_event_row(source_url, old_guid, old_guid, now, now)?,
            signer,
        )?);
    }

    tx.commit().map_err(db::DbError::from)?;
    Ok(signed_rows)
}

/// Logs one rejection reason of ADR 0051 section 2.
///
/// `source_url` and `canonical_url` are the two URLs the crawler reported
/// for this submission, unchanged by the classification. `reason` is one of
/// `source_conflict`, `record_conflict` or `guid_change_pending`.
fn log_submission_classification(
    feed_guid: &str,
    canonical_url: &str,
    source_url: &str,
    reason: &str,
) {
    tracing::info!(
        feed_guid,
        canonical_url,
        source_url,
        reason,
        "ADR 0051 section 2: submission classified"
    );
}

fn non_empty_trimmed(value: Option<&str>) -> Option<&str> {
    let value = value?.trim();
    if value.is_empty() { None } else { Some(value) }
}

fn is_platform_owner_name(name: &str) -> bool {
    classify_platform_owner(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::{
        build_source_contributor_claims, build_source_entity_links, build_source_platform_claims,
        derive_release_artist, normalize_role,
    };
    use crate::ingest::{IngestFeedData, IngestLink, IngestPerson};

    #[test]
    fn normalize_role_lowercases_and_collapses_whitespace() {
        assert_eq!(normalize_role(None), None);
        assert_eq!(normalize_role(Some("   ")), None);
        assert_eq!(
            normalize_role(Some("  Music   Contributor  ")).as_deref(),
            Some("music contributor")
        );
        assert_eq!(normalize_role(Some("Host")).as_deref(), Some("host"));
    }

    #[test]
    fn source_link_claims_preserve_payload_extraction_paths() {
        let links = vec![IngestLink {
            position: 0,
            link_type: "website".into(),
            url: "https://example.com/artist".into(),
            extraction_path: "feed.atom:link[@rel='alternate']".into(),
        }];

        let claims = build_source_entity_links("feed-1", "feed", "feed-1", &links, 123);
        assert_eq!(claims.len(), 1);
        assert_eq!(
            claims[0].extraction_path,
            "feed.atom:link[@rel='alternate']"
        );
    }

    #[test]
    fn source_contributor_claims_normalize_duplicate_input_positions() {
        let persons = vec![
            IngestPerson {
                position: 0,
                name: "Alice".into(),
                role: Some("Vocals".into()),
                group_name: None,
                href: None,
                img: None,
                npub: Some("npub1alice".into()),
            },
            IngestPerson {
                position: 0,
                name: "Bob".into(),
                role: Some("Guitar".into()),
                group_name: None,
                href: None,
                img: None,
                npub: None,
            },
        ];

        let claims = build_source_contributor_claims("feed-1", "feed", "feed-1", &persons, 123);
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].position, 0);
        assert_eq!(claims[0].npub.as_deref(), Some("npub1alice"));
        assert_eq!(claims[1].position, 1);
    }

    fn empty_feed() -> IngestFeedData {
        IngestFeedData {
            feed_guid: "feed-guid".into(),
            title: "Release Title".into(),
            description: None,
            image_url: None,
            language: None,
            explicit: false,
            itunes_type: None,
            raw_medium: Some("music".into()),
            last_build_date: None,
            new_feed_url: None,
            locked: None,
            locked_owner: None,
            author_name: None,
            owner_name: None,
            pub_date: None,
            remote_items: vec![],
            persons: vec![],
            entity_ids: vec![],
            links: vec![],
            feed_payment_routes: vec![],
            live_items: vec![],
            tracks: vec![],
        }
    }

    #[test]
    fn derive_release_artist_prefers_author_name() {
        let mut feed = empty_feed();
        feed.author_name = Some("Real Artist".into());
        feed.owner_name = Some("Wavlake".into());
        feed.links.push(IngestLink {
            position: 0,
            link_type: "website".into(),
            url: "https://wavlake.com/dj-omegaman".into(),
            extraction_path: "feed.link".into(),
        });

        assert_eq!(
            derive_release_artist(&feed),
            ("Real Artist".to_string(), "itunes_author")
        );
    }

    #[test]
    fn derive_release_artist_falls_back_to_non_platform_owner_name() {
        let mut feed = empty_feed();
        feed.owner_name = Some("  Indie Collective  ".into());

        assert_eq!(
            derive_release_artist(&feed),
            ("Indie Collective".to_string(), "itunes_owner")
        );
    }

    #[test]
    fn derive_release_artist_owner_wavlake_with_no_author_gives_placeholder() {
        let mut feed = empty_feed();
        feed.owner_name = Some("Wavlake".into());

        assert_eq!(
            derive_release_artist(&feed),
            ("Unknown Artist".to_string(), "placeholder")
        );
    }

    #[test]
    fn derive_release_artist_gives_placeholder_when_no_author_or_owner() {
        let feed = empty_feed();

        assert_eq!(
            derive_release_artist(&feed),
            ("Unknown Artist".to_string(), "placeholder")
        );
    }

    #[test]
    fn source_platform_claims_capture_canonical_url_link_and_owner() {
        let mut feed = empty_feed();
        feed.owner_name = Some("Wavlake".into());
        feed.links.push(IngestLink {
            position: 0,
            link_type: "website".into(),
            url: "https://wavlake.com/dj-omegaman".into(),
            extraction_path: "feed.link".into(),
        });

        let claims = build_source_platform_claims(
            "feed-guid",
            "https://wavlake.com/feed/music/abc123",
            feed.owner_name.as_deref(),
            &feed.links,
            123,
        );

        assert_eq!(claims.len(), 3);
        assert!(claims.iter().any(|claim| {
            claim.platform_key == "wavlake"
                && claim.url.as_deref() == Some("https://wavlake.com/feed/music/abc123")
                && claim.extraction_path == "request.canonical_url"
        }));
        assert!(claims.iter().any(|claim| {
            claim.platform_key == "wavlake"
                && claim.url.as_deref() == Some("https://wavlake.com/dj-omegaman")
                && claim.extraction_path == "feed.link"
        }));
        assert!(claims.iter().any(|claim| {
            claim.platform_key == "wavlake"
                && claim.owner_name.as_deref() == Some("Wavlake")
                && claim.extraction_path == "feed.owner_name"
        }));
    }
}

fn build_source_entity_id_claims(
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
    entity_ids: &[ingest::IngestEntityId],
    now: i64,
) -> Vec<model::SourceEntityIdClaim> {
    let mut seen = HashSet::new();
    entity_ids
        .iter()
        .filter(|claim| seen.insert((claim.scheme.clone(), claim.value.clone())))
        .map(|claim| model::SourceEntityIdClaim {
            id: None,
            feed_guid: feed_guid.to_string(),
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            position: claim.position,
            scheme: claim.scheme.clone(),
            value: claim.value.clone(),
            source: "podcast_txt".to_string(),
            extraction_path: format!("{entity_type}.podcast:txt"),
            observed_at: now,
        })
        .collect()
}

fn build_source_entity_links(
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
    links: &[ingest::IngestLink],
    now: i64,
) -> Vec<model::SourceEntityLink> {
    let mut seen = HashSet::new();
    links
        .iter()
        .filter(|link| seen.insert((link.link_type.clone(), link.url.clone())))
        .map(|link| {
            let extraction_path = if link.extraction_path.trim().is_empty() {
                match link.link_type.as_str() {
                    "self_feed" => "feed.atom:link[@rel='self']",
                    "website" => "feed.link[*]",
                    "web_page" => "entity.link[*]",
                    "content_stream" => "live_item.@contentLink",
                    _ => "entity.link",
                }
                .to_string()
            } else {
                link.extraction_path.clone()
            };
            model::SourceEntityLink {
                id: None,
                feed_guid: feed_guid.to_string(),
                entity_type: entity_type.to_string(),
                entity_id: entity_id.to_string(),
                position: link.position,
                link_type: link.link_type.clone(),
                url: link.url.clone(),
                source: "rss_link".to_string(),
                extraction_path,
                observed_at: now,
            }
        })
        .collect()
}

#[expect(
    clippy::too_many_arguments,
    reason = "staged source enclosures are built from primary plus alternate fields"
)]
fn build_source_item_enclosures(
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
    primary_url: Option<&str>,
    primary_mime_type: Option<&str>,
    primary_bytes: Option<i64>,
    alternate_enclosures: &[ingest::IngestAlternateEnclosure],
    now: i64,
) -> Vec<model::SourceItemEnclosure> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    if let Some(url) = primary_url.map(str::trim).filter(|value| !value.is_empty()) {
        seen.insert(url.to_string());
        out.push(model::SourceItemEnclosure {
            id: None,
            feed_guid: feed_guid.to_string(),
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            position: 0,
            url: url.to_string(),
            mime_type: primary_mime_type.map(str::to_string),
            bytes: primary_bytes,
            rel: None,
            title: None,
            is_primary: true,
            source: "rss_enclosure".to_string(),
            extraction_path: format!("{entity_type}.enclosure"),
            observed_at: now,
        });
    }

    for enclosure in alternate_enclosures {
        let url = enclosure.url.trim();
        if url.is_empty() || !seen.insert(url.to_string()) {
            continue;
        }
        out.push(model::SourceItemEnclosure {
            id: None,
            feed_guid: feed_guid.to_string(),
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            position: i64::try_from(out.len()).unwrap_or(i64::MAX),
            url: url.to_string(),
            mime_type: enclosure.mime_type.clone(),
            bytes: enclosure.bytes,
            rel: enclosure.rel.clone(),
            title: enclosure.title.clone(),
            is_primary: false,
            source: "podcast_alternate_enclosure".to_string(),
            extraction_path: if enclosure.extraction_path.trim().is_empty() {
                format!("{entity_type}.podcast:alternateEnclosure")
            } else {
                enclosure.extraction_path.clone()
            },
            observed_at: now,
        });
    }

    out
}

fn build_source_item_transcripts(
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
    transcripts: &[ingest::IngestTranscript],
    now: i64,
) -> Vec<model::SourceItemTranscript> {
    let mut out = Vec::new();
    for transcript in transcripts {
        let url = transcript.url.trim();
        if url.is_empty() {
            continue;
        }
        out.push(model::SourceItemTranscript {
            id: None,
            feed_guid: feed_guid.to_string(),
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            position: i64::try_from(out.len()).unwrap_or(i64::MAX),
            url: url.to_string(),
            mime_type: transcript.mime_type.clone(),
            language: transcript.language.clone(),
            rel: transcript.rel.clone(),
            source: "podcast_transcript".to_string(),
            extraction_path: format!("{entity_type}.podcast:transcript"),
            observed_at: now,
        });
    }
    out
}

fn build_source_platform_claims(
    feed_guid: &str,
    canonical_url: &str,
    owner_name: Option<&str>,
    links: &[ingest::IngestLink],
    now: i64,
) -> Vec<model::SourcePlatformClaim> {
    let mut seen = HashSet::new();
    let mut claims = Vec::new();

    if let Some(platform_key) = classify_platform_url(canonical_url) {
        let url = canonical_url.trim().to_string();
        seen.insert((
            platform_key.to_string(),
            Some(url.clone()),
            None::<String>,
            "request.canonical_url".to_string(),
        ));
        claims.push(model::SourcePlatformClaim {
            id: None,
            feed_guid: feed_guid.to_string(),
            platform_key: platform_key.to_string(),
            url: Some(url),
            owner_name: None,
            source: "platform_classifier".to_string(),
            extraction_path: "request.canonical_url".to_string(),
            observed_at: now,
        });
    }

    for link in links {
        let Some(platform_key) = classify_platform_url(&link.url) else {
            continue;
        };
        let url = link.url.trim().to_string();
        let extraction_path = if link.extraction_path.trim().is_empty() {
            "feed.link".to_string()
        } else {
            link.extraction_path.clone()
        };
        if !seen.insert((
            platform_key.to_string(),
            Some(url.clone()),
            None::<String>,
            extraction_path.clone(),
        )) {
            continue;
        }
        claims.push(model::SourcePlatformClaim {
            id: None,
            feed_guid: feed_guid.to_string(),
            platform_key: platform_key.to_string(),
            url: Some(url),
            owner_name: None,
            source: "platform_classifier".to_string(),
            extraction_path,
            observed_at: now,
        });
    }

    if let Some(owner_name) = non_empty_trimmed(owner_name)
        && let Some(platform_key) = classify_platform_owner(owner_name)
    {
        let owner_name = owner_name.to_string();
        if seen.insert((
            platform_key.to_string(),
            None::<String>,
            Some(owner_name.clone()),
            "feed.owner_name".to_string(),
        )) {
            claims.push(model::SourcePlatformClaim {
                id: None,
                feed_guid: feed_guid.to_string(),
                platform_key: platform_key.to_string(),
                url: None,
                owner_name: Some(owner_name),
                source: "platform_classifier".to_string(),
                extraction_path: "feed.owner_name".to_string(),
                observed_at: now,
            });
        }
    }

    claims
}

fn classify_platform_url(url: &str) -> Option<&'static str> {
    let url = reqwest::Url::parse(url).ok()?;
    let host = url.host_str()?.trim().to_ascii_lowercase();
    match host.as_str() {
        "wavlake.com" | "www.wavlake.com" => Some("wavlake"),
        "fountain.fm" | "www.fountain.fm" | "feeds.fountain.fm" => Some("fountain"),
        "rssblue.com" | "www.rssblue.com" | "feeds.rssblue.com" => Some("rss_blue"),
        "podhome.fm" | "serve.podhome.fm" => Some("podhome"),
        "justcast.com" | "feed.justcast.com" => Some("justcast"),
        _ => None,
    }
}

fn classify_platform_owner(owner_name: &str) -> Option<&'static str> {
    match owner_name.trim().to_ascii_lowercase().as_str() {
        "wavlake" => Some("wavlake"),
        "fountain" => Some("fountain"),
        "rss blue" | "rssblue" => Some("rss_blue"),
        "podhome" => Some("podhome"),
        "justcast" => Some("justcast"),
        _ => None,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "claim builder mirrors the stored source-claim columns"
)]
fn push_source_release_claim(
    claims: &mut Vec<model::SourceReleaseClaim>,
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
    claim_type: &str,
    claim_value: Option<String>,
    extraction_path: &str,
    now: i64,
) {
    let Some(claim_value) = claim_value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    else {
        return;
    };
    claims.push(model::SourceReleaseClaim {
        id: None,
        feed_guid: feed_guid.to_string(),
        entity_type: entity_type.to_string(),
        entity_id: entity_id.to_string(),
        position: 0,
        claim_type: claim_type.to_string(),
        claim_value,
        source: "rss_metadata".to_string(),
        extraction_path: extraction_path.to_string(),
        observed_at: now,
    });
}

// ── ApiError ─────────────────────────────────────────────────────────────────

/// HTTP error response returned by all handlers; serializes to `{"error":"..."}`.
// RFC 6750 compliant — 2026-03-12
// CRIT-03 Debug derive — 2026-03-13
#[derive(Debug)]
pub struct ApiError {
    /// HTTP status code sent to the client.
    pub status: StatusCode,
    /// Human-readable error message included in the JSON body.
    pub message: String,
    /// Optional `WWW-Authenticate` header value for 401/403 responses (RFC 6750 section 3).
    pub www_authenticate: Option<HeaderValue>,
}

#[derive(Serialize, ToSchema)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    // RFC 6750 compliant — 2026-03-12
    fn into_response(self) -> Response {
        let body = Json(ErrorBody {
            error: self.message,
        });
        if let Some(challenge) = self.www_authenticate {
            let mut headers = HeaderMap::new();
            headers.insert("WWW-Authenticate", challenge);
            (self.status, headers, body).into_response()
        } else {
            (self.status, body).into_response()
        }
    }
}

// Mutex safety compliant — 2026-03-12
impl From<db::DbError> for ApiError {
    fn from(e: db::DbError) -> Self {
        let message = match e {
            db::DbError::Rusqlite(inner) => format!("database error: {inner}"),
            db::DbError::Json(inner) => format!("json error: {inner}"),
            db::DbError::Poisoned => "database mutex poisoned".to_string(),
            db::DbError::Other(msg) => msg,
        };
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message,
            www_authenticate: None,
        }
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        Self::from(db::DbError::from(e))
    }
}

// ── spawn_db helpers ─────────────────────────────────────────────────────────

/// Runs a blocking closure with a **read-only** pooled connection on a
/// `spawn_blocking` task. Uses the reader pool so multiple read handlers
/// can run concurrently under WAL mode.
///
/// # Errors
///
/// Returns `ApiError` (HTTP 500) if the reader pool is exhausted, the
/// spawned task panics, or the closure returns a `DbError`.
// Issue-WAL-POOL — 2026-03-14
pub async fn spawn_db<F, T>(pool: db_pool::DbPool, f: F) -> Result<T, ApiError>
where
    F: FnOnce(&rusqlite::Connection) -> Result<T, db::DbError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let conn = pool.reader()?;
        f(&conn)
    })
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?
    .map_err(ApiError::from)
}

/// Runs a blocking closure with the **writer** connection (shared reference)
/// on a `spawn_blocking` task. Uses the single writer mutex — `SQLite` allows
/// only one concurrent writer.
///
/// Use this for handlers that write via `&Connection` (e.g. `INSERT`,
/// `upsert_peer_node`). For handlers that need `&mut Connection` (e.g.
/// transactions), use [`spawn_db_mut`].
///
/// # Errors
///
/// Returns `ApiError` (HTTP 500) if the writer mutex is poisoned, the
/// spawned task panics, or the closure returns a `DbError`.
// Issue-WAL-POOL — 2026-03-14
pub async fn spawn_db_write<F, T>(pool: db_pool::DbPool, f: F) -> Result<T, ApiError>
where
    F: FnOnce(&rusqlite::Connection) -> Result<T, db::DbError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let conn = pool
            .writer()
            .lock()
            .map_err(|_poison| db::DbError::Poisoned)?;
        f(&conn)
    })
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?
    .map_err(ApiError::from)
}

/// Runs a blocking closure with the **writer** connection (exclusive reference)
/// on a `spawn_blocking` task. Uses the single writer mutex — `SQLite` allows
/// only one concurrent writer.
///
/// Use this for handlers that need `&mut Connection` (e.g. transactions).
///
/// # Errors
///
/// Returns `ApiError` (HTTP 500) if the writer mutex is poisoned, the
/// spawned task panics, or the closure returns a `DbError`.
// Issue-WAL-POOL — 2026-03-14
pub async fn spawn_db_mut<F, T>(pool: db_pool::DbPool, f: F) -> Result<T, ApiError>
where
    F: FnOnce(&mut rusqlite::Connection) -> Result<T, db::DbError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut conn = pool
            .writer()
            .lock()
            .map_err(|_poison| db::DbError::Poisoned)?;
        f(&mut conn)
    })
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?
    .map_err(ApiError::from)
}

// ── SP-08 CORS — 2026-03-13 ──────────────────────────────────────────────────

/// Builds the CORS middleware layer used by both primary and readonly routers.
// Issue #18 configurable CORS origin — 2026-03-13
fn build_cors_layer() -> CorsLayer {
    let cors = CorsLayer::new();

    let cors = match std::env::var("CORS_ALLOW_ORIGIN") {
        Ok(origin) if !origin.is_empty() => {
            let header_value: HeaderValue = origin
                .parse()
                .expect("CORS_ALLOW_ORIGIN must be a valid header value");
            cors.allow_origin(header_value)
        }
        _ => cors.allow_origin(Any),
    };

    cors.allow_methods([
        Method::GET,
        Method::POST,
        Method::PATCH,
        Method::DELETE,
        Method::OPTIONS,
    ])
    .allow_headers([
        header::AUTHORIZATION,
        header::CONTENT_TYPE,
        axum::http::HeaderName::from_static("x-admin-token"),
        // Finding-3 separate sync token — 2026-03-13
        axum::http::HeaderName::from_static("x-sync-token"),
    ])
    .max_age(Duration::from_secs(CORS_MAX_AGE_SECS))
}

// ── Router ────────────────────────────────────────────────────────────────────

/// Builds the full read-write router used by the primary node.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::<Arc<AppState>>::new()
        .route(
            "/api",
            get(|| async { Html(crate::openapi::api_explorer_html()) }),
        )
        .route(
            "/api.html",
            get(|| async { Html(crate::openapi::api_explorer_html()) }),
        )
        .route(
            "/openapi.json",
            get(|| async { Json(crate::openapi::primary_document()) }),
        )
        .route("/ingest/feed", post(handle_ingest_feed))
        .route("/sync/events", get(handle_sync_events))
        .route("/sync/reconcile", post(handle_sync_reconcile))
        .route("/sync/register", post(handle_sync_register))
        .route("/sync/peers", get(handle_sync_peers))
        .route("/node/info", get(handle_node_info))
        // Route versioning compliant — 2026-03-12
        .route(
            "/v1/feeds/{guid}",
            delete(handle_retire_feed).patch(handle_patch_feed),
        )
        .route(
            "/v1/feeds/{guid}/tracks/{track_guid}",
            patch(handle_patch_feed_track).delete(handle_remove_track),
        )
        .route("/v1/feeds/{guid}/copies/resolve", post(handle_resolve_copy))
        .route(
            "/v1/feeds/{guid}/guid-change",
            post(handle_guid_change_decision),
        )
        .route("/v1/tracks/{guid}", patch(handle_patch_track))
        .route(
            "/v1/blocks",
            post(handle_create_block).get(handle_list_blocks),
        )
        .route("/v1/blocks/{block_id}", delete(handle_delete_block))
        .route("/health", get(|| async { "ok" }))
        .merge(query::query_routes())
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(build_cors_layer())
        .with_state(state)
}

/// Read-only router for community nodes.
// FG-05 community peers — 2026-03-13
pub fn build_readonly_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/api",
            get(|| async { Html(crate::openapi::api_explorer_html()) }),
        )
        .route(
            "/api.html",
            get(|| async { Html(crate::openapi::api_explorer_html()) }),
        )
        .route(
            "/openapi.json",
            get(|| async { Json(crate::openapi::readonly_document()) }),
        )
        .route("/sync/events", get(handle_sync_events))
        .route("/sync/peers", get(handle_sync_peers))
        .route("/node/info", get(handle_node_info))
        .route("/health", get(|| async { "ok" }))
        .merge(query::query_routes())
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(build_cors_layer())
        .with_state(state)
}

// ── POST /ingest/feed ─────────────────────────────────────────────────────────

/// Reports whether `reason`, taken from `VerifierChain::run`'s rejection
/// error, is the content-hash verifier's no-change sentinel.
///
/// `VerifierChain::run` wraps every `Fail` message as `"[<verifier name>]
/// <message>"` before returning it, so a bare comparison against
/// [`crate::verifiers::content_hash::NO_CHANGE_SENTINEL`] never matches. This
/// applies the same wrapping so the comparison matches the value the chain
/// actually returns.
fn is_no_change_reason(reason: &str) -> bool {
    use crate::verify::Verifier as _;
    reason
        == format!(
            "[{}] {}",
            crate::verifiers::content_hash::ContentHashVerifier.name(),
            crate::verifiers::content_hash::NO_CHANGE_SENTINEL
        )
}

/// The destination and the name of the ADR 0052 trigger that moved a record
/// (task 006). `trigger` is one of `"self link"`, `"new-feed-url"` or
/// `"permanent redirect"`, and appears verbatim in the move warning and log:
/// `moved from <old> to <new> (ADR 0052 <trigger>)`.
struct MoveTarget {
    new_feed_url: String,
    trigger: &'static str,
}

/// ADR 0052 sections 1 and 2, task 006: the one test for a source-triggered
/// move.
///
/// A submission moves the record on the first of these that matches:
///
/// - `classify_submission` gives `Mirror`, and `source_url` or
///   `canonical_url` equals the record's declared `itunes:new-feed-url`:
///   trigger `new-feed-url`.
/// - `classify_submission` gives `Mirror`, and one of them equals the
///   record's declared self link: trigger `self link`.
/// - `classify_submission` gives `Update`, `source_url` is the record's
///   stored source URL, `redirects` is not empty, every hop is `301` or
///   `308`, and `canonical_url` is not the stored source URL: trigger
///   `permanent redirect`, target `canonical_url`.
///
/// Returns the move target and its trigger, or `None`. Both the read phase
/// and the write phase of `handle_ingest_feed` call this function, so the
/// two phases use one rule and cannot disagree.
///
/// # Errors
///
/// Returns [`ApiError`] if a database read fails.
fn move_target(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    source_url: &str,
    canonical_url: &str,
    redirects: &[ingest::RedirectHop],
) -> Result<Option<MoveTarget>, ApiError> {
    match db::classify_submission(conn, feed_guid, source_url, canonical_url)? {
        db::SubmissionClass::Mirror { .. } => {
            if let Some(new_feed_url) = db::get_declared_new_feed_url(conn, feed_guid)?
                .filter(|declared| declared == source_url || declared == canonical_url)
            {
                return Ok(Some(MoveTarget {
                    new_feed_url,
                    trigger: "new-feed-url",
                }));
            }
            let self_link = db::get_declared_self_url(conn, feed_guid)?
                .filter(|declared| declared == source_url || declared == canonical_url);
            Ok(self_link.map(|new_feed_url| MoveTarget {
                new_feed_url,
                trigger: "self link",
            }))
        }
        db::SubmissionClass::Update => {
            let Some(stored_source_url) = db::get_feed(conn, feed_guid)?.map(|feed| feed.feed_url)
            else {
                return Ok(None);
            };
            if source_url != stored_source_url
                || redirects.is_empty()
                || canonical_url == stored_source_url
                || !redirects
                    .iter()
                    .all(|hop| hop.status == 301 || hop.status == 308)
            {
                return Ok(None);
            }
            Ok(Some(MoveTarget {
                new_feed_url: canonical_url.to_string(),
                trigger: "permanent redirect",
            }))
        }
        db::SubmissionClass::RecordConflict
        | db::SubmissionClass::GuidChange { .. }
        | db::SubmissionClass::NewFeed => Ok(None),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "single ingest flow — splitting would obscure the sequential validation steps"
)]
#[expect(
    clippy::needless_collect,
    reason = "events_for_fanout snapshot is required because event_rows is consumed by ingest_transaction"
)]
async fn handle_ingest_feed(
    State(state): State<Arc<AppState>>,
    // ADR 0052 sections 1 and 2, task 006: `mut` lets the read phase set
    // `force_reingest` on a move, before the verifier chain runs.
    Json(mut req): Json<ingest::IngestFeedRequest>,
) -> Result<Json<ingest::IngestResponse>, ApiError> {
    let started_at = Instant::now();
    let log_canonical_url = req.canonical_url.clone();
    let log_source_url = req.source_url.clone();
    let log_http_status = req.http_status;
    let log_feed_guid = req
        .feed_data
        .as_ref()
        .map_or_else(String::new, |feed_data| feed_data.feed_guid.clone());
    let log_raw_medium = req
        .feed_data
        .as_ref()
        .and_then(|feed_data| feed_data.raw_medium.clone())
        .unwrap_or_default();
    let state2 = Arc::clone(&state);
    // Mutex safety compliant — 2026-03-12
    let result = tokio::task::spawn_blocking(move || -> Result<IngestBlockingOutput, ApiError> {
        // ADR 0051 section 4: the node checks the crawl token before any
        // database read, before the verifier chain, and before the
        // content-hash shortcut. No VERIFIER_CHAIN value can add, remove or
        // reorder this check.
        if let Err(e) = state2.chain.authenticate(&req) {
            return Ok((
                ingest::IngestResponse {
                    accepted: false,
                    no_change: false,
                    reason: Some(e.0),
                    events_emitted: vec![],
                    warnings: vec![],
                    source_url: None,
                },
                vec![],
                vec![],
            ));
        }

        // Issue-VERIFY-READER — 2026-03-16
        // Phase 1: verify against a READ-ONLY connection (reader pool).
        // This avoids holding the writer mutex during verification, so
        // non-trivial verifiers never block the global write path.
        let read_outcome = {
            let reader = state2.db.reader().map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("reader pool error: {e}"),
                www_authenticate: None,
            })?;

            // ADR 0053 section 1: a block on the declared GUID or on either
            // URL rejects the submission, after the crawl-token check and
            // before the verifier chain runs.
            let block_guid = req.feed_data.as_ref().map(|fd| fd.feed_guid.as_str());
            if let Some(block) =
                db::find_feed_block(&reader, block_guid, &[&req.source_url, &req.canonical_url])?
            {
                tracing::info!(
                    feed_guid = block_guid.unwrap_or(""),
                    canonical_url = req.canonical_url.as_str(),
                    block_id = block.block_id.as_str(),
                    kind = block.kind.as_str(),
                    "ADR 0053 section 1: ingest rejected, feed is blocked"
                );
                return Ok((
                    ingest::IngestResponse {
                        accepted: false,
                        no_change: false,
                        reason: Some("blocked".to_string()),
                        events_emitted: vec![],
                        warnings: vec![],
                        source_url: None,
                    },
                    vec![],
                    vec![],
                ));
            }

            // ADR 0052 sections 1 and 2, task 006: a move must not stop at
            // `no_change`. Check the move here, before the chain runs, and
            // force the chain to re-ingest when it is one. The other
            // verifiers still run.
            if let Some(feed_data) = req.feed_data.as_ref()
                && move_target(
                    &reader,
                    &feed_data.feed_guid,
                    &req.source_url,
                    &req.canonical_url,
                    &req.redirects,
                )?
                .is_some()
            {
                req.force_reingest = true;
            }

            // 1. Get existing feed (read-only)
            let existing = db::get_existing_feed(&reader, &req.canonical_url)?;

            // 2. Build verify context and run chain against reader
            let ctx = verify::IngestContext {
                request: &req,
                db: &reader, // Issue-VERIFY-READER — ReadConn derefs to Connection
                existing: existing.as_ref(),
            };

            match state2.chain.run(&ctx) {
                Err(ref e) if is_no_change_reason(&e.0) => ReadPhaseOutcome::NoChange,
                Err(e) => ReadPhaseOutcome::Rejected(e.0),
                Ok(w) => ReadPhaseOutcome::Verified(w),
            }
        };
        // reader is dropped here — writer lock is never contested by verification

        let mut warnings = match read_outcome {
            ReadPhaseOutcome::Rejected(reason) => {
                return Ok((
                    ingest::IngestResponse {
                        accepted: false,
                        no_change: false,
                        reason: Some(reason),
                        events_emitted: vec![],
                        warnings: vec![],
                        source_url: None,
                    },
                    vec![],
                    vec![],
                ));
            }
            ReadPhaseOutcome::NoChange => {
                // ADR 0049 Section 1: the feed body did not change, but the
                // URL that delivered it may still be new, or a redirect may
                // have carried a different source_url. The early return
                // above skips the writer, so this step takes it here. Skip
                // it when the crawler sent no feed body to read a
                // podcast:guid from.
                let (event_ids, fanout_events) = if let Some(feed_data) = req.feed_data.as_ref() {
                    let now = db::unix_now();
                    let mut conn = state2.db.writer().lock().map_err(|_poison| ApiError {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        message: "database mutex poisoned".into(),
                        www_authenticate: None,
                    })?;

                    // ADR 0051 section 2: classify before recording an
                    // observation. Update, Mirror and NewFeed keep today's
                    // behavior here; the two conflict cases write nothing.
                    let classification = db::classify_submission(
                        &conn,
                        &feed_data.feed_guid,
                        &req.source_url,
                        &req.canonical_url,
                    )?;
                    let conflict_reason = match &classification {
                        db::SubmissionClass::RecordConflict => Some("record_conflict"),
                        db::SubmissionClass::GuidChange { .. } => Some("guid_change_pending"),
                        db::SubmissionClass::Update
                        | db::SubmissionClass::Mirror { .. }
                        | db::SubmissionClass::NewFeed => None,
                    };
                    if let Some(reason) = conflict_reason {
                        log_submission_classification(
                            &feed_data.feed_guid,
                            &req.canonical_url,
                            &req.source_url,
                            reason,
                        );
                        return Ok((
                            ingest::IngestResponse {
                                accepted: false,
                                no_change: false,
                                reason: Some(reason.to_string()),
                                events_emitted: vec![],
                                warnings: vec![],
                                source_url: None,
                            },
                            vec![],
                            vec![],
                        ));
                    }

                    let url_observation_rows = record_feed_url_observations_for_ingest(
                        &mut conn,
                        &req.canonical_url,
                        &req.source_url,
                        &feed_data.feed_guid,
                        now,
                        &state2.signer,
                    )?;

                    // ADR 0058 sections 1 and 1a: a mirror body that repeats
                    // its last submission still reaches this branch, not the
                    // write phase below. It needs its own copy summary here.
                    let mut copy_rows = Vec::new();
                    if let db::SubmissionClass::Mirror { source_url } = &classification {
                        copy_rows = record_feed_copy_for_ingest(
                            &mut conn,
                            &feed_data.feed_guid,
                            &req.canonical_url,
                            feed_data,
                            now,
                            &state2.signer,
                        )?;
                        if &req.source_url != source_url && req.source_url != req.canonical_url {
                            copy_rows.extend(record_feed_copy_for_ingest(
                                &mut conn,
                                &feed_data.feed_guid,
                                &req.source_url,
                                feed_data,
                                now,
                                &state2.signer,
                            )?);
                        }
                    }

                    let mut event_ids: Vec<String> = url_observation_rows
                        .iter()
                        .map(|signed| signed.row.event_id.clone())
                        .collect();
                    event_ids.extend(copy_rows.iter().map(|signed| signed.row.event_id.clone()));
                    let mut fanout_events = url_observation_rows
                        .into_iter()
                        .map(signed_row_to_event)
                        .collect::<Result<Vec<_>, ApiError>>()?;
                    fanout_events.extend(
                        copy_rows
                            .into_iter()
                            .map(signed_row_to_event)
                            .collect::<Result<Vec<_>, ApiError>>()?,
                    );
                    (event_ids, fanout_events)
                } else {
                    (vec![], vec![])
                };
                return Ok((
                    ingest::IngestResponse {
                        accepted: true,
                        no_change: true,
                        reason: None,
                        events_emitted: event_ids,
                        warnings: vec![],
                        source_url: None,
                    },
                    fanout_events,
                    vec![],
                ));
            }
            ReadPhaseOutcome::Verified(w) => w,
        };

        // Phase 2: mutate — acquire writer lock only after verification passed.
        let mut conn = state2.db.writer().lock().map_err(|_poison| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "database mutex poisoned".into(),
            www_authenticate: None,
        })?;

        // 3. Unwrap feed_data
        let feed_data = req.feed_data.as_ref().ok_or_else(|| ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "feed_data is required for successful ingest".into(),
            www_authenticate: None,
        })?;

        // ADR 0051 section 2: classify the submission before the first
        // write. Only Update and NewFeed may apply content to a record.
        // Mirror records an observation only, unless a trigger of ADR 0052
        // sections 1 and 2 turns it into a move; the two conflict cases
        // write nothing.
        //
        // `record_move` holds `(old_feed_url, new_feed_url, trigger)` once a
        // move is under way. Its presence, not the classification above, is
        // what the rest of the handler checks from here on.
        let mut record_move: Option<(String, String, &'static str)> = None;
        // ADR 0052 sections 4 and 5, task 007: events a GUID-change
        // transition signed, ahead of the events below that admit the body
        // as a new record. Empty unless that transition ran.
        let mut prefix_events: Vec<SignedEventRow> = Vec::new();
        // ADR 0052 sections 4 and 5, task 007: the old GUID of a transition
        // the `classify_submission` match below decided must run, held here
        // rather than run at once. See the comment where this is read,
        // immediately before step 11, for why the transition itself waits.
        let mut pending_guid_transition: Option<String> = None;
        match db::classify_submission(
            &conn,
            &feed_data.feed_guid,
            &req.source_url,
            &req.canonical_url,
        )? {
            db::SubmissionClass::Update => {
                // ADR 0053 section 3: an older copy does not replace a newer
                // copy. The rule fires only when the stored record and the
                // submission both carry `last_build_date`, and it does not
                // read `force_reingest`. A self-link move or a new-feed-url
                // move is classified as Mirror, not Update, so this arm
                // never sees one; a permanent-redirect move is classified as
                // Update and still passes through this check.
                let stored_last_build_date = db::get_feed(&conn, &feed_data.feed_guid)?
                    .and_then(|stored_feed| stored_feed.last_build_date);
                if let (Some(stored_last_build_date), Some(submitted_last_build_date)) =
                    (stored_last_build_date, feed_data.last_build_date)
                    && submitted_last_build_date < stored_last_build_date
                {
                    tracing::info!(
                        feed_guid = feed_data.feed_guid.as_str(),
                        canonical_url = req.canonical_url.as_str(),
                        stored_last_build_date,
                        submitted_last_build_date,
                        "ADR 0053 section 3: ingest rejected, submission is stale"
                    );
                    return Ok((
                        ingest::IngestResponse {
                            accepted: false,
                            no_change: false,
                            reason: Some("stale_submission".to_string()),
                            events_emitted: vec![],
                            warnings: vec![],
                            source_url: None,
                        },
                        vec![],
                        vec![],
                    ));
                }

                // ADR 0052 section 4, task 007: an ingest in the update case
                // names the record's own GUID, so a pending row at this
                // source URL means the source returned to it. Delete the
                // row, and replicate the delete.
                prefix_events.extend(record_guid_change_return_for_ingest(
                    &mut conn,
                    &req.source_url,
                    &feed_data.feed_guid,
                    db::unix_now(),
                    &state2.signer,
                )?);

                // ADR 0052 section 2, task 006: a permanent redirect from
                // the stored source URL moves the record. `move_target` is
                // the one test for this; the read phase above calls the
                // same function, so the two places cannot disagree.
                if let Some(target) = move_target(
                    &conn,
                    &feed_data.feed_guid,
                    &req.source_url,
                    &req.canonical_url,
                    &req.redirects,
                )? {
                    warnings.push(format!(
                        "moved from {} to {} (ADR 0052 {})",
                        req.source_url, target.new_feed_url, target.trigger
                    ));
                    record_move =
                        Some((req.source_url.clone(), target.new_feed_url, target.trigger));
                }
            }
            db::SubmissionClass::NewFeed => {}
            db::SubmissionClass::Mirror { source_url } => {
                // ADR 0052 sections 1 and 2: the record moves when its
                // declared self link or its declared new-feed-url — each
                // written only by a source ingest — names exactly this
                // submission's URL. The node reads `feeds.declared_self_url`
                // and `feeds.declared_new_feed_url` for this decision, and
                // never `source_entity_links` (ADR 0052 Guards): a mirror
                // body cannot supply this evidence. `move_target` is the one
                // test for this; the read phase above calls the same
                // function, so the two places cannot disagree.
                let target = move_target(
                    &conn,
                    &feed_data.feed_guid,
                    &req.source_url,
                    &req.canonical_url,
                    &req.redirects,
                )?;

                if let Some(target) = target {
                    warnings.push(format!(
                        "moved from {source_url} to {} (ADR 0052 {})",
                        target.new_feed_url, target.trigger
                    ));
                    record_move = Some((source_url, target.new_feed_url, target.trigger));
                    // Falls through: the rest of the handler applies the
                    // body as an update, at the new URL (step 7b).
                } else {
                    log_submission_classification(
                        &feed_data.feed_guid,
                        &req.canonical_url,
                        &req.source_url,
                        "source_conflict",
                    );
                    let now = db::unix_now();
                    let url_observation_rows = record_feed_url_observations_for_ingest(
                        &mut conn,
                        &req.canonical_url,
                        &req.source_url,
                        &feed_data.feed_guid,
                        now,
                        &state2.signer,
                    )?;

                    // ADR 0058 sections 1 and 1a: the mirror records a copy
                    // summary of itself, in the same writer lock as the URL
                    // observation above.
                    let mut copy_rows = record_feed_copy_for_ingest(
                        &mut conn,
                        &feed_data.feed_guid,
                        &req.canonical_url,
                        feed_data,
                        now,
                        &state2.signer,
                    )?;
                    if req.source_url != source_url && req.source_url != req.canonical_url {
                        copy_rows.extend(record_feed_copy_for_ingest(
                            &mut conn,
                            &feed_data.feed_guid,
                            &req.source_url,
                            feed_data,
                            now,
                            &state2.signer,
                        )?);
                    }

                    let mut event_ids: Vec<String> = url_observation_rows
                        .iter()
                        .map(|signed| signed.row.event_id.clone())
                        .collect();
                    event_ids.extend(copy_rows.iter().map(|signed| signed.row.event_id.clone()));
                    let mut fanout_events = url_observation_rows
                        .into_iter()
                        .map(signed_row_to_event)
                        .collect::<Result<Vec<_>, ApiError>>()?;
                    fanout_events.extend(
                        copy_rows
                            .into_iter()
                            .map(signed_row_to_event)
                            .collect::<Result<Vec<_>, ApiError>>()?,
                    );
                    return Ok((
                        ingest::IngestResponse {
                            accepted: false,
                            no_change: false,
                            reason: Some("source_conflict".to_string()),
                            events_emitted: event_ids,
                            warnings: vec![],
                            source_url: Some(source_url),
                        },
                        fanout_events,
                        vec![],
                    ));
                }
            }
            db::SubmissionClass::RecordConflict => {
                log_submission_classification(
                    &feed_data.feed_guid,
                    &req.canonical_url,
                    &req.source_url,
                    "record_conflict",
                );
                return Ok((
                    ingest::IngestResponse {
                        accepted: false,
                        no_change: false,
                        reason: Some("record_conflict".to_string()),
                        events_emitted: vec![],
                        warnings: vec![],
                        source_url: None,
                    },
                    vec![],
                    vec![],
                ));
            }
            db::SubmissionClass::GuidChange { held_guid } => {
                // ADR 0052 section 4: `approve` runs the transition at the
                // next submission of the same new GUID from the source URL.
                // The node keeps no other memory of the decision: this reads
                // the row fresh on every submission.
                let approved = db::get_guid_change(&conn, &req.source_url)
                    .map_err(ApiError::from)?
                    .is_some_and(|row| {
                        row.new_guid == feed_data.feed_guid
                            && row.decision.as_deref() == Some("approve")
                    });
                let guid_origin = model::guid_origin_matches(&feed_data.feed_guid, &req.source_url);
                if guid_origin || approved {
                    // ADR 0052 sections 4 and 5, task 007 items 1 and 3: a
                    // transition must run, either because the new GUID is
                    // the UUIDv5 of the source URL, or because the operator
                    // already approved this same new GUID. It does not run
                    // here: `pending_guid_transition` records the old GUID,
                    // and `guid_change_transition` itself runs immediately
                    // before step 11 below, once the track-count check and
                    // every build step between here and there has passed.
                    // Retiring `held_guid` here, before those checks, would
                    // leave it retired with no replacement and no payment
                    // route on a later rejection (for example the track
                    // limit). This handler falls through to admit the body,
                    // exactly as the NewFeed arm does.
                    let trigger = if guid_origin { "UUIDv5" } else { "approved" };
                    warnings.push(format!(
                        "GUID changed from {held_guid} to {} (ADR 0052 {trigger})",
                        feed_data.feed_guid
                    ));
                    pending_guid_transition = Some(held_guid);
                } else {
                    // ADR 0052 section 4, task 007 item 2: otherwise, the
                    // change needs the operator. Write or update the pending
                    // row and answer guid_change_pending, or
                    // guid_change_rejected when the operator already
                    // rejected this same new GUID.
                    let now = db::unix_now();
                    let outcome = record_guid_change_pending_for_ingest(
                        &mut conn,
                        &req.source_url,
                        &held_guid,
                        &feed_data.feed_guid,
                        now,
                        &state2.signer,
                    )?;
                    let (reason, events) = match outcome {
                        GuidChangePendingOutcome::Rejected => ("guid_change_rejected", vec![]),
                        GuidChangePendingOutcome::Pending(events) => {
                            ("guid_change_pending", events)
                        }
                    };
                    log_submission_classification(
                        &feed_data.feed_guid,
                        &req.canonical_url,
                        &req.source_url,
                        reason,
                    );
                    let event_ids: Vec<String> = events
                        .iter()
                        .map(|signed| signed.row.event_id.clone())
                        .collect();
                    let fanout_events = events
                        .into_iter()
                        .map(signed_row_to_event)
                        .collect::<Result<Vec<_>, ApiError>>()?;
                    return Ok((
                        ingest::IngestResponse {
                            accepted: false,
                            no_change: false,
                            reason: Some(reason.to_string()),
                            events_emitted: event_ids,
                            warnings: vec![],
                            source_url: None,
                        },
                        fanout_events,
                        vec![],
                    ));
                }
            }
        }

        let is_musicl = medium::is_musicl(feed_data.raw_medium.as_deref());
        let tracks: &[ingest::IngestTrackData] = if is_musicl { &[] } else { &feed_data.tracks };
        let live_items: &[ingest::IngestLiveItemData] = if is_musicl {
            &[]
        } else {
            &feed_data.live_items
        };

        // 3b. Enforce track count limit to prevent DB growth attacks.
        if tracks.len() > MAX_TRACKS_PER_INGEST {
            return Err(ApiError {
                status: StatusCode::BAD_REQUEST,
                message: format!(
                    "feed contains {} tracks, maximum is {MAX_TRACKS_PER_INGEST}",
                    tracks.len()
                ),
                www_authenticate: None,
            });
        }

        // 4. Build feed-scoped source claims needed for identity resolution.
        let now = db::unix_now();
        let feed_guid_str = feed_data.feed_guid.as_str();
        let feed_remote_items: Vec<model::FeedRemoteItemRaw> = feed_data
            .remote_items
            .iter()
            .map(|item| model::FeedRemoteItemRaw {
                id: None,
                feed_guid: feed_data.feed_guid.clone(),
                position: item.position,
                medium: item.medium.clone(),
                remote_feed_guid: item.remote_feed_guid.clone(),
                remote_feed_url: item.remote_feed_url.clone(),
                rel: item.rel.clone(),
                source: "podcast_remote_item".to_string(),
            })
            .collect();
        let mut source_entity_ids = build_source_entity_id_claims(
            &feed_data.feed_guid,
            "feed",
            &feed_data.feed_guid,
            &feed_data.entity_ids,
            now,
        );
        let mut source_entity_links = build_source_entity_links(
            &feed_data.feed_guid,
            "feed",
            &feed_data.feed_guid,
            &feed_data.links,
            now,
        );

        // 5. Phase 3 source-first transition: keep a deterministic feed-scoped
        // compatibility artist/credit for the published release artist text
        // without invoking cross-feed resolution in ingest.
        //
        // ADR 0049 §5: the feed release artist comes from one RSS element,
        // and the field that names its source travels beside it.
        let (artist_name, release_artist_source) = derive_release_artist(feed_data);
        let feed_artist_credit =
            db::get_or_create_feed_scoped_source_text_credit(&conn, &artist_name, feed_guid_str)?;
        let feed_artist_id = feed_artist_credit
            .names
            .first()
            .map(|name| name.artist_id.clone())
            .ok_or_else(|| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!(
                    "feed artist credit {} missing primary artist name",
                    feed_artist_credit.id
                ),
                www_authenticate: None,
            })?;
        let feed_artist =
            db::get_artist_by_id(&conn, &feed_artist_id)?.ok_or_else(|| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("feed artist {feed_artist_id} missing after credit creation"),
                www_authenticate: None,
            })?;
        let existing_live_events =
            db::get_live_events_for_feed(&conn, feed_guid_str).map_err(ApiError::from)?;

        // 7. Compute newest_item_at and oldest_item_at from track pub_dates
        let pub_dates: Vec<i64> = tracks
            .iter()
            .filter_map(|t| t.pub_date)
            .chain(live_items.iter().filter_map(|li| {
                (li.status.eq_ignore_ascii_case("ended") && li.enclosure_url.is_some())
                    .then_some(li.pub_date)
                    .flatten()
            }))
            .collect();

        let newest_item_at = pub_dates.iter().copied().max();
        let oldest_item_at = pub_dates.iter().copied().min();

        // 7b. ADR 0052 sections 1 and 2: a move takes the new URL. ADR 0049
        // Section 1 otherwise applies: a known GUID keeps its stored
        // feed_url, and a GUID new to this node takes the URL it arrived
        // through.
        let feed_url = if let Some((_, new_feed_url, _)) = &record_move {
            new_feed_url.clone()
        } else {
            db::get_feed(&conn, feed_guid_str)?.map_or_else(
                || req.canonical_url.clone(),
                |existing_feed| existing_feed.feed_url,
            )
        };

        // 8. Build Feed struct
        let feed = model::Feed {
            feed_guid: feed_data.feed_guid.clone(),
            feed_url,
            title: feed_data.title.clone(),
            title_lower: feed_data.title.to_lowercase(),
            artist_credit_id: feed_artist_credit.id,
            description: feed_data.description.clone(),
            image_url: feed_data.image_url.clone(),
            publisher: derive_publisher_name(feed_data),
            language: feed_data.language.clone(),
            explicit: feed_data.explicit,
            itunes_type: feed_data.itunes_type.clone(),
            release_artist: Some(artist_name.clone()),
            release_artist_sort: None,
            release_date: feed_data.pub_date.or(oldest_item_at),
            release_kind: Some("unknown".to_string()),
            #[expect(
                clippy::cast_possible_wrap,
                reason = "episode counts never approach i64::MAX"
            )]
            episode_count: tracks.len() as i64,
            newest_item_at,
            oldest_item_at,
            created_at: now,
            updated_at: now,
            raw_medium: feed_data.raw_medium.clone(),
            last_build_date: feed_data.last_build_date,
            release_artist_source: Some(release_artist_source.to_string()),
        };
        let track_publisher = feed.publisher.clone();

        // 8b. Build feed-level payment routes
        let feed_routes: Vec<model::FeedPaymentRoute> = if is_musicl {
            Vec::new()
        } else {
            feed_data
                .feed_payment_routes
                .iter()
                .map(|r| model::FeedPaymentRoute {
                    id: None,
                    feed_guid: feed_data.feed_guid.clone(),
                    recipient_name: r.recipient_name.clone(),
                    route_type: r.route_type.clone(),
                    address: r.address.clone(),
                    custom_key: r.custom_key.clone(),
                    custom_value: r.custom_value.clone(),
                    split: r.split,
                    fee: r.fee,
                })
                .collect()
        };

        let live_events: Vec<model::LiveEvent> = live_items
            .iter()
            .filter(|item| matches!(item.status.as_str(), "pending" | "live"))
            .map(|item| model::LiveEvent {
                live_item_guid: item.live_item_guid.clone(),
                feed_guid: feed_data.feed_guid.clone(),
                title: item.title.clone(),
                content_link: item.content_link.clone(),
                status: item.status.clone(),
                scheduled_start: item.start_at,
                scheduled_end: item.end_at,
                created_at: now,
                updated_at: now,
            })
            .collect();
        let live_events_for_sse = live_events.clone();

        let mut source_contributor_claims = build_source_contributor_claims(
            &feed_data.feed_guid,
            "feed",
            &feed_data.feed_guid,
            &feed_data.persons,
            now,
        );
        let mut source_release_claims = Vec::new();
        let mut source_item_enclosures = Vec::new();
        let mut source_item_transcripts: Vec<model::SourceItemTranscript> = Vec::new();
        let source_platform_claims = build_source_platform_claims(
            &feed_data.feed_guid,
            &req.canonical_url,
            feed_data.owner_name.as_deref(),
            &feed_data.links,
            now,
        );
        let effective_release_date = feed_data.pub_date.or(oldest_item_at);
        push_source_release_claim(
            &mut source_release_claims,
            &feed_data.feed_guid,
            "feed",
            &feed_data.feed_guid,
            "release_date",
            effective_release_date.map(|v| v.to_string()),
            if feed_data.pub_date.is_some() {
                "feed.pub_date"
            } else {
                "oldest_item.pub_date"
            },
            now,
        );
        // ADR 0043: lastBuildDate is kept as its own claim. It is the feed
        // build time and never supplies the release date.
        push_source_release_claim(
            &mut source_release_claims,
            &feed_data.feed_guid,
            "feed",
            &feed_data.feed_guid,
            "last_build_date",
            feed_data.last_build_date.map(|v| v.to_string()),
            "feed.last_build_date",
            now,
        );
        push_source_release_claim(
            &mut source_release_claims,
            &feed_data.feed_guid,
            "feed",
            &feed_data.feed_guid,
            "description",
            feed_data.description.clone(),
            "feed.description",
            now,
        );
        push_source_release_claim(
            &mut source_release_claims,
            &feed_data.feed_guid,
            "feed",
            &feed_data.feed_guid,
            "language",
            feed_data.language.clone(),
            "feed.language",
            now,
        );
        push_source_release_claim(
            &mut source_release_claims,
            &feed_data.feed_guid,
            "feed",
            &feed_data.feed_guid,
            "image_url",
            feed_data.image_url.clone(),
            "feed.image_url",
            now,
        );
        push_source_release_claim(
            &mut source_release_claims,
            &feed_data.feed_guid,
            "feed",
            &feed_data.feed_guid,
            "raw_medium",
            feed_data.raw_medium.clone(),
            "feed.raw_medium",
            now,
        );
        push_source_release_claim(
            &mut source_release_claims,
            &feed_data.feed_guid,
            "feed",
            &feed_data.feed_guid,
            "itunes_type",
            feed_data.itunes_type.clone(),
            "feed.itunes_type",
            now,
        );

        // 9. Build track tuples
        let mut track_tuples: Vec<db::TrackIngestBundle> =
            Vec::with_capacity(feed_data.tracks.len());

        // Track artist credits for event generation
        let mut track_credits: Vec<model::ArtistCredit> = Vec::with_capacity(tracks.len());

        for track_data in tracks {
            source_contributor_claims.extend(build_source_contributor_claims(
                &feed_data.feed_guid,
                "track",
                &track_data.track_guid,
                &track_data.persons,
                now,
            ));
            source_entity_ids.extend(build_source_entity_id_claims(
                &feed_data.feed_guid,
                "track",
                &track_data.track_guid,
                &track_data.entity_ids,
                now,
            ));
            source_entity_links.extend(build_source_entity_links(
                &feed_data.feed_guid,
                "track",
                &track_data.track_guid,
                &track_data.links,
                now,
            ));
            source_item_enclosures.extend(build_source_item_enclosures(
                &feed_data.feed_guid,
                "track",
                &track_data.track_guid,
                track_data.enclosure_url.as_deref(),
                track_data.enclosure_type.as_deref(),
                track_data.enclosure_bytes,
                &track_data.alternate_enclosures,
                now,
            ));
            source_item_transcripts.extend(build_source_item_transcripts(
                &feed_data.feed_guid,
                "track",
                &track_data.track_guid,
                &track_data.transcripts,
                now,
            ));
            push_source_release_claim(
                &mut source_release_claims,
                &feed_data.feed_guid,
                "track",
                &track_data.track_guid,
                "release_date",
                track_data.pub_date.map(|v| v.to_string()),
                "track.pub_date",
                now,
            );
            push_source_release_claim(
                &mut source_release_claims,
                &feed_data.feed_guid,
                "track",
                &track_data.track_guid,
                "description",
                track_data.description.clone(),
                "track.description",
                now,
            );

            // Per-track artist resolution (feed-scoped)
            // Phase 3 source-first transition: keep feed-scoped compatibility
            // credits without invoking cross-feed artist resolution here.
            let (track_credit_id, track_credit) = if let Some(author) = &track_data.author_name {
                let credit =
                    db::get_or_create_feed_scoped_source_text_credit(&conn, author, feed_guid_str)?;
                (credit.id, credit)
            } else {
                (feed_artist_credit.id, feed_artist_credit.clone())
            };

            let track = model::Track {
                track_guid: track_data.track_guid.clone(),
                feed_guid: feed_data.feed_guid.clone(),
                artist_credit_id: track_credit_id,
                title: track_data.title.clone(),
                title_lower: track_data.title.to_lowercase(),
                pub_date: track_data.pub_date,
                duration_secs: track_data.duration_secs,
                image_url: track_data.image_url.clone(),
                publisher: track_publisher.clone(),
                language: track_data
                    .language
                    .clone()
                    .or_else(|| feed_data.language.clone()),
                enclosure_url: track_data.enclosure_url.clone(),
                enclosure_type: track_data.enclosure_type.clone(),
                enclosure_bytes: track_data.enclosure_bytes,
                track_number: track_data.track_number,
                season: track_data.season,
                explicit: track_data.explicit,
                description: track_data.description.clone(),
                track_artist: Some(
                    track_data
                        .author_name
                        .clone()
                        .unwrap_or_else(|| artist_name.clone()),
                ),
                track_artist_sort: None,
                created_at: now,
                updated_at: now,
            };

            let routes: Vec<model::PaymentRoute> = track_data
                .payment_routes
                .iter()
                .map(|r| model::PaymentRoute {
                    id: None,
                    track_guid: track_data.track_guid.clone(),
                    feed_guid: feed_data.feed_guid.clone(),
                    recipient_name: r.recipient_name.clone(),
                    route_type: r.route_type.clone(),
                    address: r.address.clone(),
                    custom_key: r.custom_key.clone(),
                    custom_value: r.custom_value.clone(),
                    split: r.split,
                    fee: r.fee,
                })
                .collect();

            let vts: Vec<model::ValueTimeSplit> = track_data
                .value_time_splits
                .iter()
                .map(|v| model::ValueTimeSplit {
                    id: None,
                    source_feed_guid: feed_data.feed_guid.clone(),
                    source_track_guid: track_data.track_guid.clone(),
                    start_time_secs: v.start_time_secs,
                    duration_secs: v.duration_secs,
                    remote_feed_guid: v.remote_feed_guid.clone(),
                    remote_item_guid: v.remote_item_guid.clone(),
                    split: v.split,
                    created_at: now,
                })
                .collect();

            let track_remote_items: Vec<model::TrackRemoteItemRaw> = track_data
                .remote_items
                .iter()
                .map(|r| model::TrackRemoteItemRaw {
                    id: None,
                    feed_guid: feed_data.feed_guid.clone(),
                    track_guid: track_data.track_guid.clone(),
                    position: r.position,
                    medium: r.medium.clone(),
                    remote_feed_guid: r.remote_feed_guid.clone(),
                    remote_feed_url: r.remote_feed_url.clone(),
                    rel: r.rel.clone(),
                    source: "podcast_remote_item".into(),
                })
                .collect();

            track_tuples.push((track, routes, vts, track_remote_items));
            track_credits.push(track_credit);
        }

        for live_item in live_items {
            source_contributor_claims.extend(build_source_contributor_claims(
                &feed_data.feed_guid,
                "live_item",
                &live_item.live_item_guid,
                &live_item.persons,
                now,
            ));
            source_entity_ids.extend(build_source_entity_id_claims(
                &feed_data.feed_guid,
                "live_item",
                &live_item.live_item_guid,
                &live_item.entity_ids,
                now,
            ));
            source_entity_links.extend(build_source_entity_links(
                &feed_data.feed_guid,
                "live_item",
                &live_item.live_item_guid,
                &live_item.links,
                now,
            ));
            source_item_enclosures.extend(build_source_item_enclosures(
                &feed_data.feed_guid,
                "live_item",
                &live_item.live_item_guid,
                live_item.enclosure_url.as_deref(),
                live_item.enclosure_type.as_deref(),
                live_item.enclosure_bytes,
                &live_item.alternate_enclosures,
                now,
            ));
            source_item_transcripts.extend(build_source_item_transcripts(
                &feed_data.feed_guid,
                "live_item",
                &live_item.live_item_guid,
                &live_item.transcripts,
                now,
            ));
            push_source_release_claim(
                &mut source_release_claims,
                &feed_data.feed_guid,
                "live_item",
                &live_item.live_item_guid,
                "release_date",
                live_item.pub_date.map(|v| v.to_string()),
                "live_item.pub_date",
                now,
            );
            push_source_release_claim(
                &mut source_release_claims,
                &feed_data.feed_guid,
                "live_item",
                &live_item.live_item_guid,
                "description",
                live_item.description.clone(),
                "live_item.description",
                now,
            );

            if !(live_item.status.eq_ignore_ascii_case("ended")
                && live_item.enclosure_url.is_some())
            {
                continue;
            }

            let (track_credit_id, track_credit) = if let Some(author) = &live_item.author_name {
                let credit =
                    db::get_or_create_feed_scoped_source_text_credit(&conn, author, feed_guid_str)?;
                (credit.id, credit)
            } else {
                (feed_artist_credit.id, feed_artist_credit.clone())
            };

            let track = model::Track {
                track_guid: live_item.live_item_guid.clone(),
                feed_guid: feed_data.feed_guid.clone(),
                artist_credit_id: track_credit_id,
                title: live_item.title.clone(),
                title_lower: live_item.title.to_lowercase(),
                pub_date: live_item.pub_date,
                duration_secs: live_item.duration_secs,
                image_url: live_item.image_url.clone(),
                publisher: track_publisher.clone(),
                language: live_item
                    .language
                    .clone()
                    .or_else(|| feed_data.language.clone()),
                enclosure_url: live_item.enclosure_url.clone(),
                enclosure_type: live_item.enclosure_type.clone(),
                enclosure_bytes: live_item.enclosure_bytes,
                track_number: live_item.track_number,
                season: live_item.season,
                explicit: live_item.explicit,
                description: live_item.description.clone(),
                track_artist: Some(
                    live_item
                        .author_name
                        .clone()
                        .unwrap_or_else(|| artist_name.clone()),
                ),
                track_artist_sort: None,
                created_at: now,
                updated_at: now,
            };

            let routes: Vec<model::PaymentRoute> = live_item
                .payment_routes
                .iter()
                .map(|r| model::PaymentRoute {
                    id: None,
                    track_guid: live_item.live_item_guid.clone(),
                    feed_guid: feed_data.feed_guid.clone(),
                    recipient_name: r.recipient_name.clone(),
                    route_type: r.route_type.clone(),
                    address: r.address.clone(),
                    custom_key: r.custom_key.clone(),
                    custom_value: r.custom_value.clone(),
                    split: r.split,
                    fee: r.fee,
                })
                .collect();

            let vts: Vec<model::ValueTimeSplit> = live_item
                .value_time_splits
                .iter()
                .map(|v| model::ValueTimeSplit {
                    id: None,
                    source_feed_guid: feed_data.feed_guid.clone(),
                    source_track_guid: live_item.live_item_guid.clone(),
                    start_time_secs: v.start_time_secs,
                    duration_secs: v.duration_secs,
                    remote_feed_guid: v.remote_feed_guid.clone(),
                    remote_item_guid: v.remote_item_guid.clone(),
                    split: v.split,
                    created_at: now,
                })
                .collect();

            let track_remote_items: Vec<model::TrackRemoteItemRaw> = live_item
                .remote_items
                .iter()
                .map(|r| model::TrackRemoteItemRaw {
                    id: None,
                    feed_guid: feed_data.feed_guid.clone(),
                    track_guid: live_item.live_item_guid.clone(),
                    position: r.position,
                    medium: r.medium.clone(),
                    remote_feed_guid: r.remote_feed_guid.clone(),
                    remote_feed_url: r.remote_feed_url.clone(),
                    rel: r.rel.clone(),
                    source: "podcast_remote_item".into(),
                })
                .collect();

            track_tuples.push((track, routes, vts, track_remote_items));
            track_credits.push(track_credit);
        }

        let source_contributor_claims =
            db::dedupe_source_contributor_claims(&source_contributor_claims);
        let source_entity_ids = db::dedupe_source_entity_ids(&source_entity_ids);
        let source_entity_links = db::dedupe_source_entity_links(&source_entity_links);
        let source_release_claims = db::dedupe_source_release_claims(&source_release_claims);
        let source_item_enclosures = db::dedupe_source_item_enclosures(&source_item_enclosures);
        let source_item_transcripts = db::dedupe_source_item_transcripts(&source_item_transcripts);

        // 9b. ADR 0053 section 4: log each change of the payment recipients
        // of the feed and of each track, before the ingest transaction
        // writes the new state. A new feed or a new track logs nothing,
        // because there is no stored recipient set to compare it to.
        if db::get_feed_by_guid(&conn, &feed_data.feed_guid)?.is_some() {
            let old_feed_set = model::feed_recipient_set(&db::get_feed_payment_routes_for_feed(
                &conn,
                &feed_data.feed_guid,
            )?);
            let new_feed_set = model::feed_recipient_set(&feed_routes);
            if old_feed_set != new_feed_set {
                tracing::warn!(
                    feed_guid = feed_data.feed_guid.as_str(),
                    track_guid = "",
                    old_recipients = %serde_json::to_string(&old_feed_set).unwrap_or_default(),
                    new_recipients = %serde_json::to_string(&new_feed_set).unwrap_or_default(),
                    "ADR 0053 section 4: feed payment recipients changed"
                );
            }
        }
        for (track, routes, _vts, _remote_items) in &track_tuples {
            if db::get_track_for_feed(&conn, &feed_data.feed_guid, &track.track_guid)?.is_none() {
                continue;
            }
            let old_track_set = model::recipient_set(&db::get_payment_routes_for_feed_track(
                &conn,
                &feed_data.feed_guid,
                &track.track_guid,
            )?);
            let new_track_set = model::recipient_set(routes);
            if old_track_set != new_track_set {
                tracing::warn!(
                    feed_guid = feed_data.feed_guid.as_str(),
                    track_guid = track.track_guid.as_str(),
                    old_recipients = %serde_json::to_string(&old_track_set).unwrap_or_default(),
                    new_recipients = %serde_json::to_string(&new_track_set).unwrap_or_default(),
                    "ADR 0053 section 4: track payment recipients changed"
                );
            }
        }

        // 10. Build event rows — Issue-WRITE-AMP — 2026-03-14
        // Only emit events for entities whose fields actually changed
        // compared to what is stored in the DB.
        // Issue-SEQ-INTEGRITY — 2026-03-14: EventRows no longer carry
        // signatures. The signer is passed to ingest_transaction which
        // signs each event after the DB assigns its seq.
        let event_rows = db::build_diff_events(
            &conn,
            &feed_artist,
            &feed_artist_credit,
            &feed,
            &feed_remote_items,
            &source_contributor_claims,
            &source_entity_ids,
            &source_entity_links,
            &source_release_claims,
            &source_item_enclosures,
            &source_item_transcripts,
            &source_platform_claims,
            &feed_routes,
            &live_events,
            &track_tuples,
            &track_credits,
            now,
            &warnings,
        )
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("failed to build diff events: {e}"),
            www_authenticate: None,
        })?;

        // ADR 0052 sections 4 and 5, task 007: a GUID-change transition runs
        // here, immediately before step 11 admits the new record, and only
        // here. Every check that can still reject the submission — the
        // track-count limit (step 3b), and every build step from here back
        // to the classify_submission match (4, 5, 7, 7b, 8, 8b, 9, 9b, 10) —
        // has already passed by this point, because each of them returns
        // early on failure. Running the transition any earlier would retire
        // `old_guid` before one of those checks could still reject the
        // submission, leaving it retired with no replacement and no payment
        // route. Running it here still keeps it strictly before
        // `db::ingest_transaction` below: `feeds.feed_url` is UNIQUE, and
        // that call inserts the new record at the same `feed_url` the old
        // record still holds until this call frees it.
        if let Some(old_guid) = &pending_guid_transition {
            prefix_events.extend(guid_change_transition(
                &mut conn,
                old_guid,
                &feed_data.feed_guid,
                &req.source_url,
                &state2.signer,
                now,
            )?);
        }

        // Collect event_ids and snapshot event data before moving event_rows.
        // ADR 0052 sections 4 and 5, task 007: `prefix_events` carries the
        // FeedRetired/FeedGuidSuperseded/FeedGuidChangeObserved events the
        // GUID-change transition above already signed, ahead of the events
        // below that admit this body as a new record.
        let mut event_ids: Vec<String> = prefix_events
            .iter()
            .map(|signed| signed.row.event_id.clone())
            .chain(event_rows.iter().map(|r| r.event_id.clone()))
            .collect();

        // Snapshot events for fan-out (event_rows is consumed by ingest_transaction)
        // Issue-SEQ-INTEGRITY — 2026-03-14: EventRow no longer carries signed_by/signature.
        let events_for_fanout: Vec<db::EventRow> = event_rows
            .iter()
            .map(|r| db::EventRow {
                event_id: r.event_id.clone(),
                event_type: r.event_type.clone(),
                payload_json: r.payload_json.clone(),
                subject_guid: r.subject_guid.clone(),
                created_at: r.created_at,
                warnings: r.warnings.clone(),
            })
            .collect();
        let ingested_feed = feed.clone();

        // 11. Run ingest transaction (signer signs after DB assigns seq)
        // Issue-SEQ-INTEGRITY — 2026-03-14
        let seqs = db::ingest_transaction(
            &mut conn,
            feed_artist,
            feed_artist_credit,
            feed,
            feed_remote_items,
            source_contributor_claims,
            source_entity_ids,
            source_entity_links,
            source_release_claims,
            source_item_enclosures,
            source_item_transcripts,
            source_platform_claims,
            feed_routes,
            live_events,
            track_tuples,
            event_rows,
            &state2.signer,
        )?;

        // 11a. ADR 0052 sections 1 and 2, task 006: only an ingest in the
        // update or new-feed case records the move declarations a source
        // body makes for itself: the self link, the `itunes:new-feed-url`
        // value, and the `podcast:locked` fact. A move of any trigger starts
        // as a mirror or an update submission, so it never writes these
        // columns; the values already on the record are what decided the
        // move.
        //
        // The write runs right after `ingest_transaction`, under the same
        // writer lock, rather than through a new parameter on that function.
        // The lock already serializes every writer, so this UPDATE cannot
        // interleave with another ingest; adding the write here keeps
        // `ingest_transaction`'s already-large parameter list unchanged.
        if record_move.is_none() {
            let declared_self_url = feed_data
                .links
                .iter()
                .find(|link| link.link_type == "self_feed")
                .map(|link| link.url.as_str());
            db::set_declared_self_url(&conn, feed_guid_str, declared_self_url)?;
            db::set_feed_move_declarations(
                &conn,
                feed_guid_str,
                feed_data.new_feed_url.as_deref(),
                feed_data.locked,
                feed_data.locked_owner.as_deref(),
            )?;
        }

        // ADR 0052 sections 1 and 2: a move updates the record to the new
        // URL. ADR 0056 removed the proof flow, so a move no longer revokes
        // a token.
        if let Some((old_feed_url, new_feed_url, trigger)) = &record_move {
            tracing::info!(
                feed_guid = feed_guid_str,
                old_feed_url = old_feed_url.as_str(),
                new_feed_url = new_feed_url.as_str(),
                trigger = *trigger,
                "ADR 0052: a source trigger moved the record"
            );
        }

        // 11b. Search index + quality scores are now written inside
        // ingest_transaction (Issue-5 ingest atomic — 2026-03-13).

        // 11c. ADR 0049 Section 1: record which URL gave this podcast:guid.
        let url_observation_rows = record_feed_url_observations_for_ingest(
            &mut conn,
            &req.canonical_url,
            &req.source_url,
            &ingested_feed.feed_guid,
            now,
            &state2.signer,
        )?;
        event_ids.extend(
            url_observation_rows
                .iter()
                .map(|signed| signed.row.event_id.clone()),
        );

        // 13. Update crawl cache
        db::upsert_feed_crawl_cache(&conn, &req.canonical_url, &req.content_hash, now)?;

        // 14. Reconstruct events with assigned seqs + signatures for fan-out
        // Issue-SEQ-INTEGRITY — 2026-03-14: signatures come from ingest_transaction.
        let mut signed_rows: Vec<SignedEventRow> = prefix_events;
        signed_rows.extend(events_for_fanout.into_iter().zip(seqs).map(
            |(row, (seq, signed_by, signature))| SignedEventRow {
                row,
                seq,
                signed_by,
                signature,
            },
        ));
        signed_rows.extend(url_observation_rows);
        let fanout_events: Vec<event::Event> = signed_rows
            .into_iter()
            .map(signed_row_to_event)
            .collect::<Result<Vec<_>, ApiError>>()?;

        let live_sse_frames = build_live_sse_frames_for_feed(
            &conn,
            &feed_data.feed_guid,
            &existing_live_events,
            &live_events_for_sse,
            &fanout_events,
        )
        .map_err(ApiError::from)?;

        Ok((
            ingest::IngestResponse {
                accepted: true,
                no_change: false,
                reason: None,
                events_emitted: event_ids,
                warnings,
                source_url: None,
            },
            fanout_events,
            live_sse_frames,
        ))
    })
    .await;

    let elapsed = started_at.elapsed();
    let elapsed_ms = elapsed.as_millis();
    let result = match result {
        Ok(result) => result,
        Err(e) => {
            tracing::error!(
                canonical_url = %log_canonical_url,
                source_url = %log_source_url,
                http_status = log_http_status,
                feed_guid = %log_feed_guid,
                raw_medium = %log_raw_medium,
                elapsed_ms,
                error = %e,
                "ingest request panicked"
            );
            return Err(ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("internal task panic: {e}"),
                www_authenticate: None,
            });
        }
    };

    let (response, fanout_events, live_sse_frames) = match result {
        Ok(output) => output,
        Err(err) => {
            tracing::error!(
                canonical_url = %log_canonical_url,
                source_url = %log_source_url,
                http_status = log_http_status,
                feed_guid = %log_feed_guid,
                raw_medium = %log_raw_medium,
                elapsed_ms,
                status = err.status.as_u16(),
                message = %err.message,
                "ingest request failed"
            );
            return Err(err);
        }
    };

    if !response.accepted {
        tracing::warn!(
            canonical_url = %log_canonical_url,
            source_url = %log_source_url,
            http_status = log_http_status,
            feed_guid = %log_feed_guid,
            raw_medium = %log_raw_medium,
            elapsed_ms,
            no_change = response.no_change,
            reason = response.reason.as_deref().unwrap_or(""),
            "ingest request rejected"
        );
    } else if elapsed >= Duration::from_secs(INGEST_SLOW_WARNING_SECS) {
        tracing::warn!(
            canonical_url = %log_canonical_url,
            source_url = %log_source_url,
            http_status = log_http_status,
            feed_guid = %log_feed_guid,
            raw_medium = %log_raw_medium,
            elapsed_ms,
            no_change = response.no_change,
            events_emitted = response.events_emitted.len(),
            "ingest request completed slowly"
        );
    }

    // Fire-and-forget fan-out to push subscribers.
    if !fanout_events.is_empty() {
        // Issue-SSE-PUBLISH — 2026-03-14
        publish_events_to_sse(&state.sse_registry, &fanout_events);
        publish_sse_frames(&state.sse_registry, &live_sse_frames);

        let db_fanout = state.db.clone();
        let client_fanout = state.push_client.clone();
        let subscribers_fanout = Arc::clone(&state.push_subscribers);
        tokio::spawn(fan_out_push(
            db_fanout,
            client_fanout,
            subscribers_fanout,
            fanout_events,
        ));
    } else if !live_sse_frames.is_empty() {
        publish_sse_frames(&state.sse_registry, &live_sse_frames);
    }

    Ok(Json(response))
}

// ── GET /sync/events ──────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct SyncEventsQuery {
    #[serde(default)]
    after_seq: i64,
    limit: Option<i64>,
}

// Mutex safety compliant — 2026-03-12
async fn handle_sync_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<SyncEventsQuery>,
) -> Result<Json<sync::SyncEventsResponse>, ApiError> {
    check_sync_token(&headers, state.sync_token.as_deref())?;

    let after_seq = params.after_seq;
    // Issue-NEGATIVE-LIMIT — 2026-03-15
    let capped_limit = params.limit.unwrap_or(500).clamp(1, 1000);

    let result = spawn_db(state.db.clone(), move |conn| {
        let events = db::get_events_since(conn, after_seq, capped_limit)?;

        let has_more = events.len() == usize::try_from(capped_limit).unwrap_or(usize::MAX);
        let next_seq = events.last().map_or(after_seq, |e| e.seq);

        Ok(sync::SyncEventsResponse {
            events,
            has_more,
            next_seq,
        })
    })
    .await?;

    Ok(Json(result))
}

// ── POST /sync/reconcile ──────────────────────────────────────────────────────

// Issue-RECONCILE-AUTH — 2026-03-16
// Mutex safety compliant — 2026-03-12
async fn handle_sync_reconcile(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<sync::ReconcileRequest>,
) -> Result<Json<sync::ReconcileResponse>, ApiError> {
    // Issue-RECONCILE-AUTH — 2026-03-16: require same auth as /sync/register.
    check_sync_token(&headers, state.sync_token.as_deref())?;

    // Availability: cap the size of the `have` set to prevent memory exhaustion.
    if req.have.len() > MAX_RECONCILE_HAVE {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: format!("have array exceeds maximum size of {MAX_RECONCILE_HAVE}"),
            www_authenticate: None,
        });
    }

    // Finding-5 reconcile pagination — 2026-03-13
    // Issue-RECONCILE-AUTH — 2026-03-16: reconcile is now read-only; the
    // upsert_node_sync_state write was removed because reconcile's contract is
    // set-difference comparison, not cursor bookkeeping.  The cursor is already
    // maintained by apply_event (via SYNC_CURSOR_KEY) and by push-success
    // recording, so the write here was redundant and created an unnecessary
    // writer-lock dependency.
    let result = spawn_db(state.db.clone(), move |conn| {
        let (our_refs, refs_truncated) =
            db::get_event_refs_since(conn, req.since_seq, MAX_RECONCILE_REFS)?;

        let our_ids: HashSet<String> = our_refs.iter().map(|r| r.event_id.clone()).collect();
        let their_ids: HashSet<String> = req.have.iter().map(|r| r.event_id.clone()).collect();

        let missing_ids: HashSet<&String> = our_ids.difference(&their_ids).collect();

        let unknown_to_us: Vec<sync::EventRef> = req
            .have
            .into_iter()
            .filter(|r| !our_ids.contains(&r.event_id))
            .collect();

        let all_events = db::get_events_since(conn, req.since_seq, MAX_RECONCILE_EVENTS)?;
        let events_capped =
            i64::try_from(all_events.len()).unwrap_or(i64::MAX) >= MAX_RECONCILE_EVENTS;
        let send_to_node: Vec<crate::event::Event> = all_events
            .into_iter()
            .filter(|e| missing_ids.contains(&e.event_id))
            .collect();

        let has_more = refs_truncated || events_capped;
        let next_seq = our_refs
            .iter()
            .map(|r| r.seq)
            .max()
            .unwrap_or(req.since_seq);

        Ok(sync::ReconcileResponse {
            send_to_node,
            unknown_to_us,
            has_more,
            next_seq,
        })
    })
    .await?;

    Ok(Json(result))
}

// ── fan_out_push ──────────────────────────────────────────────────────────────

// SP-04 push retry — 2026-03-13
// Mutex safety compliant — 2026-03-12
#[allow(
    clippy::unused_async,
    reason = "must be async because tokio::spawn requires a Future"
)]
async fn fan_out_push(
    db: db_pool::DbPool,
    client: reqwest::Client,
    subscribers: Arc<RwLock<HashMap<String, String>>>,
    events: Vec<event::Event>,
) {
    fan_out_push_inner(db, client, subscribers, events).await;
}

/// Public entry point for integration tests that need to exercise push fan-out
/// with retry logic. Not part of the stable API — test-only.
// SP-04 push retry — 2026-03-13
#[allow(
    clippy::unused_async,
    reason = "async signature for convenience in test await context"
)]
#[expect(
    clippy::implicit_hasher,
    reason = "test-only API; generic hasher adds no value"
)]
pub async fn fan_out_push_public(
    db: db_pool::DbPool,
    client: reqwest::Client,
    subscribers: Arc<RwLock<HashMap<String, String>>>,
    events: Vec<event::Event>,
) {
    fan_out_push_inner(db, client, subscribers, events).await;
}

/// Maximum number of push attempts per peer (initial + retries).
const PUSH_MAX_ATTEMPTS: u64 = 3;

/// Number of consecutive push failures before a peer is evicted from the
/// in-memory subscriber cache. Delegates to the shared constant in `db`.
// SP-04 push retry — 2026-03-13
// Issue-PEER-THRESHOLD — 2026-03-16
const PUSH_EVICTION_THRESHOLD: i64 = db::MAX_PEER_FAILURES;

/// Issue-PUSH-BOUNDS — 2026-03-16: maximum concurrent push tasks to prevent
/// unbounded in-flight push requests when there are many peers.
pub const MAX_CONCURRENT_PUSHES: usize = 16;

// SP-04 push retry — 2026-03-13
// Issue-PUSH-BOUNDS — 2026-03-16: Arc-shared batch, bounded concurrency without
// detached per-peer task buildup, response-body rejection inspection.
#[allow(
    clippy::needless_pass_by_value,
    reason = "values are cloned into spawned tasks; ownership transfer is intentional"
)]
async fn fan_out_push_inner(
    db: db_pool::DbPool,
    client: reqwest::Client,
    subscribers: Arc<RwLock<HashMap<String, String>>>,
    events: Vec<event::Event>,
) {
    let peers: Vec<(String, String)> = {
        let Ok(guard) = subscribers.read() else {
            tracing::error!("fanout: push_subscribers RwLock poisoned; skipping fan-out");
            return;
        };
        guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    };

    // Issue-PUSH-BOUNDS — 2026-03-16: serialize the batch once and share via
    // Arc<String> to eliminate per-peer cloning of the event vector.
    let body = sync::PushRequest { events };
    let Ok(serialized_batch) = serde_json::to_string(&body) else {
        tracing::error!("fanout: failed to serialize push batch; skipping fan-out");
        return;
    };
    let batch = Arc::new(serialized_batch);

    futures_util::stream::StreamExt::for_each_concurrent(
        futures_util::stream::iter(peers),
        MAX_CONCURRENT_PUSHES,
        move |(pubkey, push_url)| {
            let client2 = client.clone();
            let db2 = db.clone();
            let subs2 = Arc::clone(&subscribers);
            let batch2 = Arc::clone(&batch);

            async move {
                // Issue-SYNC-SSRF — 2026-03-16: defense-in-depth re-validation at push time.
                // Skipped under test-util because wiremock binds to 127.0.0.1.
                #[cfg(not(feature = "test-util"))]
                {
                    if let Ok(parsed) = url::Url::parse(&push_url) {
                        if !fetch_guard::is_url_ssrf_safe(&parsed) {
                            tracing::warn!(
                                peer = %pubkey, url = %push_url,
                                "fanout: skipping peer with unsafe push URL (SSRF blocked)"
                            );
                            return;
                        }
                    } else {
                        tracing::warn!(
                            peer = %pubkey, url = %push_url,
                            "fanout: skipping peer with unparseable push URL"
                        );
                        return;
                    }
                }

                let mut success = false;

                // SP-04 push retry — 2026-03-13
                for attempt in 0..PUSH_MAX_ATTEMPTS {
                    if attempt > 0 {
                        tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
                    }
                    match client2
                        .post(&push_url)
                        .header("content-type", "application/json")
                        .body(batch2.as_ref().clone())
                        .timeout(Duration::from_secs(10))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            let had_rejections =
                                check_push_response_rejections(resp, &push_url, &pubkey, &db2, &subs2)
                                    .await;
                            if !had_rejections {
                                success = true;
                            }
                            break;
                        }
                        Ok(resp) => {
                            tracing::warn!(
                                url = %push_url, attempt, status = %resp.status(),
                                "fanout: push returned non-success HTTP status"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                url = %push_url, attempt, error = %e,
                                "fanout: push request failed"
                            );
                        }
                    }
                }

                if success {
                    let now = db::unix_now();
                    match db2.writer().lock() {
                        Ok(conn) => {
                            if let Err(e) = db::record_push_success(&conn, &pubkey, now) {
                                tracing::error!(peer = %pubkey, error = %e, "fanout: failed to record push success");
                            }
                        }
                        Err(_) => {
                            tracing::error!(peer = %pubkey, "fanout: db mutex poisoned; cannot record push success");
                        }
                    }
                } else {
                    handle_push_failure(&db2, &subs2, &pubkey);
                }
            }
        },
    )
    .await;
}

/// Issue-PUSH-BOUNDS — 2026-03-16: inspect a 2xx push response body for
/// rejected events. If the peer reports rejected > 0, increment the failure
/// counter and log a warning so that silent divergence is surfaced.
///
/// Returns `true` if the peer reported rejected events (partial failure).
async fn check_push_response_rejections(
    resp: reqwest::Response,
    push_url: &str,
    pubkey: &str,
    db: &db_pool::DbPool,
    subscribers: &Arc<RwLock<HashMap<String, String>>>,
) -> bool {
    let body_text = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            tracing::debug!(
                url = %push_url, error = %e,
                "fanout: could not read push response body"
            );
            return false;
        }
    };

    let push_resp: sync::PushResponse = if let Ok(r) = serde_json::from_str(&body_text) {
        r
    } else {
        // Peer returned non-JSON 2xx — not necessarily an error, but we
        // cannot inspect rejection counts.
        tracing::debug!(
            url = %push_url,
            "fanout: push response was not valid PushResponse JSON"
        );
        return false;
    };

    if push_resp.rejected > 0 {
        tracing::warn!(
            peer = %pubkey, url = %push_url,
            rejected = push_resp.rejected,
            applied = push_resp.applied,
            "fanout: peer reported rejected events in 2xx response (possible divergence)"
        );
        // Treat non-zero rejections as a partial failure so the eviction
        // machinery can eventually remove a consistently-rejecting peer.
        handle_push_failure(db, subscribers, pubkey);
        return true;
    }

    false
}

// SP-04 push retry — 2026-03-13
// Mutex safety compliant — 2026-03-12
fn handle_push_failure(
    db: &db_pool::DbPool,
    subscribers: &Arc<RwLock<HashMap<String, String>>>,
    pubkey: &str,
) {
    let Ok(conn) = db.writer().lock() else {
        tracing::error!(peer = %pubkey, "fanout: db mutex poisoned; cannot track push failure");
        return;
    };
    if let Err(e) = db::increment_peer_failures(&conn, pubkey) {
        tracing::error!(peer = %pubkey, error = %e, "fanout: failed to increment failures");
        return;
    }

    let failures: i64 = conn
        .query_row(
            "SELECT consecutive_failures FROM peer_nodes WHERE node_pubkey = ?1",
            rusqlite::params![pubkey],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if failures >= PUSH_EVICTION_THRESHOLD {
        match subscribers.write() {
            Ok(mut guard) => {
                guard.remove(pubkey);
                tracing::warn!(
                    peer = %pubkey, threshold = PUSH_EVICTION_THRESHOLD,
                    "fanout: evicted peer from push cache after consecutive failures"
                );
            }
            Err(_) => {
                tracing::error!(peer = %pubkey, "fanout: push_subscribers RwLock poisoned; cannot evict");
            }
        }
    }
}

// ── POST /sync/register ───────────────────────────────────────────────────────

// CS-03 authenticated register — 2026-03-12
// Finding-3 separate sync token — 2026-03-13
// Issue-SYNC-SSRF — 2026-03-16
// Mutex safety compliant — 2026-03-12
async fn handle_sync_register(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<sync::RegisterRequest>,
) -> Result<Json<sync::RegisterResponse>, ApiError> {
    check_sync_token(&headers, state.sync_token.as_deref())?;

    let (Some(signed_at), Some(signature_hex)) = (req.signed_at, req.signature.as_deref()) else {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "sync/register requires signed_at and signature".into(),
            www_authenticate: None,
        });
    };

    let now = db::unix_now();
    if (now - signed_at).abs() > SYNC_REGISTER_MAX_SKEW_SECS {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: format!(
                "sync/register signed_at is outside the allowed skew window of {SYNC_REGISTER_MAX_SKEW_SECS} seconds"
            ),
            www_authenticate: None,
        });
    }

    let payload = sync::RegisterSigningPayload {
        node_pubkey: &req.node_pubkey,
        node_url: &req.node_url,
        signed_at,
    };
    signing::verify_json_signature(&req.node_pubkey, &payload, signature_hex).map_err(|e| {
        ApiError {
            status: StatusCode::FORBIDDEN,
            message: format!("invalid sync/register signature: {e}"),
            www_authenticate: None,
        }
    })?;

    // Issue-SYNC-SSRF — 2026-03-16: validate node_url against SSRF before storing.
    #[cfg(feature = "test-util")]
    let skip_ssrf = state.skip_ssrf_validation;
    #[cfg(not(feature = "test-util"))]
    let skip_ssrf = false;

    if !skip_ssrf {
        let url_for_check = req.node_url.clone();
        tokio::task::spawn_blocking(move || fetch_guard::validate_node_url(&url_for_check))
            .await
            .map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("SSRF validation task failed: {e}"),
                www_authenticate: None,
            })?
            .map_err(|e| ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message: e,
                www_authenticate: None,
            })?;
    }

    verify_sync_register_target(&req.node_url, &req.node_pubkey, skip_ssrf).await?;

    let pubkey = req.node_pubkey.clone();
    let url = req.node_url.clone();

    // Issue-WAL-POOL — 2026-03-14: uses writer (upsert_peer_node writes)
    spawn_db_write(state.db.clone(), move |conn| {
        db::upsert_peer_node(conn, &pubkey, &url, now)?;
        db::reset_peer_failures(conn, &pubkey)?;
        Ok(())
    })
    .await?;

    {
        let mut guard = state.push_subscribers.write().map_err(|_poison| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "push_subscribers lock poisoned".into(),
            www_authenticate: None,
        })?;
        guard.insert(req.node_pubkey.clone(), req.node_url.clone());
    }

    tracing::info!(peer = %req.node_pubkey, url = %req.node_url, "registered push peer");

    Ok(Json(sync::RegisterResponse { ok: true }))
}

// ── GET /sync/peers ───────────────────────────────────────────────────────────

async fn handle_sync_peers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<sync::PeersResponse>, ApiError> {
    check_sync_token(&headers, state.sync_token.as_deref())?;

    // Mutex safety compliant — 2026-03-12
    let result = spawn_db(state.db.clone(), move |conn| {
        let peers = db::get_push_peers(conn)?;
        let nodes = peers
            .into_iter()
            .map(|p| sync::PeerEntry {
                node_pubkey: p.node_pubkey,
                node_url: p.node_url,
                last_push_at: p.last_push_at,
            })
            .collect();
        Ok(sync::PeersResponse { nodes })
    })
    .await?;

    Ok(Json(result))
}

// ── GET /node/info ────────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize, ToSchema)]
struct NodeInfoResponse {
    node_pubkey: String,
}

async fn handle_node_info(State(state): State<Arc<AppState>>) -> Json<NodeInfoResponse> {
    Json(NodeInfoResponse {
        node_pubkey: state.node_pubkey_hex.clone(),
    })
}

fn sync_register_node_info_url(node_url: &str) -> Result<String, String> {
    let mut url = url::Url::parse(node_url).map_err(|e| format!("invalid node URL: {e}"))?;
    let path = url.path().trim_end_matches('/');
    let Some(base_path) = path.strip_suffix("/sync/push") else {
        return Err("node URL must end with /sync/push".into());
    };

    let node_info_path = if base_path.is_empty() {
        "/node/info".to_string()
    } else {
        format!("{base_path}/node/info")
    };

    url.set_path(&node_info_path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

async fn verify_sync_register_target(
    node_url: &str,
    expected_pubkey: &str,
    skip_ssrf_validation: bool,
) -> Result<(), ApiError> {
    let node_info_url = sync_register_node_info_url(node_url).map_err(|e| ApiError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        message: e,
        www_authenticate: None,
    })?;

    let mut client_builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5));

    if !skip_ssrf_validation {
        // Issue-SYNC-SSRF-REBIND — 2026-03-25: resolve and validate the exact
        // node/info URL inside spawn_blocking, then pin those addresses into
        // the verification client so the ownership check cannot be redirected
        // to a different host via DNS rebinding between validation and fetch.
        let node_info_url_for_resolve = node_info_url.clone();
        let (hostname, resolved_addrs) = tokio::task::spawn_blocking(move || {
            let parsed = url::Url::parse(&node_info_url_for_resolve)
                .map_err(|e| format!("invalid node/info URL: {e}"))?;
            fetch_guard::resolve_and_validate_url(&parsed)
        })
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("node/info SSRF validation task failed: {e}"),
            www_authenticate: None,
        })?
        .map_err(|e| ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: format!("node/info URL rejected: {e}"),
            www_authenticate: None,
        })?;

        for addr in resolved_addrs {
            client_builder = client_builder.resolve(&hostname, addr);
        }
    }

    let client = client_builder
        .build()
        .expect("sync/register node-info client uses only safe options");

    let resp = client
        .get(&node_info_url)
        .send()
        .await
        .map_err(|e| ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: format!("failed to verify node URL ownership via {node_info_url}: {e}"),
            www_authenticate: None,
        })?;

    if !resp.status().is_success() {
        return Err(ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: format!(
                "node URL ownership check returned HTTP {} from {node_info_url}",
                resp.status()
            ),
            www_authenticate: None,
        });
    }

    let info: NodeInfoResponse = resp.json().await.map_err(|e| ApiError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        message: format!("invalid node/info response from {node_info_url}: {e}"),
        www_authenticate: None,
    })?;

    if info.node_pubkey != expected_pubkey {
        return Err(ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: format!(
                "node/info pubkey mismatch for {node_info_url}: expected {expected_pubkey}, got {}",
                info.node_pubkey
            ),
            www_authenticate: None,
        });
    }

    Ok(())
}

// ── Admin auth helper ─────────────────────────────────────────────────────────

// CS-02 constant-time — 2026-03-12
fn check_admin_token(headers: &HeaderMap, expected: &str) -> Result<(), ApiError> {
    if expected.is_empty() {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            message: "admin token not configured on this node".into(),
            www_authenticate: None,
        });
    }
    let provided = headers
        .get("X-Admin-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let h1 = Sha256::digest(provided.as_bytes());
    let h2 = Sha256::digest(expected.as_bytes());
    if bool::from(h1.ct_eq(&h2)) {
        Ok(())
    } else {
        Err(ApiError {
            status: StatusCode::FORBIDDEN,
            message: "invalid or missing X-Admin-Token".into(),
            www_authenticate: None,
        })
    }
}

// ── Sync endpoint auth helper ────────────────────────────────────────────────

// Finding-3 separate sync token — 2026-03-13
// CS-02 constant-time — 2026-03-12
/// Checks authentication for sync endpoints.
///
/// Sync replication endpoints require a dedicated `SYNC_TOKEN` and never
/// accept `X-Admin-Token`.
fn check_sync_token(headers: &HeaderMap, sync_token: Option<&str>) -> Result<(), ApiError> {
    let Some(expected_sync) = sync_token else {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            message: "sync token not configured on this node".into(),
            www_authenticate: None,
        });
    };

    if expected_sync.is_empty() {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            message: "sync token not configured on this node".into(),
            www_authenticate: None,
        });
    }

    let provided = headers
        .get("X-Sync-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let h1 = Sha256::digest(provided.as_bytes());
    let h2 = Sha256::digest(expected_sync.as_bytes());
    if bool::from(h1.ct_eq(&h2)) {
        return Ok(());
    }

    Err(ApiError {
        status: StatusCode::FORBIDDEN,
        message: "invalid or missing X-Sync-Token".into(),
        www_authenticate: None,
    })
}

// ── DELETE /feeds/{guid} ───────────────────────────────────────────────────

/// Query parameters of `DELETE /v1/feeds/{guid}`.
///
/// ADR 0053 Section 1. `block` defaults to `true`: the retirement also blocks
/// the feed GUID and its stored URL. `block=false` needs the admin token.
#[derive(Debug, Deserialize)]
struct RetireParams {
    block: Option<bool>,
}

#[expect(
    clippy::too_many_lines,
    reason = "event signing, SSE publish, and fan-out all live in one handler"
)]
async fn handle_retire_feed(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(guid): Path<String>,
    Query(params): Query<RetireParams>,
) -> Result<StatusCode, ApiError> {
    let block = params.block.unwrap_or(true);
    let state2 = Arc::clone(&state);
    let guid2 = guid.clone();
    // Mutex safety compliant — 2026-03-12
    let result =
        tokio::task::spawn_blocking(move || -> Result<Option<Vec<event::Event>>, ApiError> {
            let mut conn = state2.db.writer().lock().map_err(|_poison| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "database mutex poisoned".into(),
                www_authenticate: None,
            })?;

            // Auth inside lock scope: keeps auth and DB write under one lock.
            check_admin_token(&headers, &state2.admin_token)?;

            // ADR 0053 Section 1: block=false retires with no block, and only
            // the admin token may ask for that. A publisher's bearer token
            // passed the check above but does not pass this one.
            if !block && !headers.contains_key("X-Admin-Token") {
                return Err(ApiError {
                    status: StatusCode::FORBIDDEN,
                    message: "ADR 0053 Section 1: block=false needs the X-Admin-Token header"
                        .into(),
                    www_authenticate: None,
                });
            }

            // Look up the feed — 404 if not found.
            let feed = db::get_feed_by_guid(&conn, &guid2)?.ok_or_else(|| ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("feed {guid2} not found"),
                www_authenticate: None,
            })?;

            // Fetch tracks to remove from search index.
            let tracks = db::get_tracks_for_feed(&conn, &guid2)?;

            // Remove search index entries (best-effort).
            for track in &tracks {
                let _ = crate::search::delete_from_search_index(
                    &conn,
                    "track",
                    &db::canonical_track_entity_id(&track.feed_guid, &track.track_guid),
                    "",
                    &track.title,
                    track.description.as_deref().unwrap_or(""),
                    "",
                );
            }
            let _ = crate::search::delete_from_search_index(
                &conn,
                "feed",
                &feed.feed_guid,
                "",
                &feed.title,
                feed.description.as_deref().unwrap_or(""),
                feed.raw_medium.as_deref().unwrap_or(""),
            );

            // Build and sign a FeedRetired event.
            let now = db::unix_now();

            let event_id = uuid::Uuid::new_v4().to_string();
            let payload = event::FeedRetiredPayload {
                feed_guid: guid2.clone(),
                reason: Some("admin retired via API".to_string()),
            };
            let payload_json = serde_json::to_string(&payload).map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("failed to serialize FeedRetired payload: {e}"),
                www_authenticate: None,
            })?;

            // ADR 0053 Section 1: a retirement blocks the GUID and the stored
            // URL in the same transaction, unless the caller asked otherwise.
            let blocks = if block {
                vec![
                    db::FeedBlock {
                        block_id: uuid::Uuid::new_v4().to_string(),
                        kind: db::FeedBlockKind::Guid,
                        value: feed.feed_guid.clone(),
                        reason: "retired".to_string(),
                        blocked_at: now,
                    },
                    db::FeedBlock {
                        block_id: uuid::Uuid::new_v4().to_string(),
                        kind: db::FeedBlockKind::Url,
                        value: feed.feed_url.clone(),
                        reason: "retired".to_string(),
                        blocked_at: now,
                    },
                ]
            } else {
                Vec::new()
            };

            // Issue-SEQ-INTEGRITY — 2026-03-14: signer passed to delete_feed_with_event
            // which signs after the DB assigns seq. It returns the FeedRetired
            // event and one FeedBlocked event per new block, ready to fan out.
            let events = db::delete_feed_with_event(
                &mut conn,
                &guid2,
                &event_id,
                &payload_json,
                &guid2,
                &state2.signer,
                now,
                &[],
                &blocks,
            )
            .map_err(ApiError::from)?;

            Ok(Some(events))
        })
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("internal task panic: {e}"),
            www_authenticate: None,
        })?;

    let fanout_events = result?;

    // Fire-and-forget fan-out.
    if let Some(events) = fanout_events
        && !events.is_empty()
    {
        let db_fanout = state.db.clone();
        let client_fanout = state.push_client.clone();
        let subscribers_fanout = Arc::clone(&state.push_subscribers);
        tokio::spawn(fan_out_push(
            db_fanout,
            client_fanout,
            subscribers_fanout,
            events,
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── DELETE /feeds/{guid}/tracks/{track_guid} ────────────────────────────────

async fn handle_remove_track(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((guid, track_guid)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let state2 = Arc::clone(&state);
    let guid2 = guid.clone();
    let track_guid2 = track_guid.clone();
    // Mutex safety compliant — 2026-03-12
    let result =
        tokio::task::spawn_blocking(move || -> Result<Option<Vec<event::Event>>, ApiError> {
            let mut conn = state2.db.writer().lock().map_err(|_poison| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "database mutex poisoned".into(),
                www_authenticate: None,
            })?;

            // Auth inside lock scope: keeps auth and DB write under one lock.
            check_admin_token(&headers, &state2.admin_token)?;

            // Look up the track — 404 if not found.
            let track =
                db::get_track_for_feed(&conn, &guid2, &track_guid2)?.ok_or_else(|| ApiError {
                    status: StatusCode::NOT_FOUND,
                    message: format!("track {track_guid2} not found"),
                    www_authenticate: None,
                })?;

            // Remove search index entry (best-effort).
            let _ = crate::search::delete_from_search_index(
                &conn,
                "track",
                &db::canonical_track_entity_id(&track.feed_guid, &track.track_guid),
                "",
                &track.title,
                track.description.as_deref().unwrap_or(""),
                "",
            );

            // Build and sign a TrackRemoved event.
            let now = db::unix_now();

            let event_id = uuid::Uuid::new_v4().to_string();
            let payload = event::TrackRemovedPayload {
                track_guid: track_guid2.clone(),
                feed_guid: guid2.clone(),
            };
            let payload_json = serde_json::to_string(&payload).map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("failed to serialize TrackRemoved payload: {e}"),
                www_authenticate: None,
            })?;
            // Issue-SEQ-INTEGRITY — 2026-03-14: signer passed to delete_track_with_event
            // which signs after the DB assigns seq.
            let (seq, signed_by, signature) = db::delete_track_with_event(
                &mut conn,
                &guid2,
                &track_guid2,
                &event_id,
                &payload_json,
                &track_guid2,
                &state2.signer,
                now,
                &[],
            )
            .map_err(ApiError::from)?;

            // Build event for fan-out.
            let tagged = format!(r#"{{"type":"track_removed","data":{payload_json}}}"#);
            let ev_payload =
                serde_json::from_str::<event::EventPayload>(&tagged).map_err(|e| ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!("failed to deserialize TrackRemoved event for fan-out: {e}"),
                    www_authenticate: None,
                })?;

            let fanout_event = event::Event {
                event_id,
                event_type: event::EventType::TrackRemoved,
                payload: ev_payload,
                subject_guid: track_guid2,
                signed_by,
                signature,
                seq,
                created_at: now,
                warnings: vec![],
                payload_json,
            };

            Ok(Some(vec![fanout_event]))
        })
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("internal task panic: {e}"),
            www_authenticate: None,
        })?;

    let fanout_events = result?;

    // Fire-and-forget fan-out.
    if let Some(events) = fanout_events
        && !events.is_empty()
    {
        let db_fanout = state.db.clone();
        let client_fanout = state.push_client.clone();
        let subscribers_fanout = Arc::clone(&state.push_subscribers);
        tokio::spawn(fan_out_push(
            db_fanout,
            client_fanout,
            subscribers_fanout,
            events,
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── POST /v1/blocks, GET /v1/blocks, DELETE /v1/blocks/{block_id} ──────────
//
// ADR 0053 section 1: an operator creates, lists and deletes a durable,
// signed block on a feed GUID or an exact feed URL. Every route needs
// `X-Admin-Token`. There is no bearer alternative — a publisher does not
// manage blocks on its own feed, only the operator does.

/// Request body for `POST /v1/blocks`.
#[derive(Deserialize)]
struct CreateBlockRequest {
    kind: db::FeedBlockKind,
    value: String,
    reason: String,
}

/// Response body for a `409 Conflict` from `POST /v1/blocks`: the
/// `block_id` of the row that already blocks this pair.
#[derive(Serialize)]
struct BlockConflictBody {
    block_id: String,
}

/// Response body for `GET /v1/blocks`.
#[derive(Serialize)]
struct ListBlocksResponse {
    blocks: Vec<db::FeedBlock>,
}

/// Outcome of the write phase of `handle_create_block`.
enum CreateBlockOutcome {
    /// The kind/value pair already had a row; carries its `block_id`.
    Conflict(String),
    /// A new row was inserted and its `FeedBlocked` event signed.
    Created(db::FeedBlock, Box<event::Event>),
}

async fn handle_create_block(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateBlockRequest>,
) -> Result<Response, ApiError> {
    check_admin_token(&headers, &state.admin_token)?;

    let kind = req.kind;
    // The signed event carries the value that the row stores.
    let value = kind.normalize(&req.value);
    let reason = req.reason.trim().to_string();
    if value.is_empty() || reason.is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "value and reason must not be empty".into(),
            www_authenticate: None,
        });
    }

    let state2 = Arc::clone(&state);
    let outcome = spawn_db_mut(state.db.clone(), move |conn| {
        // Issue-CHECKED-TX — 2026-03-16: conn is freshly acquired from the
        // writer lock, no nesting.
        let tx = conn.transaction()?;

        if let Some(existing) = db::get_feed_block_by_pair(&tx, kind, &value)? {
            return Ok(CreateBlockOutcome::Conflict(existing.block_id));
        }

        let now = db::unix_now();
        let block = db::FeedBlock {
            block_id: uuid::Uuid::new_v4().to_string(),
            kind,
            value,
            reason,
            blocked_at: now,
        };
        db::insert_feed_block(&tx, &block)?;

        let event_id = uuid::Uuid::new_v4().to_string();
        let payload = event::FeedBlockedPayload {
            block_id: block.block_id.clone(),
            kind: block.kind,
            value: block.value.clone(),
            reason: block.reason.clone(),
            blocked_at: block.blocked_at,
        };
        let payload_json = serde_json::to_string(&payload)?;
        // Issue-SEQ-INTEGRITY — 2026-03-14: sign after insert to include seq.
        let (seq, signed_by, signature) = db::insert_event(
            &tx,
            &event_id,
            &event::EventType::FeedBlocked,
            &payload_json,
            &block.block_id,
            &state2.signer,
            now,
            &[],
        )?;

        tx.commit()?;

        // Build event for fan-out AFTER commit.
        let tagged = format!(r#"{{"type":"feed_blocked","data":{payload_json}}}"#);
        let ev_payload = serde_json::from_str::<event::EventPayload>(&tagged)?;
        let fanout_event = event::Event {
            event_id,
            event_type: event::EventType::FeedBlocked,
            payload: ev_payload,
            subject_guid: block.block_id.clone(),
            signed_by,
            signature,
            seq,
            created_at: now,
            warnings: vec![],
            payload_json,
        };

        Ok(CreateBlockOutcome::Created(block, Box::new(fanout_event)))
    })
    .await?;

    match outcome {
        CreateBlockOutcome::Conflict(block_id) => {
            Ok((StatusCode::CONFLICT, Json(BlockConflictBody { block_id })).into_response())
        }
        CreateBlockOutcome::Created(block, fanout_event) => {
            // Fire-and-forget fan-out, as handle_remove_track does.
            let db_fanout = state.db.clone();
            let client_fanout = state.push_client.clone();
            let subscribers_fanout = Arc::clone(&state.push_subscribers);
            tokio::spawn(fan_out_push(
                db_fanout,
                client_fanout,
                subscribers_fanout,
                vec![*fanout_event],
            ));
            Ok((StatusCode::CREATED, Json(block)).into_response())
        }
    }
}

async fn handle_list_blocks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ListBlocksResponse>, ApiError> {
    check_admin_token(&headers, &state.admin_token)?;

    let mut blocks = spawn_db(state.db.clone(), db::list_feed_blocks).await?;
    blocks.sort_by_key(|b| b.blocked_at);

    Ok(Json(ListBlocksResponse { blocks }))
}

async fn handle_delete_block(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(block_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    check_admin_token(&headers, &state.admin_token)?;

    let state2 = Arc::clone(&state);
    let block_id2 = block_id.clone();
    let fanout_event = spawn_db_mut(state.db.clone(), move |conn| {
        let tx = conn.transaction()?;

        if !db::delete_feed_block(&tx, &block_id2)? {
            // No row to remove: drop the transaction (a no-op rollback) and
            // sign nothing.
            return Ok(None);
        }

        let now = db::unix_now();
        let event_id = uuid::Uuid::new_v4().to_string();
        let payload = event::FeedUnblockedPayload {
            block_id: block_id2.clone(),
        };
        let payload_json = serde_json::to_string(&payload)?;
        let (seq, signed_by, signature) = db::insert_event(
            &tx,
            &event_id,
            &event::EventType::FeedUnblocked,
            &payload_json,
            &block_id2,
            &state2.signer,
            now,
            &[],
        )?;

        tx.commit()?;

        let tagged = format!(r#"{{"type":"feed_unblocked","data":{payload_json}}}"#);
        let ev_payload = serde_json::from_str::<event::EventPayload>(&tagged)?;
        let fanout_event = event::Event {
            event_id,
            event_type: event::EventType::FeedUnblocked,
            payload: ev_payload,
            subject_guid: block_id2,
            signed_by,
            signature,
            seq,
            created_at: now,
            warnings: vec![],
            payload_json,
        };

        Ok(Some(fanout_event))
    })
    .await?;

    let Some(fanout_event) = fanout_event else {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("block {block_id} not found"),
            www_authenticate: None,
        });
    };

    // Fire-and-forget fan-out, as handle_remove_track does.
    let db_fanout = state.db.clone();
    let client_fanout = state.push_client.clone();
    let subscribers_fanout = Arc::clone(&state.push_subscribers);
    tokio::spawn(fan_out_push(
        db_fanout,
        client_fanout,
        subscribers_fanout,
        vec![fanout_event],
    ));

    Ok(StatusCode::NO_CONTENT)
}

// ── PATCH /feeds/{guid} ────────────────────────────────────────────────────
// REST semantics compliant (RFC 7396) — 2026-03-12
// Issue-12 PATCH emits events — 2026-03-13
// Issue-13 PATCH 404 check — 2026-03-13

#[derive(Deserialize)]
struct PatchFeedRequest {
    feed_url: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

/// The result of [`relocate_feed`].
enum RelocateOutcome {
    /// Another record already holds `new_url` as its own stored source URL
    /// (ADR 0052's relocation rule). The caller's transaction commits
    /// nothing.
    Conflict,
    /// The relocation applied. Carries the signed `FeedUpserted` event.
    Relocated(SignedEventRow),
}

/// Relocates `feed_guid` to `new_url` inside the caller's transaction: sets
/// `feed_url`, clears `last_build_date`, `declared_self_url` and
/// `declared_new_feed_url`, and signs one `FeedUpserted` event carrying
/// `reason`. `PATCH /v1/feeds/{guid}` and `POST /v1/feeds/{guid}/copies/resolve`
/// both call this function so a relocation always clears the same fields
/// (ADR 0058 sections 4 and 5; ADR 0052 §2 task 006).
///
/// `declared_self_url` and `declared_new_feed_url` are local to the primary:
/// neither is a field of the `Feed` model, so the `FeedUpserted` event cannot
/// carry them, and a replica never clears its own copy (ADR 0052 section 2
/// owns both columns).
///
/// Returns [`RelocateOutcome::Conflict`] and writes nothing when another
/// record already holds `new_url` as its stored source URL.
///
/// # Errors
///
/// Returns [`db::DbError`] if a query, the update, or event serialization
/// fails.
fn relocate_feed(
    tx: &rusqlite::Transaction,
    feed_guid: &str,
    new_url: &str,
    reason: &str,
    signer: &signing::NodeSigner,
    now: i64,
) -> Result<RelocateOutcome, db::DbError> {
    let conflict: Option<String> = tx
        .query_row(
            "SELECT feed_guid FROM feeds WHERE feed_url = ?1 AND feed_guid <> ?2",
            params![new_url, feed_guid],
            |row| row.get(0),
        )
        .optional()?;
    if conflict.is_some() {
        return Ok(RelocateOutcome::Conflict);
    }

    tx.execute(
        "UPDATE feeds SET feed_url = ?1, last_build_date = NULL WHERE feed_guid = ?2",
        params![new_url, feed_guid],
    )?;
    // ADR 0058 section 5: cleared in the same transaction as feed_url, so
    // the next body from new_url applies with no stale rule and records its
    // own self link.
    db::set_declared_self_url(tx, feed_guid, None)?;
    // ADR 0052 section 2, task 006: a stale new-feed-url declaration must
    // not point back at a URL the record just left.
    db::clear_declared_new_feed_url(tx, feed_guid)?;

    let feed = db::get_feed_by_guid(tx, feed_guid)?
        .ok_or_else(|| db::DbError::Other(format!("feed {feed_guid} vanished after relocation")))?;

    let event_id = uuid::Uuid::new_v4().to_string();
    let payload = event::FeedUpsertedPayload {
        feed,
        reason: Some(reason.to_string()),
    };
    let payload_json = serde_json::to_string(&payload)?;
    // Issue-SEQ-INTEGRITY — 2026-03-14: sign after insert to include seq.
    let (seq, signed_by, signature) = db::insert_event(
        tx,
        &event_id,
        &event::EventType::FeedUpserted,
        &payload_json,
        feed_guid,
        signer,
        now,
        &[],
    )?;

    Ok(RelocateOutcome::Relocated(SignedEventRow {
        row: db::EventRow {
            event_id,
            event_type: event::EventType::FeedUpserted,
            payload_json,
            subject_guid: feed_guid.to_string(),
            created_at: now,
            warnings: vec![],
        },
        seq,
        signed_by,
        signature,
    }))
}

async fn handle_patch_feed(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(guid): Path<String>,
    Json(req): Json<PatchFeedRequest>,
) -> Result<StatusCode, ApiError> {
    let state2 = Arc::clone(&state);
    let guid2 = guid.clone();
    // Mutex safety compliant — 2026-03-12
    // Finding-2 atomic mutation+event — 2026-03-13
    let result =
        tokio::task::spawn_blocking(move || -> Result<Option<Vec<event::Event>>, ApiError> {
            let mut conn = state2.db.writer().lock().map_err(|_poison| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "database mutex poisoned".into(),
                www_authenticate: None,
            })?;

            // Auth inside lock scope: keeps auth and DB write under one lock.
            check_admin_token(&headers, &state2.admin_token)?;

            // Issue-13 PATCH 404 check — 2026-03-13
            // Look up the feed — 404 if not found.
            db::get_feed_by_guid(&conn, &guid2)
                .map_err(ApiError::from)?
                .ok_or_else(|| ApiError {
                    status: StatusCode::NOT_FOUND,
                    message: format!("feed {guid2} not found"),
                    www_authenticate: None,
                })?;

            let Some(new_url) = &req.feed_url else {
                return Ok(None);
            };

            // ADR 0058 section 5: a relocation by feed_url needs a reason.
            let reason = req.reason.as_deref().unwrap_or("").trim();
            if reason.is_empty() {
                return Err(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    message: "reason must not be empty when patching feed_url".into(),
                    www_authenticate: None,
                });
            }

            // Wrap mutation + event insert in a single transaction.
            // Issue-CHECKED-TX — 2026-03-16: conn is freshly acquired from writer lock, no nesting.
            let tx = conn
                .transaction()
                .map_err(|e| ApiError::from(db::DbError::from(e)))?;

            let now = db::unix_now();
            let signed = match relocate_feed(&tx, &guid2, new_url, reason, &state2.signer, now)
                .map_err(ApiError::from)?
            {
                RelocateOutcome::Conflict => {
                    return Err(ApiError {
                        status: StatusCode::CONFLICT,
                        message: format!("{new_url} is already the source URL of another record"),
                        www_authenticate: None,
                    });
                }
                RelocateOutcome::Relocated(signed) => signed,
            };

            tx.commit()
                .map_err(|e| ApiError::from(db::DbError::from(e)))?;

            Ok(Some(vec![signed_row_to_event(signed)?]))
        })
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("internal task panic: {e}"),
            www_authenticate: None,
        })?;

    let fanout_events = result?;

    // Fire-and-forget fan-out.
    if let Some(events) = fanout_events
        && !events.is_empty()
    {
        // Issue-SSE-PUBLISH — 2026-03-14
        publish_events_to_sse(&state.sse_registry, &events);

        let db_fanout = state.db.clone();
        let client_fanout = state.push_client.clone();
        let subscribers_fanout = Arc::clone(&state.push_subscribers);
        tokio::spawn(fan_out_push(
            db_fanout,
            client_fanout,
            subscribers_fanout,
            events,
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── POST /v1/feeds/{guid}/copies/resolve ────────────────────────────────────
// ADR 0058 sections 4 and 5: the operator keeps the source or relocates the
// record. Admin token only, following the handle_create_block pattern of one
// signed event and a post-commit fan-out.

/// Request body for `POST /v1/feeds/{guid}/copies/resolve`.
#[derive(Deserialize)]
struct ResolveCopyRequest {
    url: String,
    decision: String,
    reason: String,
}

/// Response body for a resolved copy: the IDs of every event the resolution
/// signed, in the order they were signed. `relocate` signs a `FeedUpserted`
/// and a `FeedCopyResolved`; `keep_source` signs only the latter.
#[derive(Serialize)]
struct ResolveCopyResponse {
    event_ids: Vec<String>,
}

/// The result of the resolve transaction, decided before any fan-out.
enum ResolveCopyOutcome {
    /// `feed_guid` names no record, or the record has no `feed_copies` row
    /// at the given `url`.
    NotFound,
    /// `decision` was `relocate`, and another record already holds `url` as
    /// its own stored source URL. Nothing was written.
    Conflict,
    /// The resolution applied. Carries every signed event for fan-out.
    Resolved {
        event_ids: Vec<String>,
        events: Vec<SignedEventRow>,
    },
}

#[expect(
    clippy::too_many_lines,
    reason = "one transaction covers both the optional relocation and the FeedCopyResolved event, following the handle_create_block pattern"
)]
async fn handle_resolve_copy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(guid): Path<String>,
    Json(req): Json<ResolveCopyRequest>,
) -> Result<Response, ApiError> {
    check_admin_token(&headers, &state.admin_token)?;

    let reason = req.reason.trim().to_string();
    if reason.is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "reason must not be empty".into(),
            www_authenticate: None,
        });
    }
    if req.decision != "keep_source" && req.decision != "relocate" {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: format!(
                "decision must be \"keep_source\" or \"relocate\", got {:?}",
                req.decision
            ),
            www_authenticate: None,
        });
    }

    let decision = req.decision.clone();
    let url = req.url.clone();
    let guid2 = guid.clone();
    let state2 = Arc::clone(&state);
    let outcome = spawn_db_mut(state.db.clone(), move |conn| {
        let tx = conn.transaction()?;

        if db::get_feed_by_guid(&tx, &guid2)?.is_none() {
            return Ok(ResolveCopyOutcome::NotFound);
        }
        let Some(row) = db::get_feed_copy(&tx, &guid2, &url)? else {
            return Ok(ResolveCopyOutcome::NotFound);
        };

        let now = db::unix_now();
        let mut event_ids = Vec::new();
        let mut events = Vec::new();

        // ADR 0058 section 4: `relocate` clears the section 5 fields in the
        // same transaction as the resolution, using the row's own URL.
        if decision == "relocate" {
            match relocate_feed(&tx, &guid2, &row.url, &reason, &state2.signer, now)? {
                RelocateOutcome::Conflict => return Ok(ResolveCopyOutcome::Conflict),
                RelocateOutcome::Relocated(signed) => {
                    event_ids.push(signed.row.event_id.clone());
                    events.push(signed);
                }
            }
        }

        // The resolution names the row's own summary_digest — not a
        // recomputed one — so it holds until that summary next changes.
        let event_id = uuid::Uuid::new_v4().to_string();
        let payload = event::FeedCopyResolvedPayload {
            feed_guid: guid2.clone(),
            url: row.url.clone(),
            decision: decision.clone(),
            reason: reason.clone(),
            resolved_at: now,
            resolved_digest: row.summary_digest.clone(),
        };
        let payload_json = serde_json::to_string(&payload)?;
        let (seq, signed_by, signature) = db::insert_event(
            &tx,
            &event_id,
            &event::EventType::FeedCopyResolved,
            &payload_json,
            &guid2,
            &state2.signer,
            now,
            &[],
        )?;
        db::set_feed_copy_resolution(
            &tx,
            &guid2,
            &row.url,
            &decision,
            &reason,
            now,
            &row.summary_digest,
        )?;

        tx.commit()?;

        event_ids.push(event_id.clone());
        events.push(SignedEventRow {
            row: db::EventRow {
                event_id,
                event_type: event::EventType::FeedCopyResolved,
                payload_json,
                subject_guid: guid2.clone(),
                created_at: now,
                warnings: vec![],
            },
            seq,
            signed_by,
            signature,
        });

        Ok(ResolveCopyOutcome::Resolved { event_ids, events })
    })
    .await?;

    match outcome {
        ResolveCopyOutcome::NotFound => Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("feed {guid} not found, or it has no copy at that url"),
            www_authenticate: None,
        }),
        ResolveCopyOutcome::Conflict => Err(ApiError {
            status: StatusCode::CONFLICT,
            message: "the copy's url is already the source URL of another record".into(),
            www_authenticate: None,
        }),
        ResolveCopyOutcome::Resolved { event_ids, events } => {
            let fanout_events = events
                .into_iter()
                .map(signed_row_to_event)
                .collect::<Result<Vec<_>, _>>()?;

            // Fire-and-forget fan-out, as handle_create_block does.
            publish_events_to_sse(&state.sse_registry, &fanout_events);
            let db_fanout = state.db.clone();
            let client_fanout = state.push_client.clone();
            let subscribers_fanout = Arc::clone(&state.push_subscribers);
            tokio::spawn(fan_out_push(
                db_fanout,
                client_fanout,
                subscribers_fanout,
                fanout_events,
            ));

            Ok((StatusCode::OK, Json(ResolveCopyResponse { event_ids })).into_response())
        }
    }
}

// ── POST /v1/feeds/{guid}/guid-change ───────────────────────────────────────
// ADR 0052 section 4, task 007: the operator decides a pending GUID change.
// `{guid}` in the path is the old GUID the pending row names, following the
// admin-route-with-events-and-fan-out pattern of handle_resolve_copy.

/// Request body for `POST /v1/feeds/{guid}/guid-change`.
#[derive(Deserialize)]
struct GuidChangeDecisionRequest {
    decision: String,
    reason: String,
}

/// Response body for a decided GUID change.
#[derive(Serialize)]
struct GuidChangeDecisionResponse {
    event_id: String,
}

/// The result of the decision transaction, decided before any fan-out.
enum GuidChangeDecisionOutcome {
    /// No pending row names `{guid}` as its old GUID.
    NotFound,
    /// The decision applied. Carries the signed `FeedGuidChangeDecided`
    /// event for fan-out.
    Decided(SignedEventRow),
}

async fn handle_guid_change_decision(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(guid): Path<String>,
    Json(req): Json<GuidChangeDecisionRequest>,
) -> Result<Response, ApiError> {
    check_admin_token(&headers, &state.admin_token)?;

    let reason = req.reason.trim().to_string();
    if reason.is_empty() {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "reason must not be empty".into(),
            www_authenticate: None,
        });
    }
    if req.decision != "approve" && req.decision != "reject" {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: format!(
                "decision must be \"approve\" or \"reject\", got {:?}",
                req.decision
            ),
            www_authenticate: None,
        });
    }

    let decision = req.decision.clone();
    let guid2 = guid.clone();
    let state2 = Arc::clone(&state);
    let outcome = spawn_db_mut(state.db.clone(), move |conn| {
        let tx = conn.transaction()?;

        let Some(row) = db::get_guid_change_for_old_guid(&tx, &guid2)? else {
            return Ok(GuidChangeDecisionOutcome::NotFound);
        };

        let now = db::unix_now();
        // The node does not keep the body of the submission that opened the
        // row: `approve` only sets the decision, and the transition runs at
        // the next submission of `new_guid` from `source_url` (ADR 0052
        // section 4).
        db::set_guid_change_decision(&tx, &row.source_url, &decision, &reason, now)?;

        let event_id = uuid::Uuid::new_v4().to_string();
        let payload = event::FeedGuidChangeDecidedPayload {
            source_url: row.source_url.clone(),
            old_guid: row.old_guid.clone(),
            new_guid: row.new_guid.clone(),
            decision: decision.clone(),
            reason: reason.clone(),
            decided_at: now,
        };
        let payload_json = serde_json::to_string(&payload)?;
        let (seq, signed_by, signature) = db::insert_event(
            &tx,
            &event_id,
            &event::EventType::FeedGuidChangeDecided,
            &payload_json,
            &row.old_guid,
            &state2.signer,
            now,
            &[],
        )?;

        tx.commit()?;

        Ok(GuidChangeDecisionOutcome::Decided(SignedEventRow {
            row: db::EventRow {
                event_id,
                event_type: event::EventType::FeedGuidChangeDecided,
                payload_json,
                subject_guid: row.old_guid,
                created_at: now,
                warnings: vec![],
            },
            seq,
            signed_by,
            signature,
        }))
    })
    .await?;

    match outcome {
        GuidChangeDecisionOutcome::NotFound => Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("no pending GUID change names {guid} as its old GUID"),
            www_authenticate: None,
        }),
        GuidChangeDecisionOutcome::Decided(signed) => {
            let event_id = signed.row.event_id.clone();
            let fanout_events = vec![signed_row_to_event(signed)?];

            publish_events_to_sse(&state.sse_registry, &fanout_events);
            let db_fanout = state.db.clone();
            let client_fanout = state.push_client.clone();
            let subscribers_fanout = Arc::clone(&state.push_subscribers);
            tokio::spawn(fan_out_push(
                db_fanout,
                client_fanout,
                subscribers_fanout,
                fanout_events,
            ));

            Ok((
                StatusCode::OK,
                Json(GuidChangeDecisionResponse { event_id }),
            )
                .into_response())
        }
    }
}

// ── PATCH /tracks/{guid} ───────────────────────────────────────────────────
// REST semantics compliant (RFC 7396) — 2026-03-12
// Issue-12 PATCH emits events — 2026-03-13
// Issue-13 PATCH 404 check — 2026-03-13

#[derive(Deserialize)]
struct PatchTrackRequest {
    enclosure_url: Option<String>,
}

enum PatchTrackOutcome {
    NoContent,
    Updated(Vec<event::Event>),
    Conflict(Vec<AmbiguousTrackGuidCandidate>),
}

fn ambiguous_track_candidates_for_tracks(
    tracks: &[model::Track],
) -> Vec<AmbiguousTrackGuidCandidate> {
    tracks
        .iter()
        .map(|track| AmbiguousTrackGuidCandidate {
            feed_guid: track.feed_guid.clone(),
            href: canonical_track_href(&track.feed_guid, &track.track_guid),
        })
        .collect()
}

fn patch_resolved_track(
    conn: &mut rusqlite::Connection,
    headers: &HeaderMap,
    state: &AppState,
    track: &model::Track,
    track_guid: &str,
    req: &PatchTrackRequest,
) -> Result<PatchTrackOutcome, ApiError> {
    check_admin_token(headers, &state.admin_token)?;

    let Some(new_url) = &req.enclosure_url else {
        return Ok(PatchTrackOutcome::NoContent);
    };

    // Wrap mutation + event insert in a single transaction.
    // Issue-CHECKED-TX — 2026-03-16: conn is freshly acquired from writer lock, no nesting.
    let tx = conn
        .transaction()
        .map_err(|e| ApiError::from(db::DbError::from(e)))?;

    tx.execute(
        "UPDATE tracks SET enclosure_url = ?1 WHERE feed_guid = ?2 AND track_guid = ?3",
        params![new_url, track.feed_guid, track_guid],
    )
    .map_err(|e| ApiError::from(db::DbError::from(e)))?;

    // Issue-12 PATCH emits events — 2026-03-13
    // Re-read the track after the update to capture current state.
    let updated_track = db::get_track_for_feed(&tx, &track.feed_guid, track_guid)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("track {track_guid} vanished after update"),
            www_authenticate: None,
        })?;

    // Look up payment routes and value-time splits.
    let routes = db::get_payment_routes_for_feed_track(&tx, &track.feed_guid, track_guid)
        .map_err(ApiError::from)?;
    let value_time_splits =
        db::get_value_time_splits_for_feed_track(&tx, &track.feed_guid, track_guid)
            .map_err(ApiError::from)?;

    // Build and sign a TrackUpserted event.
    let now = db::unix_now();
    let event_id = uuid::Uuid::new_v4().to_string();
    let payload = event::TrackUpsertedPayload {
        track: updated_track,
        routes,
        value_time_splits,
    };
    let payload_json = serde_json::to_string(&payload).map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("failed to serialize TrackUpserted payload: {e}"),
        www_authenticate: None,
    })?;
    // Issue-SEQ-INTEGRITY — 2026-03-14: sign after insert to include seq.
    let (seq, signed_by, signature) = db::insert_event(
        &tx,
        &event_id,
        &event::EventType::TrackUpserted,
        &payload_json,
        track_guid,
        &state.signer,
        now,
        &[],
    )
    .map_err(ApiError::from)?;

    tx.commit()
        .map_err(|e| ApiError::from(db::DbError::from(e)))?;

    // Build event for fan-out AFTER commit.
    let tagged = format!(r#"{{"type":"track_upserted","data":{payload_json}}}"#);
    let ev_payload =
        serde_json::from_str::<event::EventPayload>(&tagged).map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("failed to deserialize TrackUpserted event for fan-out: {e}"),
            www_authenticate: None,
        })?;

    let fanout_event = event::Event {
        event_id,
        event_type: event::EventType::TrackUpserted,
        payload: ev_payload,
        subject_guid: track_guid.to_string(),
        signed_by,
        signature,
        seq,
        created_at: now,
        warnings: vec![],
        payload_json,
    };

    Ok(PatchTrackOutcome::Updated(vec![fanout_event]))
}

async fn handle_patch_track(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(guid): Path<String>,
    Json(req): Json<PatchTrackRequest>,
) -> Result<Response, ApiError> {
    let state2 = Arc::clone(&state);
    let guid2 = guid.clone();
    // Mutex safety compliant — 2026-03-12
    // Finding-2 atomic mutation+event — 2026-03-13
    let result = tokio::task::spawn_blocking(move || -> Result<PatchTrackOutcome, ApiError> {
        let mut conn = state2.db.writer().lock().map_err(|_poison| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "database mutex poisoned".into(),
            www_authenticate: None,
        })?;

        let tracks = db::get_tracks_by_guid(&conn, &guid2).map_err(ApiError::from)?;
        let Some(track) = tracks.first() else {
            return Err(ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("track {guid2} not found"),
                www_authenticate: None,
            });
        };
        if tracks.len() > 1 {
            return Ok(PatchTrackOutcome::Conflict(
                ambiguous_track_candidates_for_tracks(&tracks),
            ));
        }

        patch_resolved_track(&mut conn, &headers, &state2, track, &guid2, &req)
    })
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?;

    match result? {
        PatchTrackOutcome::NoContent => Ok(StatusCode::NO_CONTENT.into_response()),
        PatchTrackOutcome::Updated(events) => {
            publish_events_to_sse(&state.sse_registry, &events);

            let db_fanout = state.db.clone();
            let client_fanout = state.push_client.clone();
            let subscribers_fanout = Arc::clone(&state.push_subscribers);
            tokio::spawn(fan_out_push(
                db_fanout,
                client_fanout,
                subscribers_fanout,
                events,
            ));

            Ok(StatusCode::NO_CONTENT.into_response())
        }
        PatchTrackOutcome::Conflict(candidates) => {
            Ok(ambiguous_track_guid_response(&guid, candidates))
        }
    }
}

async fn handle_patch_feed_track(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((feed_guid, track_guid)): Path<(String, String)>,
    Json(req): Json<PatchTrackRequest>,
) -> Result<Response, ApiError> {
    let state2 = Arc::clone(&state);
    let feed_guid2 = feed_guid.clone();
    let track_guid2 = track_guid.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<PatchTrackOutcome, ApiError> {
        let mut conn = state2.db.writer().lock().map_err(|_poison| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "database mutex poisoned".into(),
            www_authenticate: None,
        })?;

        let track = db::get_track_for_feed(&conn, &feed_guid2, &track_guid2)
            .map_err(ApiError::from)?
            .ok_or_else(|| ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("track {track_guid2} not found in feed {feed_guid2}"),
                www_authenticate: None,
            })?;

        patch_resolved_track(&mut conn, &headers, &state2, &track, &track_guid2, &req)
    })
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?;

    match result? {
        PatchTrackOutcome::NoContent => Ok(StatusCode::NO_CONTENT.into_response()),
        PatchTrackOutcome::Updated(events) => {
            publish_events_to_sse(&state.sse_registry, &events);

            let db_fanout = state.db.clone();
            let client_fanout = state.push_client.clone();
            let subscribers_fanout = Arc::clone(&state.push_subscribers);
            tokio::spawn(fan_out_push(
                db_fanout,
                client_fanout,
                subscribers_fanout,
                events,
            ));

            Ok(StatusCode::NO_CONTENT.into_response())
        }
        PatchTrackOutcome::Conflict(candidates) => {
            Ok(ambiguous_track_guid_response(&track_guid, candidates))
        }
    }
}

// ── OpenAPI schema registration (ADR 0044) ──────────────────────────────────

/// A named `OpenAPI` schema, paired for insertion into `components.schemas`.
type SchemaEntry = (
    String,
    utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
);

/// Pushes `T`'s own schema, plus every schema `T` references, onto `schemas`.
fn register_schema<T: ToSchema>(schemas: &mut Vec<SchemaEntry>) {
    schemas.push((T::name().into_owned(), T::schema()));
    T::schemas(schemas);
}

/// Schemas for every documented JSON response this module returns.
///
/// Read by `openapi::spec_value` to fill `components.schemas`. No response
/// references these schemas yet (ADR 0044 task 001); a later task adds that.
pub(crate) fn response_schemas() -> Vec<SchemaEntry> {
    let mut schemas = Vec::new();
    register_schema::<AmbiguousTrackGuidBody>(&mut schemas);
    register_schema::<ErrorBody>(&mut schemas);
    register_schema::<NodeInfoResponse>(&mut schemas);
    register_schema::<db::FeedBlock>(&mut schemas);
    schemas
}
