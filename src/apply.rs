//! Shared event-application logic used by both the poll-loop fallback and the
//! push-receiver handler in community mode.
//!
//! `apply_single_event` writes one verified event to the local DB idempotently.
//! `apply_events` verifies signatures and drives the per-event apply loop,
//! returning counts of applied/duplicate/rejected events.

use std::sync::Arc;

use crate::{api, db, event, signing};

// ── ApplyOutcome ─────────────────────────────────────────────────────────────

/// Per-event outcome returned by [`apply_single_event`].
// CRIT-03 Debug derive — 2026-03-13
#[derive(Debug)]
pub enum ApplyOutcome {
    /// Event was new and written to the DB; contains the assigned `seq`.
    Applied(i64),
    /// Event already existed in the DB (`INSERT OR IGNORE` was a no-op).
    Duplicate,
}

// ── ApplySummary ─────────────────────────────────────────────────────────────

/// Aggregate counts returned by [`apply_events`].
// CRIT-03 Debug derive — 2026-03-13
#[derive(Debug)]
pub(crate) struct ApplySummary {
    pub applied:   usize,
    pub duplicate: usize,
    pub rejected:  usize,
    /// Highest seq applied during this batch (0 if nothing was applied).
    pub max_seq:   i64,
}

// ── apply_single_event ───────────────────────────────────────────────────────

