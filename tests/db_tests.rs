#![allow(
    clippy::too_many_lines,
    reason = "db regression tests inline full fixture setup and assertions for determinism"
)]

mod common;

use rusqlite::params;

/// Inserts a feed and one track. ADR 0034 §11: `feeds` and `tracks` carry no
/// artist credit.
fn seed_feed_with_track(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    track_guid: &str,
    title: &str,
) {
    let now = common::now();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![
            feed_guid,
            format!("https://example.com/{feed_guid}.xml"),
            title,
            title.to_lowercase(),
            now
        ],
    )
    .expect("insert feed");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, title, title_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![track_guid, feed_guid, title, title.to_lowercase(), now],
    )
    .expect("insert track");
}

// ---------------------------------------------------------------------------
// 1. Schema creation on fresh :memory: DB
// ---------------------------------------------------------------------------

#[test]
fn schema_creates_all_tables() {
    let conn = common::test_db();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap();
    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    // Dead schema removed — 2026-03-13: feed_type, artist_location, manifest_source
    // ADR 0034 §11 — 2026-09-28: the artist credit, resolver and review
    // tables are gone. `feeds` and `tracks` carry no artist credit.
    let expected = [
        "entity_quality",
        "events",
        "feed_crawl_cache",
        "feed_payment_routes",
        "feed_remote_items_raw",
        "feeds",
        "live_events",
        "node_sync_state",
        "payment_routes",
        "peer_nodes",
        "schema_migrations",
        "search_index",
        "search_entities",
        "source_contributor_claims",
        "source_entity_links",
        "source_entity_ids",
        "source_item_enclosures",
        "source_platform_claims",
        "source_release_claims",
        "tracks",
        "value_time_splits",
    ];
    for name in &expected {
        assert!(tables.contains(&name.to_string()), "missing table: {name}");
    }
}

// ---------------------------------------------------------------------------
// 2. Lookup table seeding
// ---------------------------------------------------------------------------
// ADR 0034 §11 — 2026-09-28: `artist_type` and `rel_type` are dropped. The
// lookup-table seeding test that read them is removed with them.

// ---------------------------------------------------------------------------
// 3. Schema idempotency (via migration system)
// ---------------------------------------------------------------------------

#[test]
fn schema_idempotent() {
    // Opening the same database file twice must not error; the migration
    // system should detect that all migrations are already applied and
    // skip them.
    let tmp = std::env::temp_dir().join("stophammer_db_test_idem.db");
    let _ = std::fs::remove_file(&tmp); // clean slate
    let conn = stophammer::db::open_db(&tmp);

    // Migration count should be stable after first open.
    let migration_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
        .unwrap();
    assert!(
        migration_count > 0,
        "expected at least one recorded migration"
    );

    drop(conn);

    // Second open — migrations must be skipped, count unchanged.
    let conn2 = stophammer::db::open_db(&tmp);
    let migration_count2: i64 = conn2
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        migration_count, migration_count2,
        "a second open must not re-apply or duplicate a migration row"
    );

    drop(conn2);
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn direct_feed_delete_cleans_legacy_child_rows() {
    let conn = common::test_db();
    let now = common::now();

    seed_feed_with_track(&conn, "feed-delete-a", "track-delete-a", "Delete A");
    seed_feed_with_track(&conn, "feed-delete-b", "track-delete-b", "Delete B");
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, address, route_type, split) VALUES ('feed-delete-a', 'node://feed', 'node', 100)",
        [],
    )
    .expect("insert feed route");
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, address, route_type, split) VALUES ('track-delete-a', 'feed-delete-a', 'node://track', 'node', 100)",
        [],
    )
    .expect("insert track route");
    conn.execute(
        "INSERT INTO value_time_splits (source_track_guid, start_time_secs, remote_feed_guid, remote_item_guid, split, created_at) \
         VALUES ('track-delete-a', 0, 'remote-feed', 'remote-item', 100, ?1)",
        params![now],
    )
    .expect("insert value time split");
    conn.execute(
        "INSERT INTO feed_remote_items_raw (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
         VALUES ('feed-delete-a', 0, 'music', 'remote-feed', NULL, 'podcast_remote_item')",
        [],
    )
    .expect("insert remote item");
    conn.execute(
        "INSERT INTO source_entity_ids (feed_guid, entity_type, entity_id, scheme, value, source, extraction_path, observed_at) \
         VALUES ('feed-delete-a', 'feed', 'feed-delete-a', 'guid', 'src', 'test', '/feed', ?1)",
        params![now],
    )
    .expect("insert source entity id");
    conn.execute("DELETE FROM feeds WHERE feed_guid = 'feed-delete-a'", [])
        .expect("direct feed delete should succeed");

    for (table, predicate) in [
        ("feeds", "feed_guid = 'feed-delete-a'"),
        ("tracks", "feed_guid = 'feed-delete-a'"),
        ("feed_payment_routes", "feed_guid = 'feed-delete-a'"),
        ("payment_routes", "track_guid = 'track-delete-a'"),
        ("value_time_splits", "source_track_guid = 'track-delete-a'"),
        ("feed_remote_items_raw", "feed_guid = 'feed-delete-a'"),
        ("source_entity_ids", "feed_guid = 'feed-delete-a'"),
    ] {
        let query = format!("SELECT COUNT(*) FROM {table} WHERE {predicate}");
        let count: i64 = conn
            .query_row(&query, [], |row| row.get(0))
            .expect("count child rows");
        assert_eq!(
            count, 0,
            "{table} rows should be cleaned on direct feed delete"
        );
    }
}

#[test]
fn direct_track_delete_cleans_legacy_child_rows() {
    let conn = common::test_db();
    let now = common::now();

    seed_feed_with_track(
        &conn,
        "feed-track-delete",
        "track-track-delete",
        "Track Delete",
    );
    seed_feed_with_track(
        &conn,
        "feed-track-delete-b",
        "track-track-delete-b",
        "Track Delete B",
    );
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, address, route_type, split) VALUES ('track-track-delete', 'feed-track-delete', 'node://track', 'node', 100)",
        [],
    )
    .expect("insert payment route");
    conn.execute(
        "INSERT INTO value_time_splits (source_track_guid, start_time_secs, remote_feed_guid, remote_item_guid, split, created_at) \
         VALUES ('track-track-delete', 0, 'remote-feed', 'remote-item', 100, ?1)",
        params![now],
    )
    .expect("insert value time split");

    conn.execute(
        "DELETE FROM tracks WHERE track_guid = 'track-track-delete'",
        [],
    )
    .expect("direct track delete should succeed");

    for table in ["payment_routes", "value_time_splits"] {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count child rows");
        assert_eq!(
            count, 0,
            "{table} rows should be cleaned on direct track delete"
        );
    }
}

