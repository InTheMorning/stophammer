//! Queue helpers for incremental resolver work.

/// Dirty mask for canonical release/recording state rebuilds.
pub const DIRTY_CANONICAL_STATE: i64 = crate::db::RESOLVER_DIRTY_CANONICAL_STATE;
/// Dirty mask for canonical promotion rows.
pub const DIRTY_CANONICAL_PROMOTIONS: i64 = crate::db::RESOLVER_DIRTY_CANONICAL_PROMOTIONS;
/// Dirty mask for canonical search rows.
pub const DIRTY_CANONICAL_SEARCH: i64 = crate::db::RESOLVER_DIRTY_CANONICAL_SEARCH;
/// Dirty mask reserved for incremental artist identity work.
pub const DIRTY_ARTIST_IDENTITY: i64 = crate::db::RESOLVER_DIRTY_ARTIST_IDENTITY;

/// Phase 1 queue mask: canonical derived state only.
pub const PHASE1_DIRTY_MASK: i64 =
    DIRTY_CANONICAL_STATE | DIRTY_CANONICAL_PROMOTIONS | DIRTY_CANONICAL_SEARCH;

/// Marks a feed dirty for phase 1 canonical resolver work.
///
/// # Errors
///
/// Returns [`crate::db::DbError`] if the queue upsert fails.
pub fn mark_feed_phase1_dirty(
    conn: &rusqlite::Connection,
    feed_guid: &str,
) -> Result<(), crate::db::DbError> {
    crate::db::mark_feed_dirty(conn, feed_guid, PHASE1_DIRTY_MASK)
}
