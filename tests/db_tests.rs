#![allow(
    clippy::too_many_lines,
    reason = "db regression tests inline full fixture setup and assertions for determinism"
)]

mod common;

use rusqlite::params;

fn seed_artist(
    conn: &rusqlite::Connection,
    artist_id: &str,
    name: &str,
) -> stophammer::model::Artist {
    let now = common::now();
    let artist = stophammer::model::Artist {
        artist_id: artist_id.to_string(),
        name: name.to_string(),
        name_lower: name.to_lowercase(),
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: now,
        updated_at: now,
    };
    stophammer::db::upsert_artist_if_absent(conn, &artist).expect("upsert artist");
    artist
}

fn seed_feed_with_track(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    track_guid: &str,
    title: &str,
) {
    let now = common::now();
    let artist = seed_artist(
        conn,
        &format!("artist-{feed_guid}"),
        &format!("Artist {feed_guid}"),
    );
    let credit = stophammer::db::get_or_create_artist_credit(
        conn,
        &artist.name,
        &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
        Some(feed_guid),
    )
    .expect("artist credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            feed_guid,
            format!("https://example.com/{feed_guid}.xml"),
            title,
            title.to_lowercase(),
            credit.id,
            now
        ],
    )
    .expect("insert feed");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![track_guid, feed_guid, credit.id, title, title.to_lowercase(), now],
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
    let expected = [
        "artist_aliases",
        "artist_credit",
        "artist_credit_name",
        "artist_type",
        "artists",
        "entity_quality",
        "entity_source",
        "events",
        "external_ids",
        "feed_crawl_cache",
        "feed_payment_routes",
        "feed_remote_items_raw",
        "feeds",
        "live_events",
        "node_sync_state",
        "payment_routes",
        "peer_nodes",
        "proof_challenges",
        "proof_tokens",
        "recordings",
        "rel_type",
        "release_recordings",
        "releases",
        "schema_migrations",
        "search_index",
        "search_entities",
        "source_contributor_claims",
        "source_entity_links",
        "source_entity_ids",
        "source_feed_release_map",
        "source_item_enclosures",
        "source_item_recording_map",
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

#[test]
fn lookup_tables_seeded() {
    let conn = common::test_db();

    let artist_type_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM artist_type", [], |r| r.get(0))
        .unwrap();
    assert_eq!(artist_type_count, 6);

    // Dead schema removed — 2026-03-13: feed_type table removed

    let rel_type_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM rel_type", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rel_type_count, 35);
}

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

    // Seed counts should be correct after first open.
    let artist_type_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM artist_type", [], |r| r.get(0))
        .unwrap();
    assert_eq!(artist_type_count, 6);

    drop(conn);

    // Second open — migrations must be skipped, data intact.
    let conn2 = stophammer::db::open_db(&tmp);
    let artist_type_count2: i64 = conn2
        .query_row("SELECT COUNT(*) FROM artist_type", [], |r| r.get(0))
        .unwrap();
    assert_eq!(artist_type_count2, 6);

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
    let feed_route_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, address, route_type, split) VALUES ('track-delete-a', 'feed-delete-a', 'node://track', 'node', 100)",
        [],
    )
    .expect("insert track route");
    let track_route_id = conn.last_insert_rowid();
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
    conn.execute(
        "INSERT INTO live_events_legacy \
         (live_item_guid, feed_guid, title, content_link, status, scheduled_start, scheduled_end, created_at, updated_at) \
         VALUES ('legacy-live-delete-a', 'feed-delete-a', 'Legacy Event', NULL, 'pending', NULL, NULL, ?1, ?1)",
        params![now],
    )
    .expect("insert legacy live event");
    conn.execute(
        "INSERT INTO wallet_endpoints (route_type, normalized_address, custom_key, custom_value, wallet_id, created_at) \
         VALUES ('node', 'node://wallet-test', '', '', NULL, ?1)",
        params![now],
    )
    .expect("insert wallet endpoint");
    let endpoint_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO wallet_feed_route_map (route_id, endpoint_id, created_at) VALUES (?1, ?2, ?3)",
        params![feed_route_id, endpoint_id, now],
    )
    .expect("insert wallet feed route map");
    conn.execute(
        "INSERT INTO wallet_track_route_map (route_id, endpoint_id, created_at) VALUES (?1, ?2, ?3)",
        params![track_route_id, endpoint_id, now],
    )
    .expect("insert wallet track route map");

    conn.execute("DELETE FROM feeds WHERE feed_guid = 'feed-delete-a'", [])
        .expect("direct feed delete should succeed");

    for (table, predicate) in [
        ("feeds", "feed_guid = 'feed-delete-a'"),
        ("tracks", "feed_guid = 'feed-delete-a'"),
        ("feed_payment_routes", "feed_guid = 'feed-delete-a'"),
        ("payment_routes", "track_guid = 'track-delete-a'"),
        ("value_time_splits", "source_track_guid = 'track-delete-a'"),
        ("feed_remote_items_raw", "feed_guid = 'feed-delete-a'"),
        ("live_events_legacy", "feed_guid = 'feed-delete-a'"),
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

    let wallet_feed_route_map_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wallet_feed_route_map WHERE route_id = ?1",
            params![feed_route_id],
            |row| row.get(0),
        )
        .expect("count wallet feed route map rows");
    assert_eq!(
        wallet_feed_route_map_count, 0,
        "wallet_feed_route_map rows should be cleaned on direct feed delete"
    );

    let wallet_track_route_map_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wallet_track_route_map WHERE route_id = ?1",
            params![track_route_id],
            |row| row.get(0),
        )
        .expect("count wallet track route map rows");
    assert_eq!(
        wallet_track_route_map_count, 0,
        "wallet_track_route_map rows should be cleaned on direct feed delete"
    );
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
    let route_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO value_time_splits (source_track_guid, start_time_secs, remote_feed_guid, remote_item_guid, split, created_at) \
         VALUES ('track-track-delete', 0, 'remote-feed', 'remote-item', 100, ?1)",
        params![now],
    )
    .expect("insert value time split");
    conn.execute(
        "INSERT INTO wallet_endpoints (route_type, normalized_address, custom_key, custom_value, wallet_id, created_at) \
         VALUES ('node', 'node://track-wallet-test', '', '', NULL, ?1)",
        params![now],
    )
    .expect("insert wallet endpoint");
    let endpoint_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO wallet_track_route_map (route_id, endpoint_id, created_at) VALUES (?1, ?2, ?3)",
        params![route_id, endpoint_id, now],
    )
    .expect("insert wallet track route map");

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

    let wallet_track_route_map_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wallet_track_route_map WHERE route_id = ?1",
            params![route_id],
            |row| row.get(0),
        )
        .expect("count wallet track route map rows");
    assert_eq!(
        wallet_track_route_map_count, 0,
        "wallet_track_route_map rows should be cleaned on direct track delete"
    );
}