// ADR 0034 §11 — 2026-09-28: `artist_credit` is dropped from the current
// schema. `null_scoped_artist_credit_dedup_reuses_existing_row` tested the
// same unique index that `migrations_dedup_legacy_null_scoped_artist_credits`
// below still covers, by replaying the historical migration files. The
// current-schema copy of the test is removed with the table.

#[test]
fn migrations_dedup_legacy_null_scoped_artist_credits() {
    let tmp = std::env::temp_dir().join("stophammer_artist_credit_null_scope.db");
    let _ = std::fs::remove_file(&tmp);
    let conn = rusqlite::Connection::open(&tmp).expect("open sqlite");
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;",
    )
    .expect("set pragmas");

    for sql in [
        include_str!("../migrations/0001_baseline.sql"),
        include_str!("../migrations/0002_artist_credit_feed_scope.sql"),
        include_str!("../migrations/0003_search_entities_unique.sql"),
        include_str!("../migrations/0004_proof_level.sql"),
        include_str!("../migrations/0005_live_events_and_remote_items.sql"),
        include_str!("../migrations/0006_source_claim_staging.sql"),
        include_str!("../migrations/0007_source_link_and_release_claims.sql"),
        include_str!("../migrations/0008_source_contributor_role_norm.sql"),
        include_str!("../migrations/0009_source_item_enclosures.sql"),
        include_str!("../migrations/0010_source_platform_claims.sql"),
        include_str!("../migrations/0011_canonical_release_recording.sql"),
        include_str!("../migrations/0012_resolver_queue.sql"),
        include_str!("../migrations/0013_artist_identity_reviews.sql"),
        include_str!("../migrations/0014_resolved_overlay_tables.sql"),
        include_str!("../migrations/0015_live_events_feed_scoped_key.sql"),
        include_str!("../migrations/0019_feed_delete_cleanup_triggers.sql"),
    ] {
        conn.execute_batch(sql).expect("apply migration");
    }

    let now = common::now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) VALUES ('artist-legacy-a', 'Legacy A', 'legacy a', ?1, ?1)",
        params![now],
    )
    .expect("insert artist a");
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) VALUES ('artist-legacy-b', 'Legacy B', 'legacy b', ?1, ?1)",
        params![now],
    )
    .expect("insert artist b");
    conn.execute(
        "INSERT INTO artist_credit (id, display_name, created_at, feed_guid) VALUES (100, 'Legacy Artist', ?1, NULL)",
        params![now],
    )
    .expect("insert credit 100");
    conn.execute(
        "INSERT INTO artist_credit (id, display_name, created_at, feed_guid) VALUES (101, 'Legacy Artist', ?1, NULL)",
        params![now + 1],
    )
    .expect("insert credit 101");
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) VALUES (100, 'artist-legacy-a', 0, 'Legacy A', '')",
        [],
    )
    .expect("insert credit name a");
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) VALUES (101, 'artist-legacy-b', 0, 'Legacy B', '')",
        [],
    )
    .expect("insert credit name b");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) VALUES ('feed-legacy-credit', 'https://example.com/legacy-credit.xml', 'Legacy Credit', 'legacy credit', 101, ?1, ?1)",
        params![now],
    )
    .expect("insert feed referencing duplicate credit");

    conn.execute_batch(include_str!(
        "../migrations/0020_artist_credit_null_scope_dedup.sql"
    ))
    .expect("apply dedup migration");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artist_credit WHERE display_name = 'Legacy Artist' AND feed_guid IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("count deduped credits");
    assert_eq!(
        count, 1,
        "migration should dedupe legacy null-scoped artist credits"
    );

    let feed_credit_id: i64 = conn
        .query_row(
            "SELECT artist_credit_id FROM feeds WHERE feed_guid = 'feed-legacy-credit'",
            [],
            |row| row.get(0),
        )
        .expect("load repointed feed credit");
    assert_eq!(
        feed_credit_id, 100,
        "references should be repointed to canonical artist credit"
    );

    let name_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artist_credit_name WHERE artist_credit_id = 100",
            [],
            |row| row.get(0),
        )
        .expect("count merged credit names");
    assert_eq!(
        name_count, 1,
        "legacy duplicate names at the same position collapse safely"
    );

    drop(conn);
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn route_writes_normalize_empty_custom_fields_but_reads_stay_none() {
    let conn = common::test_db();
    seed_feed_with_track(
        &conn,
        "feed-route-normalize",
        "track-route-normalize",
        "Route Normalize",
    );

    let track_route = stophammer::model::PaymentRoute {
        id: None,
        track_guid: "track-route-normalize".into(),
        feed_guid: "feed-route-normalize".into(),
        recipient_name: Some("Track Route".into()),
        route_type: stophammer::model::RouteType::Node,
        address: "node://track".into(),
        custom_key: None,
        custom_value: None,
        split: 100,
        fee: false,
    };
    stophammer::db::replace_payment_routes(&conn, "track-route-normalize", &[track_route])
        .expect("replace track routes");

    let feed_route = stophammer::model::FeedPaymentRoute {
        id: None,
        feed_guid: "feed-route-normalize".into(),
        recipient_name: Some("Feed Route".into()),
        route_type: stophammer::model::RouteType::Node,
        address: "node://feed".into(),
        custom_key: None,
        custom_value: None,
        split: 100,
        fee: false,
    };
    stophammer::db::replace_feed_payment_routes(&conn, "feed-route-normalize", &[feed_route])
        .expect("replace feed routes");

    let raw_track: (String, String) = conn
        .query_row(
            "SELECT custom_key, custom_value FROM payment_routes WHERE track_guid = 'track-route-normalize'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load raw stored track route fields");
    assert_eq!(raw_track, (String::new(), String::new()));

    let raw_feed: (String, String) = conn
        .query_row(
            "SELECT custom_key, custom_value FROM feed_payment_routes WHERE feed_guid = 'feed-route-normalize'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load raw stored feed route fields");
    assert_eq!(raw_feed, (String::new(), String::new()));

    let stored_track = stophammer::db::get_payment_routes_for_track(&conn, "track-route-normalize")
        .expect("load normalized track routes");
    assert_eq!(stored_track.len(), 1);
    assert_eq!(stored_track[0].custom_key, None);
    assert_eq!(stored_track[0].custom_value, None);

    let stored_feed =
        stophammer::db::get_feed_payment_routes_for_feed(&conn, "feed-route-normalize")
            .expect("load normalized feed routes");
    assert_eq!(stored_feed.len(), 1);
    assert_eq!(stored_feed[0].custom_key, None);
    assert_eq!(stored_feed[0].custom_value, None);
}

