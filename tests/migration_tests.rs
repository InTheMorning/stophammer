// Issue-MIGRATIONS — 2026-03-14

mod common;

use std::fs;
use std::path::PathBuf;

const ALLOWED_DROP_TABLE_LINES: &[&str] = &[
    "DROP TABLE IF EXISTS source_item_recording_map;",
    "DROP TABLE IF EXISTS source_feed_release_map;",
    "DROP TABLE IF EXISTS release_recordings;",
    "DROP TABLE IF EXISTS recordings;",
    "DROP TABLE IF EXISTS releases;",
    "DROP TABLE IF EXISTS wallet_merge_apply_entry;",
    "DROP TABLE IF EXISTS wallet_merge_apply_batch;",
    "DROP TABLE IF EXISTS wallet_identity_override;",
    "DROP TABLE IF EXISTS wallet_identity_review;",
    "DROP TABLE IF EXISTS wallet_identity_review_legacy_0023;",
    "DROP TABLE IF EXISTS wallet_identity_review_legacy_0024;",
    "DROP TABLE IF EXISTS wallet_artist_links;",
    "DROP TABLE IF EXISTS wallet_id_redirect;",
    "DROP TABLE IF EXISTS wallet_feed_route_map;",
    "DROP TABLE IF EXISTS wallet_track_route_map;",
    "DROP TABLE IF EXISTS wallet_aliases;",
    "DROP TABLE IF EXISTS wallet_endpoints;",
    "DROP TABLE IF EXISTS wallets;",
    "DROP TABLE IF EXISTS proof_challenges;",
    "DROP TABLE IF EXISTS proof_tokens;",
    // Migration 0047 (ADR 0034 §11): the feeds/tracks rebuild, and every
    // table the ADR lists to drop.
    "DROP TABLE feeds;",
    "DROP TABLE tracks;",
    "DROP TABLE IF EXISTS artist_credit_name;",
    "DROP TABLE IF EXISTS artist_aliases;",
    "DROP TABLE IF EXISTS artist_credit;",
    "DROP TABLE IF EXISTS artists;",
    "DROP TABLE IF EXISTS artist_type;",
    "DROP TABLE IF EXISTS rel_type;",
    "DROP TABLE IF EXISTS external_ids;",
    "DROP TABLE IF EXISTS feed_rel;",
    "DROP TABLE IF EXISTS track_rel;",
    "DROP TABLE IF EXISTS artist_artist_rel;",
    "DROP TABLE IF EXISTS artist_tag;",
    "DROP TABLE IF EXISTS artist_id_redirect;",
    "DROP TABLE IF EXISTS feed_tag;",
    "DROP TABLE IF EXISTS track_tag;",
    "DROP TABLE IF EXISTS tags;",
    "DROP TABLE IF EXISTS resolver_queue;",
    "DROP TABLE IF EXISTS resolver_state;",
    "DROP TABLE IF EXISTS resolved_entity_sources_by_feed;",
    "DROP TABLE IF EXISTS resolved_external_ids_by_feed;",
    "DROP TABLE IF EXISTS artist_identity_override;",
    "DROP TABLE IF EXISTS artist_identity_review;",
    "DROP TABLE IF EXISTS entity_field_status;",
    "DROP TABLE IF EXISTS entity_source;",
    "DROP TABLE IF EXISTS payment_routes_legacy_0032;",
    "DROP TABLE IF EXISTS track_rel_legacy_0032;",
    "DROP TABLE IF EXISTS track_tag_legacy_0032;",
    "DROP TABLE IF EXISTS track_remote_items_raw_legacy_0032;",
    "DROP TABLE IF EXISTS value_time_splits_legacy_0032;",
    "DROP TABLE IF EXISTS tracks_legacy_0032;",
    "DROP TABLE IF EXISTS live_events_legacy;",
];

// ---------------------------------------------------------------------------
// migrations_are_idempotent: open_db twice on the same file, assert success
// and that table structure is correct after both opens.
// ---------------------------------------------------------------------------

#[test]
fn migrations_are_idempotent() {
    let tmp = std::env::temp_dir().join("stophammer_migration_idempotent.db");
    let _ = std::fs::remove_file(&tmp);

    // First open — applies all migrations.
    let conn1 = stophammer::db::open_db(&tmp);
    let tables_before = table_names(&conn1);
    assert!(
        tables_before.contains(&"feeds".to_string()),
        "feeds table must exist after first open"
    );
    assert!(
        !tables_before.contains(&"artists".to_string()),
        "ADR 0034 §11: migration 0047 drops artists, so it must not exist after first open"
    );
    assert!(
        tables_before.contains(&"schema_migrations".to_string()),
        "schema_migrations table must exist after first open"
    );
    drop(conn1);

    // Second open — simulates a restart; migrations must be skipped.
    let conn2 = stophammer::db::open_db(&tmp);
    let tables_after = table_names(&conn2);
    assert_eq!(
        tables_before, tables_after,
        "table set must be identical after restart"
    );

    drop(conn2);
    let _ = std::fs::remove_file(&tmp);
}

// ---------------------------------------------------------------------------
// migration_runs_only_once: verify schema_migrations records the right
// version number exactly once.
// ---------------------------------------------------------------------------

