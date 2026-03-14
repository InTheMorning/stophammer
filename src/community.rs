// Rust guideline compliant (M-APP-ERROR, M-MODULE-DOCS) — 2026-03-09

//! Community node mode: syncs events from a primary node and serves a
//! read-only API with a push-receive endpoint.
//!
//! On startup the community node:
//! 1. Registers itself with the Cloudflare tracker (fire-and-forget).
//! 2. Registers its push URL with the primary (`POST /sync/register`).
//! 3. Restores its `last_seq` cursor from the local DB.
//! 4. Runs a poll-loop fallback: polls the primary only when no push has
//!    been received for `push_timeout_secs` (default 90s).
//!
//! The push handler (`POST /sync/push`) is served on the same port and
//! updates `last_push_at` so the poll-loop stays quiet while pushes arrive.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::Serialize;

use crate::{apply, db, sync};

/// Seconds between retry attempts when polling for the primary's pubkey.
///
/// The primary may still be starting when the community node boots.
/// 2 seconds is short enough for fast startup but long enough to avoid
/// hammering a booting primary.
const RETRY_BACKOFF_SECS: u64 = 2;

/// Maximum number of events accepted in a single `POST /sync/push` request.
const MAX_PUSH_EVENTS: usize = 1_000;

/// Maximum request body size (bytes) for the push endpoint.
const MAX_PUSH_BODY_BYTES: usize = 2 * 1024 * 1024;

// ── CommunityConfig ──────────────────────────────────────────────────────────

/// Runtime configuration for a community (read-only replica) node.
///
/// This struct has four fields. The M-DESIGN-FOR-AI guideline recommends a
/// builder pattern for types with four or more constructor parameters, but
/// that applies to library APIs consumed by external callers. Here the struct
/// is constructed exactly once in `main.rs` from environment variables; a
/// builder would add boilerplate with no safety or usability benefit for an
/// application binary. Plain struct initialisation is the idiomatic choice.
// CRIT-03 Debug derive — 2026-03-13
#[derive(Debug)]
pub struct CommunityConfig {
    /// Base URL of the primary node, e.g. `"http://primary.example.com:8008"`.
    pub primary_url: String,
    /// Base URL of the tracker, e.g. `"https://stophammer-tracker.workers.dev"`.
    pub tracker_url: String,
    /// This node's public address, e.g. `"http://mynode.example.com:8008"`.
    pub node_address: String,
    /// Seconds between poll-loop iterations. Default: 300.
    pub poll_interval_secs: u64,
    /// Seconds of silence before the fallback poll fires. Default: 90.
    pub push_timeout_secs: i64,
}

// ── CommunityState ───────────────────────────────────────────────────────────

/// Shared state for the push-receive endpoint.
// CRIT-03 Debug derive — 2026-03-13
#[derive(Debug)]
pub struct CommunityState {
    /// Local database handle.
    pub db:                 db::Db,
    /// Hex-encoded ed25519 public key of the authoritative primary node.
    pub primary_pubkey_hex: String,
    /// Unix timestamp (seconds) of the last successfully received push.
    /// Stored as i64 with `Relaxed` ordering (monotonic read, no cross-thread
    /// happens-before needed — a stale read at most delays one poll cycle).
    pub last_push_at:       Arc<AtomicI64>,
    /// Issue-SSE-PUBLISH — 2026-03-14: SSE registry shared with the readonly
    /// router so that events applied via push are published to SSE clients.
    pub sse_registry:       Option<Arc<crate::api::SseRegistry>>,
}

// ── Tracker registration body ────────────────────────────────────────────────

#[derive(Serialize)]
struct RegisterBody<'a> {
    pubkey:  &'a str,
    address: &'a str,
}

// ── run_community_sync ───────────────────────────────────────────────────────