#[test]
fn migration_normalizes_legacy_route_null_custom_fields() {
    let tmp = std::env::temp_dir().join("stophammer_route_custom_normalization.db");
    let _ = std::fs::remove_file(&tmp);
    let conn = rusqlite::Connection::open(&tmp).expect("open sqlite");
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;",
    )
    .expect("set pragmas");

    for sql in [
        include_str!("../migrations/0001_baseline.sql"),
        include_str!("../migrations/0002_artist_credit_feed_scope.sql"),
        include_str!("../migrations/0003_search_entities_unique.sql"),
        include_str!("../migrations/0004_proof_level.sql"),
        include_str!("../migrations/0005_live_events_and_remote_items.sql"),
        include_str!("../migrations/0006_source_claim_staging.sql"),
        include_str!("../migrations/0007_source_link_and_release_claims.sql"),
        include_str!("../migrations/0008_source_contributor_role_norm.sql"),
        include_str!("../migrations/0009_source_item_enclosures.sql"),
        include_str!("../migrations/0010_source_platform_claims.sql"),
        include_str!("../migrations/0011_canonical_release_recording.sql"),
        include_str!("../migrations/0012_resolver_queue.sql"),
        include_str!("../migrations/0013_artist_identity_reviews.sql"),
        include_str!("../migrations/0014_resolved_overlay_tables.sql"),
        include_str!("../migrations/0015_live_events_feed_scoped_key.sql"),
        include_str!("../migrations/0019_feed_delete_cleanup_triggers.sql"),
        include_str!("../migrations/0020_artist_credit_null_scope_dedup.sql"),
    ] {
        conn.execute_batch(sql).expect("apply migration");
    }

    let now = common::now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) VALUES ('artist-route-null', 'Route Null', 'route null', ?1, ?1)",
        params![now],
    )
    .expect("insert artist");
    conn.execute(
        "INSERT INTO artist_credit (id, display_name, created_at, feed_guid) VALUES (200, 'Route Null', ?1, 'feed-route-null')",
        params![now],
    )
    .expect("insert artist credit");
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) VALUES (200, 'artist-route-null', 0, 'Route Null', '')",
        [],
    )
    .expect("insert artist credit name");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) VALUES ('feed-route-null', 'https://example.com/route-null.xml', 'Route Null', 'route null', 200, ?1, ?1)",
        params![now],
    )
    .expect("insert feed");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, created_at, updated_at) VALUES ('track-route-null', 'feed-route-null', 200, 'Route Null Track', 'route null track', ?1, ?1)",
        params![now],
    )
    .expect("insert track");
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) VALUES ('track-route-null', 'feed-route-null', 'Legacy Track', 'node', 'node://legacy-track', NULL, NULL, 100, 0)",
        [],
    )
    .expect("insert legacy track route");
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) VALUES ('feed-route-null', 'Legacy Feed', 'node', 'node://legacy-feed', NULL, NULL, 100, 0)",
        [],
    )
    .expect("insert legacy feed route");

    conn.execute_batch(include_str!(
        "../migrations/0021_route_custom_value_normalization.sql"
    ))
    .expect("apply normalization migration");

    let raw_track: (String, String) = conn
        .query_row(
            "SELECT custom_key, custom_value FROM payment_routes WHERE track_guid = 'track-route-null'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load normalized track route");
    assert_eq!(raw_track, (String::new(), String::new()));

    let raw_feed: (String, String) = conn
        .query_row(
            "SELECT custom_key, custom_value FROM feed_payment_routes WHERE feed_guid = 'feed-route-null'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load normalized feed route");
    assert_eq!(raw_feed, (String::new(), String::new()));

    drop(conn);
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn replace_live_events_allows_same_live_item_guid_in_multiple_feeds() {
    let conn = common::test_db();
    let now = common::now();

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?6)",
        params!["feed-a", "https://example.com/a.xml", "Feed A", "feed a", now, now],
    )
    .expect("insert feed a");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?6)",
        params!["feed-b", "https://example.com/b.xml", "Feed B", "feed b", now, now],
    )
    .expect("insert feed b");

    let shared_guid = "shared-live-item";
    stophammer::db::replace_live_events_for_feed(
        &conn,
        "feed-a",
        &[stophammer::model::LiveEvent {
            live_item_guid: shared_guid.into(),
            feed_guid: "feed-a".into(),
            title: "Live A".into(),
            content_link: None,
            status: "live".into(),
            scheduled_start: Some(now),
            scheduled_end: None,
            created_at: now,
            updated_at: now,
            live_value_uri: None,
            live_value_protocol: None,
        }],
    )
    .expect("replace live events a");
    stophammer::db::replace_live_events_for_feed(
        &conn,
        "feed-b",
        &[stophammer::model::LiveEvent {
            live_item_guid: shared_guid.into(),
            feed_guid: "feed-b".into(),
            title: "Live B".into(),
            content_link: None,
            status: "live".into(),
            scheduled_start: Some(now + 1),
            scheduled_end: None,
            created_at: now,
            updated_at: now + 1,
            live_value_uri: None,
            live_value_protocol: None,
        }],
    )
    .expect("replace live events b");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM live_events WHERE live_item_guid = ?1",
            params![shared_guid],
            |row| row.get(0),
        )
        .expect("count shared live events");
    assert_eq!(count, 2);
}

// ---------------------------------------------------------------------------
// Helper: insert a minimal feed or track. ADR 0034 §11: neither carries an
// artist credit any more.
// ---------------------------------------------------------------------------

/// Insert a minimal feed and return its `feed_guid`.
fn insert_feed(conn: &rusqlite::Connection, guid: &str) -> String {
    let now = common::now();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![
            guid,
            format!("https://example.com/{guid}"),
            "Test Feed",
            "test feed",
            now,
        ],
    )
    .unwrap();
    guid.to_string()
}