#[test]
fn migration_runs_only_once() {
    let conn = common::test_db();

    let version: i64 = conn
        .query_row(
            "SELECT version FROM schema_migrations WHERE version = 1",
            [],
            |r| r.get(0),
        )
        .expect("migration 1 should be recorded");
    assert_eq!(version, 1);

    let applied_at: i64 = conn
        .query_row(
            "SELECT applied_at FROM schema_migrations WHERE version = 1",
            [],
            |r| r.get(0),
        )
        .expect("applied_at should be set");
    assert!(
        applied_at > 0,
        "applied_at must be a positive unix timestamp"
    );

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
        .expect("count migrations");
    assert_eq!(
        count,
        i64::try_from(migration_paths().len()).expect("migration count should fit i64"),
        "schema_migrations count should match the number of migration files"
    );
}

#[test]
fn open_db_repairs_feed_scoped_track_identity_when_0032_was_skipped() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("legacy-high-watermark.db");

    {
        let conn = rusqlite::Connection::open(&db_path).expect("open legacy db");
        apply_migration_files_through_0031(&conn);
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_migrations (version, applied_at)
            VALUES (32, 1);",
        )
        .expect("mark high migration watermark");

        assert!(
            !table_has_column(&conn, "track_remote_items_raw", "feed_guid"),
            "legacy fixture should start with pre-0032 track_remote_items_raw"
        );
    }

    let conn = stophammer::db::open_db(&db_path);

    assert!(
        table_has_column(&conn, "track_remote_items_raw", "feed_guid"),
        "open_db should repair skipped 0032 track remote item schema"
    );
    assert!(
        table_has_composite_pk(&conn, "tracks", &["track_guid", "feed_guid"]),
        "open_db should repair skipped 0032 track primary key schema"
    );
}

#[test]
fn open_db_repairs_source_contributor_npub_when_0033_was_skipped() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("legacy-high-watermark-npub.db");

    {
        let conn = rusqlite::Connection::open(&db_path).expect("open legacy db");
        apply_migration_files_through_0032(&conn);
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_migrations (version, applied_at)
            VALUES (33, 1);",
        )
        .expect("mark high migration watermark");

        assert!(
            !table_has_column(&conn, "source_contributor_claims", "npub"),
            "legacy fixture should start without source_contributor_claims.npub"
        );
    }

    let conn = stophammer::db::open_db(&db_path);

    assert!(
        table_has_column(&conn, "source_contributor_claims", "npub"),
        "open_db should repair skipped 0033 contributor npub schema"
    );
}

// ---------------------------------------------------------------------------
// no_drop_table_in_migrations: scan every migration SQL for DROP TABLE to
// guard against accidental data destruction.
// ---------------------------------------------------------------------------

#[test]
fn no_drop_table_in_migrations() {
    for migration_path in migration_paths() {
        let sql = fs::read_to_string(&migration_path).expect("read migration SQL");
        for (line_no, line) in sql.lines().enumerate() {
            let trimmed = line.trim();
            // Skip SQL comments
            if trimmed.starts_with("--") {
                continue;
            }
            if ALLOWED_DROP_TABLE_LINES.contains(&trimmed) {
                continue;
            }
            assert!(
                !trimmed.to_lowercase().contains("drop table"),
                "migration {} line {} contains an unexpected DROP TABLE: {trimmed}",
                migration_path.display(),
                line_no + 1,
            );
        }
    }
}

/// Every table ADR 0034 §11 names for migration 0047 to drop. Shared by
/// [`removed_legacy_tables_stay_absent_and_kept_tables_remain_present`] and
/// the migration-0047 acceptance test.
const ADR_0034_DROPPED_TABLES: &[&str] = &[
    // The artist credit.
    "artists",
    "artist_aliases",
    "artist_credit",
    "artist_credit_name",
    "artist_type",
    "rel_type",
    "external_ids",
    // Relationships and tags.
    "feed_rel",
    "track_rel",
    "artist_artist_rel",
    "artist_tag",
    "artist_id_redirect",
    "tags",
    "feed_tag",
    "track_tag",
    // The resolver and the review.
    "resolver_queue",
    "resolver_state",
    "resolved_entity_sources_by_feed",
    "resolved_external_ids_by_feed",
    "artist_identity_override",
    "artist_identity_review",
    "entity_field_status",
    "entity_source",
    // The wallets.
    "wallets",
    "wallet_aliases",
    "wallet_artist_links",
    "wallet_endpoints",
    "wallet_feed_route_map",
    "wallet_id_redirect",
    "wallet_identity_override",
    "wallet_identity_review",
    "wallet_identity_review_legacy_0023",
    "wallet_identity_review_legacy_0024",
    "wallet_merge_apply_batch",
    "wallet_merge_apply_entry",
    "wallet_track_route_map",
    // Leftover copies.
    "tracks_legacy_0032",
    "payment_routes_legacy_0032",
    "track_rel_legacy_0032",
    "track_tag_legacy_0032",
    "track_remote_items_raw_legacy_0032",
    "value_time_splits_legacy_0032",
    "live_events_legacy",
];

fn table_exists(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
        rusqlite::params![name],
        |row| row.get(0),
    )
    .expect("query sqlite_master")
}

