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
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use governor::{Quota, RateLimiter, clock::DefaultClock, state::keyed::DefaultKeyedStateStore};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{db, db_pool, event, ingest, medium, model, proof, query, signing, sync, verify};

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

/// Access token lifetime in seconds (1 hour) for proof-of-possession tokens.
///
/// Must match [`crate::proof::TOKEN_TTL_SECS`] so the `expires_at` returned
/// in the assertion response reflects the actual token expiry.
const PROOF_TOKEN_TTL_SECS: i64 = 3600;

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

type IngestBlockingOutput = (
    ingest::IngestResponse,
    Vec<event::Event>,
    Vec<(String, SseFrame)>,
);

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
        self.senders.read().map(|g| g.len()).unwrap_or(0)
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
        event::EventPayload::FeedUpserted(p) => {
            vec![p.artist.artist_id.clone()]
        }
        event::EventPayload::TrackUpserted(p) => p
            .artist_credit
            .names
            .iter()
            .map(|n| n.artist_id.clone())
            .collect(),
        event::EventPayload::ArtistCreditCreated(p) => p
            .artist_credit
            .names
            .iter()
            .map(|n| n.artist_id.clone())
            .collect(),
        event::EventPayload::ArtistMerged(p) => {
            vec![p.target_artist_id.clone()]
        }
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

/// Maximum number of pending proof challenges allowed across the whole node.
/// Prevents unbounded table growth by cycling through many valid feed GUIDs.
const MAX_PENDING_CHALLENGES_TOTAL: i64 = 5_000;

/// Maximum length (bytes) for the `requester_nonce` field in proof challenge requests.
const MAX_NONCE_BYTES: usize = 256;

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
    /// When true, skip SSRF validation of feed URLs during proof assertion.
    /// Only intended for test environments where mock servers use localhost.
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

fn derive_feed_artist_name(feed_data: &ingest::IngestFeedData) -> String {
    if let Some(author_name) = non_empty_trimmed(feed_data.author_name.as_deref()) {
        return author_name.to_string();
    }

    if let Some(wavlake_slug_name) = wavlake_artist_name_from_links(&feed_data.links) {
        return wavlake_slug_name;
    }

    if let Some(owner_name) = non_empty_trimmed(feed_data.owner_name.as_deref())
        && !is_platform_owner_name(owner_name)
    {
        return owner_name.to_string();
    }

    "Unknown Artist".to_string()
}

fn non_empty_trimmed(value: Option<&str>) -> Option<&str> {
    let value = value?.trim();
    if value.is_empty() { None } else { Some(value) }
}

fn is_platform_owner_name(name: &str) -> bool {
    classify_platform_owner(name).is_some()
}

fn wavlake_artist_name_from_links(links: &[ingest::IngestLink]) -> Option<String> {
    links
        .iter()
        .filter(|link| link.link_type == "website")
        .find_map(|link| {
            let url = reqwest::Url::parse(&link.url).ok()?;
            let host = url.host_str()?.to_ascii_lowercase();
            if host != "wavlake.com" && host != "www.wavlake.com" {
                return None;
            }

            let slug = url.path_segments()?.find(|segment| !segment.is_empty())?;
            let slug = slug.trim();
            if slug.is_empty() || matches!(slug, "feed" | "music" | "node") {
                return None;
            }

            Some(humanize_slug(slug))
        })
}

fn humanize_slug(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(capitalize_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = String::new();
    out.extend(first.to_uppercase());
    out.push_str(chars.as_str());
    out
}

#[cfg(test)]
mod tests {
    use super::{
        build_source_contributor_claims, build_source_entity_links, build_source_platform_claims,
        derive_feed_artist_name, normalize_role, wavlake_artist_name_from_links,
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
            },
            IngestPerson {
                position: 0,
                name: "Bob".into(),
                role: Some("Guitar".into()),
                group_name: None,
                href: None,
                img: None,
            },
        ];

        let claims = build_source_contributor_claims("feed-1", "feed", "feed-1", &persons, 123);
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].position, 0);
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
    fn derive_feed_artist_prefers_author_name() {
        let mut feed = empty_feed();
        feed.author_name = Some("Real Artist".into());
        feed.owner_name = Some("Wavlake".into());
        feed.links.push(IngestLink {
            position: 0,
            link_type: "website".into(),
            url: "https://wavlake.com/dj-omegaman".into(),
            extraction_path: "feed.link".into(),
        });

        assert_eq!(derive_feed_artist_name(&feed), "Real Artist");
    }

    #[test]
    fn derive_feed_artist_uses_wavlake_profile_slug_before_platform_owner() {
        let mut feed = empty_feed();
        feed.owner_name = Some("Wavlake".into());
        feed.links.push(IngestLink {
            position: 0,
            link_type: "website".into(),
            url: "https://wavlake.com/dead-reckoning-band".into(),
            extraction_path: "feed.link".into(),
        });

        assert_eq!(
            wavlake_artist_name_from_links(&feed.links).as_deref(),
            Some("Dead Reckoning Band")
        );
        assert_eq!(derive_feed_artist_name(&feed), "Dead Reckoning Band");
    }

    #[test]
    fn derive_feed_artist_does_not_fall_back_to_release_title() {
        let mut feed = empty_feed();
        feed.owner_name = Some("Wavlake".into());

        assert_eq!(derive_feed_artist_name(&feed), "Unknown Artist");
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

#[derive(Serialize)]
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
    Router::new()
        .route("/ingest/feed", post(handle_ingest_feed))
        .route("/sync/events", get(handle_sync_events))
        .route("/sync/reconcile", post(handle_sync_reconcile))
        .route("/sync/register", post(handle_sync_register))
        .route("/sync/peers", get(handle_sync_peers))
        .route("/node/info", get(handle_node_info))
        .route("/admin/artists/merge", post(handle_admin_merge_artists))
        .route("/admin/artists/alias", post(handle_admin_add_alias))
        .route(
            "/admin/artist-identity/reviews/{id}/resolve",
            post(handle_admin_resolve_artist_identity_review),
        )
        .route(
            "/admin/artist-identity/reviews/pending",
            get(handle_admin_pending_artist_identity_reviews),
        )
        .route(
            "/admin/wallet-identity/reviews/pending",
            get(handle_admin_pending_wallet_identity_reviews),
        )
        .route(
            "/admin/wallet-identity/reviews/{id}/resolve",
            post(handle_admin_resolve_wallet_identity_review),
        )
        .route(
            "/v1/diagnostics/feeds/{guid}",
            get(handle_admin_feed_diagnostics),
        )
        .route(
            "/v1/diagnostics/artists/{id}",
            get(handle_admin_artist_diagnostics),
        )
        .route(
            "/v1/diagnostics/wallets/{id}",
            get(handle_admin_wallet_diagnostics),
        )
        .route(
            "/admin/diagnostics/feeds/{guid}",
            get(handle_admin_feed_diagnostics),
        )
        .route(
            "/admin/diagnostics/artists/{id}",
            get(handle_admin_artist_diagnostics),
        )
        .route(
            "/admin/diagnostics/wallets/{id}",
            get(handle_admin_wallet_diagnostics),
        )
        // Route versioning compliant — 2026-03-12
        .route(
            "/v1/feeds/{guid}",
            delete(handle_retire_feed).patch(handle_patch_feed),
        )
        .route(
            "/v1/feeds/{guid}/tracks/{track_guid}",
            delete(handle_remove_track),
        )
        .route("/v1/tracks/{guid}", patch(handle_patch_track))
        .route("/v1/proofs/challenge", post(handle_proofs_challenge))
        .route("/v1/proofs/assert", post(handle_proofs_assert))
        .route("/v1/events", get(handle_sse_events))
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
        .route("/sync/events", get(handle_sync_events))
        .route("/sync/peers", get(handle_sync_peers))
        .route("/node/info", get(handle_node_info))
        .route("/v1/events", get(handle_sse_events))
        .route("/health", get(|| async { "ok" }))
        .merge(query::query_routes())
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(build_cors_layer())
        .with_state(state)
}

