//! Background resolver worker.

use crate::{db, db_pool, signing};

/// Summary of one resolver batch.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResolverBatchResult {
    pub skipped_import_active: bool,
    pub stale_import_active_ignored: bool,
    pub claimed: usize,
    pub resolved: usize,
    pub failed: usize,
    pub canonical_state_events_emitted: usize,
    pub artist_seed_artists: usize,
    pub artist_candidate_groups: usize,
    pub artist_groups_processed: usize,
    pub artist_merges_applied: usize,
}

/// Runs one resolver batch against the queue.
///
/// # Errors
///
/// Returns [`db::DbError`] if queue coordination or resolution queries fail
/// before individual feed-level error handling can record failures.
pub fn run_batch(
    db_pool: &db_pool::DbPool,
    worker_id: &str,
    limit: i64,
) -> Result<ResolverBatchResult, db::DbError> {
    run_batch_with_signer(db_pool, worker_id, limit, None)
}

/// Runs one resolver batch and optionally emits primary-authored resolved-state
/// events when a signer is supplied.
///
/// # Errors
///
/// Returns [`db::DbError`] if queue coordination or resolution queries fail
/// before individual feed-level error handling can record failures.
pub fn run_batch_with_signer(
    db_pool: &db_pool::DbPool,
    worker_id: &str,
    limit: i64,
    signer: Option<&signing::NodeSigner>,
) -> Result<ResolverBatchResult, db::DbError> {
    let mut conn = db_pool
        .writer()
        .lock()
        .map_err(|_poison| db::DbError::Poisoned)?;
    let import_state = db::resolver_import_state(&conn)?;
    if import_state.active {
        return Ok(ResolverBatchResult {
            skipped_import_active: true,
            ..ResolverBatchResult::default()
        });
    }
    let stale_import_active_ignored = import_state.stale;

    let claimed = db::claim_dirty_feeds(&mut conn, worker_id, limit, db::unix_now())?;
    let mut result = ResolverBatchResult {
        stale_import_active_ignored,
        claimed: claimed.len(),
        ..ResolverBatchResult::default()
    };

    for entry in claimed {
        match resolve_feed(&mut conn, &entry.feed_guid, entry.dirty_mask, signer) {
            Ok(feed_result) => {
                db::complete_dirty_feed(&conn, &entry.feed_guid, worker_id)?;
                result.resolved += 1;
                result.canonical_state_events_emitted += feed_result.canonical_state_events_emitted;
                result.artist_seed_artists += feed_result.seed_artists;
                result.artist_candidate_groups += feed_result.candidate_groups;
                result.artist_groups_processed += feed_result.groups_processed;
                result.artist_merges_applied += feed_result.merges_applied;
            }
            Err(err) => {
                db::fail_dirty_feed(&conn, &entry.feed_guid, worker_id, &err.to_string())?;
                result.failed += 1;
            }
        }
    }

    Ok(result)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ResolveFeedResult {
    canonical_state_events_emitted: usize,
    seed_artists: usize,
    candidate_groups: usize,
    groups_processed: usize,
    merges_applied: usize,
}

fn resolve_feed(
    conn: &mut rusqlite::Connection,
    feed_guid: &str,
    dirty_mask: i64,
    signer: Option<&signing::NodeSigner>,
) -> Result<ResolveFeedResult, db::DbError> {
    // Resolver work is derived-state only. Source feed/track rows and staged
    // source claims remain the preserved authoritative layer.
    let mut result = ResolveFeedResult::default();
    if dirty_mask & crate::resolver::queue::DIRTY_SOURCE_READ_MODELS != 0 {
        db::sync_source_read_models_for_feed(conn, feed_guid)?;
    }
    if dirty_mask & crate::resolver::queue::DIRTY_CANONICAL_STATE != 0 {
        db::sync_canonical_state_for_feed(conn, feed_guid)?;
        if let Some(signer) = signer
            && db::emit_canonical_feed_state_event(conn, feed_guid, signer)?.is_some()
        {
            result.canonical_state_events_emitted += 1;
        }
    }
    if dirty_mask & crate::resolver::queue::DIRTY_CANONICAL_PROMOTIONS != 0 {
        db::sync_canonical_promotions_for_feed(conn, feed_guid)?;
    }
    if dirty_mask & crate::resolver::queue::DIRTY_CANONICAL_SEARCH != 0 {
        db::sync_canonical_search_index_for_feed(conn, feed_guid)?;
    }
    if dirty_mask & crate::resolver::queue::DIRTY_ARTIST_IDENTITY != 0 {
        let stats = db::resolve_artist_identity_for_feed(conn, feed_guid)?;
        result.seed_artists = stats.seed_artists;
        result.candidate_groups = stats.candidate_groups;
        result.groups_processed = stats.groups_processed;
        result.merges_applied = stats.merges_applied;
    }
    Ok(result)
}

/// Runs the resolver loop forever.
pub async fn run_forever(
    db_pool: db_pool::DbPool,
    interval_secs: u64,
    batch_size: i64,
    worker_id: String,
    signer: Option<std::sync::Arc<signing::NodeSigner>>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
    loop {
        interval.tick().await;
        match run_batch_with_signer(&db_pool, &worker_id, batch_size, signer.as_deref()) {
            Ok(summary) if summary.skipped_import_active => {
                tracing::info!("resolver: import_active=true, skipping batch");
            }
            Ok(summary) if summary.stale_import_active_ignored => {
                tracing::warn!(
                    claimed = summary.claimed,
                    resolved = summary.resolved,
                    failed = summary.failed,
                    canonical_state_events_emitted = summary.canonical_state_events_emitted,
                    "resolver: stale import_active heartbeat ignored"
                );
            }
            Ok(summary) if summary.claimed > 0 => {
                tracing::info!(
                    claimed = summary.claimed,
                    resolved = summary.resolved,
                    failed = summary.failed,
                    canonical_state_events_emitted = summary.canonical_state_events_emitted,
                    artist_seed_artists = summary.artist_seed_artists,
                    artist_candidate_groups = summary.artist_candidate_groups,
                    artist_groups_processed = summary.artist_groups_processed,
                    artist_merges_applied = summary.artist_merges_applied,
                    "resolver: completed batch"
                );
            }
            Ok(_summary) => {}
            Err(err) => tracing::error!(error = %err, "resolver: batch failed"),
        }
    }
}