#[test]
fn removed_legacy_tables_stay_absent_and_kept_tables_remain_present() {
    let conn = common::test_db();

    for name in [
        "feed_type",
        "artist_location",
        "manifest_source",
        "source_item_recording_map",
        "source_feed_release_map",
        "release_recordings",
        "recordings",
        "releases",
        "proof_challenges",
        "proof_tokens",
    ] {
        assert!(
            !table_exists(&conn, name),
            "legacy table {name} should not exist in schema"
        );
    }

    for name in ADR_0034_DROPPED_TABLES {
        assert!(
            !table_exists(&conn, name),
            "ADR 0034 §11 table {name} should not exist in schema"
        );
    }

    for name in [
        "entity_quality",
        "search_entities",
        "search_index",
        "feeds",
        "tracks",
    ] {
        assert!(table_exists(&conn, name), "table {name} should still exist");
    }
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn table_names(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .expect("prepare table list query");
    stmt.query_map([], |row| row.get(0))
        .expect("query tables")
        .collect::<Result<_, _>>()
        .expect("collect table names")
}

fn table_has_column(conn: &rusqlite::Connection, table_name: &str, column_name: &str) -> bool {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table_name})"))
        .expect("prepare table info");
    stmt.query_map([], |row| row.get::<_, String>(1))
        .expect("query table info")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect column names")
        .iter()
        .any(|name| name == column_name)
}

fn table_has_composite_pk(
    conn: &rusqlite::Connection,
    table_name: &str,
    column_names: &[&str],
) -> bool {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table_name})"))
        .expect("prepare table info");
    let pk_columns = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
        })
        .expect("query table info")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect table info");

    column_names.iter().all(|expected| {
        pk_columns
            .iter()
            .any(|(name, pk_position)| name == expected && *pk_position > 0)
    })
}

fn apply_migration_files_through_0031(conn: &rusqlite::Connection) {
    apply_migration_files_through(conn, "0031_track_artist_lower_index.sql");
}

fn apply_migration_files_through_0032(conn: &rusqlite::Connection) {
    apply_migration_files_through(conn, "0032_feed_scoped_track_identity.sql");
}

fn apply_migration_files_through(conn: &rusqlite::Connection, upper_file_name: &str) {
    for path in migration_paths() {
        let file_name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .expect("migration path should have UTF-8 file name");
        if file_name <= upper_file_name {
            let sql = fs::read_to_string(&path).expect("read migration SQL");
            conn.execute_batch(&sql).expect("apply migration SQL");
        }
    }
}

fn migration_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir("migrations")
        .expect("read migrations directory")
        .map(|entry| entry.expect("read migration entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    paths.sort();
    paths
}

// ---------------------------------------------------------------------------
// ADR 0043: migration 0034 adds feeds.last_build_date. Its version equals an
// array position that an existing index has already recorded, so the runner
// skips it. open_db must repair the column.
// ---------------------------------------------------------------------------

#[test]
fn open_db_repairs_feed_last_build_date_when_0034_was_skipped() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("legacy-high-watermark-last-build-date.db");

    {
        let conn = rusqlite::Connection::open(&db_path).expect("open legacy db");
        apply_migration_files_through(&conn, "0033_source_contributor_npub.sql");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_migrations (version, applied_at)
            VALUES (99, 1);",
        )
        .expect("mark high migration watermark");

        assert!(
            !table_has_column(&conn, "feeds", "last_build_date"),
            "legacy fixture should start without feeds.last_build_date"
        );
    }

    let conn = stophammer::db::open_db(&db_path);

    assert!(
        table_has_column(&conn, "feeds", "last_build_date"),
        "open_db should repair skipped 0034 feed last_build_date schema"
    );
}

// ---------------------------------------------------------------------------
// ADR 0049: migration 0035 adds `rel` to both remote-item tables. A row
// written before the migration must read `rel` as null afterward.
// ---------------------------------------------------------------------------

#[test]
fn remote_item_rel_column_exists_and_legacy_row_reads_null() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("remote-item-rel-legacy-row.db");

    let conn = rusqlite::Connection::open(&db_path).expect("open db");
    apply_migration_files_through(&conn, "0034_feed_last_build_date.sql");
    // Migration 0032 turns foreign_keys back on. This test only cares about
    // the rel column, not about seeding valid parent feed/track rows.
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("disable foreign keys for this fixture");

    assert!(
        !table_has_column(&conn, "feed_remote_items_raw", "rel"),
        "feed_remote_items_raw must start without rel"
    );
    assert!(
        !table_has_column(&conn, "track_remote_items_raw", "rel"),
        "track_remote_items_raw must start without rel"
    );

    conn.execute(
        "INSERT INTO feed_remote_items_raw \
         (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
         VALUES ('legacy-feed', 0, 'music', 'legacy-remote-guid', NULL, 'podcast_remote_item')",
        [],
    )
    .expect("insert legacy feed remote item");
    conn.execute(
        "INSERT INTO track_remote_items_raw \
         (feed_guid, track_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
         VALUES ('legacy-feed', 'legacy-track', 0, 'publisher', 'legacy-remote-guid-2', NULL, 'podcast_remote_item')",
        [],
    )
    .expect("insert legacy track remote item");

    let sql_0035 =
        fs::read_to_string("migrations/0035_remote_item_rel.sql").expect("read migration 0035");
    conn.execute_batch(&sql_0035).expect("apply migration 0035");

    assert!(
        table_has_column(&conn, "feed_remote_items_raw", "rel"),
        "feed_remote_items_raw must gain rel after migration 0035"
    );
    assert!(
        table_has_column(&conn, "track_remote_items_raw", "rel"),
        "track_remote_items_raw must gain rel after migration 0035"
    );

    let feed_rel: Option<String> = conn
        .query_row(
            "SELECT rel FROM feed_remote_items_raw WHERE feed_guid = 'legacy-feed' AND position = 0",
            [],
            |r| r.get(0),
        )
        .expect("read feed remote item rel");
    assert_eq!(
        feed_rel, None,
        "a feed remote item row written before migration 0035 must read rel as null"
    );

    let track_rel: Option<String> = conn
        .query_row(
            "SELECT rel FROM track_remote_items_raw WHERE track_guid = 'legacy-track' AND position = 0",
            [],
            |r| r.get(0),
        )
        .expect("read track remote item rel");
    assert_eq!(
        track_rel, None,
        "a track remote item row written before migration 0035 must read rel as null"
    );
}