/// Insert a minimal track and return its `track_guid`.
fn insert_track(conn: &rusqlite::Connection, track_guid: &str, feed_guid: &str) -> String {
    let now = common::now();
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, title, title_lower, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![track_guid, feed_guid, "Test Track", "test track", now],
    )
    .unwrap();
    track_guid.to_string()
}

// ---------------------------------------------------------------------------
// 10. Feed upsert
// ---------------------------------------------------------------------------

#[test]
fn feed_upsert() {
    let conn = common::test_db();
    let now = common::now();
    let guid = "feed-001";

    // Initial insert.
    insert_feed(&conn, guid);
    let title: String = conn
        .query_row(
            "SELECT title FROM feeds WHERE feed_guid = ?1",
            params![guid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(title, "Test Feed");

    // Upsert: update title.
    conn.execute(
        "UPDATE feeds SET title = ?1, title_lower = ?2, updated_at = ?3 WHERE feed_guid = ?4",
        params!["Updated Feed", "updated feed", now, guid],
    )
    .unwrap();

    let updated_title: String = conn
        .query_row(
            "SELECT title FROM feeds WHERE feed_guid = ?1",
            params![guid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(updated_title, "Updated Feed");
}

// ---------------------------------------------------------------------------
// 11. Track upsert
// ---------------------------------------------------------------------------

#[test]
fn track_upsert() {
    let conn = common::test_db();
    let now = common::now();
    let fg = insert_feed(&conn, "feed-t1");
    let tg = "track-001";

    insert_track(&conn, tg, &fg);

    let title: String = conn
        .query_row(
            "SELECT title FROM tracks WHERE track_guid = ?1",
            params![tg],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(title, "Test Track");

    // Update.
    conn.execute(
        "UPDATE tracks SET title = ?1, title_lower = ?2, updated_at = ?3 WHERE track_guid = ?4",
        params!["Updated Track", "updated track", now, tg],
    )
    .unwrap();

    let updated: String = conn
        .query_row(
            "SELECT title FROM tracks WHERE track_guid = ?1",
            params![tg],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(updated, "Updated Track");
}

// ---------------------------------------------------------------------------
// 12. Payment route replace (delete + insert cycle)
// ---------------------------------------------------------------------------

#[test]
fn payment_route_replace() {
    let conn = common::test_db();
    let fg = insert_feed(&conn, "feed-pr");
    let tg = insert_track(&conn, "track-pr", &fg);

    // Insert initial routes.
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split)
         VALUES (?1, ?2, 'Alice', 'keysend', 'node-abc', 90)",
        params![&tg, &fg],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split)
         VALUES (?1, ?2, 'App', 'keysend', 'node-xyz', 10)",
        params![&tg, &fg],
    )
    .unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM payment_routes WHERE track_guid = ?1",
            params![&tg],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);

    // Replace: delete all, then insert new set.
    conn.execute(
        "DELETE FROM payment_routes WHERE track_guid = ?1",
        params![&tg],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split)
         VALUES (?1, ?2, 'Bob', 'keysend', 'node-bob', 100)",
        params![&tg, &fg],
    )
    .unwrap();

    let new_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM payment_routes WHERE track_guid = ?1",
            params![&tg],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(new_count, 1);

    let recipient: String = conn
        .query_row(
            "SELECT recipient_name FROM payment_routes WHERE track_guid = ?1",
            params![&tg],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(recipient, "Bob");
}

// ---------------------------------------------------------------------------
// 13. Feed payment route replace
// ---------------------------------------------------------------------------

#[test]
fn feed_payment_route_replace() {
    let conn = common::test_db();
    let fg = insert_feed(&conn, "feed-fpr");

    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split)
         VALUES (?1, 'Host', 'keysend', 'node-host', 95)",
        params![&fg],
    )
    .unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feed_payment_routes WHERE feed_guid = ?1",
            params![&fg],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    // Replace.
    conn.execute(
        "DELETE FROM feed_payment_routes WHERE feed_guid = ?1",
        params![&fg],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split)
         VALUES (?1, 'New Host', 'keysend', 'node-new', 80)",
        params![&fg],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split)
         VALUES (?1, 'App', 'keysend', 'node-app', 20)",
        params![&fg],
    )
    .unwrap();

    let new_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feed_payment_routes WHERE feed_guid = ?1",
            params![&fg],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(new_count, 2);
}

// ---------------------------------------------------------------------------
// 14. Value time split replace (delete + insert cycle)
// ---------------------------------------------------------------------------

#[test]
fn value_time_split_replace() {
    let conn = common::test_db();
    let now = common::now();
    let fg = insert_feed(&conn, "feed-vts");
    let tg = insert_track(&conn, "track-vts", &fg);

    // Insert two VTS entries.
    conn.execute(
        "INSERT INTO value_time_splits (source_track_guid, start_time_secs, duration_secs, remote_feed_guid, remote_item_guid, split, created_at)
         VALUES (?1, 0, 60, 'remote-feed-1', 'remote-item-1', 50, ?2)",
        params![&tg, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO value_time_splits (source_track_guid, start_time_secs, duration_secs, remote_feed_guid, remote_item_guid, split, created_at)
         VALUES (?1, 60, 120, 'remote-feed-2', 'remote-item-2', 50, ?2)",
        params![&tg, now],
    )
    .unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM value_time_splits WHERE source_track_guid = ?1",
            params![&tg],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);

    // Replace cycle.
    conn.execute(
        "DELETE FROM value_time_splits WHERE source_track_guid = ?1",
        params![&tg],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO value_time_splits (source_track_guid, start_time_secs, duration_secs, remote_feed_guid, remote_item_guid, split, created_at)
         VALUES (?1, 0, 180, 'remote-feed-3', 'remote-item-3', 100, ?2)",
        params![&tg, now],
    )
    .unwrap();

    let new_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM value_time_splits WHERE source_track_guid = ?1",
            params![&tg],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(new_count, 1);
}

// ---------------------------------------------------------------------------
// 15. Event insert monotonic seq
// ---------------------------------------------------------------------------