#[test]
fn null_scoped_artist_credit_dedup_reuses_existing_row() {
    let conn = common::test_db();
    let now = common::now();

    conn.execute(
        "INSERT INTO artist_credit (display_name, feed_guid, created_at) VALUES ('Legacy Artist', NULL, ?1)",
        params![now],
    )
    .expect("insert initial legacy artist credit");
    conn.execute(
        "INSERT OR IGNORE INTO artist_credit (display_name, feed_guid, created_at) VALUES ('Legacy Artist', NULL, ?1)",
        params![now + 1],
    )
    .expect("duplicate insert should be ignored");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artist_credit WHERE display_name = 'Legacy Artist' AND feed_guid IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("count artist credits");
    assert_eq!(
        count, 1,
        "normalized unique index should collapse NULL-scoped duplicates"
    );
}

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
        include_str!("../migrations/0016_wallet_entities.sql"),
        include_str!("../migrations/0017_wallet_force_confidence_override.sql"),
        include_str!("../migrations/0018_wallet_merge_apply_batches.sql"),
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
        include_str!("../migrations/0016_wallet_entities.sql"),
        include_str!("../migrations/0017_wallet_force_confidence_override.sql"),
        include_str!("../migrations/0018_wallet_merge_apply_batches.sql"),
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
fn ingest_transaction_builds_deterministic_release_and_recording_rows() {
    let mut conn = common::test_db();
    let now = common::now();

    let artist = stophammer::model::Artist {
        artist_id: "artist-canon-1".into(),
        name: "Canon Artist".into(),
        name_lower: "canon artist".into(),
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: now,
        updated_at: now,
    };
    let artist_credit = stophammer::model::ArtistCredit {
        id: 9001,
        display_name: "Canon Artist".into(),
        feed_guid: Some("feed-canon-1".into()),
        created_at: now,
        names: vec![stophammer::model::ArtistCreditName {
            id: 0,
            artist_credit_id: 9001,
            artist_id: artist.artist_id.clone(),
            position: 0,
            name: "Canon Artist".into(),
            join_phrase: String::new(),
        }],
    };
    let feed = stophammer::model::Feed {
        feed_guid: "feed-canon-1".into(),
        feed_url: "https://example.com/feed-canon-1.xml".into(),
        title: "Release Title".into(),
        title_lower: "release title".into(),
        artist_credit_id: artist_credit.id,
        description: Some("Release description".into()),
        image_url: Some("https://example.com/release.jpg".into()),
        publisher: None,
        language: None,
        explicit: false,
        itunes_type: None,
        release_artist: Some(artist_credit.display_name.clone()),
        release_artist_sort: None,
        release_date: Some(now),
        release_kind: None,
        episode_count: 2,
        newest_item_at: Some(now),
        oldest_item_at: Some(now - 3600),
        created_at: now,
        updated_at: now,
        raw_medium: Some("music".into()),
    };
    let track_a = stophammer::model::Track {
        track_guid: "track-canon-a".into(),
        feed_guid: feed.feed_guid.clone(),
        artist_credit_id: artist_credit.id,
        title: "Track A".into(),
        title_lower: "track a".into(),
        pub_date: Some(now),
        duration_secs: Some(180),
        image_url: None,
        language: None,
        enclosure_url: Some("https://example.com/a.mp3".into()),
        enclosure_type: Some("audio/mpeg".into()),
        enclosure_bytes: Some(111),
        track_number: Some(2),
        season: None,
        explicit: false,
        description: None,
        track_artist: Some(artist_credit.display_name.clone()),
        track_artist_sort: None,
        created_at: now,
        updated_at: now,
    };
    let track_b = stophammer::model::Track {
        track_guid: "track-canon-b".into(),
        feed_guid: feed.feed_guid.clone(),
        artist_credit_id: artist_credit.id,
        title: "Track B".into(),
        title_lower: "track b".into(),
        pub_date: Some(now - 10),
        duration_secs: Some(120),
        image_url: None,
        language: None,
        enclosure_url: Some("https://example.com/b.mp3".into()),
        enclosure_type: Some("audio/mpeg".into()),
        enclosure_bytes: Some(222),
        track_number: Some(1),
        season: None,
        explicit: false,
        description: None,
        track_artist: Some(artist_credit.display_name.clone()),
        track_artist_sort: None,
        created_at: now,
        updated_at: now,
    };
    let tracks = vec![
        (track_a.clone(), vec![], vec![]),
        (track_b.clone(), vec![], vec![]),
    ];

    let event_rows = stophammer::db::build_diff_events(
        &conn,
        &artist,
        &artist_credit,
        &feed,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &tracks,
        &[],
        now,
        &[],
    )
    .expect("build diff events");

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("canonical-sync.key");
    let signer = stophammer::signing::NodeSigner::load_or_create(&signer_path).expect("signer");

    stophammer::db::ingest_transaction(
        &mut conn,
        artist,
        artist_credit,
        feed.clone(),
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        tracks,
        event_rows,
        &signer,
    )
    .expect("ingest transaction");

    stophammer::db::sync_canonical_state_for_feed(&conn, &feed.feed_guid)
        .expect("sync canonical state");

    let feed_map: (String, String, i64) = conn
        .query_row(
            "SELECT release_id, match_type, confidence FROM source_feed_release_map WHERE feed_guid = ?1",
            params![feed.feed_guid],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("feed map");
    assert_eq!(feed_map.1, "exact_release_signature_v1");
    assert_eq!(feed_map.2, 95);

    let release_row: (String, String, i64, Option<i64>) = conn
        .query_row(
            "SELECT release_id, title, artist_credit_id, release_date \
             FROM releases WHERE release_id = ?1",
            params![feed_map.0.clone()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("release row");
    assert_eq!(release_row.0, feed_map.0);
    assert_eq!(release_row.1, "Release Title");
    assert_eq!(release_row.2, 9001);
    assert_eq!(release_row.3, feed.oldest_item_at);

    let recording_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM recordings", [], |r| r.get(0))
        .expect("count recordings");
    assert_eq!(recording_count, 2);

    let release_tracks: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT position, source_track_guid FROM release_recordings \
                 WHERE release_id = ?1 ORDER BY position",
            )
            .expect("prepare release_recordings");
        stmt.query_map(params![release_row.0.clone()], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .expect("query release_recordings")
        .collect::<Result<_, _>>()
        .expect("collect release_recordings")
    };
    assert_eq!(
        release_tracks,
        vec![
            (1, "track-canon-b".to_string()),
            (2, "track-canon-a".to_string())
        ]
    );

    let recording_maps: Vec<(String, String, i64)> = {
        let mut stmt = conn
            .prepare(
                "SELECT track_guid, match_type, confidence FROM source_item_recording_map \
                 ORDER BY track_guid",
            )
            .expect("prepare recording maps");
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .expect("query recording maps")
            .collect::<Result<_, _>>()
            .expect("collect recording maps")
    };
    assert_eq!(
        recording_maps,
        vec![
            (
                "track-canon-a".to_string(),
                "exact_recording_signature_v1".to_string(),
                95
            ),
            (
                "track-canon-b".to_string(),
                "exact_recording_signature_v1".to_string(),
                95
            ),
        ]
    );
}

#[test]
fn canonical_release_dedupes_duplicate_recording_memberships_within_one_feed() {
    let mut conn = common::test_db();
    let now = common::now();

    let artist = stophammer::model::Artist {
        artist_id: "artist-dup-release-1".into(),
        name: "Duplicate Release Artist".into(),
        name_lower: "duplicate release artist".into(),
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: now,
        updated_at: now,
    };
    let artist_credit = stophammer::model::ArtistCredit {
        id: 9011,
        display_name: "Duplicate Release Artist".into(),
        feed_guid: Some("feed-dup-release-1".into()),
        created_at: now,
        names: vec![stophammer::model::ArtistCreditName {
            id: 0,
            artist_credit_id: 9011,
            artist_id: artist.artist_id.clone(),
            position: 0,
            name: "Duplicate Release Artist".into(),
            join_phrase: String::new(),
        }],
    };
    let feed = stophammer::model::Feed {
        feed_guid: "feed-dup-release-1".into(),
        feed_url: "https://example.com/feed-dup-release-1.xml".into(),
        title: "Duplicate Release".into(),
        title_lower: "duplicate release".into(),
        artist_credit_id: artist_credit.id,
        description: None,
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        itunes_type: None,
        release_artist: Some(artist_credit.display_name.clone()),
        release_artist_sort: None,
        release_date: Some(now),
        release_kind: None,
        episode_count: 2,
        newest_item_at: Some(now),
        oldest_item_at: Some(now - 60),
        created_at: now,
        updated_at: now,
        raw_medium: Some("music".into()),
    };
    let track_a = stophammer::model::Track {
        track_guid: "track-dup-release-a".into(),
        feed_guid: feed.feed_guid.clone(),
        artist_credit_id: artist_credit.id,
        title: "Same Song".into(),
        title_lower: "same song".into(),
        pub_date: Some(now - 60),
        duration_secs: Some(180),
        image_url: None,
        language: None,
        enclosure_url: Some("https://cdn.example.com/dup-a.mp3".into()),
        enclosure_type: Some("audio/mpeg".into()),
        enclosure_bytes: Some(1111),
        track_number: Some(1),
        season: None,
        explicit: false,
        description: None,
        track_artist: Some(artist_credit.display_name.clone()),
        track_artist_sort: None,
        created_at: now,
        updated_at: now,
    };
    let track_b = stophammer::model::Track {
        track_guid: "track-dup-release-b".into(),
        feed_guid: feed.feed_guid.clone(),
        artist_credit_id: artist_credit.id,
        title: "Same Song".into(),
        title_lower: "same song".into(),
        pub_date: Some(now),
        duration_secs: Some(180),
        image_url: None,
        language: None,
        enclosure_url: Some("https://cdn.example.com/dup-b.mp3".into()),
        enclosure_type: Some("audio/mpeg".into()),
        enclosure_bytes: Some(2222),
        track_number: Some(2),
        season: None,
        explicit: false,
        description: None,
        track_artist: Some(artist_credit.display_name.clone()),
        track_artist_sort: None,
        created_at: now,
        updated_at: now,
    };
    let tracks = vec![
        (track_a.clone(), vec![], vec![]),
        (track_b.clone(), vec![], vec![]),
    ];

    let event_rows = stophammer::db::build_diff_events(
        &conn,
        &artist,
        &artist_credit,
        &feed,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &tracks,
        &[],
        now,
        &[],
    )
    .expect("build diff events");

    let signer = common::temp_signer("duplicate-release-membership");
    stophammer::db::ingest_transaction(
        &mut conn,
        artist,
        artist_credit,
        feed.clone(),
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        tracks,
        event_rows,
        &signer,
    )
    .expect("ingest transaction");

    stophammer::db::sync_canonical_state_for_feed(&conn, &feed.feed_guid)
        .expect("sync canonical state");

    let release_id: String = conn
        .query_row(
            "SELECT release_id FROM source_feed_release_map WHERE feed_guid = ?1",
            params![feed.feed_guid],
            |row| row.get(0),
        )
        .expect("release id");
    let recording_ids: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT recording_id FROM source_item_recording_map \
                 WHERE track_guid IN (?1, ?2) ORDER BY track_guid",
            )
            .expect("prepare recording ids");
        stmt.query_map(params![track_a.track_guid, track_b.track_guid], |row| {
            row.get(0)
        })
        .expect("query recording ids")
        .collect::<Result<_, _>>()
        .expect("collect recording ids")
    };
    assert_eq!(recording_ids.len(), 2);
    assert_eq!(recording_ids[0], recording_ids[1]);

    let release_tracks: Vec<(i64, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT position, recording_id, source_track_guid FROM release_recordings \
                 WHERE release_id = ?1 ORDER BY position",
            )
            .expect("prepare release_recordings");
        stmt.query_map(params![release_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .expect("query release_recordings")
        .collect::<Result<_, _>>()
        .expect("collect release_recordings")
    };
    assert_eq!(
        release_tracks,
        vec![(
            1,
            recording_ids[0].clone(),
            "track-dup-release-a".to_string()
        )]
    );
}

#[test]
fn replace_live_events_allows_same_live_item_guid_in_multiple_feeds() {
    let conn = common::test_db();
    let now = common::now();

    let credit_a = stophammer::db::get_or_create_artist_credit(
        &conn,
        "Artist A",
        &[("artist-a".into(), "Artist A".into(), String::new())],
        Some("feed-a"),
    )
    .expect("credit a");
    let credit_b = stophammer::db::get_or_create_artist_credit(
        &conn,
        "Artist B",
        &[("artist-b".into(), "Artist B".into(), String::new())],
        Some("feed-b"),
    )
    .expect("credit b");

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, ?7)",
        params![
            "feed-a",
            "https://example.com/a.xml",
            "Feed A",
            "feed a",
            credit_a.id,
            now,
            now
        ],
    )
    .expect("insert feed a");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, ?7)",
        params![
            "feed-b",
            "https://example.com/b.xml",
            "Feed B",
            "feed b",
            credit_b.id,
            now,
            now
        ],
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