// ---------------------------------------------------------------------------
// ADR 0049 task 003: migration 0035 is the 29th entry in MIGRATIONS. ADR 0046
// records that a production database already recorded version 29 before this
// migration existed, so the runner skips it there. open_db must repair the
// column on both tables.
// ---------------------------------------------------------------------------

#[test]
fn open_db_repairs_remote_item_rel_when_0035_was_skipped() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("legacy-high-watermark-remote-item-rel.db");

    {
        let conn = rusqlite::Connection::open(&db_path).expect("open legacy db");
        apply_migration_files_through(&conn, "0034_feed_last_build_date.sql");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_migrations (version, applied_at)
            VALUES (99, 1);",
        )
        .expect("mark high migration watermark");

        assert!(
            !table_has_column(&conn, "feed_remote_items_raw", "rel"),
            "legacy fixture should start without feed_remote_items_raw.rel"
        );
        assert!(
            !table_has_column(&conn, "track_remote_items_raw", "rel"),
            "legacy fixture should start without track_remote_items_raw.rel"
        );
    }

    let conn = stophammer::db::open_db(&db_path);

    assert!(
        table_has_column(&conn, "feed_remote_items_raw", "rel"),
        "open_db should repair skipped 0035 feed_remote_items_raw.rel schema"
    );
    assert!(
        table_has_column(&conn, "track_remote_items_raw", "rel"),
        "open_db should repair skipped 0035 track_remote_items_raw.rel schema"
    );
}

// ---------------------------------------------------------------------------
// ADR 0049 task 004: migration 0036 adds feed_url_observations and seeds it
// from the feeds table already on disk, so a feed's own URL gets an
// observation whose observed_at equals the feed's created_at.
// ---------------------------------------------------------------------------

#[test]
fn migration_0036_seeds_feed_url_observations_from_existing_feeds() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("feed-url-observations-seed.db");

    let conn = rusqlite::Connection::open(&db_path).expect("open db");
    apply_migration_files_through(&conn, "0035_remote_item_rel.sql");
    // The feeds row below has no matching artist_credit row; this test only
    // cares about the seed, not about a fully valid feed.
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("disable foreign keys for this fixture");

    assert!(
        !table_has_column(&conn, "feed_url_observations", "url"),
        "feed_url_observations must not exist before migration 0036"
    );

    conn.execute(
        "INSERT INTO feeds \
         (feed_guid, feed_url, title, title_lower, artist_credit_id, explicit, \
          episode_count, created_at, updated_at) \
         VALUES ('legacy-feed', 'https://example.com/legacy.xml', 'Legacy Feed', \
                 'legacy feed', 1, 0, 0, 1700000000, 1700000100)",
        [],
    )
    .expect("insert legacy feed");

    let sql_0036 = fs::read_to_string("migrations/0036_feed_url_observations.sql")
        .expect("read migration 0036");
    conn.execute_batch(&sql_0036).expect("apply migration 0036");

    let (feed_guid, observed_at): (String, i64) = conn
        .query_row(
            "SELECT feed_guid, observed_at FROM feed_url_observations \
             WHERE url = 'https://example.com/legacy.xml'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("read the seeded observation");

    assert_eq!(
        feed_guid, "legacy-feed",
        "the seed must name the feed's own feed_guid"
    );
    assert_eq!(
        observed_at, 1_700_000_000,
        "the seed's observed_at must equal the feed's created_at"
    );
}

// ---------------------------------------------------------------------------
// ADR 0049 task 004: migration 0036 is the 30th entry in MIGRATIONS. ADR 0046
// records that a production database already recorded version 29, one below
// 30, so the runner applies this migration there without a repair.
// ---------------------------------------------------------------------------