#[test]
fn event_insert_monotonic_seq() {
    let conn = common::test_db();
    let now = common::now();

    for i in 1..=5 {
        conn.execute(
            "INSERT INTO events (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at)
             VALUES (?1, 'feed.updated', '{}', 'feed-001', 'node-a', 'sig-a', ?2, ?3)",
            params![format!("evt-{i}"), i, now],
        )
        .unwrap();
    }

    let mut stmt = conn
        .prepare("SELECT seq FROM events ORDER BY seq ASC")
        .unwrap();
    let seqs: Vec<i64> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
}

// ---------------------------------------------------------------------------
// 16. Event insert idempotent
// ---------------------------------------------------------------------------

#[test]
fn event_insert_idempotent() {
    let conn = common::test_db();
    let now = common::now();

    conn.execute(
        "INSERT INTO events (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at)
         VALUES ('evt-dup', 'feed.updated', '{}', 'feed-001', 'node-a', 'sig-a', 1, ?1)",
        params![now],
    )
    .unwrap();

    // Second insert with same event_id should fail (PK constraint).
    let result = conn.execute(
        "INSERT INTO events (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at)
         VALUES ('evt-dup', 'feed.updated', '{}', 'feed-001', 'node-a', 'sig-a', 2, ?1)",
        params![now],
    );
    assert!(result.is_err());

    // OR IGNORE variant: succeeds but inserts nothing.
    conn.execute(
        "INSERT OR IGNORE INTO events (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at)
         VALUES ('evt-dup', 'feed.updated', '{}', 'feed-001', 'node-a', 'sig-a', 2, ?1)",
        params![now],
    )
    .unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);

    // seq should still be 1 (original).
    let seq: i64 = conn
        .query_row(
            "SELECT seq FROM events WHERE event_id = 'evt-dup'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(seq, 1);
}

// ---------------------------------------------------------------------------
// 17. Events pagination (get_events_since)
// ---------------------------------------------------------------------------

