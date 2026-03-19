//! Background resolver worker.

use crate::{db, db_pool};

/// Summary of one resolver batch.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResolverBatchResult {
    pub skipped_import_active: bool,
    pub claimed: usize,
    pub resolved: usize,
    pub failed: usize,
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
    let mut conn = db_pool
        .writer()
        .lock()
        .map_err(|_poison| db::DbError::Poisoned)?;
    if db::resolver_import_active(&conn)? {
        return Ok(ResolverBatchResult {
            skipped_import_active: true,
            ..ResolverBatchResult::default()
        });
    }

    let claimed = db::claim_dirty_feeds(&mut conn, worker_id, limit, db::unix_now())?;
    let mut result = ResolverBatchResult {
        claimed: claimed.len(),
        ..ResolverBatchResult::default()
    };

    for entry in claimed {
        match resolve_feed(&mut conn, &entry.feed_guid, entry.dirty_mask) {
            Ok(()) => {
                db::complete_dirty_feed(&conn, &entry.feed_guid, worker_id)?;
                result.resolved += 1;
            }
            Err(err) => {
                db::fail_dirty_feed(&conn, &entry.feed_guid, worker_id, &err.to_string())?;
                result.failed += 1;
            }
        }
    }

    Ok(result)
}

fn resolve_feed(
    conn: &mut rusqlite::Connection,
    feed_guid: &str,
    dirty_mask: i64,
) -> Result<(), db::DbError> {
    if dirty_mask & crate::resolver::queue::DIRTY_CANONICAL_STATE != 0 {
        db::sync_canonical_state_for_feed(conn, feed_guid)?;
    }
    if dirty_mask & crate::resolver::queue::DIRTY_CANONICAL_PROMOTIONS != 0 {
        db::sync_canonical_promotions_for_feed(conn, feed_guid)?;
    }
    if dirty_mask & crate::resolver::queue::DIRTY_CANONICAL_SEARCH != 0 {
        db::sync_canonical_search_index_for_feed(conn, feed_guid)?;
    }
    if dirty_mask & crate::resolver::queue::DIRTY_ARTIST_IDENTITY != 0 {
        let _ = db::resolve_artist_identity_for_feed(conn, feed_guid)?;
    }
    Ok(())
}

/// Runs the resolver loop forever.
pub async fn run_forever(
    db_pool: db_pool::DbPool,
    interval_secs: u64,
    batch_size: i64,
    worker_id: String,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
    loop {
        interval.tick().await;
        match run_batch(&db_pool, &worker_id, batch_size) {
            Ok(summary) if summary.skipped_import_active => {
                tracing::info!("resolver: import_active=true, skipping batch");
            }
            Ok(summary) if summary.claimed > 0 => {
                tracing::info!(
                    claimed = summary.claimed,
                    resolved = summary.resolved,
                    failed = summary.failed,
                    "resolver: completed batch"
                );
            }
            Ok(_summary) => {}
            Err(err) => tracing::error!(error = %err, "resolver: batch failed"),
        }
    }
}