#[test]
fn open_db_runs_feed_url_observations_migration_at_the_adr_0046_watermark() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir
        .path()
        .join("adr-0046-watermark-feed-url-observations.db");

    {
        let conn = rusqlite::Connection::open(&db_path).expect("open legacy db");
        apply_migration_files_through(&conn, "0035_remote_item_rel.sql");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_migrations (version, applied_at)
            VALUES (29, 1);",
        )
        .expect("mark the ADR 0046 recorded watermark");

        assert!(
            !table_has_column(&conn, "feed_url_observations", "url"),
            "legacy fixture should start without feed_url_observations"
        );
    }

    let conn = stophammer::db::open_db(&db_path);

    assert!(
        table_has_column(&conn, "feed_url_observations", "url"),
        "migration 0036 must run at the recorded watermark of 29, with no repair"
    );

    // ADR 0049 task 007 added migration 0037, ADR 0053 task 001 added
    // migration 0038, ADR 0052 task 001 added migration 0039, ADR 0058 task
    // 001 added migration 0040, ADR 0052 task 006 added migration 0041, ADR
    // 0052 task 007 added migration 0042, ADR 0060 added migration 0043, ADR
    // 0064 task 002 added migration 0044, ADR 0067 task 001 added migration
    // 0045, ADR 0056 task 002 added migration 0046, and ADR 0034 task 003
    // added migration 0047, after this fixture was written. The fixture
    // still stops at 0035, so open_db also runs 0037 (entry 31), 0038 (entry
    // 32), 0039 (entry 33), 0040 (entry 34), 0041 (entry 35), 0042 (entry
    // 36), 0043 (entry 37), 0044 (entry 38), 0045 (entry 39), 0046 (entry 40)
    // 0047 (entry 41) and 0048 (entry 42), twelve migrations past the 0036
    // this test names.
    let recorded_version: i64 = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .expect("read recorded migration version");
    assert_eq!(
        recorded_version, 42,
        "the runner must record version 42 after migrations 0036, 0037, 0038, 0039, 0040, \
         0041, 0042, 0043, 0044, 0045, 0046, 0047 and 0048 run"
    );
}

// ---------------------------------------------------------------------------
// ADR 0049 task 007: migration 0037 adds `release_artist_source` to `feeds`.
// A row written before the migration must read it as null.
// ---------------------------------------------------------------------------

#[test]
fn feed_release_artist_source_column_exists_and_legacy_row_reads_null() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("feed-release-artist-source-legacy-row.db");

    let conn = rusqlite::Connection::open(&db_path).expect("open db");
    apply_migration_files_through(&conn, "0036_feed_url_observations.sql");
    // Migration 0032 turns foreign_keys back on. This test only cares about
    // the release_artist_source column, not about seeding a valid artist
    // credit row.
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("disable foreign keys for this fixture");

    assert!(
        !table_has_column(&conn, "feeds", "release_artist_source"),
        "feeds must start without release_artist_source"
    );

    conn.execute(
        "INSERT INTO feeds \
         (feed_guid, feed_url, title, title_lower, artist_credit_id, explicit, \
          episode_count, created_at, updated_at) \
         VALUES ('legacy-feed', 'https://example.com/legacy-ras.xml', 'Legacy Feed', \
                 'legacy feed', 1, 0, 0, 1700000000, 1700000100)",
        [],
    )
    .expect("insert legacy feed");

    let sql_0037 = fs::read_to_string("migrations/0037_feed_release_artist_source.sql")
        .expect("read migration 0037");
    conn.execute_batch(&sql_0037).expect("apply migration 0037");

    assert!(
        table_has_column(&conn, "feeds", "release_artist_source"),
        "feeds must gain release_artist_source after migration 0037"
    );

    let source: Option<String> = conn
        .query_row(
            "SELECT release_artist_source FROM feeds WHERE feed_guid = 'legacy-feed'",
            [],
            |r| r.get(0),
        )
        .expect("read release_artist_source");
    assert_eq!(
        source, None,
        "a feed row written before migration 0037 must read release_artist_source as null"
    );
}

// ---------------------------------------------------------------------------
// ADR 0049 task 007: migration 0037 is the 31st entry in MIGRATIONS. ADR 0046
// records that a production database already recorded version 29, two below
// 31, so the runner applies this migration there without a repair.
// ---------------------------------------------------------------------------

#[test]
fn open_db_runs_feed_release_artist_source_migration_at_the_adr_0046_watermark() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir
        .path()
        .join("adr-0046-watermark-release-artist-source.db");

    {
        let conn = rusqlite::Connection::open(&db_path).expect("open legacy db");
        apply_migration_files_through(&conn, "0036_feed_url_observations.sql");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_migrations (version, applied_at)
            VALUES (29, 1);",
        )
        .expect("mark the ADR 0046 recorded watermark");

        assert!(
            !table_has_column(&conn, "feeds", "release_artist_source"),
            "legacy fixture should start without feeds.release_artist_source"
        );
    }

    let conn = stophammer::db::open_db(&db_path);

    assert!(
        table_has_column(&conn, "feeds", "release_artist_source"),
        "migration 0037 must run at the recorded watermark of 29, with no repair"
    );

    // ADR 0053 task 001 added migration 0038, ADR 0052 task 001 added
    // migration 0039, ADR 0058 task 001 added migration 0040, ADR 0052 task
    // 006 added migration 0041, ADR 0052 task 007 added migration 0042, ADR
    // 0060 added migration 0043, ADR 0064 task 002 added migration 0044, ADR
    // 0067 task 001 added migration 0045, ADR 0056 task 002 added migration
    // 0046, and ADR 0034 task 003 added migration 0047, after this fixture
    // was written. The fixture stops at 0036, so open_db also runs 0038
    // (entry 32), 0039 (entry 33), 0040 (entry 34), 0041 (entry 35), 0042
    // (entry 36), 0043 (entry 37), 0044 (entry 38), 0045 (entry 39), 0046
    // (entry 40), 0047 (entry 41) and 0048 (entry 42), eleven migrations past
    // the 0037 this test names.
    let recorded_version: i64 = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .expect("read recorded migration version");
    assert_eq!(
        recorded_version, 42,
        "the runner must record version 42 after migrations 0037, 0038, 0039, 0040, 0041, 0042, \
         0043, 0044, 0045, 0046, 0047 and 0048 run"
    );
}