// ── POST /ingest/feed ─────────────────────────────────────────────────────────

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
    Json(req): Json<ingest::IngestFeedRequest>,
) -> Result<Json<ingest::IngestResponse>, ApiError> {
    let state2 = Arc::clone(&state);
    // Mutex safety compliant — 2026-03-12
    let result = tokio::task::spawn_blocking(move || -> Result<IngestBlockingOutput, ApiError> {
        // Issue-VERIFY-READER — 2026-03-16
        // Phase 1: verify against a READ-ONLY connection (reader pool).
        // This avoids holding the writer mutex during verification, so
        // non-trivial verifiers never block the global write path.
        let warnings = {
            let reader = state2.db.reader().map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("reader pool error: {e}"),
                www_authenticate: None,
            })?;

            // 1. Get existing feed (read-only)
            let existing = db::get_existing_feed(&reader, &req.canonical_url)?;

            // 2. Build verify context and run chain against reader
            let ctx = verify::IngestContext {
                request: &req,
                db: &reader, // Issue-VERIFY-READER — ReadConn derefs to Connection
                existing: existing.as_ref(),
            };

            match state2.chain.run(&ctx) {
                Err(ref e) if e.0 == crate::verifiers::content_hash::NO_CHANGE_SENTINEL => {
                    return Ok((
                        ingest::IngestResponse {
                            accepted: true,
                            no_change: true,
                            reason: None,
                            events_emitted: vec![],
                            warnings: vec![],
                        },
                        vec![],
                        vec![],
                    ));
                }
                Err(e) => {
                    return Ok((
                        ingest::IngestResponse {
                            accepted: false,
                            no_change: false,
                            reason: Some(e.0),
                            events_emitted: vec![],
                            warnings: vec![],
                        },
                        vec![],
                        vec![],
                    ));
                }
                Ok(w) => w,
            }
        };
        // reader is dropped here — writer lock is never contested by verification

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

        // 5. Resolve artist from high-confidence source claims before the
        // legacy feed-scoped alias fallback.
        let artist_name = derive_feed_artist_name(feed_data);
        let feed_artist = db::resolve_feed_artist_from_source_claims(
            &conn,
            &artist_name,
            feed_guid_str,
            &source_entity_ids,
            &source_entity_links,
        )?;

        // 6. Get or create artist credit for the feed artist (idempotent, feed-scoped)
        let feed_artist_credit = db::get_or_create_artist_credit(
            &conn,
            &feed_artist.name,
            &[(
                feed_artist.artist_id.clone(),
                feed_artist.name.clone(),
                String::new(),
            )],
            Some(feed_guid_str),
        )?;
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

        // 8. Build Feed struct
        let feed = model::Feed {
            feed_guid: feed_data.feed_guid.clone(),
            feed_url: req.canonical_url.clone(),
            title: feed_data.title.clone(),
            title_lower: feed_data.title.to_lowercase(),
            artist_credit_id: feed_artist_credit.id,
            description: feed_data.description.clone(),
            image_url: feed_data.image_url.clone(),
            language: feed_data.language.clone(),
            explicit: feed_data.explicit,
            itunes_type: feed_data.itunes_type.clone(),
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
        };

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
        let source_platform_claims = build_source_platform_claims(
            &feed_data.feed_guid,
            &req.canonical_url,
            feed_data.owner_name.as_deref(),
            &feed_data.links,
            now,
        );
        push_source_release_claim(
            &mut source_release_claims,
            &feed_data.feed_guid,
            "feed",
            &feed_data.feed_guid,
            "release_date",
            feed_data.pub_date.map(|v| v.to_string()),
            "feed.pub_date",
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
        let mut track_tuples: Vec<(
            model::Track,
            Vec<model::PaymentRoute>,
            Vec<model::ValueTimeSplit>,
        )> = Vec::with_capacity(tracks.len());

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
            // Issue-ARTIST-IDENTITY — 2026-03-14
            let (track_credit_id, track_credit) = if let Some(author) = &track_data.author_name {
                let track_artist = db::resolve_artist(&conn, author, Some(feed_guid_str))?;
                let credit = db::get_or_create_artist_credit(
                    &conn,
                    &track_artist.name,
                    &[(
                        track_artist.artist_id.clone(),
                        track_artist.name.clone(),
                        String::new(),
                    )],
                    Some(feed_guid_str),
                )?;
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
                enclosure_url: track_data.enclosure_url.clone(),
                enclosure_type: track_data.enclosure_type.clone(),
                enclosure_bytes: track_data.enclosure_bytes,
                track_number: track_data.track_number,
                season: track_data.season,
                explicit: track_data.explicit,
                description: track_data.description.clone(),
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
                    source_track_guid: track_data.track_guid.clone(),
                    start_time_secs: v.start_time_secs,
                    duration_secs: v.duration_secs,
                    remote_feed_guid: v.remote_feed_guid.clone(),
                    remote_item_guid: v.remote_item_guid.clone(),
                    split: v.split,
                    created_at: now,
                })
                .collect();

            track_tuples.push((track, routes, vts));
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
                let track_artist = db::resolve_artist(&conn, author, Some(feed_guid_str))?;
                let credit = db::get_or_create_artist_credit(
                    &conn,
                    &track_artist.name,
                    &[(
                        track_artist.artist_id.clone(),
                        track_artist.name.clone(),
                        String::new(),
                    )],
                    Some(feed_guid_str),
                )?;
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
                enclosure_url: live_item.enclosure_url.clone(),
                enclosure_type: live_item.enclosure_type.clone(),
                enclosure_bytes: live_item.enclosure_bytes,
                track_number: live_item.track_number,
                season: live_item.season,
                explicit: live_item.explicit,
                description: live_item.description.clone(),
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
                    source_track_guid: live_item.live_item_guid.clone(),
                    start_time_secs: v.start_time_secs,
                    duration_secs: v.duration_secs,
                    remote_feed_guid: v.remote_feed_guid.clone(),
                    remote_item_guid: v.remote_item_guid.clone(),
                    split: v.split,
                    created_at: now,
                })
                .collect();

            track_tuples.push((track, routes, vts));
            track_credits.push(track_credit);
        }

        let source_contributor_claims =
            db::dedupe_source_contributor_claims(&source_contributor_claims);
        let source_entity_ids = db::dedupe_source_entity_ids(&source_entity_ids);
        let source_entity_links = db::dedupe_source_entity_links(&source_entity_links);
        let source_release_claims = db::dedupe_source_release_claims(&source_release_claims);
        let source_item_enclosures = db::dedupe_source_item_enclosures(&source_item_enclosures);

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

        // Collect event_ids and snapshot event data before moving event_rows
        let event_ids: Vec<String> = event_rows.iter().map(|r| r.event_id.clone()).collect();

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
            source_platform_claims,
            feed_routes,
            live_events,
            track_tuples,
            event_rows,
            &state2.signer,
        )?;

        // 11b. Search index + quality scores are now written inside
        // ingest_transaction (Issue-5 ingest atomic — 2026-03-13).

        // 12. Update crawl cache
        db::upsert_feed_crawl_cache(&conn, &req.canonical_url, &req.content_hash, now)?;

        // 13. Reconstruct events with assigned seqs + signatures for fan-out
        // Issue-SEQ-INTEGRITY — 2026-03-14: signatures come from ingest_transaction.
        let fanout_events: Vec<event::Event> = events_for_fanout
            .into_iter()
            .zip(seqs.iter())
            .map(|(r, (seq, signed_by, signature))| {
                let et_str = serde_json::to_string(&r.event_type).map_err(|e| ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!("failed to serialize event type for fan-out: {e}"),
                    www_authenticate: None,
                })?;
                let et_str = et_str.trim_matches('"');
                let tagged = format!(r#"{{"type":"{et_str}","data":{}}}"#, r.payload_json);
                let payload =
                    serde_json::from_str::<event::EventPayload>(&tagged).map_err(|e| ApiError {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        message: format!("failed to deserialize event payload for fan-out: {e}"),
                        www_authenticate: None,
                    })?;
                Ok(event::Event {
                    event_id: r.event_id,
                    event_type: r.event_type,
                    payload,
                    subject_guid: r.subject_guid,
                    signed_by: signed_by.clone(),
                    signature: signature.clone(),
                    seq: *seq,
                    created_at: r.created_at,
                    warnings: r.warnings,
                    payload_json: r.payload_json,
                })
            })
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
            },
            fanout_events,
            live_sse_frames,
        ))
    })
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?;

    let (response, fanout_events, live_sse_frames) = result?;

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
                        if !proof::is_url_ssrf_safe(&parsed) {
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
        tokio::task::spawn_blocking(move || proof::validate_node_url(&url_for_check))
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

#[derive(Deserialize, Serialize)]
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
            proof::resolve_and_validate_url(&parsed)
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

// ── FG-02 SSE artist follow — 2026-03-13 ─────────────────────────────────────

/// `GET /v1/events?artists=id1,id2,...` — Server-Sent Events for artist followers.
///
/// Subscribes the client to real-time notifications for the specified artist IDs.
/// Supports `Last-Event-ID` header for replaying missed events from the ring buffer.
///
/// Enforces two availability limits:
/// - `MAX_SSE_CONNECTIONS`: total concurrent SSE connections server-wide.
/// - `MAX_SSE_REGISTRY_ARTISTS`: total unique artist entries in the registry.
async fn handle_sse_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<
    axum::response::sse::Sse<
        impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
    ApiError,
> {
    use tokio_stream::StreamExt as _;

    // Cap the number of artist IDs per SSE connection to prevent unbounded
    // channel creation in the registry (availability hardening).
    const MAX_SSE_ARTISTS: usize = 50;

    // Enforce concurrent SSE connection limit.
    if !state.sse_registry.try_acquire_connection() {
        return Err(ApiError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: format!("too many SSE connections (limit: {MAX_SSE_CONNECTIONS})"),
            www_authenticate: None,
        });
    }

    let requested_ids: Vec<String> = params
        .get("artists")
        .map(|s| {
            s.split(',')
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .take(MAX_SSE_ARTISTS)
                .collect()
        })
        .unwrap_or_default();

    // Issue-SSE-EXHAUSTION — 2026-03-15: only subscribe to artists that actually
    // exist in the database. Unknown/fake artist IDs are silently dropped so
    // attackers cannot fill the registry with phantom channels.
    let artist_ids: Vec<String> = {
        let pool = state.db.clone();
        spawn_db(pool, move |conn| {
            Ok(requested_ids
                .into_iter()
                .filter(|id| db::artist_exists(conn, id).unwrap_or(false))
                .collect())
        })
        .await?
    };

    // Issue-SSE-PUBLISH — 2026-03-14: parse Last-Event-ID as an integer seq
    // for unambiguous replay. Falls back to 0 (replay everything in ring
    // buffer) if the header is absent or not a valid integer.
    let last_seq: i64 = headers
        .get("Last-Event-ID")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    // Issue-SSE-PUBLISH — 2026-03-14: replay from ring buffer using seq-based
    // cursor instead of subject_guid matching.
    let mut replay_events: Vec<SseFrame> = Vec::new();
    if last_seq > 0 {
        for artist_id in &artist_ids {
            let recent = state.sse_registry.recent_events(artist_id);
            for frame in recent {
                if frame.seq > last_seq {
                    replay_events.push(frame);
                }
            }
        }
        // Sort by seq so replayed events arrive in order across artists.
        replay_events.sort_by_key(|f| f.seq);
    }

    // Subscribe to live broadcast channels.
    // If the registry is full for a given artist (new, not yet tracked),
    // we silently skip that artist rather than failing the whole connection.
    let mut receivers: Vec<tokio::sync::broadcast::Receiver<SseFrame>> = Vec::new();
    for artist_id in &artist_ids {
        if let Some(rx) = state.sse_registry.subscribe(artist_id) {
            receivers.push(rx);
        }
    }

    // Clone the registry Arc so the live stream can release the connection on drop.
    let registry = Arc::clone(&state.sse_registry);

    // Merge replay events as an initial stream, then live events.
    // Issue-SSE-PUBLISH — 2026-03-14: use seq as SSE id for unambiguous replay.
    let replay_stream = tokio_stream::iter(replay_events.into_iter().map(|frame| {
        let json = serde_json::to_string(&frame).unwrap_or_default();
        Ok(axum::response::sse::Event::default()
            .event(&frame.event_type)
            .id(frame.seq.to_string())
            .data(json))
    }));

    // Issue-14 SSE async stream — 2026-03-13
    // Convert each broadcast receiver into an async BroadcastStream and merge
    // them with select_all. This eliminates the 100ms busy-sleep polling loop
    // that caused 10,000 wakeups/sec at max connections.
    let live_stream = async_stream::stream! {
        // Guard: release the SSE connection slot when this stream is dropped.
        let _guard = SseConnectionGuard { registry };

        use tokio_stream::wrappers::BroadcastStream;
        use futures_util::stream::select_all;

        let streams: Vec<_> = receivers
            .into_iter()
            .map(BroadcastStream::new)
            .collect();

        let mut merged = select_all(streams);

        while let Some(item) = futures_util::StreamExt::next(&mut merged).await {
            match item {
                Ok(frame) => {
                    let json = serde_json::to_string(&frame).unwrap_or_default();
                    // Issue-SSE-PUBLISH — 2026-03-14: use seq as SSE id.
                    yield Ok(axum::response::sse::Event::default()
                        .event(&frame.event_type)
                        .id(frame.seq.to_string())
                        .data(json));
                }
                Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                    tracing::debug!(lagged = n, "SSE client lagged behind broadcast");
                }
            }
        }
    };

    let merged = replay_stream.chain(live_stream);

    Ok(axum::response::sse::Sse::new(merged).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(30))
            .text("keepalive"),
    ))
}