#[test]
fn events_pagination() {
    let conn = common::test_db();
    let now = common::now();

    for i in 1..=20 {
        conn.execute(
            "INSERT INTO events (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at)
             VALUES (?1, 'track.created', '{}', 'track-001', 'node-a', 'sig', ?2, ?3)",
            params![format!("evt-page-{i}"), i, now],
        )
        .unwrap();
    }

    // Page 1: after_seq = 0, limit = 5  -> seq 1..5
    let mut stmt = conn
        .prepare("SELECT seq FROM events WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2")
        .unwrap();
    let page1: Vec<i64> = stmt
        .query_map(params![0, 5], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(page1, vec![1, 2, 3, 4, 5]);

    // Page 2: after_seq = 5, limit = 5  -> seq 6..10
    let page2: Vec<i64> = stmt
        .query_map(params![5, 5], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(page2, vec![6, 7, 8, 9, 10]);

    // Page past end: after_seq = 20, limit = 5  -> empty
    let page_end: Vec<i64> = stmt
        .query_map(params![20, 5], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(page_end.is_empty());
}

// ---------------------------------------------------------------------------
// 18. Feed crawl cache upsert
// ---------------------------------------------------------------------------

#[test]
fn feed_crawl_cache_upsert() {
    let conn = common::test_db();
    let now = common::now();
    let url = "https://example.com/feed.xml";

    // Insert.
    conn.execute(
        "INSERT INTO feed_crawl_cache (feed_url, content_hash, crawled_at) VALUES (?1, ?2, ?3)",
        params![url, "hash-v1", now],
    )
    .unwrap();

    let hash: String = conn
        .query_row(
            "SELECT content_hash FROM feed_crawl_cache WHERE feed_url = ?1",
            params![url],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hash, "hash-v1");

    // Update (upsert via INSERT OR REPLACE, since feed_url is PK).
    conn.execute(
        "INSERT OR REPLACE INTO feed_crawl_cache (feed_url, content_hash, crawled_at) VALUES (?1, ?2, ?3)",
        params![url, "hash-v2", now + 60],
    )
    .unwrap();

    let updated_hash: String = conn
        .query_row(
            "SELECT content_hash FROM feed_crawl_cache WHERE feed_url = ?1",
            params![url],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(updated_hash, "hash-v2");

    // Only one row should exist.
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM feed_crawl_cache", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

// ---------------------------------------------------------------------------
// 19. Peer node CRUD — upsert, failure tracking, eviction
// ---------------------------------------------------------------------------

#[test]
fn peer_node_upsert() {
    let conn = common::test_db();
    let now = common::now();

    conn.execute(
        "INSERT INTO peer_nodes (node_pubkey, node_url, discovered_at) VALUES (?1, ?2, ?3)",
        params!["pk-1", "https://peer1.example.com", now],
    )
    .unwrap();

    let url: String = conn
        .query_row(
            "SELECT node_url FROM peer_nodes WHERE node_pubkey = ?1",
            params!["pk-1"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(url, "https://peer1.example.com");

    // Update URL (upsert pattern).
    conn.execute(
        "INSERT OR REPLACE INTO peer_nodes (node_pubkey, node_url, discovered_at) VALUES (?1, ?2, ?3)",
        params!["pk-1", "https://peer1-new.example.com", now],
    )
    .unwrap();

    let updated_url: String = conn
        .query_row(
            "SELECT node_url FROM peer_nodes WHERE node_pubkey = ?1",
            params!["pk-1"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(updated_url, "https://peer1-new.example.com");
}

#[test]
fn peer_node_failure_tracking() {
    let conn = common::test_db();
    let now = common::now();

    conn.execute(
        "INSERT INTO peer_nodes (node_pubkey, node_url, discovered_at) VALUES (?1, ?2, ?3)",
        params!["pk-fail", "https://failing.example.com", now],
    )
    .unwrap();

    // Increment consecutive_failures.
    conn.execute(
        "UPDATE peer_nodes SET consecutive_failures = consecutive_failures + 1 WHERE node_pubkey = ?1",
        params!["pk-fail"],
    )
    .unwrap();
    conn.execute(
        "UPDATE peer_nodes SET consecutive_failures = consecutive_failures + 1 WHERE node_pubkey = ?1",
        params!["pk-fail"],
    )
    .unwrap();

    let failures: i64 = conn
        .query_row(
            "SELECT consecutive_failures FROM peer_nodes WHERE node_pubkey = ?1",
            params!["pk-fail"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(failures, 2);

    // Reset on success.
    conn.execute(
        "UPDATE peer_nodes SET consecutive_failures = 0, last_push_at = ?1 WHERE node_pubkey = ?2",
        params![now, "pk-fail"],
    )
    .unwrap();

    let after_reset: i64 = conn
        .query_row(
            "SELECT consecutive_failures FROM peer_nodes WHERE node_pubkey = ?1",
            params!["pk-fail"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(after_reset, 0);
}

#[test]
fn peer_node_eviction() {
    let conn = common::test_db();
    let now = common::now();

    // Insert several peers, some with high failure counts.
    for i in 0..5 {
        conn.execute(
            "INSERT INTO peer_nodes (node_pubkey, node_url, discovered_at, consecutive_failures)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                format!("pk-evict-{i}"),
                format!("https://peer{i}.example.com"),
                now,
                i * 5 // 0, 5, 10, 15, 20
            ],
        )
        .unwrap();
    }

    // Evict peers with >= 10 consecutive failures.
    conn.execute(
        "DELETE FROM peer_nodes WHERE consecutive_failures >= 10",
        [],
    )
    .unwrap();

    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM peer_nodes", [], |r| r.get(0))
        .unwrap();
    // Peers with 0, 5 failures survive -> 2 remaining.
    assert_eq!(remaining, 2);
}

// ---------------------------------------------------------------------------
// 20. Ingest transaction atomicity
// ---------------------------------------------------------------------------

#[test]
fn ingest_transaction_atomicity() {
    let conn = common::test_db();
    let now = common::now();

    // Simulate a full atomic ingest: feed + track + routes + event.
    conn.execute_batch("BEGIN").unwrap();

    // Feed.
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, created_at, updated_at)
         VALUES ('feed-txn', 'https://example.com/txn', 'Txn Album', 'txn album', ?1, ?1)",
        params![now],
    )
    .unwrap();

    // Track.
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, title, title_lower, pub_date, duration_secs, created_at, updated_at)
         VALUES ('track-txn', 'feed-txn', 'Txn Song', 'txn song', ?1, 240, ?1, ?1)",
        params![now],
    )
    .unwrap();

    // Payment routes.
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split)
         VALUES ('track-txn', 'feed-txn', 'Txn Artist', 'keysend', 'node-txn', 95)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split)
         VALUES ('feed-txn', 'Txn Artist', 'keysend', 'node-txn-feed', 100)",
        [],
    )
    .unwrap();

    // Event.
    conn.execute(
        "INSERT INTO events (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at)
         VALUES ('evt-txn', 'feed.created', '{}', 'feed-txn', 'node-a', 'sig-txn', 1, ?1)",
        params![now],
    )
    .unwrap();

    conn.execute_batch("COMMIT").unwrap();

    // Verify everything landed.
    let feed_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM feeds WHERE feed_guid = 'feed-txn'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(feed_exists);

    let track_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM tracks WHERE track_guid = 'track-txn'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(track_exists);

    let route_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM payment_routes WHERE track_guid = 'track-txn'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(route_count, 1);

    let feed_route_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feed_payment_routes WHERE feed_guid = 'feed-txn'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(feed_route_count, 1);

    let event_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM events WHERE event_id = 'evt-txn'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(event_exists);
}

// ---------------------------------------------------------------------------
// Bonus: Transaction rollback leaves DB clean
// ---------------------------------------------------------------------------

#[test]
fn ingest_transaction_rollback() {
    let conn = common::test_db();
    let now = common::now();

    conn.execute_batch("BEGIN").unwrap();

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, created_at, updated_at)
         VALUES ('feed-rb', 'https://example.com/rb', 'Rollback Feed', 'rollback feed', ?1, ?1)",
        params![now],
    )
    .unwrap();

    conn.execute_batch("ROLLBACK").unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feeds WHERE feed_guid = 'feed-rb'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "rollback should have removed the feed");
}

#[test]
fn source_contributor_claims_replace_round_trip() {
    let conn = common::test_db();
    let feed_guid = insert_feed(&conn, "feed-src-claims");

    let claims = vec![
        stophammer::model::SourceContributorClaim {
            id: None,
            feed_guid: feed_guid.clone(),
            entity_type: "feed".into(),
            entity_id: feed_guid.clone(),
            position: 0,
            name: "Alice".into(),
            role: Some("vocals".into()),
            role_norm: Some("vocals".into()),
            group_name: Some("cast".into()),
            href: Some("https://example.com/alice".into()),
            img: None,
            npub: Some("npub1alice".into()),
            source: "podcast_person".into(),
            extraction_path: "channel/podcast:person".into(),
            observed_at: common::now(),
        },
        stophammer::model::SourceContributorClaim {
            id: None,
            feed_guid: feed_guid.clone(),
            entity_type: "track".into(),
            entity_id: "track-src-claims".into(),
            position: 0,
            name: "Bob".into(),
            role: Some("guitar".into()),
            role_norm: Some("guitar".into()),
            group_name: None,
            href: None,
            img: None,
            npub: None,
            source: "podcast_person".into(),
            extraction_path: "item/podcast:person".into(),
            observed_at: common::now(),
        },
    ];

    stophammer::db::replace_source_contributor_claims_for_feed(&conn, &feed_guid, &claims)
        .expect("replace contributor claims");

    let stored = stophammer::db::get_source_contributor_claims_for_feed(&conn, &feed_guid)
        .expect("get contributor claims");
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].name, "Alice");
    assert_eq!(stored[0].npub.as_deref(), Some("npub1alice"));
    assert_eq!(stored[1].entity_type, "track");
    assert_eq!(stored[1].role.as_deref(), Some("guitar"));
    assert_eq!(stored[1].role_norm.as_deref(), Some("guitar"));

    stophammer::db::replace_source_contributor_claims_for_feed(&conn, &feed_guid, &claims[..1])
        .expect("replace contributor claims again");
    let stored_again = stophammer::db::get_source_contributor_claims_for_feed(&conn, &feed_guid)
        .expect("get contributor claims again");
    assert_eq!(stored_again.len(), 1);
    assert_eq!(stored_again[0].name, "Alice");
}