// ---------------------------------------------------------------------------
// ADR 0067 task 001: migration 0045 is the 39th entry in MIGRATIONS. It adds
// source_gone_answers, the local table that counts a gone answer from a
// stored source URL.
// ---------------------------------------------------------------------------

#[test]
fn open_db_runs_source_gone_answers_migration_at_position_39() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("source-gone-answers-migration.db");

    {
        let conn = rusqlite::Connection::open(&db_path).expect("open legacy db");
        apply_migration_files_through(&conn, "0044_live_item_relay_link.sql");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_migrations (version, applied_at)
            VALUES (38, 1);",
        )
        .expect("mark the database at position 38");

        assert!(
            !table_names(&conn).contains(&"source_gone_answers".to_string()),
            "a database at position 38 must start without source_gone_answers"
        );
    }

    let conn = stophammer::db::open_db(&db_path);

    assert!(
        table_names(&conn).contains(&"source_gone_answers".to_string()),
        "migration 0045 must run at position 39 and create source_gone_answers"
    );

    // ADR 0056 task 002 added migration 0046, and ADR 0034 task 003 added
    // migration 0047, after this fixture was written. The fixture stops at
    // 0044, so open_db also runs 0046 (entry 40), 0047 (entry 41) and 0048
    // (entry 42) right after 0045 (entry 39).
    let recorded_version: i64 = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .expect("read recorded migration version");
    assert_eq!(
        recorded_version, 42,
        "the runner must record version 42 after migrations 0045, 0046, 0047 and 0048 run"
    );
}

// ---------------------------------------------------------------------------
// ADR 0056 task 002: migration 0046 is the 40th entry in MIGRATIONS. It
// drops proof_challenges and proof_tokens, and rebuilds
// trg_feeds_cleanup_before_delete without their two DELETE statements.
// ---------------------------------------------------------------------------

#[test]
fn open_db_runs_drop_proof_tables_migration_at_position_40() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("drop-proof-tables-migration.db");

    {
        let conn = rusqlite::Connection::open(&db_path).expect("open legacy db");
        apply_migration_files_through(&conn, "0045_source_gone_answers.sql");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_migrations (version, applied_at)
            VALUES (39, 1);",
        )
        .expect("mark the database at position 39");

        assert!(
            table_names(&conn).contains(&"proof_challenges".to_string()),
            "a database at position 39 must start with proof_challenges"
        );
        assert!(
            table_names(&conn).contains(&"proof_tokens".to_string()),
            "a database at position 39 must start with proof_tokens"
        );
    }

    let conn = stophammer::db::open_db(&db_path);

    let tables = table_names(&conn);
    assert!(
        !tables.contains(&"proof_challenges".to_string()),
        "migration 0046 must run at position 40 and drop proof_challenges"
    );
    assert!(
        !tables.contains(&"proof_tokens".to_string()),
        "migration 0046 must run at position 40 and drop proof_tokens"
    );

    // ADR 0034 task 003 added migration 0047, after this fixture was
    // written. The fixture stops at 0045, so open_db also runs 0047 (entry
    // 41) and 0048 (entry 42) right after 0046 (entry 40).
    let recorded_version: i64 = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .expect("read recorded migration version");
    assert_eq!(
        recorded_version, 42,
        "the runner must record version 42 after migrations 0046, 0047 and 0048 run"
    );

    let trigger_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'trigger' \
             AND name = 'trg_feeds_cleanup_before_delete'",
            [],
            |r| r.get(0),
        )
        .expect("read trg_feeds_cleanup_before_delete definition");
    assert!(
        !trigger_sql.contains("proof_tokens"),
        "trg_feeds_cleanup_before_delete must not name proof_tokens"
    );
    assert!(
        !trigger_sql.contains("proof_challenges"),
        "trg_feeds_cleanup_before_delete must not name proof_challenges"
    );
}

#[test]
fn feed_delete_succeeds_after_proof_tables_are_dropped() {
    let conn = common::test_db();
    let now = common::now();

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, \
         created_at, updated_at) \
         VALUES ('drop-proof-tables-feed', 'https://example.com/drop-proof-tables.xml', \
         'Drop Proof Tables Feed', 'drop proof tables feed', ?1, ?2)",
        rusqlite::params![now, now],
    )
    .expect("insert feed");

    conn.execute(
        "DELETE FROM feeds WHERE feed_guid = 'drop-proof-tables-feed'",
        [],
    )
    .expect("delete feed after proof tables are dropped");

    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feeds WHERE feed_guid = 'drop-proof-tables-feed'",
            [],
            |r| r.get(0),
        )
        .expect("count remaining feed rows");
    assert_eq!(remaining, 0, "feed row must be gone after delete");
}

