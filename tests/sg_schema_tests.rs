// Schema gaps SG-01..SG-08 closed — 2026-03-13

mod common;

use rusqlite::params;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn seed_feed(conn: &rusqlite::Connection) -> (i64, i64) {
    let now = stophammer::db::unix_now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["artist-sg", "SG Artist", "sg artist", now, now],
    )
    .expect("insert artist");
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        params!["SG Artist", now],
    )
    .expect("insert artist_credit");
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![credit_id, "artist-sg", 0, "SG Artist", ""],
    )
    .expect("insert artist_credit_name");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         description, explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            "feed-sg",
            "https://example.com/sg-feed.xml",
            "SG Album",
            "sg album",
            credit_id,
            "Schema gap test feed",
            0,
            0,
            now,
            now,
        ],
    )
    .expect("insert feed");
    (credit_id, now)
}

fn insert_track(
    conn: &rusqlite::Connection,
    track_guid: &str,
    feed_guid: &str,
    credit_id: i64,
    now: i64,
) {
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         description, explicit, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            track_guid,
            feed_guid,
            credit_id,
            "SG Track",
            "sg track",
            "test track",
            0,
            now,
            now,
        ],
    )
    .expect("insert track");
}

// ===========================================================================
// SG-01/02/03: FK indexes on tag and relationship tables
// ===========================================================================

#[test]
fn test_tag_fk_indexes_exist() {
    let conn = common::test_db();
    let expected = [
        "idx_artist_tag_tag",
        "idx_feed_tag_tag",
        "idx_track_tag_tag",
        "idx_aar_rel",
    ];
    for name in &expected {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name=?1",
                params![name],
                |row| row.get(0),
            )
            .expect("query sqlite_master");
        assert!(exists, "missing index: {name}");
    }
}

// ===========================================================================
// SG-04: CHECK constraint on route_type columns
// ===========================================================================

#[test]
fn test_route_type_check_constraint_payment_routes() {
    let conn = common::test_db();
    let now = stophammer::db::unix_now();
    seed_feed(&conn);
    insert_track(&conn, "track-sg-rt", "feed-sg", 1, now);

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
    let (credit_id, _) = seed_feed(&conn);
    insert_track(&conn, "track-sg-valid", "feed-sg", credit_id, now);

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
    let (credit_id, _) = seed_feed(&conn);
    insert_track(&conn, "track-sg-sp", "feed-sg", credit_id, now);

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
    let (credit_id, _) = seed_feed(&conn);
    insert_track(&conn, "track-sg-vts", "feed-sg", credit_id, now);

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
// SG-07: proof_tokens and proof_challenges cleaned on feed delete
// ===========================================================================

#[test]
fn test_feed_delete_cleans_proof_tokens() {
    let mut conn = common::test_db();
    seed_feed(&conn);

    // Insert a proof token for this feed
    conn.execute(
        "INSERT INTO proof_tokens (access_token, scope, subject_feed_guid, expires_at, created_at) \
         VALUES ('tok-sg', 'feed:write', 'feed-sg', 9999999999, 1000000)",
        [],
    )
    .expect("insert proof token");

    stophammer::db::delete_feed(&mut conn, "feed-sg").expect("delete feed");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM proof_tokens WHERE subject_feed_guid = 'feed-sg'",
            [],
            |row| row.get(0),
        )
        .expect("count tokens");
    assert_eq!(count, 0, "proof_tokens should be cleaned up on feed delete");
}

#[test]
fn test_feed_delete_cleans_proof_challenges() {
    let mut conn = common::test_db();
    seed_feed(&conn);

    conn.execute(
        "INSERT INTO proof_challenges (challenge_id, feed_guid, scope, token_binding, state, expires_at, created_at) \
         VALUES ('ch-sg', 'feed-sg', 'feed:write', 'binding', 'pending', 9999999999, 1000000)",
        [],
    )
    .expect("insert proof challenge");

    stophammer::db::delete_feed(&mut conn, "feed-sg").expect("delete feed");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM proof_challenges WHERE feed_guid = 'feed-sg'",
            [],
            |row| row.get(0),
        )
        .expect("count challenges");
    assert_eq!(
        count, 0,
        "proof_challenges should be cleaned up on feed delete"
    );
}

#[test]
fn test_feed_delete_with_event_cleans_proof_tokens() {
    let mut conn = common::test_db();
    seed_feed(&conn);

    conn.execute(
        "INSERT INTO proof_tokens (access_token, scope, subject_feed_guid, expires_at, created_at) \
         VALUES ('tok-sg-ev', 'feed:write', 'feed-sg', 9999999999, 1000000)",
        [],
    )
    .expect("insert proof token");

    let now = stophammer::db::unix_now();
    // Issue-SEQ-INTEGRITY — 2026-03-14: pass signer instead of signed_by/signature.
    let signer = common::temp_signer("sg-schema-test");
    stophammer::db::delete_feed_with_event(
        &mut conn,
        "feed-sg",
        "evt-sg-cleanup",
        r#"{"feed_guid":"feed-sg"}"#,
        "feed-sg",
        &signer,
        now,
        &[],
    )
    .expect("delete feed with event");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM proof_tokens WHERE subject_feed_guid = 'feed-sg'",
            [],
            |row| row.get(0),
        )
        .expect("count tokens");
    assert_eq!(
        count, 0,
        "proof_tokens should be cleaned up on delete_feed_with_event"
    );
}

#[test]
fn test_feed_delete_with_event_cleans_proof_challenges() {
    let mut conn = common::test_db();
    seed_feed(&conn);

    conn.execute(
        "INSERT INTO proof_challenges (challenge_id, feed_guid, scope, token_binding, state, expires_at, created_at) \
         VALUES ('ch-sg-ev', 'feed-sg', 'feed:write', 'binding', 'pending', 9999999999, 1000000)",
        [],
    )
    .expect("insert proof challenge");

    let now = stophammer::db::unix_now();
    // Issue-SEQ-INTEGRITY — 2026-03-14: pass signer instead of signed_by/signature.
    let signer = common::temp_signer("sg-schema-test2");
    stophammer::db::delete_feed_with_event(
        &mut conn,
        "feed-sg",
        "evt-sg-cleanup2",
        r#"{"feed_guid":"feed-sg"}"#,
        "feed-sg",
        &signer,
        now,
        &[],
    )
    .expect("delete feed with event");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM proof_challenges WHERE feed_guid = 'feed-sg'",
            [],
            |row| row.get(0),
        )
        .expect("count challenges");
    assert_eq!(
        count, 0,
        "proof_challenges should be cleaned up on delete_feed_with_event"
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