#[test]
fn ingest_transaction_requires_existing_track_credit_rows() {
    let mut conn = common::test_db();
    let now = common::now();
    let signer = common::temp_signer("ingest-track-credit");

    let artist = stophammer::model::Artist {
        artist_id: "artist-feed".into(),
        name: "Feed Artist".into(),
        name_lower: "feed artist".into(),
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: now,
        updated_at: now,
    };
    let artist_credit = stophammer::model::ArtistCredit {
        id: 1001,
        display_name: "Feed Artist".into(),
        feed_guid: Some("feed-credits".into()),
        created_at: now,
        names: vec![stophammer::model::ArtistCreditName {
            id: 0,
            artist_credit_id: 1001,
            artist_id: "artist-feed".into(),
            position: 0,
            name: "Feed Artist".into(),
            join_phrase: String::new(),
        }],
    };
    let track_credit = stophammer::model::ArtistCredit {
        id: 2002,
        display_name: "Track Artist".into(),
        feed_guid: Some("feed-credits".into()),
        created_at: now,
        names: vec![stophammer::model::ArtistCreditName {
            id: 0,
            artist_credit_id: 2002,
            artist_id: "artist-track".into(),
            position: 0,
            name: "Track Artist".into(),
            join_phrase: String::new(),
        }],
    };
    let feed = stophammer::model::Feed {
        feed_guid: "feed-credits".into(),
        feed_url: "https://example.com/feed-credits.xml".into(),
        title: "Feed Credits".into(),
        title_lower: "feed credits".into(),
        artist_credit_id: artist_credit.id,
        description: None,
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        itunes_type: None,
        release_artist: Some(artist_credit.display_name.clone()),
        release_artist_sort: None,
        release_date: Some(now),
        release_kind: None,
        episode_count: 1,
        newest_item_at: Some(now),
        oldest_item_at: Some(now),
        created_at: now,
        updated_at: now,
        raw_medium: Some("music".into()),
    };
    let track = stophammer::model::Track {
        track_guid: "track-credit-guid".into(),
        feed_guid: feed.feed_guid.clone(),
        artist_credit_id: track_credit.id,
        title: "Track Title".into(),
        title_lower: "track title".into(),
        pub_date: Some(now),
        duration_secs: Some(120),
        image_url: None,
        language: None,
        enclosure_url: Some("https://example.com/track.mp3".into()),
        enclosure_type: Some("audio/mpeg".into()),
        enclosure_bytes: Some(1234),
        track_number: Some(1),
        season: None,
        explicit: false,
        description: None,
        track_artist: Some(track_credit.display_name.clone()),
        track_artist_sort: None,
        created_at: now,
        updated_at: now,
    };
    let tracks = vec![(track.clone(), vec![], vec![])];

    let event_rows = stophammer::db::build_diff_events(
        &conn,
        &artist,
        &artist_credit,
        &feed,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &tracks,
        std::slice::from_ref(&track_credit),
        now,
        &[],
    )
    .expect("build diff events");

    let outcome = stophammer::db::ingest_transaction(
        &mut conn,
        artist,
        artist_credit,
        feed,
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        tracks,
        event_rows,
        &signer,
    );
    assert!(
        outcome.is_err(),
        "ingest should fail when a track references a missing credit row"
    );
}

