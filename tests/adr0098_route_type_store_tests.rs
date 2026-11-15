mod common;

use std::fs;
use std::path::PathBuf;

use rusqlite::Connection;
use stophammer::model::{PaymentRoute, RouteType};

fn database_at_migration_0064() -> Connection {
    let conn = Connection::open_in_memory().expect("open a database");
    let mut paths: Vec<PathBuf> = fs::read_dir("migrations")
        .expect("read migration directory")
        .map(|entry| entry.expect("read migration entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .collect();
    paths.sort();

    for path in paths {
        let name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .expect("migration path has a UTF-8 file name");
        if name > "0064_live_content_link_not_stream.sql" {
            break;
        }
        let sql = fs::read_to_string(path).expect("read migration");
        let needs_foreign_keys_off = sql.lines().next() == Some("-- stophammer: foreign_keys=off");
        if needs_foreign_keys_off {
            conn.execute_batch("PRAGMA foreign_keys = OFF;")
                .expect("disable foreign keys");
        }
        conn.execute_batch(&sql).expect("apply migration");
        if needs_foreign_keys_off {
            conn.execute_batch("PRAGMA foreign_keys = ON;")
                .expect("enable foreign keys");
        }
    }

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, created_at, updated_at)
         VALUES ('feed-0098', 'https://example.com/feed.xml', 'Feed', 'feed', 1, 1)",
        [],
    )
    .expect("insert feed");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, title, title_lower, created_at, updated_at)
         VALUES ('track-0098', 'feed-0098', 'Track', 'track', 1, 1)",
        [],
    )
    .expect("insert track");
    conn.execute(
        "INSERT INTO payment_routes
         (track_guid, feed_guid, route_type, address, split)
         VALUES ('track-0098', 'feed-0098', 'node', 'track-address', 100)",
        [],
    )
    .expect("insert track route");
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, route_type, address, split)
         VALUES ('feed-0098', 'wallet', 'feed-address', 100)",
        [],
    )
    .expect("insert feed route");
    conn.execute(
        "INSERT INTO feed_list_value_raw
         (feed_guid, route_type, address, split, position)
         VALUES ('feed-0098', 'keysend', 'list-address', 100, 0)",
        [],
    )
    .expect("insert list value");
    conn.execute(
        "INSERT INTO value_time_splits
         (id, source_feed_guid, source_track_guid, position, form, start_time_secs,
          remote_feed_guid, created_at)
         VALUES (1, 'feed-0098', 'track-0098', 0, 'remote', 0, 'remote-feed', 1)",
        [],
    )
    .expect("insert value time split");
    conn.execute(
        "INSERT INTO value_time_split_recipients
         (split_id, position, route_type, address, split)
         VALUES (1, 0, 'lnaddress', 'split-address', 100)",
        [],
    )
    .expect("insert split recipient");
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("disable foreign keys for migration 0065");
    conn.execute_batch(include_str!("../migrations/0065_value_block_fields.sql"))
        .expect("apply migration 0065");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign keys after migration 0065");
    conn
}