/// RAII guard that releases an SSE connection slot when the stream is dropped
/// (i.e., when the client disconnects).
struct SseConnectionGuard {
    registry: Arc<SseRegistry>,
}

impl Drop for SseConnectionGuard {
    fn drop(&mut self) {
        self.registry.release_connection();
    }
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

// ── POST /admin/artists/merge ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct MergeArtistsRequest {
    source_artist_id: String,
    target_artist_id: String,
}

#[derive(Serialize)]
struct MergeArtistsResponse {
    merged: bool,
    events_emitted: Vec<String>,
}

async fn handle_admin_merge_artists(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<MergeArtistsRequest>,
) -> Result<Json<MergeArtistsResponse>, ApiError> {
    check_admin_token(&headers, &state.admin_token)?;

    let state2 = Arc::clone(&state);
    // Mutex safety compliant — 2026-03-12
    // Finding-2 atomic mutation+event — 2026-03-13
    // Issue-SSE-PUBLISH — 2026-03-14: return (response, sse_frame_info) for SSE publish.
    let result = tokio::task::spawn_blocking(
        move || -> Result<(MergeArtistsResponse, Option<(String, SseFrame)>), ApiError> {
            let mut conn = state2.db.writer().lock().map_err(|_poison| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "database mutex poisoned".into(),
                www_authenticate: None,
            })?;

            // Issue-CHECKED-TX — 2026-03-16: conn is freshly acquired from writer lock, no nesting.
            let tx = conn
                .transaction()
                .map_err(|e| ApiError::from(db::DbError::from(e)))?;

            let transferred =
                db::merge_artists_sql(&tx, &req.source_artist_id, &req.target_artist_id)
                    .map_err(ApiError::from)?;

            let now = db::unix_now();

            let event_id = uuid::Uuid::new_v4().to_string();
            let payload = event::ArtistMergedPayload {
                source_artist_id: req.source_artist_id.clone(),
                target_artist_id: req.target_artist_id.clone(),
                aliases_transferred: transferred,
            };
            let payload_json = serde_json::to_string(&payload).map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("failed to serialize ArtistMerged payload: {e}"),
                www_authenticate: None,
            })?;
            // Issue-SEQ-INTEGRITY — 2026-03-14: sign after insert to include seq.
            let (seq, _signed_by, _signature) = db::insert_event(
                &tx,
                &event_id,
                &event::EventType::ArtistMerged,
                &payload_json,
                &req.target_artist_id,
                &state2.signer,
                now,
                &[],
            )
            .map_err(ApiError::from)?;

            tx.commit()
                .map_err(|e| ApiError::from(db::DbError::from(e)))?;

            // Issue-SSE-PUBLISH — 2026-03-14
            let sse_info = {
                let frame = SseFrame {
                    event_type: "artist_merged".to_string(),
                    subject_guid: req.target_artist_id.clone(),
                    payload: serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
                    seq,
                };
                Some((req.target_artist_id.clone(), frame))
            };

            Ok((
                MergeArtistsResponse {
                    merged: true,
                    events_emitted: vec![event_id],
                },
                sse_info,
            ))
        },
    )
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?;

    let (response, sse_info) = result?;

    // Issue-SSE-PUBLISH — 2026-03-14
    if let Some((artist_id, frame)) = sse_info {
        state.sse_registry.publish(&artist_id, frame);
    }

    Ok(Json(response))
}

// ── POST /admin/artists/alias ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct AddAliasRequest {
    artist_id: String,
    alias: String,
}

#[derive(Serialize)]
struct AddAliasResponse {
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct ResolveArtistIdentityReviewRequest {
    action: String,
    #[serde(default)]
    target_artist_id: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Serialize)]
struct ResolveArtistIdentityReviewResponse {
    review: db::ArtistIdentityReviewItem,
    resolve_stats: db::ArtistIdentityResolveStats,
}

#[derive(Debug, Deserialize)]
struct ResolveWalletIdentityReviewRequest {
    action: String,
    #[serde(default)]
    target_wallet_id: Option<String>,
    #[serde(default)]
    target_artist_id: Option<String>,
    #[serde(default)]
    value: Option<String>,
}

#[derive(Debug, Serialize)]
struct ResolveWalletIdentityReviewResponse {
    review: db::WalletReviewItem,
}

#[derive(Debug, Deserialize)]
struct PendingReviewQuery {
    #[serde(default = "default_pending_review_limit")]
    limit: usize,
}

#[derive(Debug, Serialize)]
struct PendingArtistIdentityReviewsResponse {
    reviews: Vec<db::ArtistIdentityPendingReview>,
}

#[derive(Debug, Serialize)]
struct PendingWalletIdentityReviewsResponse {
    reviews: Vec<db::WalletReviewSummary>,
}

const fn default_pending_review_limit() -> usize {
    100
}

fn validate_wallet_identity_review_action_request(
    req: &ResolveWalletIdentityReviewRequest,
) -> Result<(String, Option<String>, Option<String>), ApiError> {
    match req.action.as_str() {
        "merge" => {
            let target_wallet_id = req.target_wallet_id.clone().ok_or_else(|| ApiError {
                status: StatusCode::BAD_REQUEST,
                message: "merge action requires target_wallet_id".into(),
                www_authenticate: None,
            })?;
            if req.target_artist_id.is_some() || req.value.is_some() {
                return Err(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    message: "merge action must not include target_artist_id or value".into(),
                    www_authenticate: None,
                });
            }
            Ok((req.action.clone(), Some(target_wallet_id), None))
        }
        "do_not_merge" => {
            if req.target_wallet_id.is_some()
                || req.target_artist_id.is_some()
                || req.value.is_some()
            {
                return Err(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    message:
                        "do_not_merge action must not include target_wallet_id, target_artist_id, or value"
                            .into(),
                    www_authenticate: None,
                });
            }
            Ok((req.action.clone(), None, None))
        }
        "force_class" => {
            if req.target_wallet_id.is_some() || req.target_artist_id.is_some() {
                return Err(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    message:
                        "force_class action must not include target_wallet_id or target_artist_id"
                            .into(),
                    www_authenticate: None,
                });
            }
            let value = req.value.clone().ok_or_else(|| ApiError {
                status: StatusCode::BAD_REQUEST,
                message: "force_class action requires value".into(),
                www_authenticate: None,
            })?;
            Ok((req.action.clone(), None, Some(value)))
        }
        "force_artist_link" | "block_artist_link" => {
            let target_artist_id = req.target_artist_id.clone().ok_or_else(|| ApiError {
                status: StatusCode::BAD_REQUEST,
                message: format!("{} action requires target_artist_id", req.action),
                www_authenticate: None,
            })?;
            if req.target_wallet_id.is_some() || req.value.is_some() {
                return Err(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    message: format!(
                        "{} action must not include target_wallet_id or value",
                        req.action
                    ),
                    www_authenticate: None,
                });
            }
            Ok((req.action.clone(), Some(target_artist_id), None))
        }
        other => Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: format!("unsupported wallet identity review action: {other}"),
            www_authenticate: None,
        }),
    }
}