// ---------------------------------------------------------------------------
// ADR 0034 §11, task 003: migration 0047 rebuilds `feeds` and `tracks` with
// no `artist_credit_id`, and drops every table the ADR lists, including the
// six the first build of this task found still hold a foreign key to
// `rel_type` or `artists` (`feed_rel`, `track_rel`, `artist_artist_rel`,
// `artist_tag`, `artist_id_redirect`, `track_rel_legacy_0032`).
// ---------------------------------------------------------------------------

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the migration 0047 acceptance test builds a pre-migration fixture, seeds every \
              table from the original escalation, and checks each ADR 0034 §11 acceptance \
              criterion in one place for a single, readable narrative"
)]
fn migration_0047_drops_every_adr_0034_table_and_keeps_deletes_working() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("adr-0034-task-003-migration.db");

    {
        let conn = rusqlite::Connection::open(&db_path).expect("open legacy db");
        apply_migration_files_through(&conn, "0046_drop_proof_tables.sql");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_migrations (version, applied_at)
            VALUES (40, 1);",
        )
        .expect("mark the database at the array position of migration 0046");

        // Artist rows and a credit.
        conn.execute(
            "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
             VALUES ('a1', 'Test Artist', 'test artist', 1, 1)",
            [],
        )
        .expect("insert artist");
        conn.execute(
            "INSERT INTO artist_credit (id, display_name, feed_guid, created_at) \
             VALUES (1, 'Test Artist', 'feed-1', 1)",
            [],
        )
        .expect("insert artist_credit");
        conn.execute(
            "INSERT INTO artist_credit_name \
             (artist_credit_id, artist_id, position, name, join_phrase) \
             VALUES (1, 'a1', 0, 'Test Artist', '')",
            [],
        )
        .expect("insert artist_credit_name");
        conn.execute(
            "INSERT INTO artist_aliases (alias_lower, artist_id, created_at) \
             VALUES ('test artist', 'a1', 1)",
            [],
        )
        .expect("insert artist_aliases");

        // A feed and a track that carry the credit.
        conn.execute(
            "INSERT INTO feeds \
             (feed_guid, feed_url, title, title_lower, artist_credit_id, explicit, \
              episode_count, created_at, updated_at) \
             VALUES ('feed-1', 'https://example.com/feed1.xml', 'Feed One', 'feed one', \
                     1, 0, 1, 1, 1)",
            [],
        )
        .expect("insert feed");
        conn.execute(
            "INSERT INTO tracks \
             (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, \
              created_at, updated_at) \
             VALUES ('track-1', 'feed-1', 1, 'Track One', 'track one', 0, 1, 1)",
            [],
        )
        .expect("insert track");

        // One source_gone_answers row (ADR 0067), which must survive.
        conn.execute(
            "INSERT INTO source_gone_answers \
             (feed_guid, source_url, first_gone_at, last_gone_at, last_status) \
             VALUES ('feed-1', 'https://example.com/feed1.xml', 1, 1, 404)",
            [],
        )
        .expect("insert source_gone_answers");

        // Rows in the six tables the first build of this task found still
        // hold a foreign key to `rel_type` or `artists`, so a plain feed or
        // track delete exercises the exact regression that was found.
        conn.execute(
            "INSERT INTO rel_type (id, name, entity_pair, description) \
             VALUES (99, 'performs_on', 'artist-track', 'test role')",
            [],
        )
        .expect("insert rel_type");
        conn.execute(
            "INSERT INTO feed_rel (feed_guid_a, feed_guid_b, rel_type_id, created_at) \
             VALUES ('feed-1', 'feed-1', 99, 1)",
            [],
        )
        .expect("insert feed_rel");
        conn.execute(
            "INSERT INTO track_rel \
             (feed_guid_a, track_guid_a, feed_guid_b, track_guid_b, rel_type_id, created_at) \
             VALUES ('feed-1', 'track-1', 'feed-1', 'track-1', 99, 1)",
            [],
        )
        .expect("insert track_rel");
        conn.execute(
            "INSERT INTO artist_artist_rel (artist_id_a, artist_id_b, rel_type_id, created_at) \
             VALUES ('a1', 'a1', 99, 1)",
            [],
        )
        .expect("insert artist_artist_rel");
        conn.execute(
            "INSERT INTO tags (id, name, created_at) VALUES (1, 'test-tag', 1)",
            [],
        )
        .expect("insert tags");
        conn.execute(
            "INSERT INTO artist_tag (artist_id, tag_id, created_at) VALUES ('a1', 1, 1)",
            [],
        )
        .expect("insert artist_tag");
        conn.execute(
            "INSERT INTO artist_id_redirect (old_artist_id, new_artist_id, merged_at) \
             VALUES ('a0', 'a1', 1)",
            [],
        )
        .expect("insert artist_id_redirect");
    }

    let feeds_before = 1_i64;
    let tracks_before = 1_i64;

    let conn = stophammer::db::open_db(&db_path);

    // `feeds` and `tracks` have no `artist_credit_id`.
    assert!(
        !table_has_column(&conn, "feeds", "artist_credit_id"),
        "feeds must lose artist_credit_id"
    );
    assert!(
        !table_has_column(&conn, "tracks", "artist_credit_id"),
        "tracks must lose artist_credit_id"
    );

    // Each table of the ADR 0034 §11 list is gone.
    for name in ADR_0034_DROPPED_TABLES {
        assert!(
            !table_exists(&conn, name),
            "ADR 0034 §11 table {name} must be gone after migration 0047"
        );
    }

    // The row counts of feeds and tracks are the same.
    let feeds_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM feeds", [], |r| r.get(0))
        .expect("count feeds");
    let tracks_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("count tracks");
    assert_eq!(
        feeds_after, feeds_before,
        "feeds row count must be unchanged"
    );
    assert_eq!(
        tracks_after, tracks_before,
        "tracks row count must be unchanged"
    );

    // PRAGMA foreign_key_check gives no row.
    let mut fk_check_stmt = conn
        .prepare("PRAGMA foreign_key_check")
        .expect("prepare foreign_key_check");
    let has_violation = fk_check_stmt
        .query([])
        .expect("run foreign_key_check")
        .next()
        .expect("read foreign_key_check row")
        .is_some();
    assert!(!has_violation, "foreign_key_check must give no row");
    drop(fk_check_stmt);

    // The source_gone_answers row is still there.
    let gone_answers: i64 = conn
        .query_row("SELECT COUNT(*) FROM source_gone_answers", [], |r| r.get(0))
        .expect("count source_gone_answers");
    assert_eq!(gone_answers, 1, "source_gone_answers row must survive");

    // PRAGMA foreign_keys gives 1 again.
    let foreign_keys_on: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .expect("read foreign_keys pragma");
    assert_eq!(foreign_keys_on, 1, "foreign_keys must be back on");

    // A delete of a track succeeds with foreign keys on.
    conn.execute("DELETE FROM tracks WHERE track_guid = 'track-1'", [])
        .expect("delete track with foreign keys on");
    let tracks_left: i64 = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("count tracks after track delete");
    assert_eq!(tracks_left, 0, "the deleted track must be gone");

    // Re-seed a track so the feed delete below also exercises the cascade.
    conn.execute(
        "INSERT INTO tracks \
         (track_guid, feed_guid, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-2', 'feed-1', 'Track Two', 'track two', 0, 1, 1)",
        [],
    )
    .expect("insert second track");

    // A delete of a feed succeeds with foreign keys on, and still deletes
    // its tracks (the trigger's own `DELETE FROM tracks` statement).
    conn.execute("DELETE FROM feeds WHERE feed_guid = 'feed-1'", [])
        .expect("delete feed with foreign keys on");
    let feeds_left: i64 = conn
        .query_row("SELECT COUNT(*) FROM feeds", [], |r| r.get(0))
        .expect("count feeds after feed delete");
    let tracks_left_after_feed_delete: i64 = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("count tracks after feed delete");
    assert_eq!(feeds_left, 0, "the deleted feed must be gone");
    assert_eq!(
        tracks_left_after_feed_delete, 0,
        "the feed delete must still delete its tracks"
    );
}