/// Spawn the background sync task. Returns immediately; the task runs until
/// the process exits.
///
/// `pubkey_hex` is the hex-encoded ed25519 pubkey of this node's key, used
/// as the cursor identity in `node_sync_state` and in tracker registration.
///
/// # Panics
///
/// Panics if the `reqwest::Client` cannot be built (TLS backend unavailable).
pub async fn run_community_sync(
    config:       CommunityConfig,
    db:           db::Db,
    pubkey_hex:   String,
    last_push_at: Arc<AtomicI64>,
    sse_registry: Option<Arc<crate::api::SseRegistry>>,
) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build reqwest client");

    // 1. Fire-and-forget tracker registration.
    register_with_tracker(&client, &config.tracker_url, &pubkey_hex, &config.node_address).await;

    // 2. Register push endpoint with the primary.
    register_with_primary(
        &client,
        &config.primary_url,
        &pubkey_hex,
        &config.node_address,
    ).await;

    // 3. Load persisted cursor.
    // Mutex safety compliant — 2026-03-12
    let initial_seq = db.lock().map_or_else(|_| {
        tracing::error!("community: db mutex poisoned; starting from seq 0");
        0
    }, |conn| match db::get_node_sync_cursor(&conn, &pubkey_hex) {
        Ok(seq) => seq,
        Err(e) => {
            tracing::error!(error = %e, "community: failed to read sync cursor; starting from 0");
            0
        }
    });

    let mut last_seq = initial_seq;
    tracing::info!(primary = %config.primary_url, cursor = last_seq, "community: sync started");

    // 4. Poll-loop fallback.
    //
    // Yield strategy (M-YIELD-POINTS): each iteration contains at least one
    // async await that surrenders control to the runtime:
    //   - `poll_once` issues an HTTP request via reqwest (I/O yield).
    //   - `apply::apply_events` dispatches each DB write via `spawn_blocking`.
    //   - `tokio::time::sleep` at the bottom yields for the configured interval.
    loop {
        let now_secs = db::unix_now();

        let secs_since_push = now_secs - last_push_at.load(Ordering::Relaxed);
        if secs_since_push > config.push_timeout_secs {
            tracing::info!(seconds_since_push = secs_since_push, "community: fallback poll triggered");
            match poll_once(&client, &config.primary_url, last_seq).await {
                Err(e) => {
                    tracing::error!(error = %e, "community: poll error");
                }
                Ok(response) => {
                    let fetched = response.events.len();
                    if fetched > 0 {
                        let summary =
                            apply::apply_events(Arc::clone(&db), &pubkey_hex, response.events, sse_registry.as_ref())
                                .await;
                        if summary.applied > 0 {
                            // Advance last_seq from the primary's seq values.
                            // Mutex safety compliant — 2026-03-12
                            let new_seq = db.lock().map_or_else(|_| {
                                tracing::error!(cursor = last_seq, "community: db mutex poisoned; keeping cursor");
                                last_seq
                            }, |conn| db::get_node_sync_cursor(&conn, &pubkey_hex).unwrap_or(last_seq));
                            if new_seq > last_seq {
                                last_seq = new_seq;
                            }
                            tracing::info!(
                                applied = summary.applied, fetched, cursor = last_seq,
                                "community: poll applied events"
                            );
                        }
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(config.poll_interval_secs)).await;
    }
}

// ── Issue-4 HTTPS pubkey discovery — 2026-03-13 ─────────────────────────────

/// Validates that `primary_url` uses HTTPS for pubkey auto-discovery.
///
/// Returns `Ok(())` if the URL scheme is `https`, or if the URL is `http`
/// but `ALLOW_INSECURE_PUBKEY_DISCOVERY=true` is set (local dev/docker).
///
/// # Errors
///
/// Returns a human-readable error string if the URL is plain HTTP and the
/// escape-hatch env var is not set.
pub fn require_https_for_discovery(primary_url: &str) -> Result<(), String> {
    if primary_url.starts_with("https://") {
        return Ok(());
    }

    if primary_url.starts_with("http://") {
        tracing::warn!(
            url = %primary_url,
            "PRIMARY_PUBKEY auto-discovery URL uses plain HTTP — vulnerable to MITM"
        );

        let allowed = std::env::var("ALLOW_INSECURE_PUBKEY_DISCOVERY")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        if !allowed {
            return Err(
                "FATAL: PRIMARY_PUBKEY auto-discovery requires HTTPS. \
                 Set PRIMARY_PUBKEY env var explicitly, use an HTTPS primary URL, \
                 or set ALLOW_INSECURE_PUBKEY_DISCOVERY=true for local development."
                    .to_string(),
            );
        }
    }

    Ok(())
}

// ── fetch_primary_pubkey ─────────────────────────────────────────────────────

/// Fetches the primary node's pubkey from `GET {primary_url}/node/info`.
///
/// Retries up to `max_attempts` times with a 2-second delay — the primary
/// may still be starting when the community node boots. Returns `None` if
/// all attempts fail (caller falls back to the configured value).
pub async fn fetch_primary_pubkey(
    client:       &reqwest::Client,
    primary_url:  &str,
    max_attempts: u32,
) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct NodeInfo { node_pubkey: String }

    let url = format!("{primary_url}/node/info");
    for attempt in 1..=max_attempts {
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(info) = resp.json::<NodeInfo>().await {
                    return Some(info.node_pubkey);
                }
            }
            _ => {}
        }
        if attempt < max_attempts {
            tracing::info!(attempt, max_attempts, "community: waiting for primary node/info");
            tokio::time::sleep(Duration::from_secs(RETRY_BACKOFF_SECS)).await;
        }
    }
    tracing::warn!(url = %url, "community: could not fetch primary pubkey; using configured value");
    None
}