#[test]
fn migration_0065_preserves_rows_and_adds_null_metadata() {
    let conn = database_at_migration_0064();
    for table in [
        "payment_routes",
        "feed_payment_routes",
        "feed_list_value_raw",
        "value_time_split_recipients",
        "value_time_splits",
    ] {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count preserved rows");
        assert_eq!(count, 1, "{table} keeps its row");
        let null_fields: i64 = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM {table}
                     WHERE value_type IS NULL AND value_method IS NULL AND block_index IS NULL"
                ),
                [],
                |row| row.get(0),
            )
            .expect("count rows with null metadata");
        assert_eq!(null_fields, 1, "{table} has null metadata");
    }
    for (table, expected_type, expected_address) in [
        ("payment_routes", "node", "track-address"),
        ("feed_payment_routes", "wallet", "feed-address"),
        ("feed_list_value_raw", "keysend", "list-address"),
        ("value_time_split_recipients", "lnaddress", "split-address"),
    ] {
        let saved: (String, String) = conn
            .query_row(
                &format!("SELECT route_type, address FROM {table}"),
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read saved route");
        assert_eq!(
            saved,
            (expected_type.to_owned(), expected_address.to_owned()),
            "{table} keeps its route values"
        );
    }
    let split: (String, String, f64, String) = conn
        .query_row(
            "SELECT form, remote_feed_guid, start_time_secs, source_track_guid
             FROM value_time_splits",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("read saved value time split");
    assert_eq!(
        split,
        (
            "remote".to_owned(),
            "remote-feed".to_owned(),
            0.0,
            "track-0098".to_owned()
        )
    );
}

#[test]
fn migration_0065_accepts_other_route_text_in_each_route_table() {
    let conn = database_at_migration_0064();
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, route_type, address, split)
         VALUES ('track-0098', 'feed-0098', 'address', 'new-track', 100)",
        [],
    )
    .expect("insert other track route type");
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, route_type, address, split)
         VALUES ('feed-0098', 'address', 'new-feed', 100)",
        [],
    )
    .expect("insert other feed route type");
    conn.execute(
        "INSERT INTO feed_list_value_raw (feed_guid, route_type, address, split, position)
         VALUES ('feed-0098', 'address', 'new-list', 100, 1)",
        [],
    )
    .expect("insert other list route type");
    conn.execute(
        "INSERT INTO value_time_split_recipients
         (split_id, position, route_type, address, split)
         VALUES (1, 1, 'address', 'new-split', 100)",
        [],
    )
    .expect("insert other split route type");
}

#[test]
fn migration_0065_keeps_indexes_and_track_cleanup() {
    let conn = database_at_migration_0064();
    let indexes: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN
             ('idx_routes_track_0032', 'idx_routes_feed_0032',
              'idx_routes_feed_track_0032', 'idx_feed_routes_guid',
              'idx_feed_list_value_guid', 'idx_vts_recipients_split')",
            [],
            |row| row.get(0),
        )
        .expect("count required indexes");
    assert_eq!(indexes, 6);
    let triggers: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name IN
             ('trg_feeds_cleanup_before_delete', 'trg_tracks_cleanup_before_delete')",
            [],
            |row| row.get(0),
        )
        .expect("count required triggers");
    assert_eq!(triggers, 2);

    conn.execute(
        "DELETE FROM tracks WHERE feed_guid = 'feed-0098' AND track_guid = 'track-0098'",
        [],
    )
    .expect("delete track");
    for table in [
        "payment_routes",
        "value_time_splits",
        "value_time_split_recipients",
    ] {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count cleaned rows");
        assert_eq!(count, 0, "track deletion cleans {table}");
    }
}

#[test]
fn route_type_roundtrips_other_text_and_reads_known_text() {
    let other = RouteType::Other("address".to_owned());
    assert_eq!(
        serde_json::to_string(&other).expect("serialize route"),
        "\"address\""
    );
    assert_eq!(
        serde_json::from_str::<RouteType>("\"address\"").expect("read route"),
        other
    );
    assert_eq!(
        serde_json::from_str::<RouteType>("\"keysend\"").expect("read known route"),
        RouteType::Keysend
    );
}

#[test]
fn store_reads_an_unrecognized_route_type_as_other() {
    let conn = common::test_db();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, created_at, updated_at)
         VALUES ('feed-store-0098', 'https://example.com/store.xml', 'Feed', 'feed', 1, 1)",
        [],
    )
    .expect("insert feed");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, title, title_lower, created_at, updated_at)
         VALUES ('track-store-0098', 'feed-store-0098', 'Track', 'track', 1, 1)",
        [],
    )
    .expect("insert track");
    stophammer::db::replace_payment_routes(
        &conn,
        "track-store-0098",
        &[PaymentRoute {
            id: None,
            track_guid: "track-store-0098".to_owned(),
            feed_guid: "feed-store-0098".to_owned(),
            recipient_name: None,
            route_type: RouteType::Other("address".to_owned()),
            address: "route-address".to_owned(),
            custom_key: None,
            custom_value: None,
            split: 100,
            fee: false,

            value_type: None,
            value_method: None,
            block_index: None,
        }],
    )
    .expect("store route");
    let routes = stophammer::db::get_payment_routes_for_track(&conn, "track-store-0098")
        .expect("read routes");
    assert_eq!(routes[0].route_type, RouteType::Other("address".to_owned()));
}
