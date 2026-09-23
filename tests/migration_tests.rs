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
        tables_before.contains(&"artists".to_string()),
        "artists table must exist after first open"
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

    // Seed data must still be present (INSERT OR IGNORE must not duplicate).
    let artist_type_count: i64 = conn2
        .query_row("SELECT COUNT(*) FROM artist_type", [], |r| r.get(0))
        .expect("count artist_type");
    assert_eq!(artist_type_count, 6);

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
        "wallets",
        "wallet_endpoints",
        "wallet_aliases",
        "wallet_track_route_map",
        "wallet_feed_route_map",
        "wallet_id_redirect",
        "wallet_artist_links",
        "wallet_identity_review",
        "wallet_identity_override",
        "wallet_merge_apply_batch",
        "wallet_merge_apply_entry",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                rusqlite::params![name],
                |row| row.get(0),
            )
            .expect("query sqlite_master");
        assert!(!exists, "legacy table {name} should not exist in schema");
    }

    let name = "artist_type";
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert!(exists, "table {name} should still exist");
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

    // ADR 0049 task 007 added migration 0037 after this fixture was written.
    // The fixture still stops at 0035, so open_db also runs 0037 (entry 31),
    // one migration past the 0036 this test names.
    let recorded_version: i64 = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .expect("read recorded migration version");
    assert_eq!(
        recorded_version, 31,
        "the runner must record version 31 after migrations 0036 and 0037 run"
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

    let recorded_version: i64 = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .expect("read recorded migration version");
    assert_eq!(
        recorded_version, 31,
        "the runner must record version 31 after migration 0037 runs"
    );
}