#[test]
fn ingest_transaction_promotes_high_confidence_ids_and_sources() {
    let mut conn = common::test_db();
    let now = common::now();

    let artist = stophammer::model::Artist {
        artist_id: "artist-promote-1".into(),
        name: "Promote Artist".into(),
        name_lower: "promote artist".into(),
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: now,
        updated_at: now,
    };
    let artist_credit = stophammer::model::ArtistCredit {
        id: 9002,
        display_name: "Promote Artist".into(),
        feed_guid: Some("feed-promote-1".into()),
        created_at: now,
        names: vec![stophammer::model::ArtistCreditName {
            id: 0,
            artist_credit_id: 9002,
            artist_id: artist.artist_id.clone(),
            position: 0,
            name: "Promote Artist".into(),
            join_phrase: String::new(),
        }],
    };
    let feed = stophammer::model::Feed {
        feed_guid: "feed-promote-1".into(),
        feed_url: "https://example.com/feed-promote-1.xml".into(),
        title: "Promote Release".into(),
        title_lower: "promote release".into(),
        artist_credit_id: artist_credit.id,
        description: None,
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        itunes_type: None,
        release_artist: Some(artist_credit.display_name.clone()),
        release_artist_sort: None,
        release_date: Some(now),
        release_kind: None,
        episode_count: 1,
        newest_item_at: Some(now),
        oldest_item_at: Some(now - 60),
        created_at: now,
        updated_at: now,
        raw_medium: Some("music".into()),
    };
    let track = stophammer::model::Track {
        track_guid: "track-promote-1".into(),
        feed_guid: feed.feed_guid.clone(),
        artist_credit_id: artist_credit.id,
        title: "Promote Track".into(),
        title_lower: "promote track".into(),
        pub_date: Some(now),
        duration_secs: Some(180),
        image_url: None,
        language: None,
        enclosure_url: Some("https://cdn.example.com/promote-track.mp3".into()),
        enclosure_type: Some("audio/mpeg".into()),
        enclosure_bytes: Some(1234),
        track_number: Some(1),
        season: None,
        explicit: false,
        description: None,
        track_artist: Some(artist_credit.display_name.clone()),
        track_artist_sort: None,
        created_at: now,
        updated_at: now,
    };
    let source_entity_ids = vec![stophammer::model::SourceEntityIdClaim {
        id: None,
        feed_guid: feed.feed_guid.clone(),
        entity_type: "feed".into(),
        entity_id: feed.feed_guid.clone(),
        position: 0,
        scheme: "nostr_npub".into(),
        value: "npub1promoteartist".into(),
        source: "podcast_txt".into(),
        extraction_path: "feed.podcast:txt[@purpose='npub']".into(),
        observed_at: now,
    }];
    let source_entity_links = vec![
        stophammer::model::SourceEntityLink {
            id: None,
            feed_guid: feed.feed_guid.clone(),
            entity_type: "feed".into(),
            entity_id: feed.feed_guid.clone(),
            position: 0,
            link_type: "website".into(),
            url: "https://wavlake.com/promote-artist".into(),
            source: "rss_link".into(),
            extraction_path: "feed.link".into(),
            observed_at: now,
        },
        stophammer::model::SourceEntityLink {
            id: None,
            feed_guid: feed.feed_guid.clone(),
            entity_type: "track".into(),
            entity_id: track.track_guid.clone(),
            position: 0,
            link_type: "web_page".into(),
            url: "https://wavlake.com/track/promote-track".into(),
            source: "rss_link".into(),
            extraction_path: "entity.link".into(),
            observed_at: now,
        },
    ];
    let source_item_enclosures = vec![stophammer::model::SourceItemEnclosure {
        id: None,
        feed_guid: feed.feed_guid.clone(),
        entity_type: "track".into(),
        entity_id: track.track_guid.clone(),
        position: 0,
        url: "https://cdn.example.com/promote-track.mp3".into(),
        mime_type: Some("audio/mpeg".into()),
        bytes: Some(1234),
        rel: None,
        title: None,
        is_primary: true,
        source: "rss_enclosure".into(),
        extraction_path: "track.enclosure".into(),
        observed_at: now,
    }];
    let tracks = vec![(track.clone(), vec![], vec![])];

    let event_rows = stophammer::db::build_diff_events(
        &conn,
        &artist,
        &artist_credit,
        &feed,
        &[],
        &[],
        &source_entity_ids,
        &source_entity_links,
        &[],
        &source_item_enclosures,
        &[],
        &[],
        &[],
        &tracks,
        &[],
        now,
        &[],
    )
    .expect("build diff events");

    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("canonical-promote.key");
    let signer = stophammer::signing::NodeSigner::load_or_create(&signer_path).expect("signer");

    stophammer::db::ingest_transaction(
        &mut conn,
        artist,
        artist_credit,
        feed.clone(),
        vec![],
        vec![],
        source_entity_ids,
        source_entity_links,
        vec![],
        source_item_enclosures,
        vec![],
        vec![],
        vec![],
        tracks,
        event_rows,
        &signer,
    )
    .expect("ingest transaction");

    stophammer::db::sync_canonical_state_for_feed(&conn, &feed.feed_guid)
        .expect("sync canonical state");

    let release_id: String = conn
        .query_row(
            "SELECT release_id FROM source_feed_release_map WHERE feed_guid = ?1",
            params![feed.feed_guid],
            |row| row.get(0),
        )
        .expect("release id");
    let recording_id: String = conn
        .query_row(
            "SELECT recording_id FROM source_item_recording_map WHERE track_guid = ?1",
            params![track.track_guid],
            |row| row.get(0),
        )
        .expect("recording id");

    let release_title: String = conn
        .query_row(
            "SELECT title FROM releases WHERE release_id = ?1",
            params![release_id],
            |row| row.get(0),
        )
        .expect("release title");
    assert_eq!(release_title, "Promote Release");

    let recording_title: String = conn
        .query_row(
            "SELECT title FROM recordings WHERE recording_id = ?1",
            params![recording_id],
            |row| row.get(0),
        )
        .expect("recording title");
    assert_eq!(recording_title, "Promote Track");

    let feed_ids = stophammer::db::get_source_entity_ids_for_entity(&conn, "feed", &feed.feed_guid)
        .expect("feed source ids");
    assert_eq!(feed_ids.len(), 1);
    assert_eq!(feed_ids[0].scheme, "nostr_npub");
    assert_eq!(feed_ids[0].value, "npub1promoteartist");

    let feed_links =
        stophammer::db::get_source_entity_links_for_entity(&conn, "feed", &feed.feed_guid)
            .expect("feed source links");
    assert_eq!(feed_links.len(), 1);
    assert_eq!(feed_links[0].link_type, "website");
    assert_eq!(
        feed_links[0].url,
        "https://wavlake.com/promote-artist".to_string()
    );

    let track_links =
        stophammer::db::get_source_entity_links_for_entity(&conn, "track", &track.track_guid)
            .expect("track source links");
    assert_eq!(track_links.len(), 1);
    assert_eq!(track_links[0].link_type, "web_page");
    assert_eq!(
        track_links[0].url,
        "https://wavlake.com/track/promote-track".to_string()
    );

    let track_enclosures =
        stophammer::db::get_source_item_enclosures_for_entity(&conn, "track", &track.track_guid)
            .expect("track source enclosures");
    assert_eq!(track_enclosures.len(), 1);
    assert!(track_enclosures[0].is_primary);
    assert_eq!(
        track_enclosures[0].url,
        "https://cdn.example.com/promote-track.mp3".to_string()
    );
}

#[test]
fn exact_mirror_feeds_cluster_into_one_release_and_recordings() {
    let mut conn = common::test_db();
    let now = common::now();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("mirror-cluster.key");
    let signer = stophammer::signing::NodeSigner::load_or_create(&signer_path).expect("signer");

    for (feed_guid, credit_id, feed_url, release_page_suffix, track_suffix) in [
        (
            "feed-mirror-a",
            9101,
            "https://wavlake.com/feed/music/mirror-a",
            "https://wavlake.com/mirror-artist",
            "a",
        ),
        (
            "feed-mirror-b",
            9102,
            "https://feeds.fountain.fm/mirror-b",
            "https://fountain.fm/mirror-artist",
            "b",
        ),
    ] {
        let artist = stophammer::model::Artist {
            artist_id: "artist-mirror-1".into(),
            name: "Mirror Artist".into(),
            name_lower: "mirror artist".into(),
            sort_name: None,
            type_id: None,
            area: None,
            img_url: None,
            url: None,
            begin_year: None,
            end_year: None,
            created_at: now,
            updated_at: now,
        };
        let artist_credit = stophammer::model::ArtistCredit {
            id: credit_id,
            display_name: "Mirror Artist".into(),
            feed_guid: Some(feed_guid.into()),
            created_at: now,
            names: vec![stophammer::model::ArtistCreditName {
                id: 0,
                artist_credit_id: credit_id,
                artist_id: "artist-mirror-1".into(),
                position: 0,
                name: "Mirror Artist".into(),
                join_phrase: String::new(),
            }],
        };
        let feed = stophammer::model::Feed {
            feed_guid: feed_guid.into(),
            feed_url: feed_url.into(),
            title: "Mirror Release".into(),
            title_lower: "mirror release".into(),
            artist_credit_id: credit_id,
            description: None,
            image_url: None,
            publisher: None,
            language: None,
            explicit: false,
            itunes_type: None,
            release_artist: Some("Mirror Artist".into()),
            release_artist_sort: None,
            release_date: Some(now),
            release_kind: None,
            episode_count: 2,
            newest_item_at: Some(now),
            oldest_item_at: Some(now - 120),
            created_at: now,
            updated_at: now,
            raw_medium: Some("music".into()),
        };
        let tracks = vec![
            (
                stophammer::model::Track {
                    track_guid: format!("track-mirror-{track_suffix}-1"),
                    feed_guid: feed_guid.into(),
                    artist_credit_id: credit_id,
                    title: "Shared Song A".into(),
                    title_lower: "shared song a".into(),
                    pub_date: Some(now),
                    duration_secs: Some(180),
                    image_url: None,
                    language: None,
                    enclosure_url: Some(format!(
                        "https://cdn.example.com/{track_suffix}/shared-song-a.mp3"
                    )),
                    enclosure_type: Some("audio/mpeg".into()),
                    enclosure_bytes: Some(1000),
                    track_number: Some(1),
                    season: None,
                    explicit: false,
                    description: None,
                    track_artist: Some("Mirror Artist".into()),
                    track_artist_sort: None,
                    created_at: now,
                    updated_at: now,
                },
                vec![],
                vec![],
            ),
            (
                stophammer::model::Track {
                    track_guid: format!("track-mirror-{track_suffix}-2"),
                    feed_guid: feed_guid.into(),
                    artist_credit_id: credit_id,
                    title: "Shared Song B".into(),
                    title_lower: "shared song b".into(),
                    pub_date: Some(now),
                    duration_secs: Some(240),
                    image_url: None,
                    language: None,
                    enclosure_url: Some(format!(
                        "https://cdn.example.com/{track_suffix}/shared-song-b.mp3"
                    )),
                    enclosure_type: Some("audio/mpeg".into()),
                    enclosure_bytes: Some(2000),
                    track_number: Some(2),
                    season: None,
                    explicit: false,
                    description: None,
                    track_artist: Some("Mirror Artist".into()),
                    track_artist_sort: None,
                    created_at: now,
                    updated_at: now,
                },
                vec![],
                vec![],
            ),
        ];
        let source_entity_links = vec![stophammer::model::SourceEntityLink {
            id: None,
            feed_guid: feed_guid.into(),
            entity_type: "feed".into(),
            entity_id: feed_guid.into(),
            position: 0,
            link_type: "website".into(),
            url: release_page_suffix.into(),
            source: "rss_link".into(),
            extraction_path: "feed.link".into(),
            observed_at: now,
        }];

        let event_rows = stophammer::db::build_diff_events(
            &conn,
            &artist,
            &artist_credit,
            &feed,
            &[],
            &[],
            &[],
            &source_entity_links,
            &[],
            &[],
            &[],
            &[],
            &[],
            &tracks,
            &[],
            now,
            &[],
        )
        .expect("build diff events");

        stophammer::db::ingest_transaction(
            &mut conn,
            artist,
            artist_credit,
            feed,
            vec![],
            vec![],
            vec![],
            source_entity_links,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            tracks,
            event_rows,
            &signer,
        )
        .expect("ingest transaction");
    }

    stophammer::db::sync_canonical_state_for_feed(&conn, "feed-mirror-a")
        .expect("sync mirror canonical a");
    stophammer::db::sync_canonical_state_for_feed(&conn, "feed-mirror-b")
        .expect("sync mirror canonical b");

    let release_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM releases", [], |row| row.get(0))
        .expect("count releases");
    let recording_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM recordings", [], |row| row.get(0))
        .expect("count recordings");
    assert_eq!(release_count, 1);
    assert_eq!(recording_count, 2);

    let release_ids: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT release_id FROM source_feed_release_map ORDER BY feed_guid")
            .expect("prepare release ids");
        stmt.query_map([], |row| row.get(0))
            .expect("query release ids")
            .collect::<Result<_, _>>()
            .expect("collect release ids")
    };
    assert_eq!(release_ids.len(), 2);
    assert_eq!(release_ids[0], release_ids[1]);

    let distinct_recording_ids: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT recording_id) FROM source_item_recording_map",
            [],
            |row| row.get(0),
        )
        .expect("count distinct recording ids");
    assert_eq!(distinct_recording_ids, 2);
}