#[derive(Debug, Serialize)]
struct AdminCreditNameResponse {
    artist_id: String,
    position: i64,
    name: String,
    join_phrase: String,
}

#[derive(Debug, Serialize)]
struct AdminCreditResponse {
    id: i64,
    display_name: String,
    names: Vec<AdminCreditNameResponse>,
}

#[derive(Debug, Serialize)]
struct AdminFeedTrackDiagnosticsResponse {
    track_guid: String,
    title: String,
    artist_credit: AdminCreditResponse,
}

#[derive(Debug, Serialize)]
struct AdminFeedWalletDiagnosticsResponse {
    wallet: db::WalletDetail,
    #[serde(skip_serializing_if = "Option::is_none")]
    claim_feed: Option<db::WalletClaimFeed>,
}

#[derive(Debug, Serialize)]
struct AdminFeedDiagnosticsResponse {
    feed_guid: String,
    title: String,
    feed_url: String,
    artist_credit: AdminCreditResponse,
    tracks: Vec<AdminFeedTrackDiagnosticsResponse>,
    artist_identity_plan: db::ArtistIdentityFeedPlan,
    artist_identity_reviews: Vec<db::ArtistIdentityReviewItem>,
    wallets: Vec<AdminFeedWalletDiagnosticsResponse>,
}

#[derive(Debug, Serialize)]
struct AdminArtistFeedDiagnosticsResponse {
    feed_guid: String,
    title: String,
    feed_url: String,
    artist_credit: AdminCreditResponse,
}

#[derive(Debug, Serialize)]
struct AdminArtistTrackDiagnosticsResponse {
    track_guid: String,
    feed_guid: String,
    title: String,
    artist_credit: AdminCreditResponse,
}

#[derive(Debug, Serialize)]
struct AdminArtistReviewDiagnosticsResponse {
    feed_guid: String,
    feed_title: String,
    review: db::ArtistIdentityReviewItem,
}

#[derive(Debug, Serialize)]
struct AdminArtistDiagnosticsResponse {
    requested_artist_id: String,
    artist: crate::model::Artist,
    redirected_from: Vec<String>,
    credits: Vec<AdminCreditResponse>,
    feeds: Vec<AdminArtistFeedDiagnosticsResponse>,
    tracks: Vec<AdminArtistTrackDiagnosticsResponse>,
    wallets: Vec<db::WalletDetail>,
    unlinked_feed_wallets: Vec<AdminFeedWalletDiagnosticsResponse>,
    reviews: Vec<AdminArtistReviewDiagnosticsResponse>,
}

#[derive(Debug, Serialize)]
struct AdminWalletDiagnosticsResponse {
    requested_wallet_id: String,
    wallet: db::WalletDetail,
    redirected_from: Vec<String>,
    reviews: Vec<db::WalletReviewItem>,
    claim_feeds: Vec<db::WalletClaimFeed>,
    alias_peers: Vec<db::WalletAliasPeer>,
}

fn admin_credit_response(
    conn: &rusqlite::Connection,
    credit_id: i64,
) -> Result<AdminCreditResponse, db::DbError> {
    let credit = db::get_artist_credit(conn, credit_id)?
        .ok_or_else(|| db::DbError::Other(format!("artist credit not found: {credit_id}")))?;
    Ok(AdminCreditResponse {
        id: credit.id,
        display_name: credit.display_name,
        names: credit
            .names
            .into_iter()
            .map(|name| AdminCreditNameResponse {
                artist_id: name.artist_id,
                position: name.position,
                name: name.name,
                join_phrase: name.join_phrase,
            })
            .collect(),
    })
}

fn resolve_current_artist_id_for_admin(
    conn: &rusqlite::Connection,
    artist_id: &str,
) -> Result<Option<String>, db::DbError> {
    let mut current = artist_id.to_string();
    for _ in 0..32 {
        let redirect = conn
            .query_row(
                "SELECT new_artist_id FROM artist_id_redirect WHERE old_artist_id = ?1",
                params![current],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match redirect {
            Some(next) if next != current => current = next,
            _ => break,
        }
    }
    if db::get_artist_by_id(conn, &current)?.is_some() {
        Ok(Some(current))
    } else {
        Ok(None)
    }
}

fn admin_artist_redirected_from(
    conn: &rusqlite::Connection,
    artist_id: &str,
) -> Result<Vec<String>, db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT old_artist_id
         FROM artist_id_redirect
         WHERE new_artist_id = ?1
         ORDER BY old_artist_id",
    )?;
    stmt.query_map(params![artist_id], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()
        .map_err(Into::into)
}