#[test]
fn source_contributor_claims_replace_dedupes_duplicate_unique_keys() {
    let conn = common::test_db();
    let feed_guid = insert_feed(&conn, "feed-src-claims-dedupe");

    let claim = stophammer::model::SourceContributorClaim {
        id: None,
        feed_guid: feed_guid.clone(),
        entity_type: "feed".into(),
        entity_id: feed_guid.clone(),
        position: 0,
        name: "Alice".into(),
        role: None,
        role_norm: None,
        group_name: None,
        href: None,
        img: None,
        npub: None,
        source: "podcast_person".into(),
        extraction_path: "channel/podcast:person".into(),
        observed_at: common::now(),
    };

    stophammer::db::replace_source_contributor_claims_for_feed(
        &conn,
        &feed_guid,
        &[claim.clone(), claim],
    )
    .expect("replace contributor claims");

    let stored = stophammer::db::get_source_contributor_claims_for_feed(&conn, &feed_guid)
        .expect("get contributor claims");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].name, "Alice");
}

#[test]
fn source_entity_ids_replace_round_trip() {
    let conn = common::test_db();
    let feed_guid = insert_feed(&conn, "feed-src-ids");

    let claims = vec![
        stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: feed_guid.clone(),
            entity_type: "feed".into(),
            entity_id: feed_guid.clone(),
            position: 0,
            scheme: "nostr_npub".into(),
            value: "npub1example".into(),
            source: "rss_link".into(),
            extraction_path: "channel/link".into(),
            observed_at: common::now(),
        },
        stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: feed_guid.clone(),
            entity_type: "track".into(),
            entity_id: "track-src-ids".into(),
            position: 0,
            scheme: "isrc".into(),
            value: "USABC1234567".into(),
            source: "rss_guid".into(),
            extraction_path: "item/guid".into(),
            observed_at: common::now(),
        },
    ];

    stophammer::db::replace_source_entity_ids_for_feed(&conn, &feed_guid, &claims)
        .expect("replace source ids");

    let stored =
        stophammer::db::get_source_entity_ids_for_feed(&conn, &feed_guid).expect("get source ids");
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].scheme, "nostr_npub");
    assert_eq!(stored[1].value, "USABC1234567");

    stophammer::db::replace_source_entity_ids_for_feed(&conn, &feed_guid, &claims[..1])
        .expect("replace source ids again");
    let stored_again = stophammer::db::get_source_entity_ids_for_feed(&conn, &feed_guid)
        .expect("get source ids again");
    assert_eq!(stored_again.len(), 1);
    assert_eq!(stored_again[0].scheme, "nostr_npub");
}

#[test]
fn source_entity_links_replace_round_trip() {
    let conn = common::test_db();
    let feed_guid = insert_feed(&conn, "feed-src-links");

    let links = vec![
        stophammer::model::SourceEntityLink {
            id: None,
            feed_guid: feed_guid.clone(),
            entity_type: "feed".into(),
            entity_id: feed_guid.clone(),
            position: 0,
            link_type: "website".into(),
            url: "https://example.com/artist".into(),
            source: "rss_link".into(),
            extraction_path: "feed.link".into(),
            observed_at: common::now(),
        },
        stophammer::model::SourceEntityLink {
            id: None,
            feed_guid: feed_guid.clone(),
            entity_type: "track".into(),
            entity_id: "track-src-links".into(),
            position: 0,
            link_type: "web_page".into(),
            url: "https://example.com/release".into(),
            source: "rss_link".into(),
            extraction_path: "track.link".into(),
            observed_at: common::now(),
        },
    ];

    stophammer::db::replace_source_entity_links_for_feed(&conn, &feed_guid, &links)
        .expect("replace source links");

    let stored = stophammer::db::get_source_entity_links_for_feed(&conn, &feed_guid)
        .expect("get source links");
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].link_type, "website");
    assert_eq!(stored[1].url, "https://example.com/release");
}

#[test]
fn source_entity_links_replace_dedupes_duplicate_unique_keys() {
    let conn = common::test_db();
    let feed_guid = insert_feed(&conn, "feed-src-links-dedupe");

    let link = stophammer::model::SourceEntityLink {
        id: None,
        feed_guid: feed_guid.clone(),
        entity_type: "feed".into(),
        entity_id: feed_guid.clone(),
        position: 0,
        link_type: "website".into(),
        url: "https://example.com/artist".into(),
        source: "rss_link".into(),
        extraction_path: "feed.link".into(),
        observed_at: common::now(),
    };

    stophammer::db::replace_source_entity_links_for_feed(&conn, &feed_guid, &[link.clone(), link])
        .expect("replace source links");

    let stored = stophammer::db::get_source_entity_links_for_feed(&conn, &feed_guid)
        .expect("get source links");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].url, "https://example.com/artist");
}

#[test]
fn source_release_claims_replace_round_trip() {
    let conn = common::test_db();
    let feed_guid = insert_feed(&conn, "feed-src-release");

    let claims = vec![
        stophammer::model::SourceReleaseClaim {
            id: None,
            feed_guid: feed_guid.clone(),
            entity_type: "feed".into(),
            entity_id: feed_guid.clone(),
            position: 0,
            claim_type: "release_date".into(),
            claim_value: "1773703560".into(),
            source: "rss_metadata".into(),
            extraction_path: "feed.pub_date".into(),
            observed_at: common::now(),
        },
        stophammer::model::SourceReleaseClaim {
            id: None,
            feed_guid: feed_guid.clone(),
            entity_type: "track".into(),
            entity_id: "track-src-release".into(),
            position: 0,
            claim_type: "description".into(),
            claim_value: "Track description".into(),
            source: "rss_metadata".into(),
            extraction_path: "track.description".into(),
            observed_at: common::now(),
        },
    ];

    stophammer::db::replace_source_release_claims_for_feed(&conn, &feed_guid, &claims)
        .expect("replace source release claims");

    let stored = stophammer::db::get_source_release_claims_for_feed(&conn, &feed_guid)
        .expect("get source release claims");
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].claim_type, "release_date");
    assert_eq!(stored[1].claim_value, "Track description");
}

#[test]
fn source_release_claims_replace_dedupes_duplicate_unique_keys() {
    let conn = common::test_db();
    let feed_guid = insert_feed(&conn, "feed-src-release-dedupe");

    let claim = stophammer::model::SourceReleaseClaim {
        id: None,
        feed_guid: feed_guid.clone(),
        entity_type: "feed".into(),
        entity_id: feed_guid.clone(),
        position: 0,
        claim_type: "description".into(),
        claim_value: "same value".into(),
        source: "rss_metadata".into(),
        extraction_path: "feed.description".into(),
        observed_at: common::now(),
    };

    stophammer::db::replace_source_release_claims_for_feed(
        &conn,
        &feed_guid,
        &[claim.clone(), claim],
    )
    .expect("replace source release claims");

    let stored = stophammer::db::get_source_release_claims_for_feed(&conn, &feed_guid)
        .expect("get source release claims");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].claim_type, "description");
}

