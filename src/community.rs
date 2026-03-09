// Rust guideline compliant (M-APP-ERROR, M-MODULE-DOCS) — 2026-03-09

//! Community node mode: syncs events from a primary node and serves a
//! read-only API.
//!
//! The community node never signs events. On startup it:
//! 1. Registers itself with the Cloudflare tracker (fire-and-forget).
//! 2. Restores its `last_seq` cursor from the local DB.
//! 3. Enters a poll loop: fetch events from the primary, verify each
//!    ed25519 signature, apply to local DB, advance cursor.
//!
//! A failed poll cycle is logged and the loop continues — a transient
//! network error does not crash the node.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::{db, event, signing};

// ── CommunityConfig ──────────────────────────────────────────────────────────

pub struct CommunityConfig {
    /// Base URL of the primary node, e.g. `"http://primary.example.com:8008"`.
    pub primary_url: String,
    /// Base URL of the tracker, e.g. `"https://stophammer-tracker.workers.dev"`.
    pub tracker_url: String,
    /// This node's public address registered with the tracker,
    /// e.g. `"http://mynode.example.com:8008"`.
    pub node_address: String,
    /// Seconds between sync polls. Default: 30.
    pub poll_interval_secs: u64,
}

// ── Tracker registration body ────────────────────────────────────────────────

#[derive(Serialize)]
struct RegisterBody<'a> {
    pubkey:  &'a str,
    address: &'a str,
}

// ── run_community_sync ───────────────────────────────────────────────────────

/// Spawn the background sync task.  Returns immediately; the task runs until
/// the process exits.
///
/// `pubkey_hex` is the hex-encoded ed25519 pubkey of this node's key, used
/// as the cursor identity in `node_sync_state` and in the tracker registration.
pub async fn run_community_sync(
    config:     CommunityConfig,
    db:         db::Db,
    pubkey_hex: String,
) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build reqwest client");

    // 1. Fire-and-forget tracker registration.
    register_with_tracker(&client, &config.tracker_url, &pubkey_hex, &config.node_address).await;

    // 2. Load persisted cursor.
    let initial_seq = {
        let conn = db.lock().unwrap();
        match db::get_node_sync_cursor(&conn, &pubkey_hex) {
            Ok(seq) => seq,
            Err(e) => {
                eprintln!("[community] failed to read sync cursor: {e}; starting from 0");
                0
            }
        }
    };

    let mut last_seq = initial_seq;
    println!("[community] sync started — primary={} cursor={last_seq}", config.primary_url);

    // 3. Poll loop.
    loop {
        match poll_once(&client, &config.primary_url, last_seq).await {
            Err(e) => {
                eprintln!("[community] poll error: {e}");
            }
            Ok(response) => {
                let fetched = response.events.len();
                if fetched > 0 {
                    let new_seq = apply_events(Arc::clone(&db), &pubkey_hex, response.events).await;
                    if new_seq > last_seq {
                        last_seq = new_seq;
                        println!("[community] applied {fetched} events — cursor now {last_seq}");
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(config.poll_interval_secs)).await;
    }
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
            println!("[community] registered with tracker at {tracker_url}");
        }
        Ok(resp) => {
            eprintln!(
                "[community] tracker registration returned HTTP {}: ignored",
                resp.status()
            );
        }
        Err(e) => {
            eprintln!("[community] tracker registration failed (ignoring): {e}");
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

// ── apply_events ─────────────────────────────────────────────────────────────

/// Verify and apply a batch of events to the local DB.
///
/// Returns the highest `seq` successfully committed (unchanged from `last_seq`
/// if nothing was applied).
///
/// Each event is applied atomically in its own `spawn_blocking` closure so
/// that a single bad event does not abort the entire batch.
async fn apply_events(db: db::Db, node_pubkey: &str, events: Vec<event::Event>) -> i64 {
    let mut highest_seq: i64 = 0;

    // Capture node_pubkey as an owned String for the closure.
    let node_pubkey = node_pubkey.to_string();

    for ev in events {
        let seq = ev.seq;
        let event_id = ev.event_id.clone();

        // Verify signature before touching the DB.
        if let Err(e) = signing::verify_event_signature(&ev) {
            eprintln!("[community] invalid signature on event {event_id} seq={seq}: {e}; skipping");
            continue;
        }

        let db2         = Arc::clone(&db);
        let node_pk     = node_pubkey.clone();

        let result = tokio::task::spawn_blocking(move || apply_single_event(&db2, &node_pk, &ev))
            .await;

        match result {
            Err(panic_err) => {
                eprintln!("[community] apply task panicked for event {event_id}: {panic_err}");
            }
            Ok(Err(db_err)) => {
                eprintln!("[community] DB error applying event {event_id}: {db_err}");
            }
            Ok(Ok(())) => {
                if seq > highest_seq {
                    highest_seq = seq;
                }
            }
        }
    }

    highest_seq
}

// ── apply_single_event ───────────────────────────────────────────────────────

fn apply_single_event(db: &db::Db, node_pubkey: &str, ev: &event::Event) -> Result<(), db::DbError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .cast_signed();

    let conn = db.lock().unwrap();

    match &ev.payload {
        event::EventPayload::ArtistUpserted(p) => {
            // Upsert artist only if not already present — preserve local created_at.
            db::upsert_artist_if_absent(&conn, &p.artist)?;
        }
        event::EventPayload::FeedUpserted(p) => {
            db::upsert_artist_if_absent(&conn, &p.artist)?;
            db::upsert_feed(&conn, &p.feed)?;
        }
        event::EventPayload::TrackUpserted(p) => {
            db::upsert_track(&conn, &p.track)?;
            db::replace_payment_routes(&conn, &p.track.track_guid, &p.routes)?;
            db::replace_value_time_splits(&conn, &p.track.track_guid, &p.value_time_splits)?;
        }
        event::EventPayload::RoutesReplaced(p) => {
            db::replace_payment_routes(&conn, &p.track_guid, &p.routes)?;
        }
        event::EventPayload::FeedRetired(p) => {
            // Retirement not yet implemented — log and skip.
            eprintln!(
                "[community] FeedRetired for {} not yet implemented; skipping",
                p.feed_guid
            );
        }
        event::EventPayload::TrackRemoved(p) => {
            // Removal not yet implemented — log and skip.
            eprintln!(
                "[community] TrackRemoved for {} not yet implemented; skipping",
                p.track_guid
            );
        }
    }

    // Insert the event row so the community node can serve it via GET /sync/events.
    db::insert_event(
        &conn,
        &ev.event_id,
        &ev.event_type,
        // Re-serialize the payload to get the canonical JSON string.
        // This is fine: the signature was already verified against the same
        // payload, so no integrity risk.
        &serde_json::to_string(&ev.payload)
            .map_err(db::DbError::Json)?,
        &ev.subject_guid,
        &ev.signed_by,
        &ev.signature,
        ev.created_at,
        &ev.warnings,
    )?;

    // Advance the cursor.
    db::upsert_node_sync_state(&conn, node_pubkey, ev.seq, now)?;

    Ok(())
}