fn admin_artist_feeds(
    conn: &rusqlite::Connection,
    artist_id: &str,
) -> Result<Vec<AdminArtistFeedDiagnosticsResponse>, db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT f.feed_guid, f.title, f.feed_url, f.artist_credit_id
         FROM artist_credit_name acn
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id
         JOIN feeds f ON f.artist_credit_id = ac.id
         WHERE acn.artist_id = ?1
         ORDER BY f.title_lower, f.feed_guid",
    )?;
    stmt.query_map(params![artist_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .map(|(feed_guid, title, feed_url, artist_credit_id)| {
        Ok(AdminArtistFeedDiagnosticsResponse {
            feed_guid,
            title,
            feed_url,
            artist_credit: admin_credit_response(conn, artist_credit_id)?,
        })
    })
    .collect::<Result<Vec<_>, db::DbError>>()
}

fn admin_artist_tracks(
    conn: &rusqlite::Connection,
    artist_id: &str,
) -> Result<Vec<AdminArtistTrackDiagnosticsResponse>, db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT t.track_guid, t.feed_guid, t.title, t.artist_credit_id
         FROM artist_credit_name acn
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id
         JOIN tracks t ON t.artist_credit_id = ac.id
         WHERE acn.artist_id = ?1
         ORDER BY t.title_lower, t.track_guid",
    )?;
    stmt.query_map(params![artist_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .map(|(track_guid, feed_guid, title, artist_credit_id)| {
        Ok(AdminArtistTrackDiagnosticsResponse {
            track_guid,
            feed_guid,
            title,
            artist_credit: admin_credit_response(conn, artist_credit_id)?,
        })
    })
    .collect::<Result<Vec<_>, db::DbError>>()
}

fn admin_artist_reviews(
    conn: &rusqlite::Connection,
    artist_id: &str,
) -> Result<Vec<AdminArtistReviewDiagnosticsResponse>, db::DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT f.feed_guid, f.title
         FROM artist_credit_name acn
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id
         JOIN feeds f ON f.artist_credit_id = ac.id
         WHERE acn.artist_id = ?1
         UNION
         SELECT DISTINCT f.feed_guid, f.title
         FROM artist_credit_name acn
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id
         JOIN tracks t ON t.artist_credit_id = ac.id
         JOIN feeds f ON f.feed_guid = t.feed_guid
         WHERE acn.artist_id = ?1
         ORDER BY 2, 1",
    )?;
    let feed_rows = stmt
        .query_map(params![artist_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut reviews = Vec::new();
    for (feed_guid, feed_title) in feed_rows {
        for review in db::list_artist_identity_reviews_for_feed(conn, &feed_guid)? {
            if review
                .artist_ids
                .iter()
                .any(|candidate| candidate == artist_id)
            {
                reviews.push(AdminArtistReviewDiagnosticsResponse {
                    feed_guid: feed_guid.clone(),
                    feed_title: feed_title.clone(),
                    review,
                });
            }
        }
    }
    Ok(reviews)
}

fn resolve_current_wallet_id_for_admin(
    conn: &rusqlite::Connection,
    wallet_id: &str,
) -> Result<Option<String>, db::DbError> {
    let mut current = wallet_id.to_string();
    for _ in 0..32 {
        let redirect = conn
            .query_row(
                "SELECT new_wallet_id FROM wallet_id_redirect WHERE old_wallet_id = ?1",
                params![current],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match redirect {
            Some(next) if next != current => current = next,
            _ => break,
        }
    }
    if db::get_wallet_detail(conn, &current)?.is_some() {
        Ok(Some(current))
    } else {
        Ok(None)
    }
}

fn admin_wallet_diagnostics_response(
    conn: &rusqlite::Connection,
    requested_wallet_id: String,
) -> Result<Option<AdminWalletDiagnosticsResponse>, db::DbError> {
    let Some(resolved_id) = resolve_current_wallet_id_for_admin(conn, &requested_wallet_id)? else {
        return Ok(None);
    };
    let Some(wallet) = db::get_wallet_detail(conn, &resolved_id)? else {
        return Ok(None);
    };

    let redirected_from = {
        let mut stmt = conn.prepare(
            "SELECT old_wallet_id
             FROM wallet_id_redirect
             WHERE new_wallet_id = ?1
             ORDER BY old_wallet_id",
        )?;
        stmt.query_map(params![resolved_id.as_str()], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?
    };

    let reviews = db::list_wallet_reviews_for_wallet(conn, &resolved_id)?;
    let claim_feeds = db::get_wallet_claim_feeds(conn, &resolved_id)?;

    let mut alias_peers = std::collections::BTreeMap::new();
    for alias in &wallet.aliases {
        for peer in db::get_wallet_alias_peers(conn, &alias.alias.to_lowercase())? {
            if peer.wallet_id != resolved_id {
                alias_peers.insert(peer.wallet_id.clone(), peer);
            }
        }
    }

    Ok(Some(AdminWalletDiagnosticsResponse {
        requested_wallet_id,
        wallet,
        redirected_from,
        reviews,
        claim_feeds,
        alias_peers: alias_peers.into_values().collect(),
    }))
}

fn admin_artist_diagnostics_response(
    conn: &rusqlite::Connection,
    requested_artist_id: String,
) -> Result<Option<AdminArtistDiagnosticsResponse>, db::DbError> {
    let Some(resolved_id) = resolve_current_artist_id_for_admin(conn, &requested_artist_id)? else {
        return Ok(None);
    };
    let Some(artist) = db::get_artist_by_id(conn, &resolved_id)? else {
        return Ok(None);
    };

    let redirected_from = admin_artist_redirected_from(conn, &resolved_id)?;
    let credits = db::get_artist_credits_for_artist(conn, &resolved_id)?
        .into_iter()
        .map(|credit| admin_credit_response(conn, credit.id))
        .collect::<Result<Vec<_>, db::DbError>>()?;
    let feeds = admin_artist_feeds(conn, &resolved_id)?;
    let tracks = admin_artist_tracks(conn, &resolved_id)?;
    let linked_wallet_ids = db::get_wallet_ids_for_artist(conn, &resolved_id)?;
    let wallets = linked_wallet_ids
        .iter()
        .filter_map(|wallet_id| db::get_wallet_detail(conn, wallet_id).ok().flatten())
        .collect::<Vec<_>>();
    let linked_wallet_ids = linked_wallet_ids
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();

    let mut unlinked_feed_wallets = std::collections::BTreeMap::new();
    for feed in &feeds {
        for wallet_id in db::get_wallet_ids_for_feed(conn, &feed.feed_guid)? {
            if linked_wallet_ids.contains(&wallet_id) {
                continue;
            }
            if let Some(wallet) = db::get_wallet_detail(conn, &wallet_id)? {
                let claim_feed = db::get_wallet_claim_feeds(conn, &wallet_id)?
                    .into_iter()
                    .find(|claim_feed| claim_feed.feed_guid == feed.feed_guid);
                unlinked_feed_wallets
                    .entry(wallet_id)
                    .or_insert(AdminFeedWalletDiagnosticsResponse { wallet, claim_feed });
            }
        }
    }

    let reviews = admin_artist_reviews(conn, &resolved_id)?;

    Ok(Some(AdminArtistDiagnosticsResponse {
        requested_artist_id,
        artist,
        redirected_from,
        credits,
        feeds,
        tracks,
        wallets,
        unlinked_feed_wallets: unlinked_feed_wallets.into_values().collect(),
        reviews,
    }))
}

async fn handle_admin_add_alias(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<AddAliasRequest>,
) -> Result<Json<AddAliasResponse>, ApiError> {
    check_admin_token(&headers, &state.admin_token)?;

    // Issue-WAL-POOL — 2026-03-14: uses writer (add_artist_alias writes)
    let result = spawn_db_write(state.db.clone(), move |conn| {
        db::add_artist_alias(conn, &req.artist_id, &req.alias)?;
        Ok(AddAliasResponse { ok: true })
    })
    .await?;

    Ok(Json(result))
}

async fn handle_admin_resolve_artist_identity_review(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<ResolveArtistIdentityReviewRequest>,
) -> Result<Json<ResolveArtistIdentityReviewResponse>, ApiError> {
    check_admin_token(&headers, &state.admin_token)?;

    match req.action.as_str() {
        "merge" if req.target_artist_id.is_none() => {
            return Err(ApiError {
                status: StatusCode::BAD_REQUEST,
                message: "merge action requires target_artist_id".into(),
                www_authenticate: None,
            });
        }
        "do_not_merge" if req.target_artist_id.is_some() => {
            return Err(ApiError {
                status: StatusCode::BAD_REQUEST,
                message: "do_not_merge action must not include target_artist_id".into(),
                www_authenticate: None,
            });
        }
        "merge" | "do_not_merge" => {}
        other => {
            return Err(ApiError {
                status: StatusCode::BAD_REQUEST,
                message: format!("unsupported artist identity review action: {other}"),
                www_authenticate: None,
            });
        }
    }

    let action = req.action;
    let target_artist_id = req.target_artist_id;
    let note = req.note;
    let outcome = spawn_db_mut(state.db.clone(), move |conn| {
        let Some(_review) = db::get_artist_identity_review(conn, id)? else {
            return Ok(None);
        };
        let outcome = db::apply_artist_identity_review_action(
            conn,
            id,
            &action,
            target_artist_id.as_deref(),
            note.as_deref(),
        )?;
        Ok(Some(outcome))
    })
    .await?;

    let outcome = outcome.ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("artist identity review {id} not found"),
        www_authenticate: None,
    })?;

    Ok(Json(ResolveArtistIdentityReviewResponse {
        review: outcome.review,
        resolve_stats: outcome.resolve_stats,
    }))
}

async fn handle_admin_pending_artist_identity_reviews(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<PendingReviewQuery>,
) -> Result<Json<PendingArtistIdentityReviewsResponse>, ApiError> {
    check_admin_token(&headers, &state.admin_token)?;

    let reviews = spawn_db(state.db.clone(), move |conn| {
        db::list_pending_artist_identity_reviews(conn, query.limit)
    })
    .await?;

    Ok(Json(PendingArtistIdentityReviewsResponse { reviews }))
}

async fn handle_admin_pending_wallet_identity_reviews(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<PendingReviewQuery>,
) -> Result<Json<PendingWalletIdentityReviewsResponse>, ApiError> {
    check_admin_token(&headers, &state.admin_token)?;

    let reviews = spawn_db(state.db.clone(), move |conn| {
        db::list_pending_wallet_reviews(conn, query.limit)
    })
    .await?;

    Ok(Json(PendingWalletIdentityReviewsResponse { reviews }))
}

async fn handle_admin_resolve_wallet_identity_review(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<ResolveWalletIdentityReviewRequest>,
) -> Result<Json<ResolveWalletIdentityReviewResponse>, ApiError> {
    check_admin_token(&headers, &state.admin_token)?;
    let (action, target_id, value) = validate_wallet_identity_review_action_request(&req)?;
    let outcome = spawn_db_mut(state.db.clone(), move |conn| {
        let review_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM wallet_identity_review WHERE id = ?1)",
            params![id],
            |row| row.get(0),
        )?;
        if !review_exists {
            return Ok(None);
        }
        let outcome = db::apply_wallet_identity_review_action(
            conn,
            id,
            &action,
            target_id.as_deref(),
            value.as_deref(),
        )?;
        Ok(Some(outcome))
    })
    .await?;

    let outcome = outcome.ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("wallet identity review {id} not found"),
        www_authenticate: None,
    })?;

    Ok(Json(ResolveWalletIdentityReviewResponse {
        review: outcome.review,
    }))
}

async fn handle_admin_feed_diagnostics(
    State(state): State<Arc<AppState>>,
    Path(guid): Path<String>,
) -> Result<Json<AdminFeedDiagnosticsResponse>, ApiError> {
    let guid_for_db = guid.clone();
    let result = spawn_db(state.db.clone(), move |conn| {
        let Some(feed) = db::get_feed_by_guid(conn, &guid_for_db)? else {
            return Ok(None);
        };
        let tracks = db::get_tracks_for_feed(conn, &guid_for_db)?;
        let artist_identity_plan = db::explain_artist_identity_for_feed(conn, &guid_for_db)?;
        let artist_identity_reviews =
            db::list_artist_identity_reviews_for_feed(conn, &guid_for_db)?;
        let wallet_ids = db::get_wallet_ids_for_feed(conn, &guid_for_db)?;

        let wallets = wallet_ids
            .into_iter()
            .filter_map(|wallet_id| {
                let wallet = db::get_wallet_detail(conn, &wallet_id).ok().flatten()?;
                let claim_feed = db::get_wallet_claim_feeds(conn, &wallet_id)
                    .ok()?
                    .into_iter()
                    .find(|claim_feed| claim_feed.feed_guid == guid_for_db);
                Some(AdminFeedWalletDiagnosticsResponse { wallet, claim_feed })
            })
            .collect::<Vec<_>>();

        let response = AdminFeedDiagnosticsResponse {
            feed_guid: feed.feed_guid,
            title: feed.title,
            feed_url: feed.feed_url,
            artist_credit: admin_credit_response(conn, feed.artist_credit_id)?,
            tracks: tracks
                .into_iter()
                .map(|track| {
                    Ok(AdminFeedTrackDiagnosticsResponse {
                        track_guid: track.track_guid,
                        title: track.title,
                        artist_credit: admin_credit_response(conn, track.artist_credit_id)?,
                    })
                })
                .collect::<Result<Vec<_>, db::DbError>>()?,
            artist_identity_plan,
            artist_identity_reviews,
            wallets,
        };

        Ok(Some(response))
    })
    .await?;

    let response = result.ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("feed {guid} not found"),
        www_authenticate: None,
    })?;

    Ok(Json(response))
}

async fn handle_admin_artist_diagnostics(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<AdminArtistDiagnosticsResponse>, ApiError> {
    let requested_artist_id = id.clone();
    let result = spawn_db(state.db.clone(), move |conn| {
        admin_artist_diagnostics_response(conn, requested_artist_id)
    })
    .await?;

    let response = result.ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("artist {id} not found"),
        www_authenticate: None,
    })?;

    Ok(Json(response))
}

async fn handle_admin_wallet_diagnostics(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<AdminWalletDiagnosticsResponse>, ApiError> {
    let requested_wallet_id = id.clone();
    let result = spawn_db(state.db.clone(), move |conn| {
        admin_wallet_diagnostics_response(conn, requested_wallet_id)
    })
    .await?;

    let response = result.ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("wallet {id} not found"),
        www_authenticate: None,
    })?;

    Ok(Json(response))
}