#[test]
fn source_platform_claims_replace_round_trip() {
    let conn = common::test_db();
    let feed_guid = insert_feed(&conn, "feed-src-platform");

    let claims = vec![
        stophammer::model::SourcePlatformClaim {
            id: None,
            feed_guid: feed_guid.clone(),
            platform_key: "wavlake".into(),
            url: Some("https://wavlake.com/feed/music/abc123".into()),
            owner_name: None,
            source: "platform_classifier".into(),
            extraction_path: "request.canonical_url".into(),
            observed_at: common::now(),
        },
        stophammer::model::SourcePlatformClaim {
            id: None,
            feed_guid: feed_guid.clone(),
            platform_key: "wavlake".into(),
            url: None,
            owner_name: Some("Wavlake".into()),
            source: "platform_classifier".into(),
            extraction_path: "feed.owner_name".into(),
            observed_at: common::now(),
        },
    ];

    stophammer::db::replace_source_platform_claims_for_feed(&conn, &feed_guid, &claims)
        .expect("replace source platform claims");

    let stored = stophammer::db::get_source_platform_claims_for_feed(&conn, &feed_guid)
        .expect("get source platform claims");
    assert_eq!(stored.len(), 2);
    assert!(stored.iter().all(|claim| claim.platform_key == "wavlake"));
    assert!(stored.iter().any(|claim| {
        claim.url.as_deref() == Some("https://wavlake.com/feed/music/abc123")
            && claim.extraction_path == "request.canonical_url"
    }));
    assert!(
        stored
            .iter()
            .any(|claim| claim.owner_name.as_deref() == Some("Wavlake"))
    );
}

#[test]
fn ingest_transaction_persists_source_claim_snapshots_and_events() {
    let mut conn = common::test_db();
    let now = common::now();

    let feed = stophammer::model::Feed {
        feed_guid: "feed-claim-ingest".into(),
        feed_url: "https://example.com/feed-claim-ingest.xml".into(),
        title: "Claim Feed".into(),
        title_lower: "claim feed".into(),
        description: None,
        image_url: None,
        publisher: None,
        language: Some("en".into()),
        explicit: false,
        itunes_type: None,
        release_artist: Some("Claim Artist".into()),
        release_artist_sort: None,
        release_date: Some(now),
        release_kind: None,
        episode_count: 0,
        newest_item_at: None,
        oldest_item_at: None,
        created_at: now,
        updated_at: now,
        raw_medium: Some("music".into()),
        last_build_date: None,
        release_artist_source: None,
    };

    let contributor_claims = vec![
        stophammer::model::SourceContributorClaim {
            id: None,
            feed_guid: feed.feed_guid.clone(),
            entity_type: "feed".into(),
            entity_id: feed.feed_guid.clone(),
            position: 0,
            name: "Claim Artist".into(),
            role: Some("bandleader".into()),
            role_norm: Some("bandleader".into()),
            group_name: Some("music".into()),
            href: Some("https://example.com/artist".into()),
            img: None,
            npub: Some("npub1claimartist".into()),
            source: "podcast_person".into(),
            extraction_path: "feed.podcast:person".into(),
            observed_at: now,
        },
        stophammer::model::SourceContributorClaim {
            id: None,
            feed_guid: feed.feed_guid.clone(),
            entity_type: "live_item".into(),
            entity_id: "live-claim-1".into(),
            position: 0,
            name: "Live Guest".into(),
            role: Some("guest".into()),
            role_norm: Some("guest".into()),
            group_name: Some("cast".into()),
            href: None,
            img: None,
            npub: None,
            source: "podcast_person".into(),
            extraction_path: "live_item.podcast:person".into(),
            observed_at: now,
        },
    ];

    let entity_id_claims = vec![
        stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: feed.feed_guid.clone(),
            entity_type: "feed".into(),
            entity_id: feed.feed_guid.clone(),
            position: 0,
            scheme: "nostr_npub".into(),
            value: "npub1claimfeed".into(),
            source: "podcast_txt".into(),
            extraction_path: "feed.podcast:txt".into(),
            observed_at: now,
        },
        stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: feed.feed_guid.clone(),
            entity_type: "track".into(),
            entity_id: "track-claim-1".into(),
            position: 0,
            scheme: "nostr_npub".into(),
            value: "npub1claimtrack".into(),
            source: "podcast_txt".into(),
            extraction_path: "track.podcast:txt".into(),
            observed_at: now,
        },
    ];

    let event_rows = stophammer::db::build_diff_events(
        &conn,
        &feed,
        &[],
        &contributor_claims,
        &entity_id_claims,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        now,
        &[],
    )
    .expect("build diff events");

    let event_types: Vec<_> = event_rows.iter().map(|e| e.event_type.clone()).collect();
    assert!(event_types.contains(&stophammer::event::EventType::SourceContributorClaimsReplaced));
    assert!(event_types.contains(&stophammer::event::EventType::SourceEntityIdsReplaced));

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("signing.key");
    let signer = stophammer::signing::NodeSigner::load_or_create(&signer_path).expect("signer");

    stophammer::db::ingest_transaction(
        &mut conn,
        feed,
        vec![],
        contributor_claims.clone(),
        entity_id_claims.clone(),
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        Vec::new(),
        vec![],
        vec![],
        event_rows,
        &signer,
    )
    .expect("ingest transaction");

    let stored_contributor_claims =
        stophammer::db::get_source_contributor_claims_for_feed(&conn, "feed-claim-ingest")
            .expect("stored contributor claims");
    let stored_entity_id_claims =
        stophammer::db::get_source_entity_ids_for_feed(&conn, "feed-claim-ingest")
            .expect("stored entity ids");

    assert_eq!(stored_contributor_claims.len(), 2);
    assert_eq!(
        stored_contributor_claims[0].npub.as_deref(),
        Some("npub1claimartist")
    );
    assert_eq!(stored_entity_id_claims.len(), 2);
    assert_eq!(stored_contributor_claims[1].entity_type, "live_item");
    assert_eq!(stored_entity_id_claims[0].scheme, "nostr_npub");
}