/// Applies a single **pre-verified** event to the local DB idempotently.
///
/// All mutations go through `INSERT OR IGNORE` / upsert variants so that
/// re-delivering the same event is safe. The caller must have already verified
/// the event signature before calling this function.
///
/// # Errors
///
/// Returns `DbError` if any database operation fails.
// Issue-17 apply atomic transaction — 2026-03-13
#[expect(clippy::too_many_lines, reason = "single event-application match covering all EventPayload variants")]
#[expect(clippy::significant_drop_tightening, reason = "conn is used across the entire event-application scope")]
pub fn apply_single_event(
    db:          &db::Db,
    node_pubkey: &str,
    ev:          &event::Event,
) -> Result<ApplyOutcome, db::DbError> {
    // Timestamp ordering compliant — 2026-03-12
    let now = db::unix_now();

    // Mutex safety compliant — 2026-03-12
    let conn = db.lock().map_err(|_poison| db::DbError::Poisoned)?;

    // Wrap the ENTIRE body in a single transaction so all writes (entity
    // upsert + quality computation + search index + event record) are
    // atomic. On error the transaction is rolled back automatically.
    let tx = conn.unchecked_transaction()?;

    // Issue-DEDUP-ORDER — 2026-03-14
    // Insert the event row FIRST (dedup guard). If the event_id already
    // exists, `INSERT OR IGNORE` returns no rows and we skip all mutations.
    let seq_opt = db::insert_event_idempotent(
        &tx,
        &ev.event_id,
        &ev.event_type,
        &ev.payload_json,
        &ev.subject_guid,
        &ev.signed_by,
        &ev.signature,
        ev.created_at,
        &ev.warnings,
    )?;

    let Some(seq) = seq_opt else {
        // Duplicate event — already applied; commit (no-op) and return early.
        tx.commit()?;
        return Ok(ApplyOutcome::Duplicate);
    };

    // Issue-PAYLOAD-INTEGRITY — 2026-03-14
    // Re-derive the payload from `payload_json` (the signed bytes) instead of
    // trusting `ev.payload`.  This closes a MITM vector where an attacker
    // could alter the deserialized struct without breaking the signature,
    // which only covers `payload_json`.
    let et_str = serde_json::to_string(&ev.event_type)?;
    let et_str = et_str.trim_matches('"');
    let tagged = format!(r#"{{"type":"{et_str}","data":{}}}"#, ev.payload_json);
    let verified_payload: event::EventPayload = serde_json::from_str(&tagged)?;

    match &verified_payload {
        event::EventPayload::ArtistUpserted(p) => {
            db::upsert_artist_if_absent(&tx, &p.artist)?;
            // Recompute artist quality + search index
            let score = crate::quality::compute_artist_quality(&tx, &p.artist.artist_id)?;
            crate::quality::store_quality(&tx, "artist", &p.artist.artist_id, score)?;
            crate::search::populate_search_index(
                &tx, "artist", &p.artist.artist_id,
                &p.artist.name, "",
                "",
                "",
            )?;
        }
        event::EventPayload::FeedUpserted(p) => {
            db::upsert_artist_if_absent(&tx, &p.artist)?;
            // Ensure artist credit exists before upserting feed
            upsert_artist_credit_if_absent(&tx, &p.artist_credit)?;
            db::upsert_feed(&tx, &p.feed)?;
            // Recompute feed quality + search index
            let score = crate::quality::compute_feed_quality(&tx, &p.feed.feed_guid)?;
            crate::quality::store_quality(&tx, "feed", &p.feed.feed_guid, score)?;
            crate::search::populate_search_index(
                &tx, "feed", &p.feed.feed_guid,
                "", &p.feed.title,
                p.feed.description.as_deref().unwrap_or(""),
                p.feed.raw_medium.as_deref().unwrap_or(""),
            )?;
        }
        event::EventPayload::TrackUpserted(p) => {
            // Ensure artist credit exists before upserting track
            upsert_artist_credit_if_absent(&tx, &p.artist_credit)?;
            db::upsert_track(&tx, &p.track)?;
            db::replace_payment_routes(&tx, &p.track.track_guid, &p.routes)?;
            db::replace_value_time_splits(&tx, &p.track.track_guid, &p.value_time_splits)?;
            // Recompute track quality + search index
            let score = crate::quality::compute_track_quality(&tx, &p.track.track_guid)?;
            crate::quality::store_quality(&tx, "track", &p.track.track_guid, score)?;
            crate::search::populate_search_index(
                &tx, "track", &p.track.track_guid,
                "", &p.track.title,
                p.track.description.as_deref().unwrap_or(""),
                "",
            )?;
        }
        event::EventPayload::RoutesReplaced(p) => {
            db::replace_payment_routes(&tx, &p.track_guid, &p.routes)?;
            // Recompute track quality (routes affect score)
            let score = crate::quality::compute_track_quality(&tx, &p.track_guid)?;
            crate::quality::store_quality(&tx, "track", &p.track_guid, score)?;
        }
        event::EventPayload::ArtistCreditCreated(p) => {
            upsert_artist_credit_if_absent(&tx, &p.artist_credit)?;
        }
        event::EventPayload::FeedRoutesReplaced(p) => {
            db::replace_feed_payment_routes(&tx, &p.feed_guid, &p.routes)?;
            // Recompute feed quality (routes affect score)
            let score = crate::quality::compute_feed_quality(&tx, &p.feed_guid)?;
            crate::quality::store_quality(&tx, "feed", &p.feed_guid, score)?;
        }
        event::EventPayload::FeedRetired(p) => {
            // Look up the feed to get search-index fields. If already gone, no-op.
            let feed_opt = db::get_feed_by_guid(&tx, &p.feed_guid)?;
            if let Some(feed) = feed_opt {
                // Fetch all tracks to remove their search index entries.
                let tracks = db::get_tracks_for_feed(&tx, &p.feed_guid)?;
                for track in &tracks {
                    let _ = crate::search::delete_from_search_index(
                        &tx,
                        "track",
                        &track.track_guid,
                        "",
                        &track.title,
                        track.description.as_deref().unwrap_or(""),
                        "",
                    ); // best-effort: index entry may not exist
                }
                // Remove the feed's search index entry.
                let _ = crate::search::delete_from_search_index(
                    &tx,
                    "feed",
                    &feed.feed_guid,
                    "",
                    &feed.title,
                    feed.description.as_deref().unwrap_or(""),
                    feed.raw_medium.as_deref().unwrap_or(""),
                );
                // Cascade-delete the feed and all child rows (using inner
                // _sql variant that works on &Connection within our tx).
                db::delete_feed_sql(&tx, &p.feed_guid)?;
            }
        }
        event::EventPayload::TrackRemoved(p) => {
            // Look up the track to get search-index fields. If already gone, no-op.
            let track_opt = db::get_track_by_guid(&tx, &p.track_guid)?;
            if let Some(track) = track_opt {
                // Remove the track's search index entry.
                let _ = crate::search::delete_from_search_index(
                    &tx,
                    "track",
                    &track.track_guid,
                    "",
                    &track.title,
                    track.description.as_deref().unwrap_or(""),
                    "",
                ); // best-effort
                // Cascade-delete the track and its child rows (using inner
                // _sql variant that works on &Connection within our tx).
                db::delete_track_sql(&tx, &p.track_guid)?;
            }
        }
        event::EventPayload::ArtistMerged(p) => {
            // Use inner _sql variant that works on &Connection within our tx.
            db::merge_artists_sql(
                &tx,
                &p.source_artist_id,
                &p.target_artist_id,
            )?;
        }
    }

    // Event row was already inserted at the top (dedup guard); now update
    // the sync cursor and commit the full transaction.
    db::upsert_node_sync_state(&tx, node_pubkey, ev.seq, now)?;
    tx.commit()?;
    Ok(ApplyOutcome::Applied(seq))
}

/// Helper: insert an artist credit and its names if they don't already exist.
fn upsert_artist_credit_if_absent(
    conn: &rusqlite::Connection,
    credit: &crate::model::ArtistCredit,
) -> Result<(), db::DbError> {
    conn.execute(
        "INSERT OR IGNORE INTO artist_credit (id, display_name, created_at) \
         VALUES (?1, ?2, ?3)",
        rusqlite::params![credit.id, credit.display_name, credit.created_at],
    )?;
    for acn in &credit.names {
        conn.execute(
            "INSERT OR IGNORE INTO artist_credit_name \
             (artist_credit_id, artist_id, position, name, join_phrase) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![acn.artist_credit_id, acn.artist_id, acn.position, acn.name, acn.join_phrase],
        )?;
    }
    Ok(())
}

// ── apply_events ─────────────────────────────────────────────────────────────

/// Verify and apply a batch of events to the local DB.
///
/// When `sse_registry` is `Some`, each successfully applied event is published
/// to the SSE broadcast channels for the relevant artist(s). This enables
/// community-node SSE clients to receive live events.
// Issue-SSE-PUBLISH — 2026-03-14
pub(crate) async fn apply_events(
    db:           db::Db,
    node_pubkey:  &str,
    events:       Vec<event::Event>,
    sse_registry: Option<&Arc<api::SseRegistry>>,
) -> ApplySummary {
    let mut summary = ApplySummary { applied: 0, duplicate: 0, rejected: 0, max_seq: 0 };
    let node_pubkey = node_pubkey.to_string();

    for ev in events {
        let seq      = ev.seq;
        let event_id = ev.event_id.clone();

        if let Err(e) = signing::verify_event_signature(&ev) {
            tracing::warn!(event_id = %event_id, seq, error = %e, "apply: invalid signature, skipping");
            summary.rejected += 1;
            continue;
        }

        // Issue-SSE-PUBLISH — 2026-03-14: clone event before moving into
        // spawn_blocking so we can publish to SSE after successful apply.
        let ev_for_sse = sse_registry.as_ref().map(|_| ev.clone());

        let db2    = Arc::clone(&db);
        let pk     = node_pubkey.clone();
        let result = tokio::task::spawn_blocking(move || apply_single_event(&db2, &pk, &ev))
            .await;

        match result {
            Err(panic_err) => {
                tracing::error!(event_id = %event_id, error = %panic_err, "apply: task panicked");
                summary.rejected += 1;
            }
            Ok(Err(db_err)) => {
                tracing::error!(event_id = %event_id, error = %db_err, "apply: DB error");
                summary.rejected += 1;
            }
            Ok(Ok(ApplyOutcome::Applied(s))) => {
                summary.applied += 1;
                if s > summary.max_seq {
                    summary.max_seq = s;
                }
                if seq > summary.max_seq {
                    summary.max_seq = seq;
                }

                // Issue-SSE-PUBLISH — 2026-03-14: publish to SSE after successful apply.
                if let (Some(registry), Some(ev_clone)) = (sse_registry, ev_for_sse) {
                    // Use the locally assigned seq (s) for the SSE frame, not
                    // the primary's seq, since SSE replay is local-DB-based.
                    let mut local_ev = ev_clone;
                    local_ev.seq = s;
                    api::publish_events_to_sse(registry, &[local_ev]);
                }
            }
            Ok(Ok(ApplyOutcome::Duplicate)) => {
                summary.duplicate += 1;
            }
        }
    }

    summary
}