// ── DELETE /feeds/{guid} ───────────────────────────────────────────────────

#[expect(
    clippy::too_many_lines,
    reason = "event signing, SSE publish, and fan-out all live in one handler"
)]
async fn handle_retire_feed(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(guid): Path<String>,
) -> Result<StatusCode, ApiError> {
    let state2 = Arc::clone(&state);
    let guid2 = guid.clone();
    // Mutex safety compliant — 2026-03-12
    // Issue-SSE-PUBLISH — 2026-03-14: return (events, artist_id) so we can
    // publish to the correct SSE channel after the entity is deleted.
    let result = tokio::task::spawn_blocking(
        move || -> Result<(Option<Vec<event::Event>>, Option<String>), ApiError> {
            let mut conn = state2.db.writer().lock().map_err(|_poison| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "database mutex poisoned".into(),
                www_authenticate: None,
            })?;

            // Auth inside lock scope: eliminates TOCTOU between auth check and DB write.
            check_admin_or_bearer_with_conn(
                &conn,
                &headers,
                &state2.admin_token,
                "feed:write",
                &guid2,
            )?;

            // Look up the feed — 404 if not found.
            let feed = db::get_feed_by_guid(&conn, &guid2)?.ok_or_else(|| ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("feed {guid2} not found"),
                www_authenticate: None,
            })?;

            // Issue-SSE-PUBLISH — 2026-03-14: capture artist_id before deletion.
            let sse_artist_id = db::get_artist_credit(&conn, feed.artist_credit_id)
                .ok()
                .flatten()
                .and_then(|c| c.names.first().map(|n| n.artist_id.clone()));

            // Fetch tracks to remove from search index.
            let tracks = db::get_tracks_for_feed(&conn, &guid2)?;

            // Remove search index entries (best-effort).
            for track in &tracks {
                let _ = crate::search::delete_from_search_index(
                    &conn,
                    "track",
                    &track.track_guid,
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
            // Issue-SEQ-INTEGRITY — 2026-03-14: signer passed to delete_feed_with_event
            // which signs after the DB assigns seq.
            let (seq, signed_by, signature) = db::delete_feed_with_event(
                &mut conn,
                &guid2,
                &event_id,
                &payload_json,
                &guid2,
                &state2.signer,
                now,
                &[],
            )
            .map_err(ApiError::from)?;

            // Build event for fan-out.
            let tagged = format!(r#"{{"type":"feed_retired","data":{payload_json}}}"#);
            let ev_payload =
                serde_json::from_str::<event::EventPayload>(&tagged).map_err(|e| ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!("failed to deserialize FeedRetired event for fan-out: {e}"),
                    www_authenticate: None,
                })?;

            let fanout_event = event::Event {
                event_id,
                event_type: event::EventType::FeedRetired,
                payload: ev_payload,
                subject_guid: guid2,
                signed_by,
                signature,
                seq,
                created_at: now,
                warnings: vec![],
                payload_json,
            };

            Ok((Some(vec![fanout_event]), sse_artist_id))
        },
    )
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?;

    let (fanout_events, sse_artist_id) = result?;

    // Fire-and-forget fan-out.
    if let Some(events) = fanout_events
        && !events.is_empty()
    {
        // Issue-SSE-PUBLISH — 2026-03-14
        if let Some(ref artist_id) = sse_artist_id {
            for ev in &events {
                let frame = SseFrame {
                    event_type: serde_json::to_string(&ev.event_type)
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_string(),
                    subject_guid: ev.subject_guid.clone(),
                    payload: serde_json::to_value(&ev.payload).unwrap_or(serde_json::Value::Null),
                    seq: ev.seq,
                };
                state.sse_registry.publish(artist_id, frame);
            }
        }

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

#[expect(
    clippy::too_many_lines,
    reason = "event signing, SSE publish, and fan-out all live in one handler"
)]
async fn handle_remove_track(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((guid, track_guid)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let state2 = Arc::clone(&state);
    let guid2 = guid.clone();
    let track_guid2 = track_guid.clone();
    // Mutex safety compliant — 2026-03-12
    // Issue-SSE-PUBLISH — 2026-03-14: return (events, artist_id) so we can
    // publish to the correct SSE channel after the entity is deleted.
    let result = tokio::task::spawn_blocking(
        move || -> Result<(Option<Vec<event::Event>>, Option<String>), ApiError> {
            let mut conn = state2.db.writer().lock().map_err(|_poison| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "database mutex poisoned".into(),
                www_authenticate: None,
            })?;

            // Auth inside lock scope: eliminates TOCTOU between auth check and DB write.
            // For bearer auth the token must be scoped to the parent feed.
            check_admin_or_bearer_with_conn(
                &conn,
                &headers,
                &state2.admin_token,
                "feed:write",
                &guid2,
            )?;

            // Look up the track — 404 if not found.
            let track = db::get_track_by_guid(&conn, &track_guid2)?.ok_or_else(|| ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("track {track_guid2} not found"),
                www_authenticate: None,
            })?;

            // Verify the track belongs to the specified feed.
            if track.feed_guid != guid2 {
                return Err(ApiError {
                    status: StatusCode::NOT_FOUND,
                    message: format!("track {track_guid2} does not belong to feed {guid2}"),
                    www_authenticate: None,
                });
            }

            // Issue-SSE-PUBLISH — 2026-03-14: capture artist_id before deletion.
            let sse_artist_id = db::get_artist_credit(&conn, track.artist_credit_id)
                .ok()
                .flatten()
                .and_then(|c| c.names.first().map(|n| n.artist_id.clone()));

            // Remove search index entry (best-effort).
            let _ = crate::search::delete_from_search_index(
                &conn,
                "track",
                &track.track_guid,
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

            Ok((Some(vec![fanout_event]), sse_artist_id))
        },
    )
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?;

    let (fanout_events, sse_artist_id) = result?;

    // Fire-and-forget fan-out.
    if let Some(events) = fanout_events
        && !events.is_empty()
    {
        // Issue-SSE-PUBLISH — 2026-03-14
        if let Some(ref artist_id) = sse_artist_id {
            for ev in &events {
                let frame = SseFrame {
                    event_type: serde_json::to_string(&ev.event_type)
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_string(),
                    subject_guid: ev.subject_guid.clone(),
                    payload: serde_json::to_value(&ev.payload).unwrap_or(serde_json::Value::Null),
                    seq: ev.seq,
                };
                state.sse_registry.publish(artist_id, frame);
            }
        }

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

// ── Bearer token extraction ────────────────────────────────────────────────

/// Build a `WWW-Authenticate` header value per RFC 6750 section 3.
///
/// When `error` is `None`, emits the minimal challenge:
///   `Bearer realm="stophammer"`
///
/// When `error` is provided (e.g. `"invalid_token"`, `"insufficient_scope"`),
/// appends the error attribute:
///   `Bearer realm="stophammer", error="invalid_token"`
// RFC 6750 compliant — 2026-03-12
#[must_use]
pub fn www_authenticate_challenge(error: Option<&str>) -> HeaderValue {
    let value = error.map_or_else(
        || r#"Bearer realm="stophammer""#.to_string(),
        |e| format!(r#"Bearer realm="stophammer", error="{e}""#),
    );
    // The constructed string is always valid ASCII header characters.
    HeaderValue::from_str(&value)
        .unwrap_or_else(|_err| HeaderValue::from_static(r#"Bearer realm="stophammer""#))
}

/// Parse `Authorization: Bearer <token>` from headers.
/// Returns `None` for missing or malformed headers (never panics).
/// Trims leading/trailing whitespace from the extracted token.
// RFC 6750 compliant — 2026-03-12
#[must_use]
pub fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get("Authorization")?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?.trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

/// Validate admin or bearer auth using an already-held connection
///
/// Accepts either `X-Admin-Token` or `Authorization: Bearer <token>`.
/// Unlike the former `check_admin_or_bearer`, this variant takes a borrowed
/// `rusqlite::Connection` so that auth validation shares the same lock scope
/// as the subsequent DB write -- eliminating the TOCTOU race where the token
/// could be invalidated between auth check and mutation.
///
/// # Errors
///
/// Returns `StatusCode::FORBIDDEN` for bad admin tokens,
/// `StatusCode::UNAUTHORIZED` with `WWW-Authenticate` for missing or invalid
/// bearer tokens (RFC 6750 section 3), and `StatusCode::FORBIDDEN` with
/// `error="insufficient_scope"` if the bearer token's subject feed does not
/// match `expected_feed_guid`.
// RFC 6750 compliant — 2026-03-12
pub fn check_admin_or_bearer_with_conn(
    conn: &rusqlite::Connection,
    headers: &HeaderMap,
    admin_token: &str,
    required_scope: &str,
    expected_feed_guid: &str,
) -> Result<(), ApiError> {
    // Prefer admin token if the header is present.
    if headers.contains_key("X-Admin-Token") {
        return check_admin_token(headers, admin_token);
    }

    // Try bearer token.  RFC 6750 compliant — 2026-03-12
    let token = extract_bearer_token(headers).ok_or_else(|| ApiError {
        status: StatusCode::UNAUTHORIZED,
        message: "missing Authorization header".into(),
        www_authenticate: Some(www_authenticate_challenge(None)),
    })?;

    let subject = proof::validate_token(conn, &token, required_scope)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "invalid_token".into(),
            www_authenticate: Some(www_authenticate_challenge(Some("invalid_token"))),
        })?;

    if subject != expected_feed_guid {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            message: "insufficient_scope".into(),
            www_authenticate: Some(www_authenticate_challenge(Some("insufficient_scope"))),
        });
    }

    Ok(())
}

// ── POST /proofs/challenge ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct ProofsChallengeRequest {
    feed_guid: String,
    scope: String,
    requester_nonce: String,
}

