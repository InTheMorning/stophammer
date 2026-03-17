mod common;

use rusqlite::params;
use stophammer::quality;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Insert prerequisite entities (artist, credit, `credit_name`, feed) and
/// return the `feed_guid`.
fn setup_feed(conn: &rusqlite::Connection) -> String {
    let now = common::now();

    conn.execute(
        "INSERT OR IGNORE INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["artist-1", "Test Artist", "test artist", now, now],
    )
    .unwrap();

    // A "null" credit with id=0 so tracks that should not earn the author
    // credit score can reference a valid FK while having artist_credit_id = 0.
    conn.execute(
        "INSERT OR IGNORE INTO artist_credit (id, display_name, created_at) VALUES (?1, ?2, ?3)",
        params![0, "", now],
    )
    .unwrap();

    conn.execute(
        "INSERT OR IGNORE INTO artist_credit (id, display_name, created_at) VALUES (?1, ?2, ?3)",
        params![1, "Test Artist", now],
    )
    .unwrap();

    conn.execute(
        "INSERT OR IGNORE INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![1, "artist-1", 0, "Test Artist", ""],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params!["feed-1", "https://example.com/feed.xml", "Test Feed", "test feed", 1, 0, now, now],
    )
    .unwrap();

    "feed-1".to_string()
}

// ---------------------------------------------------------------------------
// 1. test_compute_track_quality_all_fields — all fields + routes + VTS = 100
// ---------------------------------------------------------------------------

#[test]
fn test_compute_track_quality_all_fields() {
    let conn = common::test_db();
    let now = common::now();
    setup_feed(&conn);

    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, duration_secs, \
         enclosure_url, enclosure_type, enclosure_bytes, track_number, season, explicit, description, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params!["track-1", "feed-1", 1, "Test Track", "test track", now, 180,
                "https://example.com/track.mp3", "audio/mpeg", 5_000_000, 1, 1, 0, "A great track", now, now],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, route_type, address, split) VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["track-1", "feed-1", "node", "abc123", 100],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO value_time_splits (source_track_guid, start_time_secs, remote_feed_guid, remote_item_guid, split, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params!["track-1", 0, "remote-feed", "remote-item", 50, now],
    )
    .unwrap();

    let score = quality::compute_track_quality(&conn, "track-1").unwrap();
    assert_eq!(
        score, 100,
        "fully populated track with routes and VTS should score 100"
    );
}

// ---------------------------------------------------------------------------
// 2. test_compute_track_quality_minimal — only title + enclosure_url = 25
// ---------------------------------------------------------------------------

#[test]
fn test_compute_track_quality_minimal() {
    let conn = common::test_db();
    let now = common::now();
    setup_feed(&conn);

    // Insert track with only title and enclosure_url; artist_credit_id = 0
    // so the author credit check does not fire.
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         enclosure_url, explicit, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            "track-min",
            "feed-1",
            0,
            "Minimal Track",
            "minimal track",
            "https://example.com/min.mp3",
            0,
            now,
            now
        ],
    )
    .unwrap();

    let score = quality::compute_track_quality(&conn, "track-min").unwrap();
    // title (10) + enclosure_url (15) = 25
    assert_eq!(
        score, 25,
        "track with only title and enclosure_url should score 25"
    );
}

// ---------------------------------------------------------------------------
// 3. test_compute_track_quality_with_routes_no_vts — most fields + routes,
//    no VTS = 90
// ---------------------------------------------------------------------------

#[test]
fn test_compute_track_quality_with_routes_no_vts() {
    let conn = common::test_db();
    let now = common::now();
    setup_feed(&conn);

    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, duration_secs, \
         enclosure_url, enclosure_type, enclosure_bytes, track_number, season, explicit, description, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params!["track-routes", "feed-1", 1, "Routes Track", "routes track", now, 240,
                "https://example.com/routes.mp3", "audio/mpeg", 6_000_000, 2, 1, 0, "Track with routes", now, now],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, route_type, address, split) VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["track-routes", "feed-1", "node", "def456", 100],
    )
    .unwrap();

    let score = quality::compute_track_quality(&conn, "track-routes").unwrap();
    // title(10) + enclosure_url(15) + enclosure_type(5) + duration(10) + pub_date(5) +
    // description(10) + artist_credit(5) + track_number(5) + season(5) + routes(20) = 90
    assert_eq!(
        score, 90,
        "track with all fields and routes but no VTS should score 90"
    );
}

// ---------------------------------------------------------------------------
// 4. test_compute_track_quality_nonexistent — missing track_guid = 0
// ---------------------------------------------------------------------------

#[test]
fn test_compute_track_quality_nonexistent() {
    let conn = common::test_db();

    let score = quality::compute_track_quality(&conn, "does-not-exist").unwrap();
    assert_eq!(score, 0, "nonexistent track should score 0");
}

// ---------------------------------------------------------------------------
// 5. test_compute_feed_quality — insert feed + track + routes, verify score
// ---------------------------------------------------------------------------

#[test]
fn test_compute_feed_quality() {
    let conn = common::test_db();
    let now = common::now();
    setup_feed(&conn);

    // Update the feed to have richer metadata for a higher score.
    conn.execute(
        "UPDATE feeds SET description = ?1, image_url = ?2, language = ?3, \
         episode_count = ?4, newest_item_at = ?5, explicit = ?6, itunes_type = ?7 \
         WHERE feed_guid = ?8",
        params![
            "A test feed",
            "https://example.com/img.jpg",
            "en",
            5,
            now,
            1,
            "music",
            "feed-1"
        ],
    )
    .unwrap();

    // Insert a track so has_tracks fires.
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params!["track-feed", "feed-1", 1, "Feed Track", "feed track", 0, now, now],
    )
    .unwrap();

    // Insert a payment route so has_routes fires.
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, route_type, address, split) VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["track-feed", "feed-1", "node", "ghi789", 100],
    )
    .unwrap();

    let score = quality::compute_feed_quality(&conn, "feed-1").unwrap();
    // title(10) + description(15) + image_url(15) + language(5) + episode_count(5) +
    // artist_credit(10) + newest_item_at(5) + explicit(5) + itunes_type(5) +
    // has_tracks(10) + has_routes(15) = 100
    assert_eq!(score, 100, "fully populated feed should score 100");
}

// ---------------------------------------------------------------------------
// 6. test_store_and_get_quality — store_quality + get_quality round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_store_and_get_quality() {
    let conn = common::test_db();

    // Before storing, score should default to 0.
    let before = quality::get_quality(&conn, "track", "track-99").unwrap();
    assert_eq!(before, 0, "missing entity should return score 0");

    // Store a quality score.
    quality::store_quality(&conn, "track", "track-99", 85).unwrap();
    let after = quality::get_quality(&conn, "track", "track-99").unwrap();
    assert_eq!(after, 85, "stored score should round-trip");

    // Update (upsert) the score.
    quality::store_quality(&conn, "track", "track-99", 92).unwrap();
    let updated = quality::get_quality(&conn, "track", "track-99").unwrap();
    assert_eq!(updated, 92, "upserted score should reflect the new value");
}