#[test]
fn cross_platform_single_track_mirrors_cluster_despite_one_second_duration_drift() {
    let mut conn = common::test_db();
    let now = common::now();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("single-track-cluster.key");
    let signer = stophammer::signing::NodeSigner::load_or_create(&signer_path).expect("signer");

    for (feed_guid, credit_id, feed_url, platform_key, duration_secs, remote_items, platform_url) in [
        (
            "feed-single-fountain",
            9201,
            "https://feeds.fountain.fm/relaxed-single",
            "fountain",
            237,
            vec![],
            Some("https://feeds.fountain.fm/relaxed-single".to_string()),
        ),
        (
            "feed-single-wavlake",
            9202,
            "https://wavlake.com/feed/music/relaxed-single",
            "wavlake",
            238,
            vec![stophammer::model::FeedRemoteItemRaw {
                id: None,
                feed_guid: "feed-single-wavlake".into(),
                position: 0,
                medium: Some("publisher".into()),
                remote_feed_guid: "publisher-feed-guid-1".into(),
                remote_feed_url: Some("https://wavlake.com/relaxed-artist".into()),
                source: "podcast_remote_item".into(),
            }],
            Some("https://wavlake.com/relaxed-artist".to_string()),
        ),
    ] {
        let artist = stophammer::model::Artist {
            artist_id: "artist-relaxed-1".into(),
            name: "Relaxed Artist".into(),
            name_lower: "relaxed artist".into(),
            sort_name: None,
            type_id: None,
            area: None,
            img_url: None,
            url: None,
            begin_year: None,
            end_year: None,
            created_at: now,
            updated_at: now,
        };
        let artist_credit = stophammer::model::ArtistCredit {
            id: credit_id,
            display_name: "Relaxed Artist".into(),
            feed_guid: Some(feed_guid.into()),
            created_at: now,
            names: vec![stophammer::model::ArtistCreditName {
                id: 0,
                artist_credit_id: credit_id,
                artist_id: "artist-relaxed-1".into(),
                position: 0,
                name: "Relaxed Artist".into(),
                join_phrase: String::new(),
            }],
        };
        let feed = stophammer::model::Feed {
            feed_guid: feed_guid.into(),
            feed_url: feed_url.into(),
            title: "Relaxed Single".into(),
            title_lower: "relaxed single".into(),
            artist_credit_id: credit_id,
            description: None,
            image_url: None,
            publisher: None,
            language: None,
            explicit: false,
            itunes_type: None,
            release_artist: Some("Relaxed Artist".into()),
            release_artist_sort: None,
            release_date: Some(now),
            release_kind: None,
            episode_count: 1,
            newest_item_at: Some(now),
            oldest_item_at: Some(now - 60),
            created_at: now,
            updated_at: now,
            raw_medium: Some("music".into()),
        };
        let track = stophammer::model::Track {
            track_guid: format!("track-{feed_guid}"),
            feed_guid: feed_guid.into(),
            artist_credit_id: credit_id,
            title: "Relaxed Single".into(),
            title_lower: "relaxed single".into(),
            pub_date: Some(now),
            duration_secs: Some(duration_secs),
            image_url: None,
            language: None,
            enclosure_url: Some(format!("https://cdn.example.com/{feed_guid}.mp3")),
            enclosure_type: Some("audio/mpeg".into()),
            enclosure_bytes: Some(1234),
            track_number: Some(1),
            season: None,
            explicit: false,
            description: None,
            track_artist: Some("Relaxed Artist".into()),
            track_artist_sort: None,
            created_at: now,
            updated_at: now,
        };
        let source_platform_claims = vec![stophammer::model::SourcePlatformClaim {
            id: None,
            feed_guid: feed_guid.into(),
            platform_key: platform_key.into(),
            url: platform_url,
            owner_name: None,
            source: "platform_detector".into(),
            extraction_path: "request.canonical_url".into(),
            observed_at: now,
        }];
        let tracks = vec![(track, vec![], vec![])];

        let event_rows = stophammer::db::build_diff_events(
            &conn,
            &artist,
            &artist_credit,
            &feed,
            &remote_items,
            &[],
            &[],
            &[],
            &[],
            &[],
            &source_platform_claims,
            &[],
            &[],
            &tracks,
            &[],
            now,
            &[],
        )
        .expect("build diff events");

        stophammer::db::ingest_transaction(
            &mut conn,
            artist,
            artist_credit,
            feed,
            remote_items,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            source_platform_claims,
            vec![],
            vec![],
            tracks,
            event_rows,
            &signer,
        )
        .expect("ingest transaction");
    }

    stophammer::db::sync_canonical_state_for_feed(&conn, "feed-single-fountain")
        .expect("resync fountain feed");
    stophammer::db::sync_canonical_state_for_feed(&conn, "feed-single-wavlake")
        .expect("resync wavlake feed");

    let release_maps: Vec<(String, String, i64)> = {
        let mut stmt = conn
            .prepare(
                "SELECT release_id, match_type, confidence FROM source_feed_release_map ORDER BY feed_guid",
            )
            .expect("prepare release maps");
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .expect("query release maps")
            .collect::<Result<_, _>>()
            .expect("collect release maps")
    };
    assert_eq!(release_maps.len(), 2);
    assert_eq!(release_maps[0].0, release_maps[1].0);
    assert_eq!(release_maps[0].1, "single_track_cross_platform_release_v1");
    assert_eq!(release_maps[1].1, "single_track_cross_platform_release_v1");
    assert_eq!(release_maps[0].2, 92);
    assert_eq!(release_maps[1].2, 92);

    let recording_maps: Vec<(String, String, i64)> = {
        let mut stmt = conn
            .prepare(
                "SELECT recording_id, match_type, confidence FROM source_item_recording_map ORDER BY track_guid",
            )
            .expect("prepare recording maps");
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .expect("query recording maps")
            .collect::<Result<_, _>>()
            .expect("collect recording maps")
    };
    assert_eq!(recording_maps.len(), 2);
    assert_eq!(recording_maps[0].0, recording_maps[1].0);
    assert_eq!(
        recording_maps[0].1,
        "single_track_cross_platform_recording_v1"
    );
    assert_eq!(
        recording_maps[1].1,
        "single_track_cross_platform_recording_v1"
    );
    assert_eq!(recording_maps[0].2, 92);
    assert_eq!(recording_maps[1].2, 92);
}