// ---------------------------------------------------------------------------
// ADR 0034 §11: a migration marked `-- stophammer: foreign_keys=off` whose
// SQL leaves a foreign key violation must roll back and name its version in
// the error.
// ---------------------------------------------------------------------------

#[test]
fn migration_with_marker_and_fk_violation_fails_and_changes_nothing() {
    let mut conn = common::test_db();
    let now = common::now();

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, created_at, updated_at) \
         VALUES ('fk-violation-feed', 'https://example.com/fk-violation.xml', 'F', 'f', ?1, ?1)",
        rusqlite::params![now],
    )
    .expect("insert feed");

    let tracks_before: i64 = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("count tracks before");

    // This migration's INSERT names a feed_guid that does not exist, which
    // PRAGMA foreign_key_check must catch once the marker turns foreign keys
    // back on for the check.
    let bad_sql = "-- stophammer: foreign_keys=off\n\
        INSERT INTO tracks \
        (track_guid, feed_guid, title, title_lower, explicit, created_at, updated_at) \
        VALUES ('orphan-track', 'no-such-feed', 'Orphan', 'orphan', 0, 1, 1);";

    let result = stophammer::db::run_one_migration_for_test(&mut conn, bad_sql, 9001);

    let err = result.expect_err("a migration that leaves a foreign key violation must fail");
    let message = err.to_string();
    assert!(
        message.contains("9001"),
        "the error must name the migration version, got: {message}"
    );

    let tracks_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("count tracks after");
    assert_eq!(
        tracks_after, tracks_before,
        "a rolled-back migration must change nothing"
    );

    let applied: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 9001",
            [],
            |r| r.get(0),
        )
        .expect("count schema_migrations rows for version 9001");
    assert_eq!(
        applied, 0,
        "a rolled-back migration must not record its version"
    );

    let foreign_keys_on: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .expect("read foreign_keys pragma");
    assert_eq!(
        foreign_keys_on, 1,
        "foreign_keys must be back on after a failed migration"
    );
}