#[derive(Serialize)]
struct ProofsChallengeResponse {
    challenge_id: String,
    token_binding: String,
    state: String,
    expires_at: i64,
}

async fn handle_proofs_challenge(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ProofsChallengeRequest>,
) -> Result<(StatusCode, Json<ProofsChallengeResponse>), ApiError> {
    // Validate scope.
    if req.scope != "feed:write" {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: format!("unsupported scope: {}", req.scope),
            www_authenticate: None,
        });
    }

    if req.requester_nonce.len() < 16 {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "requester_nonce must be at least 16 characters".into(),
            www_authenticate: None,
        });
    }

    // Availability: cap nonce length to prevent oversized token_binding storage.
    if req.requester_nonce.len() > MAX_NONCE_BYTES {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: format!("requester_nonce exceeds maximum length of {MAX_NONCE_BYTES} bytes"),
            www_authenticate: None,
        });
    }

    // Mutex safety compliant — 2026-03-12
    let state2 = Arc::clone(&state);
    let result = tokio::task::spawn_blocking(move || -> Result<ProofsChallengeResponse, ApiError> {
        let mut conn = state2.db.writer().lock().map_err(|_poison| ApiError {
            status:  StatusCode::INTERNAL_SERVER_ERROR,
            message: "database mutex poisoned".into(),
            www_authenticate: None,
        })?;

        // Reclaim expired slots eagerly so stale rows do not block legitimate
        // challenge creation until the background pruner runs.
        proof::prune_expired(&mut conn).map_err(ApiError::from)?;

        if db::get_feed_by_guid(&conn, &req.feed_guid)
            .map_err(ApiError::from)?
            .is_none()
        {
            return Err(ApiError {
                status:  StatusCode::NOT_FOUND,
                message: "feed not found in database".into(),
                www_authenticate: None,
            });
        }

        // A fresh challenge for the same feed should replace any existing
        // pending challenge rather than being blocked by it.
        let superseded =
            proof::invalidate_pending_challenges_for_feed(&conn, &req.feed_guid, &req.scope)
                .map_err(ApiError::from)?;
        if superseded > 0 {
            tracing::debug!(
                feed_guid = %req.feed_guid,
                superseded,
                "proof-challenge: invalidated prior pending challenges for feed"
            );
        }

        let total_pending_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proof_challenges WHERE state = 'pending'",
                [],
                |row| row.get(0),
            )
            .map_err(ApiError::from)?;

        if total_pending_count >= MAX_PENDING_CHALLENGES_TOTAL {
            return Err(ApiError {
                status:  StatusCode::TOO_MANY_REQUESTS,
                message: format!(
                    "too many pending challenges globally (limit: {MAX_PENDING_CHALLENGES_TOTAL})"
                ),
                www_authenticate: None,
            });
        }

        let (challenge_id, token_binding) =
            proof::create_challenge(&conn, &req.feed_guid, &req.scope, &req.requester_nonce)
                .map_err(ApiError::from)?;

        // Read back the challenge to get expires_at.
        let challenge = proof::get_challenge(&conn, &challenge_id)
            .map_err(ApiError::from)?
            .ok_or_else(|| ApiError {
                status:  StatusCode::INTERNAL_SERVER_ERROR,
                message: "challenge not found after creation".into(),
                www_authenticate: None,
            })?;

        Ok(ProofsChallengeResponse {
            challenge_id,
            token_binding,
            state:      "pending".into(),
            expires_at: challenge.expires_at,
        })
    })
    .await
    .map_err(|e| ApiError {
        status:  StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?;

    result.map(|r| (StatusCode::CREATED, Json(r)))
}

// ── POST /proofs/assert ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ProofsAssertRequest {
    challenge_id: String,
    requester_nonce: String,
}

// Issue-PROOF-LEVEL — 2026-03-14
#[derive(Serialize)]
struct ProofsAssertResponse {
    access_token: String,
    scope: String,
    subject_feed_guid: String,
    expires_at: i64,
    proof_level: proof::ProofLevel,
}

// CS-01 pod:txt verification — 2026-03-12
#[expect(
    clippy::too_many_lines,
    reason = "three-phase spawn_blocking pattern for RSS verification requires sequential structure"
)]
async fn handle_proofs_assert(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ProofsAssertRequest>,
) -> Result<Json<ProofsAssertResponse>, ApiError> {
    if req.requester_nonce.len() < 16 {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "requester_nonce must be at least 16 characters".into(),
            www_authenticate: None,
        });
    }

    // ── Phase 1 (blocking): validate nonce, load challenge, look up feed_url ──
    let state2 = Arc::clone(&state);
    let req_challenge_id = req.challenge_id.clone();
    let req_nonce = req.requester_nonce.clone();

    let phase1 = tokio::task::spawn_blocking(
        move || -> Result<(String, String, String, String), ApiError> {
            // Mutex safety compliant — 2026-03-12
            let conn = state2.db.writer().lock().map_err(|_poison| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "database mutex poisoned".into(),
                www_authenticate: None,
            })?;

            // Load the challenge (404 if not found or expired).
            let challenge = proof::get_challenge(&conn, &req_challenge_id)
                .map_err(ApiError::from)?
                .ok_or_else(|| ApiError {
                    status: StatusCode::NOT_FOUND,
                    message: "challenge not found or expired".into(),
                    www_authenticate: None,
                })?;

            // Check challenge is still pending (400 if already resolved).
            if challenge.state != "pending" {
                return Err(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    message: format!("challenge already resolved as '{}'", challenge.state),
                    www_authenticate: None,
                });
            }

            // Recompute token_binding from stored token + requester_nonce.
            let expected = proof::recompute_binding(&challenge.token_binding, &req_nonce);
            let nonce_ok = expected.as_deref() == Some(&challenge.token_binding);

            if !nonce_ok {
                // Nonce mismatch: mark invalid and return 400.
                proof::resolve_challenge(&conn, &req_challenge_id, "invalid")
                    .map_err(ApiError::from)?;
                return Err(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    message: "requester_nonce does not match token binding".into(),
                    www_authenticate: None,
                });
            }

            // Look up feed_url from the feeds table using challenge's feed_guid.
            let feed = db::get_feed_by_guid(&conn, &challenge.feed_guid)
                .map_err(ApiError::from)?
                .ok_or_else(|| ApiError {
                    status: StatusCode::NOT_FOUND,
                    message: "feed not found in database".into(),
                    www_authenticate: None,
                })?;

            Ok((
                challenge.feed_guid,
                challenge.scope,
                challenge.token_binding,
                feed.feed_url,
            ))
        },
    )
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })??;

    let (feed_guid, scope, token_binding, feed_url) = phase1;

    // ── Phase 2 (async): fetch RSS and verify podcast:txt ─────────────────────

    // Issue-22 async DNS — 2026-03-13
    // SSRF guard: reject feed URLs targeting private/reserved IPs before fetching.
    // validate_feed_url uses std::net::ToSocketAddrs (blocking DNS), so we run
    // it inside spawn_blocking to avoid stalling the tokio worker thread.
    // CRIT-02 feature-gate — 2026-03-13
    #[cfg(feature = "test-util")]
    let skip_ssrf = state.skip_ssrf_validation;
    #[cfg(not(feature = "test-util"))]
    let skip_ssrf = false;

    // Issue-DNS-REBIND — 2026-03-16: capture resolved addresses for DNS pinning.
    // validate_feed_url uses std::net::ToSocketAddrs (blocking DNS), so we run
    // it inside spawn_blocking to avoid stalling the tokio worker thread.
    let resolved_addrs: Vec<std::net::SocketAddr> = if skip_ssrf {
        vec![]
    } else {
        let url_clone = feed_url.clone();
        tokio::task::spawn_blocking(move || proof::validate_feed_url(&url_clone))
            .await
            .map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("SSRF validation task failed: {e}"),
                www_authenticate: None,
            })?
            .map_err(|e| ApiError {
                status: StatusCode::BAD_REQUEST,
                message: format!("feed URL rejected: {e}"),
                www_authenticate: None,
            })?
    };

    // Issue-DNS-REBIND — 2026-03-16: use manual redirect following with DNS
    // pinning at every hop to eliminate TOCTOU rebinding attacks.
    let rss_verified = if skip_ssrf {
        let proof_client = proof::build_ssrf_safe_client();
        proof::verify_podcast_txt(&proof_client, &feed_url, &token_binding)
            .await
            .map_err(|e| ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: format!("RSS verification failed: {e}"),
                www_authenticate: None,
            })?
    } else {
        let hostname = url::Url::parse(&feed_url)
            .ok()
            .and_then(|u| u.host_str().map(String::from))
            .unwrap_or_default();
        proof::verify_podcast_txt_pinned(&feed_url, &token_binding, &hostname, &resolved_addrs)
            .await
            .map_err(|e| ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: format!("RSS verification failed: {e}"),
                www_authenticate: None,
            })?
    };

    // ── Phase 3 (blocking): resolve challenge and issue token ─────────────────
    let state3 = Arc::clone(&state);
    let challenge_id = req.challenge_id.clone();
    let feed_guid2 = feed_guid.clone();
    let scope2 = scope.clone();
    let phase1_feed_url = feed_url;

    let result = tokio::task::spawn_blocking(move || -> Result<ProofsAssertResponse, ApiError> {
        // Mutex safety compliant — 2026-03-12
        let conn = state3.db.writer().lock().map_err(|_poison| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "database mutex poisoned".into(),
            www_authenticate: None,
        })?;

        if !rss_verified {
            proof::resolve_challenge(&conn, &challenge_id, "invalid").map_err(ApiError::from)?;
            return Err(ApiError {
                status: StatusCode::BAD_REQUEST,
                message: "token_binding not found in RSS podcast:txt".into(),
                www_authenticate: None,
            });
        }

        // Issue-PROOF-RACE — 2026-03-14
        // Re-read the feed URL and reject if it changed since phase 1.
        // A concurrent PATCH could have changed the URL between phase 1
        // (which read it) and now, meaning the RSS verification in phase 2
        // was performed against a URL that is no longer current.
        let current_feed = db::get_feed_by_guid(&conn, &feed_guid2)
            .map_err(ApiError::from)?
            .ok_or_else(|| ApiError {
                status: StatusCode::NOT_FOUND,
                message: "feed not found in database".into(),
                www_authenticate: None,
            })?;
        if current_feed.feed_url != phase1_feed_url {
            return Err(ApiError {
                status: StatusCode::CONFLICT,
                message: "feed URL changed during verification; retry".into(),
                www_authenticate: None,
            });
        }

        // Mark the challenge as valid. If rows == 0 the challenge was already
        // resolved by a concurrent request (TOCTOU between Phase 1 and Phase 3).
        let rows =
            proof::resolve_challenge(&conn, &challenge_id, "valid").map_err(ApiError::from)?;
        if rows == 0 {
            return Err(ApiError {
                status: StatusCode::BAD_REQUEST,
                message: "challenge already resolved (concurrent request)".into(),
                www_authenticate: None,
            });
        }

        // Issue an access token.
        // Issue-PROOF-LEVEL — 2026-03-14
        let proof_level = proof::ProofLevel::RssOnly;
        let access_token = proof::issue_token(&conn, &scope2, &feed_guid2, &proof_level)
            .map_err(ApiError::from)?;

        // Compute expires_at for the response.
        let now = db::unix_now();
        let expires_at = now + PROOF_TOKEN_TTL_SECS;

        Ok(ProofsAssertResponse {
            access_token,
            scope: scope2,
            subject_feed_guid: feed_guid2,
            expires_at,
            proof_level,
        })
    })
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("internal task panic: {e}"),
        www_authenticate: None,
    })?;

    result.map(Json)
}

