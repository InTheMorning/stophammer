// Schema gaps SG-01..SG-08 closed — 2026-03-13

mod common;

use rusqlite::params;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Seeds a feed. ADR 0034 §11: `feeds` carries no artist credit.
fn seed_feed(conn: &rusqlite::Connection) -> i64 {
    let now = stophammer::db::unix_now();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, \
         description, explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            "feed-sg",
            "https://example.com/sg-feed.xml",
            "SG Album",
            "sg album",
            "Schema gap test feed",
            0,
            0,
            now,
        ],
    )
    .expect("insert feed");
    now
}

/// Inserts a track. ADR 0034 §11: `tracks` carries no artist credit.
fn insert_track(conn: &rusqlite::Connection, track_guid: &str, feed_guid: &str, now: i64) {
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, title, title_lower, \
         description, explicit, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![
            track_guid,
            feed_guid,
            "SG Track",
            "sg track",
            "test track",
            0,
            now,
        ],
    )
    .expect("insert track");
}

// ===========================================================================
// ADR 0034 §11 — 2026-09-28: `artist_artist_rel` and its indexes, including
// `idx_aar_rel`, are dropped. `test_tag_fk_indexes_exist` is removed with
// the table.
// ===========================================================================
// SG-04: CHECK constraint on route_type columns
// ===========================================================================

#[test]
fn test_route_type_check_constraint_payment_routes() {
    let conn = common::test_db();
    let now = stophammer::db::unix_now();
    seed_feed(&conn);
    insert_track(&conn, "track-sg-rt", "feed-sg", now);

    let result = conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, route_type, address, split) \
         VALUES ('track-sg-rt', 'feed-sg', 'INVALID', 'addr', 100)",
        [],
    );
    assert!(
        result.is_err(),
        "payment_routes should reject invalid route_type"
    );
}

#[test]
fn test_route_type_check_constraint_feed_payment_routes() {
    let conn = common::test_db();
    seed_feed(&conn);

    let result = conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, route_type, address, split) \
         VALUES ('feed-sg', 'BOGUS', 'addr', 100)",
        [],
    );
    assert!(
        result.is_err(),
        "feed_payment_routes should reject invalid route_type"
    );
}

#[test]
fn test_route_type_check_accepts_valid_values() {
    let conn = common::test_db();
    let now = stophammer::db::unix_now();
    seed_feed(&conn);
    insert_track(&conn, "track-sg-valid", "feed-sg", now);

    for rt in &["node", "wallet", "keysend", "lnaddress"] {
        conn.execute(
            "INSERT INTO payment_routes (track_guid, feed_guid, route_type, address, split) \
             VALUES ('track-sg-valid', 'feed-sg', ?1, 'addr', 100)",
            params![rt],
        )
        .unwrap_or_else(|e| panic!("payment_routes should accept route_type={rt}: {e}"));
    }

    for rt in &["node", "wallet", "keysend", "lnaddress"] {
        conn.execute(
            "INSERT INTO feed_payment_routes (feed_guid, route_type, address, split) \
             VALUES ('feed-sg', ?1, 'addr', 100)",
            params![rt],
        )
        .unwrap_or_else(|e| panic!("feed_payment_routes should accept route_type={rt}: {e}"));
    }
}

// ===========================================================================
// SG-05: CHECK constraint on split columns (>= 0)
// ===========================================================================

#[test]
fn test_split_check_constraint_payment_routes() {
    let conn = common::test_db();
    let now = stophammer::db::unix_now();
    seed_feed(&conn);
    insert_track(&conn, "track-sg-sp", "feed-sg", now);

    let result = conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, route_type, address, split) \
         VALUES ('track-sg-sp', 'feed-sg', 'node', 'addr', -1)",
        [],
    );
    assert!(
        result.is_err(),
        "payment_routes should reject negative split"
    );
}

#[test]
fn test_split_check_constraint_feed_payment_routes() {
    let conn = common::test_db();
    seed_feed(&conn);

    let result = conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, route_type, address, split) \
         VALUES ('feed-sg', 'node', 'addr', -1)",
        [],
    );
    assert!(
        result.is_err(),
        "feed_payment_routes should reject negative split"
    );
}

#[test]
fn test_split_check_constraint_value_time_splits() {
    let conn = common::test_db();
    let now = stophammer::db::unix_now();
    seed_feed(&conn);
    insert_track(&conn, "track-sg-vts", "feed-sg", now);

    let result = conn.execute(
        "INSERT INTO value_time_splits (source_track_guid, start_time_secs, \
         remote_feed_guid, remote_item_guid, split, created_at) \
         VALUES ('track-sg-vts', 0, 'rfeed', 'ritem', -1, ?1)",
        params![now],
    );
    assert!(
        result.is_err(),
        "value_time_splits should reject negative split"
    );
}

// ===========================================================================
// SG-08: events.seq UNIQUE index
// ===========================================================================

#[test]
fn test_events_seq_unique_index_exists() {
    let conn = common::test_db();
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='idx_events_seq_unique'",
            [],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert!(
        exists,
        "missing UNIQUE index idx_events_seq_unique on events(seq)"
    );
}
