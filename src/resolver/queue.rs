//! Queue helpers for incremental resolver work.

/// Dirty mask for canonical release/recording state rebuilds.
pub const DIRTY_CANONICAL_STATE: i64 = crate::db::RESOLVER_DIRTY_CANONICAL_STATE;
/// Dirty mask for canonical promotion rows.
pub const DIRTY_CANONICAL_PROMOTIONS: i64 = crate::db::RESOLVER_DIRTY_CANONICAL_PROMOTIONS;
/// Dirty mask for canonical search rows.
pub const DIRTY_CANONICAL_SEARCH: i64 = crate::db::RESOLVER_DIRTY_CANONICAL_SEARCH;
/// Dirty mask reserved for incremental artist identity work.
pub const DIRTY_ARTIST_IDENTITY: i64 = crate::db::RESOLVER_DIRTY_ARTIST_IDENTITY;
/// Dirty mask for source-layer search and quality read models.
pub const DIRTY_SOURCE_READ_MODELS: i64 = crate::db::RESOLVER_DIRTY_SOURCE_READ_MODELS;

/// Dirty mask for canonical derived state only.
pub const CANONICAL_DIRTY_MASK: i64 =
    DIRTY_CANONICAL_STATE | DIRTY_CANONICAL_PROMOTIONS | DIRTY_CANONICAL_SEARCH;
/// Dirty mask for source feed/track search and quality rows.
pub const SOURCE_READ_MODEL_DIRTY_MASK: i64 = DIRTY_SOURCE_READ_MODELS;
/// Default dirty mask for normal write paths.
pub const DEFAULT_DIRTY_MASK: i64 =
    SOURCE_READ_MODEL_DIRTY_MASK | CANONICAL_DIRTY_MASK | DIRTY_ARTIST_IDENTITY;

/// Marks a feed dirty for normal resolver work.
///
/// # Errors
///
/// Returns [`crate::db::DbError`] if the queue upsert fails.
pub fn mark_feed_dirty_for_resolver(
    conn: &rusqlite::Connection,
    feed_guid: &str,
) -> Result<(), crate::db::DbError> {
    crate::db::mark_feed_dirty(conn, feed_guid, DEFAULT_DIRTY_MASK)
}