// ── PATCH /feeds/{guid} ────────────────────────────────────────────────────
// REST semantics compliant (RFC 7396) — 2026-03-12
// Issue-12 PATCH emits events — 2026-03-13
// Issue-13 PATCH 404 check — 2026-03-13

#[derive(Deserialize)]
struct PatchFeedRequest {
    feed_url: Option<String>,
}

#[expect(
    clippy::too_many_lines,
    reason = "event signing and fan-out follow the handle_retire_feed pattern"
)]
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

            // Auth inside lock scope: eliminates TOCTOU between auth check and DB write.
            check_admin_or_bearer_with_conn(
                &conn,
                &headers,
                &state2.admin_token,
                "feed:write",
                &guid2,
            )?;

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

            // Wrap mutation + event insert in a single transaction.
            // Issue-CHECKED-TX — 2026-03-16: conn is freshly acquired from writer lock, no nesting.
            let tx = conn
                .transaction()
                .map_err(|e| ApiError::from(db::DbError::from(e)))?;

            tx.execute(
                "UPDATE feeds SET feed_url = ?1 WHERE feed_guid = ?2",
                params![new_url, guid2],
            )
            .map_err(|e| ApiError::from(db::DbError::from(e)))?;

            // Finding-6 token revocation on URL change — 2026-03-13
            // Existing tokens were proved against the OLD feed URL's podcast:txt.
            // After a URL change, the artist must re-prove ownership.
            crate::proof::revoke_tokens_for_feed(&tx, &guid2).map_err(ApiError::from)?;

            // Issue-12 PATCH emits events — 2026-03-13
            // Re-read the feed after the update to capture current state.
            let feed = db::get_feed_by_guid(&tx, &guid2)
                .map_err(ApiError::from)?
                .ok_or_else(|| ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!("feed {guid2} vanished after update"),
                    www_authenticate: None,
                })?;

            // Look up the artist credit and artist for the event payload.
            let artist_credit = db::get_artist_credit(&tx, feed.artist_credit_id)
                .map_err(ApiError::from)?
                .ok_or_else(|| ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!(
                        "artist credit {} not found for feed {guid2}",
                        feed.artist_credit_id
                    ),
                    www_authenticate: None,
                })?;

            let artist_id = artist_credit
                .names
                .first()
                .map_or("", |n| n.artist_id.as_str());
            let artist = db::get_artist_by_id(&tx, artist_id)
                .map_err(ApiError::from)?
                .ok_or_else(|| ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!("artist {artist_id} not found for feed {guid2}"),
                    www_authenticate: None,
                })?;

            // Build and sign a FeedUpserted event.
            let now = db::unix_now();
            let event_id = uuid::Uuid::new_v4().to_string();
            let payload = event::FeedUpsertedPayload {
                feed,
                artist,
                artist_credit,
            };
            let payload_json = serde_json::to_string(&payload).map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("failed to serialize FeedUpserted payload: {e}"),
                www_authenticate: None,
            })?;
            // Issue-SEQ-INTEGRITY — 2026-03-14: sign after insert to include seq.
            let (seq, signed_by, signature) = db::insert_event(
                &tx,
                &event_id,
                &event::EventType::FeedUpserted,
                &payload_json,
                &guid2,
                &state2.signer,
                now,
                &[],
            )
            .map_err(ApiError::from)?;

            tx.commit()
                .map_err(|e| ApiError::from(db::DbError::from(e)))?;

            // Build event for fan-out AFTER commit.
            let tagged = format!(r#"{{"type":"feed_upserted","data":{payload_json}}}"#);
            let ev_payload =
                serde_json::from_str::<event::EventPayload>(&tagged).map_err(|e| ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!("failed to deserialize FeedUpserted event for fan-out: {e}"),
                    www_authenticate: None,
                })?;

            let fanout_event = event::Event {
                event_id,
                event_type: event::EventType::FeedUpserted,
                payload: ev_payload,
                subject_guid: guid2,
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

// ── PATCH /tracks/{guid} ───────────────────────────────────────────────────
// REST semantics compliant (RFC 7396) — 2026-03-12
// Issue-12 PATCH emits events — 2026-03-13
// Issue-13 PATCH 404 check — 2026-03-13

#[derive(Deserialize)]
struct PatchTrackRequest {
    enclosure_url: Option<String>,
}

#[expect(
    clippy::too_many_lines,
    reason = "event signing and fan-out follow the handle_retire_feed pattern"
)]
async fn handle_patch_track(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(guid): Path<String>,
    Json(req): Json<PatchTrackRequest>,
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

            // Auth inside lock scope: eliminates TOCTOU between auth check and DB write.
            // Look up the track first to find its parent feed guid, then validate
            // the bearer token against that feed.
            // Issue-13 PATCH 404 check — 2026-03-13
            let track = db::get_track_by_guid(&conn, &guid2)
                .map_err(ApiError::from)?
                .ok_or_else(|| ApiError {
                    status: StatusCode::NOT_FOUND,
                    message: format!("track {guid2} not found"),
                    www_authenticate: None,
                })?;

            check_admin_or_bearer_with_conn(
                &conn,
                &headers,
                &state2.admin_token,
                "feed:write",
                &track.feed_guid,
            )?;

            let Some(new_url) = &req.enclosure_url else {
                return Ok(None);
            };

            // Wrap mutation + event insert in a single transaction.
            // Issue-CHECKED-TX — 2026-03-16: conn is freshly acquired from writer lock, no nesting.
            let tx = conn
                .transaction()
                .map_err(|e| ApiError::from(db::DbError::from(e)))?;

            tx.execute(
                "UPDATE tracks SET enclosure_url = ?1 WHERE track_guid = ?2",
                params![new_url, guid2],
            )
            .map_err(|e| ApiError::from(db::DbError::from(e)))?;

            // Issue-12 PATCH emits events — 2026-03-13
            // Re-read the track after the update to capture current state.
            let updated_track = db::get_track_by_guid(&tx, &guid2)
                .map_err(ApiError::from)?
                .ok_or_else(|| ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!("track {guid2} vanished after update"),
                    www_authenticate: None,
                })?;

            // Look up the artist credit for the event payload.
            let artist_credit = db::get_artist_credit(&tx, updated_track.artist_credit_id)
                .map_err(ApiError::from)?
                .ok_or_else(|| ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!(
                        "artist credit {} not found for track {guid2}",
                        updated_track.artist_credit_id
                    ),
                    www_authenticate: None,
                })?;

            // Look up payment routes and value-time splits.
            let routes = db::get_payment_routes_for_track(&tx, &guid2).map_err(ApiError::from)?;
            let value_time_splits =
                db::get_value_time_splits_for_track(&tx, &guid2).map_err(ApiError::from)?;

            // Build and sign a TrackUpserted event.
            let now = db::unix_now();
            let event_id = uuid::Uuid::new_v4().to_string();
            let payload = event::TrackUpsertedPayload {
                track: updated_track,
                routes,
                value_time_splits,
                artist_credit,
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
                &guid2,
                &state2.signer,
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
                subject_guid: guid2,
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
