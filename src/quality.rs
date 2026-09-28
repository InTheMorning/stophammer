//! Entity quality scoring.
//!
//! Quality scores are integers in the range 0–100 that reflect how complete
//! and rich an entity's metadata is. Higher scores indicate more fully
//! populated records. The scores are stored in `entity_quality` and can be
//! used to weight search results via the `search` module.

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::{self, DbError};

// SP-05 epoch guard — 2026-03-12
/// Returns the current unix timestamp in seconds.
fn unix_now() -> i64 {
    crate::db::unix_now()
}

/// Fields fetched from the `feeds` table for quality computation.
struct FeedFields {
    title: Option<String>,
    description: Option<String>,
    image_url: Option<String>,
    language: Option<String>,
    episode_count: i64,
    release_artist: Option<String>,
    newest_item_at: Option<i64>,
    explicit: i64,
    itunes_type: Option<String>,
}

/// Computes a quality score (0–100) for a feed based on field completeness.
///
/// Scoring breakdown:
/// - `title` present and non-empty: 10
/// - `description` present and non-empty: 15
/// - `image_url` present: 15
/// - `language` present: 5
/// - `episode_count` > 0: 5
/// - has tracks: 10
/// - has payment routes (feed-level or track-level): 15
/// - a non-empty `release_artist` (ADR 0034 §11): 10
/// - `newest_item_at` present: 5
/// - `explicit` explicitly set (non-zero): 5
/// - `itunes_type` present: 5
///
/// # Errors
///
/// Returns [`DbError`] if any SQL query fails.
pub fn compute_feed_quality(conn: &Connection, feed_guid: &str) -> Result<i64, DbError> {
    let mut score: i64 = 0;

    let row: Option<FeedFields> = conn
        .query_row(
            "SELECT title, description, image_url, language, episode_count, \
                release_artist, newest_item_at, explicit, itunes_type \
         FROM feeds WHERE feed_guid = ?1",
            params![feed_guid],
            |row| {
                Ok(FeedFields {
                    title: row.get(0)?,
                    description: row.get(1)?,
                    image_url: row.get(2)?,
                    language: row.get(3)?,
                    episode_count: row.get(4)?,
                    release_artist: row.get(5)?,
                    newest_item_at: row.get(6)?,
                    explicit: row.get(7)?,
                    itunes_type: row.get(8)?,
                })
            },
        )
        .optional()?;

    if let Some(f) = row {
        if f.title.as_ref().is_some_and(|t| !t.is_empty()) {
            score += 10;
        }
        if f.description.as_ref().is_some_and(|d| !d.is_empty()) {
            score += 15;
        }
        if f.image_url.is_some() {
            score += 15;
        }
        if f.language.is_some() {
            score += 5;
        }
        if f.episode_count > 0 {
            score += 5;
        }
        if f.release_artist.as_ref().is_some_and(|a| !a.is_empty()) {
            score += 10;
        }
        if f.newest_item_at.is_some() {
            score += 5;
        }
        if f.explicit != 0 {
            score += 5;
        }
        if f.itunes_type.is_some() {
            score += 5;
        }
    }

    // Has tracks?
    let track_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tracks WHERE feed_guid = ?1 LIMIT 1",
        params![feed_guid],
        |row| row.get(0),
    )?;
    if track_count > 0 {
        score += 10;
    }

    // Has payment routes? (feed-level or any track-level route for this feed)
    let has_routes: bool = conn.query_row(
        "SELECT EXISTS( \
             SELECT 1 FROM feed_payment_routes WHERE feed_guid = ?1 \
             UNION ALL \
             SELECT 1 FROM payment_routes WHERE feed_guid = ?1 \
             LIMIT 1 \
         )",
        params![feed_guid],
        |row| row.get(0),
    )?;
    if has_routes {
        score += 15;
    }

    Ok(score)
}

/// Fields fetched from the `tracks` table for quality computation.
struct TrackFields {
    title: Option<String>,
    enclosure_url: Option<String>,
    enclosure_type: Option<String>,
    duration_secs: Option<i64>,
    pub_date: Option<i64>,
    description: Option<String>,
    track_number: Option<i64>,
    season: Option<i64>,
    track_artist: Option<String>,
}