// ── register_with_tracker ────────────────────────────────────────────────────

async fn register_with_tracker(
    client:       &reqwest::Client,
    tracker_url:  &str,
    pubkey_hex:   &str,
    node_address: &str,
) {
    let url  = format!("{tracker_url}/nodes/register");
    let body = RegisterBody { pubkey: pubkey_hex, address: node_address };

    match client.post(&url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!(tracker = %tracker_url, "community: registered with tracker");
        }
        Ok(resp) => {
            tracing::warn!(
                tracker = %tracker_url, status = %resp.status(),
                "community: tracker registration returned non-success; ignored"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "community: tracker registration failed (ignoring)");
        }
    }
}

// ── register_with_primary ────────────────────────────────────────────────────

// CS-03 authenticated register — 2026-03-12
/// Announces this node's push URL to the primary via `POST /sync/register`.
///
/// Reads the `ADMIN_TOKEN` environment variable and sends it as
/// `X-Admin-Token` header so the primary can authenticate the request.
///
/// Errors are logged and swallowed — the poll-loop fallback handles catch-up
/// when the primary is unreachable at startup.
async fn register_with_primary(
    client:       &reqwest::Client,
    primary_url:  &str,
    pubkey_hex:   &str,
    node_address: &str,
) {
    let url  = format!("{primary_url}/sync/register");
    let body = sync::RegisterRequest {
        node_pubkey: pubkey_hex.to_string(),
        node_url:    format!("{node_address}/sync/push"),
    };

    // Finding-3 separate sync token — 2026-03-13
    // Prefer SYNC_TOKEN (new, least-privilege); fall back to ADMIN_TOKEN (legacy).
    let sync_token  = std::env::var("SYNC_TOKEN").ok().filter(|s| !s.is_empty());
    let admin_token = std::env::var("ADMIN_TOKEN").unwrap_or_default();

    let mut request = client.post(&url).json(&body);
    if let Some(ref token) = sync_token {
        request = request.header("X-Sync-Token", token.as_str());
    } else if !admin_token.is_empty() {
        tracing::warn!(
            "DEPRECATED: registering with primary via ADMIN_TOKEN. \
             Set SYNC_TOKEN env var for least-privilege sync registration."
        );
        request = request.header("X-Admin-Token", &admin_token);
    } else {
        tracing::warn!(
            "Neither SYNC_TOKEN nor ADMIN_TOKEN env var is set — \
             push registration with primary will be rejected with 403."
        );
    }

    match request.send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!(primary = %primary_url, "community: registered push endpoint with primary");
        }
        Ok(resp) => {
            tracing::warn!(
                primary = %primary_url, status = %resp.status(),
                "community: primary registration returned non-success; ignored"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "community: primary registration failed (ignoring)");
        }
    }
}