#[test]
fn canonical_read_helpers_return_release_recording_and_source_evidence() {
    let mut conn = common::test_db();
    let now = common::now();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("canonical-read-helpers.key");
    let signer = stophammer::signing::NodeSigner::load_or_create(&signer_path).expect("signer");

    for (feed_guid, credit_id, feed_url, track_suffix, website_url) in [
        (
            "feed-canon-read-a",
            9301,
            "https://feeds.rssblue.com/canon-read-a",
            "a",
            "https://artist.example.com/releases/canon-read",
        ),
        (
            "feed-canon-read-b",
            9302,
            "https://wavlake.com/feed/music/canon-read-b",
            "b",
            "https://artist.example.com/releases/canon-read",
        ),
    ] {
        let artist = stophammer::model::Artist {
            artist_id: "artist-canon-read-1".into(),
            name: "Canon Read Artist".into(),
            name_lower: "canon read artist".into(),
            sort_name: None,
            type_id: None,
            area: None,
            img_url: None,
            url: None,
            begin_year: None,
            end_year: None,
            created_at: now,
            updated_at: now,
        };
        let artist_credit = stophammer::model::ArtistCredit {
            id: credit_id,
            display_name: "Canon Read Artist".into(),
            feed_guid: Some(feed_guid.into()),
            created_at: now,
            names: vec![stophammer::model::ArtistCreditName {
                id: 0,
                artist_credit_id: credit_id,
                artist_id: "artist-canon-read-1".into(),
                position: 0,
                name: "Canon Read Artist".into(),
                join_phrase: String::new(),
            }],
        };
        let feed = stophammer::model::Feed {
            feed_guid: feed_guid.into(),
            feed_url: feed_url.into(),
            title: "Canon Read Release".into(),
            title_lower: "canon read release".into(),
            artist_credit_id: credit_id,
            description: Some("A canonical read test release".into()),
            image_url: Some("https://cdn.example.com/canon-read-cover.jpg".into()),
            publisher: None,
            language: Some("en".into()),
            explicit: false,
            itunes_type: None,
            release_artist: Some("Canon Read Artist".into()),
            release_artist_sort: None,
            release_date: Some(now),
            release_kind: None,
            episode_count: 1,
            newest_item_at: Some(now),
            oldest_item_at: Some(now),
            created_at: now,
            updated_at: now,
            raw_medium: Some("music".into()),
        };
        let source_entity_links = vec![stophammer::model::SourceEntityLink {
            id: None,
            feed_guid: feed_guid.into(),
            entity_type: "feed".into(),
            entity_id: feed_guid.into(),
            position: 0,
            link_type: "website".into(),
            url: website_url.into(),
            source: "rss_link".into(),
            extraction_path: "feed.link".into(),
            observed_at: now,
        }];
        let source_platform_claims = vec![stophammer::model::SourcePlatformClaim {
            id: None,
            feed_guid: feed_guid.into(),
            platform_key: if feed_guid.ends_with('a') {
                "rss_blue".into()
            } else {
                "wavlake".into()
            },
            url: Some(feed_url.into()),
            owner_name: None,
            source: "feed_url".into(),
            extraction_path: "request.canonical_url".into(),
            observed_at: now,
        }];
        let source_item_enclosures = vec![stophammer::model::SourceItemEnclosure {
            id: None,
            feed_guid: feed_guid.into(),
            entity_type: "track".into(),
            entity_id: format!("track-canon-read-{track_suffix}"),
            position: 0,
            url: format!("https://cdn.example.com/{track_suffix}/canon-read-song.mp3"),
            mime_type: Some("audio/mpeg".into()),
            bytes: Some(2048),
            rel: None,
            title: None,
            is_primary: true,
            source: "enclosure".into(),
            extraction_path: "track.enclosure".into(),
            observed_at: now,
        }];
        let source_contributor_claims = vec![stophammer::model::SourceContributorClaim {
            id: None,
            feed_guid: feed_guid.into(),
            entity_type: "track".into(),
            entity_id: format!("track-canon-read-{track_suffix}"),
            position: 0,
            name: "Canon Read Artist".into(),
            role: Some("Vocals".into()),
            role_norm: Some("vocals".into()),
            group_name: None,
            href: None,
            img: None,
            source: "podcast_person".into(),
            extraction_path: "track.podcast:person[0]".into(),
            observed_at: now,
        }];
        let source_entity_ids = vec![stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: feed_guid.into(),
            entity_type: "feed".into(),
            entity_id: feed_guid.into(),
            position: 0,
            scheme: "nostr_npub".into(),
            value: "npub1canonreadartist".into(),
            source: "podcast_txt".into(),
            extraction_path: "feed.podcast:txt[@purpose='npub']".into(),
            observed_at: now,
        }];
        let source_release_claims = vec![stophammer::model::SourceReleaseClaim {
            id: None,
            feed_guid: feed_guid.into(),
            entity_type: "feed".into(),
            entity_id: feed_guid.into(),
            position: 0,
            claim_type: "release_date".into(),
            claim_value: now.to_string(),
            source: "rss_pub_date".into(),
            extraction_path: "feed.pubDate".into(),
            observed_at: now,
        }];
        let tracks = vec![(
            stophammer::model::Track {
                track_guid: format!("track-canon-read-{track_suffix}"),
                feed_guid: feed_guid.into(),
                artist_credit_id: credit_id,
                title: "Canon Read Song".into(),
                title_lower: "canon read song".into(),
                pub_date: Some(now),
                duration_secs: Some(201),
                image_url: None,
                language: Some("en".into()),
                enclosure_url: Some(format!(
                    "https://cdn.example.com/{track_suffix}/canon-read-song.mp3"
                )),
                enclosure_type: Some("audio/mpeg".into()),
                enclosure_bytes: Some(2048),
                track_number: Some(1),
                season: None,
                explicit: false,
                description: Some("A test song".into()),
                track_artist: Some("Canon Read Artist".into()),
                track_artist_sort: None,
                created_at: now,
                updated_at: now,
            },
            vec![],
            vec![],
        )];

        let event_rows = stophammer::db::build_diff_events(
            &conn,
            &artist,
            &artist_credit,
            &feed,
            &[],
            &source_contributor_claims,
            &source_entity_ids,
            &source_entity_links,
            &source_release_claims,
            &source_item_enclosures,
            &source_platform_claims,
            &[],
            &[],
            &tracks,
            &[],
            now,
            &[],
        )
        .expect("build diff events");

        stophammer::db::ingest_transaction(
            &mut conn,
            artist,
            artist_credit,
            feed,
            vec![],
            source_contributor_claims,
            source_entity_ids,
            source_entity_links,
            source_release_claims,
            source_item_enclosures,
            source_platform_claims,
            vec![],
            vec![],
            tracks,
            event_rows,
            &signer,
        )
        .expect("ingest transaction");
    }

    stophammer::db::sync_canonical_state_for_feed(&conn, "feed-canon-read-a")
        .expect("sync canonical state a");
    stophammer::db::sync_canonical_state_for_feed(&conn, "feed-canon-read-b")
        .expect("sync canonical state b");

    let release_id: String = conn
        .query_row(
            "SELECT DISTINCT release_id FROM source_feed_release_map WHERE feed_guid = 'feed-canon-read-a'",
            [],
            |row| row.get(0),
        )
        .expect("release id");
    let recording_id: String = conn
        .query_row(
            "SELECT DISTINCT recording_id FROM source_item_recording_map WHERE track_guid = 'track-canon-read-a'",
            [],
            |row| row.get(0),
        )
        .expect("recording id");

    let (release_title, release_desc): (String, Option<String>) = conn
        .query_row(
            "SELECT title, description FROM releases WHERE release_id = ?1",
            params![release_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("release row");
    assert_eq!(release_title, "Canon Read Release");
    assert_eq!(
        release_desc.as_deref(),
        Some("A canonical read test release")
    );

    let recording_title: String = conn
        .query_row(
            "SELECT title FROM recordings WHERE recording_id = ?1",
            params![recording_id],
            |row| row.get(0),
        )
        .expect("recording row");
    assert_eq!(recording_title, "Canon Read Song");

    let release_track_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM release_recordings WHERE release_id = ?1 AND recording_id = ?2",
            params![release_id, recording_id],
            |row| row.get(0),
        )
        .expect("release track count");
    assert_eq!(release_track_count, 1);

    let release_map_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM source_feed_release_map WHERE release_id = ?1",
            params![release_id],
            |row| row.get(0),
        )
        .expect("release map count");
    assert_eq!(release_map_count, 2);

    let recording_map_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM source_item_recording_map WHERE recording_id = ?1",
            params![recording_id],
            |row| row.get(0),
        )
        .expect("recording map count");
    assert_eq!(recording_map_count, 2);

    let feed_links =
        stophammer::db::get_source_entity_links_for_entity(&conn, "feed", "feed-canon-read-a")
            .expect("feed links");
    assert_eq!(feed_links.len(), 1);
    assert_eq!(feed_links[0].link_type, "website");

    let feed_ids =
        stophammer::db::get_source_entity_ids_for_entity(&conn, "feed", "feed-canon-read-a")
            .expect("feed ids");
    assert_eq!(feed_ids.len(), 1);
    assert_eq!(feed_ids[0].scheme, "nostr_npub");

    let track_contributors = stophammer::db::get_source_contributor_claims_for_entity(
        &conn,
        "track",
        "track-canon-read-a",
    )
    .expect("track contributors");
    assert_eq!(track_contributors.len(), 1);
    assert_eq!(track_contributors[0].role_norm.as_deref(), Some("vocals"));

    let feed_release_claims =
        stophammer::db::get_source_release_claims_for_entity(&conn, "feed", "feed-canon-read-a")
            .expect("feed release claims");
    assert_eq!(feed_release_claims.len(), 1);
    assert_eq!(feed_release_claims[0].claim_type, "release_date");

    let track_enclosures =
        stophammer::db::get_source_item_enclosures_for_entity(&conn, "track", "track-canon-read-a")
            .expect("track enclosures");
    assert_eq!(track_enclosures.len(), 1);
    assert!(track_enclosures[0].is_primary);

    let feed_platforms =
        stophammer::db::get_source_platform_claims_for_feed(&conn, "feed-canon-read-a")
            .expect("feed platforms");
    assert_eq!(feed_platforms.len(), 1);
    assert_eq!(feed_platforms[0].platform_key, "rss_blue");
}