/// Computes a quality score (0–100) for a track based on field completeness.
///
/// Scoring breakdown:
/// - `title` present and non-empty: 10
/// - `enclosure_url` present: 15
/// - `enclosure_type` present: 5
/// - `duration_secs` > 0: 10
/// - `pub_date` present: 5
/// - `description` present and non-empty: 10
/// - a non-empty `track_artist`, or else a non-empty `release_artist` on the
///   track's feed (ADR 0034 §11): 5
/// - `track_number` present: 5
/// - `season` present: 5
/// - has payment routes: 20
/// - has value time splits: 10
///
/// # Errors
///
/// Returns [`DbError`] if any SQL query fails.
pub fn compute_track_quality_for_feed_track(
    conn: &Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<i64, DbError> {
    let mut score: i64 = 0;

    let row: Option<TrackFields> = conn
        .query_row(
            "SELECT title, enclosure_url, enclosure_type, duration_secs, pub_date, \
                description, track_number, season, track_artist \
         FROM tracks WHERE feed_guid = ?1 AND track_guid = ?2",
            params![feed_guid, track_guid],
            |row| {
                Ok(TrackFields {
                    title: row.get(0)?,
                    enclosure_url: row.get(1)?,
                    enclosure_type: row.get(2)?,
                    duration_secs: row.get(3)?,
                    pub_date: row.get(4)?,
                    description: row.get(5)?,
                    track_number: row.get(6)?,
                    season: row.get(7)?,
                    track_artist: row.get(8)?,
                })
            },
        )
        .optional()?;

    if let Some(t) = row {
        if t.title.as_ref().is_some_and(|s| !s.is_empty()) {
            score += 10;
        }
        if t.enclosure_url.is_some() {
            score += 15;
        }
        if t.enclosure_type.is_some() {
            score += 5;
        }
        if t.duration_secs.is_some_and(|d| d > 0) {
            score += 10;
        }
        if t.pub_date.is_some() {
            score += 5;
        }
        if t.description.as_ref().is_some_and(|s| !s.is_empty()) {
            score += 10;
        }
        let has_track_artist = t.track_artist.as_ref().is_some_and(|a| !a.is_empty());
        let has_feed_release_artist = if has_track_artist {
            false
        } else {
            let feed_release_artist: Option<String> = conn
                .query_row(
                    "SELECT release_artist FROM feeds WHERE feed_guid = ?1",
                    params![feed_guid],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            feed_release_artist.as_ref().is_some_and(|a| !a.is_empty())
        };
        if has_track_artist || has_feed_release_artist {
            score += 5;
        }
        if t.track_number.is_some() {
            score += 5;
        }
        if t.season.is_some() {
            score += 5;
        }
    }

    // Has payment routes?
    let has_routes: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM payment_routes WHERE feed_guid = ?1 AND track_guid = ?2)",
        params![feed_guid, track_guid],
        |row| row.get(0),
    )?;
    if has_routes {
        score += 20;
    }

    // Has value time splits?
    let has_vts: bool = conn.query_row(
        "SELECT EXISTS( \
             SELECT 1 FROM value_time_splits \
             WHERE source_track_guid = ?2 AND (source_feed_guid = ?1 OR source_feed_guid IS NULL) \
         )",
        params![feed_guid, track_guid],
        |row| row.get(0),
    )?;
    if has_vts {
        score += 10;
    }

    Ok(score)
}

/// Compatibility wrapper that resolves a raw `track_guid` when it is unique.
///
/// # Errors
///
/// Returns [`DbError`] if the lookup fails or the raw `track_guid` is
/// ambiguous across feeds.
pub fn compute_track_quality(conn: &Connection, track_guid: &str) -> Result<i64, DbError> {
    let tracks = db::get_tracks_by_guid(conn, track_guid)?;
    match tracks.as_slice() {
        [] => Ok(0),
        [track] => compute_track_quality_for_feed_track(conn, &track.feed_guid, &track.track_guid),
        _ => Err(DbError::Other(format!(
            "track_guid {track_guid} is ambiguous; compute quality with feed scope"
        ))),
    }
}

/// Upserts a quality score into the `entity_quality` table.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL upsert fails.
pub fn store_quality(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    score: i64,
) -> Result<(), DbError> {
    let now = unix_now();

    conn.execute(
        "INSERT INTO entity_quality (entity_type, entity_id, score, computed_at) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(entity_type, entity_id) \
         DO UPDATE SET score = excluded.score, computed_at = excluded.computed_at",
        params![entity_type, entity_id, score, now],
    )?;

    Ok(())
}

/// Returns the quality score for an entity, defaulting to 0 if none is stored.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_quality(conn: &Connection, entity_type: &str, entity_id: &str) -> Result<i64, DbError> {
    let score: Option<i64> = conn
        .query_row(
            "SELECT score FROM entity_quality \
         WHERE entity_type = ?1 AND entity_id = ?2",
            params![entity_type, entity_id],
            |row| row.get(0),
        )
        .optional()?;

    Ok(score.unwrap_or(0))
}