// ── poll_once ────────────────────────────────────────────────────────────────

async fn poll_once(
    client:      &reqwest::Client,
    primary_url: &str,
    after_seq:   i64,
) -> Result<crate::sync::SyncEventsResponse, String> {
    let url = format!("{primary_url}/sync/events?after_seq={after_seq}&limit=500");

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("GET {url} returned HTTP {status}"));
    }

    resp.json::<crate::sync::SyncEventsResponse>()
        .await
        .map_err(|e| format!("failed to deserialise sync response: {e}"))
}

// ── build_community_push_router ──────────────────────────────────────────────

/// Builds the router for the community push endpoint.
///
/// Merged into the read-only router in `main.rs` so that community nodes
/// serve both `GET /sync/events` (via readonly router) and `POST /sync/push`.
pub fn build_community_push_router(state: Arc<CommunityState>) -> Router {
    Router::new()
        .route("/sync/push", post(handle_sync_push))
        .layer(DefaultBodyLimit::max(MAX_PUSH_BODY_BYTES))
        .with_state(state)
}

// ── POST /sync/push ──────────────────────────────────────────────────────────

// Flow: verify each event signature → apply idempotently → update last_push_at
// → return counts.
async fn handle_sync_push(
    State(state): State<Arc<CommunityState>>,
    Json(req): Json<sync::PushRequest>,
) -> Result<Json<sync::PushResponse>, StatusCode> {
    // Availability: reject oversized push batches to prevent resource exhaustion.
    if req.events.len() > MAX_PUSH_EVENTS {
        return Err(StatusCode::BAD_REQUEST);
    }

    let now = db::unix_now();

    // Filter to events signed by the known primary pubkey before applying.
    // Events with unexpected signers are counted as rejected.
    let mut pre_rejected = 0usize;
    let trusted: Vec<crate::event::Event> = req.events.into_iter().filter(|ev| {
        if ev.signed_by == state.primary_pubkey_hex {
            true
        } else {
            tracing::warn!(
                event_id = %ev.event_id, signed_by = %ev.signed_by,
                expected = %state.primary_pubkey_hex,
                "push: event signed by unknown key"
            );
            pre_rejected += 1;
            false
        }
    }).collect();

    // Signature verification + DB apply happens inside apply_events.
    // Cursor is keyed on the primary pubkey, consistent with the poll-loop.
    // Issue-SSE-PUBLISH — 2026-03-14: pass SSE registry so applied events
    // are published to community-node SSE clients.
    let summary = apply::apply_events(
        Arc::clone(&state.db),
        &state.primary_pubkey_hex,
        trusted,
        state.sse_registry.as_ref(),
    ).await;

    if summary.applied > 0 || summary.duplicate > 0 {
        state.last_push_at.store(now, Ordering::Relaxed);
    }

    if summary.applied > 0 || summary.rejected > 0 || pre_rejected > 0 {
        tracing::info!(
            applied = summary.applied, duplicate = summary.duplicate,
            rejected = summary.rejected + pre_rejected,
            "push: batch processed"
        );
    }

    Ok(Json(sync::PushResponse {
        applied:   summary.applied,
        rejected:  summary.rejected + pre_rejected,
        duplicate: summary.duplicate,
    }))
}