#[test]
fn canonical_rebuild_prefers_richer_source_metadata_over_smallest_guid() {
    let mut conn = common::test_db();
    let now = common::now();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("canonical-representative.key");
    let signer = stophammer::signing::NodeSigner::load_or_create(&signer_path).expect("signer");

    for (
        feed_guid,
        credit_id,
        feed_description,
        image_url,
        oldest_item_at,
        feed_updated_at,
        track_guid,
        track_description,
        track_pub_date,
        track_updated_at,
    ) in [
        (
            "feed-meta-a",
            9401,
            None,
            None,
            None,
            now - 500,
            "track-meta-a",
            None,
            None,
            now - 500,
        ),
        (
            "feed-meta-z",
            9402,
            Some("Preferred release description"),
            Some("https://cdn.example.com/preferred-cover.jpg"),
            Some(now - 60),
            now,
            "track-meta-z",
            Some("Preferred track description"),
            Some(now - 30),
            now,
        ),
    ] {
        let artist = stophammer::model::Artist {
            artist_id: format!("artist-meta-{credit_id}"),
            name: "Metadata Artist".into(),
            name_lower: "metadata artist".into(),
            sort_name: None,
            type_id: None,
            area: None,
            img_url: None,
            url: None,
            begin_year: None,
            end_year: None,
            created_at: now,
            updated_at: now,
        };
        let artist_credit = stophammer::model::ArtistCredit {
            id: credit_id,
            display_name: "Metadata Artist".into(),
            feed_guid: Some(feed_guid.into()),
            created_at: now,
            names: vec![stophammer::model::ArtistCreditName {
                id: 0,
                artist_credit_id: credit_id,
                artist_id: format!("artist-meta-{credit_id}"),
                position: 0,
                name: "Metadata Artist".into(),
                join_phrase: String::new(),
            }],
        };
        let feed = stophammer::model::Feed {
            feed_guid: feed_guid.into(),
            feed_url: format!("https://example.com/{feed_guid}.xml"),
            title: "Representative Release".into(),
            title_lower: "representative release".into(),
            artist_credit_id: credit_id,
            description: feed_description.map(str::to_string),
            image_url: image_url.map(str::to_string),
            publisher: None,
            language: None,
            explicit: false,
            itunes_type: None,
            release_artist: Some("Metadata Artist".into()),
            release_artist_sort: None,
            release_date: oldest_item_at,
            release_kind: None,
            episode_count: 1,
            newest_item_at: oldest_item_at,
            oldest_item_at,
            created_at: now,
            updated_at: feed_updated_at,
            raw_medium: Some("music".into()),
        };
        let tracks = vec![(
            stophammer::model::Track {
                track_guid: track_guid.into(),
                feed_guid: feed_guid.into(),
                artist_credit_id: credit_id,
                title: "Representative Song".into(),
                title_lower: "representative song".into(),
                pub_date: track_pub_date,
                duration_secs: Some(200),
                image_url: image_url.map(str::to_string),
                language: None,
                enclosure_url: Some(format!("https://cdn.example.com/{track_guid}.mp3")),
                enclosure_type: Some("audio/mpeg".into()),
                enclosure_bytes: Some(2048),
                track_number: Some(1),
                season: None,
                explicit: false,
                description: track_description.map(str::to_string),
                track_artist: Some("Metadata Artist".into()),
                track_artist_sort: None,
                created_at: now,
                updated_at: track_updated_at,
            },
            vec![],
            vec![],
        )];

        let event_rows = stophammer::db::build_diff_events(
            &conn,
            &artist,
            &artist_credit,
            &feed,
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &tracks,
            &[],
            now,
            &[],
        )
        .expect("build diff events");

        stophammer::db::ingest_transaction(
            &mut conn,
            artist,
            artist_credit,
            feed,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            tracks,
            event_rows,
            &signer,
        )
        .expect("ingest transaction");
    }

    stophammer::db::sync_canonical_state_for_feed(&conn, "feed-meta-a")
        .expect("sync canonical state a");
    stophammer::db::sync_canonical_state_for_feed(&conn, "feed-meta-z")
        .expect("sync canonical state z");

    let release_id: String = conn
        .query_row(
            "SELECT DISTINCT release_id FROM source_feed_release_map WHERE feed_guid = 'feed-meta-a'",
            [],
            |row| row.get(0),
        )
        .expect("release id");
    let (release_artist_credit_id, release_description, release_image_url, release_date): (
        i64,
        Option<String>,
        Option<String>,
        Option<i64>,
    ) = conn
        .query_row(
            "SELECT artist_credit_id, description, image_url, release_date FROM releases WHERE release_id = ?1",
            params![release_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("release row");
    assert_eq!(release_artist_credit_id, 9402);
    assert_eq!(
        release_description.as_deref(),
        Some("Preferred release description")
    );
    assert_eq!(
        release_image_url.as_deref(),
        Some("https://cdn.example.com/preferred-cover.jpg")
    );
    assert_eq!(release_date, Some(now - 60));

    let recording_id: String = conn
        .query_row(
            "SELECT DISTINCT recording_id FROM source_item_recording_map WHERE track_guid = 'track-meta-a'",
            [],
            |row| row.get(0),
        )
        .expect("recording id");
    let recording_artist_credit_id: i64 = conn
        .query_row(
            "SELECT artist_credit_id FROM recordings WHERE recording_id = ?1",
            params![recording_id],
            |row| row.get(0),
        )
        .expect("recording artist credit");
    assert_eq!(recording_artist_credit_id, 9402);
}

// ---------------------------------------------------------------------------
// Helper: insert an artist and return its artist_id.
// ---------------------------------------------------------------------------

fn insert_artist(conn: &rusqlite::Connection, id: &str, name: &str) -> String {
    let now = common::now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, sort_name, type_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
        params![id, name, name.to_lowercase(), name, now],
    )
    .unwrap();
    id.to_string()
}

/// Create an artist credit for a single artist and return the credit id.
fn insert_single_credit(conn: &rusqlite::Connection, artist_id: &str, display: &str) -> i64 {
    let now = common::now();
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        params![display, now],
    )
    .unwrap();
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name)
         VALUES (?1, ?2, 0, ?3)",
        params![credit_id, artist_id, display],
    )
    .unwrap();
    credit_id
}

/// Insert a minimal feed and return its `feed_guid`.
fn insert_feed(conn: &rusqlite::Connection, guid: &str, credit_id: i64) -> String {
    let now = common::now();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            guid,
            format!("https://example.com/{guid}"),
            "Test Feed",
            "test feed",
            credit_id,
            now,
        ],
    )
    .unwrap();
    guid.to_string()
}

/// Insert a minimal track and return its `track_guid`.
fn insert_track(
    conn: &rusqlite::Connection,
    track_guid: &str,
    feed_guid: &str,
    credit_id: i64,
) -> String {
    let now = common::now();
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![track_guid, feed_guid, credit_id, "Test Track", "test track", now],
    )
    .unwrap();
    track_guid.to_string()
}

// ---------------------------------------------------------------------------
// 4. Artist insert + alias auto-registration
// ---------------------------------------------------------------------------

#[test]
fn artist_insert_and_alias() {
    let conn = common::test_db();
    let now = common::now();
    let id = "art-001";
    insert_artist(&conn, id, "Alice Band");

    // Manually register an alias (production code does this on insert).
    conn.execute(
        "INSERT OR IGNORE INTO artist_aliases (alias_lower, artist_id, created_at)
         VALUES (?1, ?2, ?3)",
        params!["alice band", id, now],
    )
    .unwrap();

    let alias_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artist_aliases WHERE artist_id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(alias_count, 1);
}

// ---------------------------------------------------------------------------
// 5. Artist resolve via alias
// ---------------------------------------------------------------------------

