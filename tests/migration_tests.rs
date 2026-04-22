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

fn migration_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir("migrations")
        .expect("read migrations directory")
        .map(|entry| entry.expect("read migration entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    paths.sort();
    paths
}