#[test]
fn artist_resolve_via_alias() {
    let conn = common::test_db();
    let now = common::now();
    let id = "art-002";
    insert_artist(&conn, id, "The Rolling Stones");

    // Register a shortened alias.
    conn.execute(
        "INSERT OR IGNORE INTO artist_aliases (alias_lower, artist_id, created_at)
         VALUES (?1, ?2, ?3)",
        params!["rolling stones", id, now],
    )
    .unwrap();

    let resolved: String = conn
        .query_row(
            "SELECT artist_id FROM artist_aliases WHERE alias_lower = ?1",
            params!["rolling stones"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(resolved, id);
}

// ---------------------------------------------------------------------------
// 6. Artist resolve via name_lower
// ---------------------------------------------------------------------------

#[test]
fn artist_resolve_via_name_lower() {
    let conn = common::test_db();
    insert_artist(&conn, "art-003", "Portishead");

    let resolved: String = conn
        .query_row(
            "SELECT artist_id FROM artists WHERE name_lower = ?1",
            params!["portishead"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(resolved, "art-003");
}

// ---------------------------------------------------------------------------
// 7. Artist resolve creates new
// ---------------------------------------------------------------------------

#[test]
fn artist_resolve_creates_new() {
    let conn = common::test_db();
    let name = "Brand New Artist";
    let name_lower = name.to_lowercase();

    // Lookup — should find nothing.
    let existing = conn.query_row(
        "SELECT artist_id FROM artists WHERE name_lower = ?1",
        params![&name_lower],
        |r| r.get::<_, String>(0),
    );
    assert!(existing.is_err());

    // Create on miss.
    let new_id = "art-new-001";
    insert_artist(&conn, new_id, name);

    let resolved: String = conn
        .query_row(
            "SELECT artist_id FROM artists WHERE name_lower = ?1",
            params![&name_lower],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(resolved, new_id);
}

// ---------------------------------------------------------------------------
// 8. Artist merge
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 9. Artist credit creation — single and multi-artist
// ---------------------------------------------------------------------------

#[test]
fn artist_credit_single() {
    let conn = common::test_db();
    insert_artist(&conn, "art-s1", "Solo");
    let cid = insert_single_credit(&conn, "art-s1", "Solo");

    let display: String = conn
        .query_row(
            "SELECT display_name FROM artist_credit WHERE id = ?1",
            params![cid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(display, "Solo");
}

#[test]
fn artist_credit_multi() {
    let conn = common::test_db();
    let now = common::now();
    insert_artist(&conn, "art-m1", "Alice");
    insert_artist(&conn, "art-m2", "Bob");

    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        params!["Alice & Bob", now],
    )
    .unwrap();
    let cid = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase)
         VALUES (?1, 'art-m1', 0, 'Alice', ' & ')",
        params![cid],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase)
         VALUES (?1, 'art-m2', 1, 'Bob', '')",
        params![cid],
    )
    .unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artist_credit_name WHERE artist_credit_id = ?1",
            params![cid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

// ---------------------------------------------------------------------------
// 10. Feed upsert
// ---------------------------------------------------------------------------

#[test]
fn feed_upsert() {
    let conn = common::test_db();
    let now = common::now();
    insert_artist(&conn, "art-f1", "Feed Artist");
    let cid = insert_single_credit(&conn, "art-f1", "Feed Artist");
    let guid = "feed-001";

    // Initial insert.
    insert_feed(&conn, guid, cid);
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
    insert_artist(&conn, "art-t1", "Track Artist");
    let cid = insert_single_credit(&conn, "art-t1", "Track Artist");
    let fg = insert_feed(&conn, "feed-t1", cid);
    let tg = "track-001";

    insert_track(&conn, tg, &fg, cid);

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
    insert_artist(&conn, "art-pr", "PR Artist");
    let cid = insert_single_credit(&conn, "art-pr", "PR Artist");
    let fg = insert_feed(&conn, "feed-pr", cid);
    let tg = insert_track(&conn, "track-pr", &fg, cid);

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
    insert_artist(&conn, "art-fpr", "FPR Artist");
    let cid = insert_single_credit(&conn, "art-fpr", "FPR Artist");
    let fg = insert_feed(&conn, "feed-fpr", cid);

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
    insert_artist(&conn, "art-vts", "VTS Artist");
    let cid = insert_single_credit(&conn, "art-vts", "VTS Artist");
    let fg = insert_feed(&conn, "feed-vts", cid);
    let tg = insert_track(&conn, "track-vts", &fg, cid);

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
#[expect(
    clippy::too_many_lines,
    reason = "integration test exercises full transaction atomicity"
)]
fn ingest_transaction_atomicity() {
    let conn = common::test_db();
    let now = common::now();

    // Simulate a full atomic ingest: artist + credit + feed + track + routes + event.
    conn.execute_batch("BEGIN").unwrap();

    // Artist.
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, sort_name, type_id, created_at, updated_at)
         VALUES ('art-txn', 'Txn Artist', 'txn artist', 'Txn Artist', 1, ?1, ?1)",
        params![now],
    )
    .unwrap();

    // Alias.
    conn.execute(
        "INSERT OR IGNORE INTO artist_aliases (alias_lower, artist_id, created_at)
         VALUES ('txn artist', 'art-txn', ?1)",
        params![now],
    )
    .unwrap();

    // Credit.
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES ('Txn Artist', ?1)",
        params![now],
    )
    .unwrap();
    let cid = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name)
         VALUES (?1, 'art-txn', 0, 'Txn Artist')",
        params![cid],
    )
    .unwrap();

    // Feed.
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at)
         VALUES ('feed-txn', 'https://example.com/txn', 'Txn Album', 'txn album', ?1, ?2, ?2)",
        params![cid, now],
    )
    .unwrap();

    // Track.
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, duration_secs, created_at, updated_at)
         VALUES ('track-txn', 'feed-txn', ?1, 'Txn Song', 'txn song', ?2, 240, ?2, ?2)",
        params![cid, now],
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
    let artist_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM artists WHERE artist_id = 'art-txn'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(artist_exists);

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

    let alias_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM artist_aliases WHERE alias_lower = 'txn artist' AND artist_id = 'art-txn'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(alias_exists);
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
        "INSERT INTO artists (artist_id, name, name_lower, sort_name, type_id, created_at, updated_at)
         VALUES ('art-rb', 'Rollback Artist', 'rollback artist', 'Rollback Artist', 1, ?1, ?1)",
        params![now],
    )
    .unwrap();

    conn.execute_batch("ROLLBACK").unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE artist_id = 'art-rb'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "rollback should have removed the artist");
}

#[test]
fn source_contributor_claims_replace_round_trip() {
    let conn = common::test_db();
    let artist_id = insert_artist(&conn, "art-src-claims", "Source Claims");
    let credit_id = insert_single_credit(&conn, &artist_id, "Source Claims");
    let feed_guid = insert_feed(&conn, "feed-src-claims", credit_id);

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
    let artist_id = insert_artist(&conn, "art-src-claims-dedupe", "Source Claims Dedupe");
    let credit_id = insert_single_credit(&conn, &artist_id, "Source Claims Dedupe");
    let feed_guid = insert_feed(&conn, "feed-src-claims-dedupe", credit_id);

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
    let artist_id = insert_artist(&conn, "art-src-ids", "Source IDs");
    let credit_id = insert_single_credit(&conn, &artist_id, "Source IDs");
    let feed_guid = insert_feed(&conn, "feed-src-ids", credit_id);

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
    let artist_id = insert_artist(&conn, "art-src-links", "Source Links");
    let credit_id = insert_single_credit(&conn, &artist_id, "Source Links");
    let feed_guid = insert_feed(&conn, "feed-src-links", credit_id);

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
    let artist_id = insert_artist(&conn, "art-src-links-dedupe", "Source Links Dedupe");
    let credit_id = insert_single_credit(&conn, &artist_id, "Source Links Dedupe");
    let feed_guid = insert_feed(&conn, "feed-src-links-dedupe", credit_id);

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
    let artist_id = insert_artist(&conn, "art-src-release", "Source Release");
    let credit_id = insert_single_credit(&conn, &artist_id, "Source Release");
    let feed_guid = insert_feed(&conn, "feed-src-release", credit_id);

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
    let artist_id = insert_artist(&conn, "art-src-release-dedupe", "Source Release Dedupe");
    let credit_id = insert_single_credit(&conn, &artist_id, "Source Release Dedupe");
    let feed_guid = insert_feed(&conn, "feed-src-release-dedupe", credit_id);

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
    let artist_id = insert_artist(&conn, "art-src-platform", "Source Platform");
    let credit_id = insert_single_credit(&conn, &artist_id, "Source Platform");
    let feed_guid = insert_feed(&conn, "feed-src-platform", credit_id);

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

    let artist = seed_artist(&conn, "artist-feed-claim-ingest", "Claim Artist");
    let artist_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist.name,
        &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
        Some("feed-claim-ingest"),
    )
    .expect("artist credit");

    let feed = stophammer::model::Feed {
        feed_guid: "feed-claim-ingest".into(),
        feed_url: "https://example.com/feed-claim-ingest.xml".into(),
        title: "Claim Feed".into(),
        title_lower: "claim feed".into(),
        artist_credit_id: artist_credit.id,
        description: None,
        image_url: None,
        publisher: None,
        language: Some("en".into()),
        explicit: false,
        itunes_type: None,
        release_artist: Some(artist_credit.display_name.clone()),
        release_artist_sort: None,
        release_date: Some(now),
        release_kind: None,
        episode_count: 0,
        newest_item_at: None,
        oldest_item_at: None,
        created_at: now,
        updated_at: now,
        raw_medium: Some("music".into()),
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
        &artist,
        &artist_credit,
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
        artist,
        artist_credit,
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
    assert_eq!(stored_entity_id_claims.len(), 2);
    assert_eq!(stored_contributor_claims[1].entity_type, "live_item");
    assert_eq!(stored_entity_id_claims[0].scheme, "nostr_npub");
}
