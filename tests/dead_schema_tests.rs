// Dead schema removed — 2026-03-13

mod common;

// ---------------------------------------------------------------------------
// FG-06: Verify feed_type table does NOT exist after schema applies
// ---------------------------------------------------------------------------

#[test]
fn feed_type_table_does_not_exist() {
    let conn = common::test_db();
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='feed_type'",
            [],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert!(!exists, "feed_type table should not exist in schema");
}

// ---------------------------------------------------------------------------
// FG-08: Verify artist_location table does NOT exist after schema applies
// ---------------------------------------------------------------------------

#[test]
fn artist_location_table_does_not_exist() {
    let conn = common::test_db();
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='artist_location'",
            [],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert!(!exists, "artist_location table should not exist in schema");
}

// ---------------------------------------------------------------------------
// FG-10: Verify manifest_source table does NOT exist after schema applies
// ---------------------------------------------------------------------------

#[test]
fn manifest_source_table_does_not_exist() {
    let conn = common::test_db();
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='manifest_source'",
            [],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert!(!exists, "manifest_source table should not exist in schema");
}

// ---------------------------------------------------------------------------
// Verify artist_type, track_rel, feed_rel are KEPT
// ---------------------------------------------------------------------------

#[test]
fn kept_tables_still_exist() {
    let conn = common::test_db();
    let kept = ["artist_type", "track_rel", "feed_rel"];
    for name in &kept {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                rusqlite::params![name],
                |row| row.get(0),
            )
            .expect("query sqlite_master");
        assert!(exists, "table {name} should still exist");
    }
}

// ---------------------------------------------------------------------------
// Verify migration system is idempotent across restarts
// ---------------------------------------------------------------------------

#[test]
fn migration_idempotent() {
    // Opening the same database path twice must not error (migrations
    // only run once; the schema_migrations table prevents re-application).
    let tmp = std::env::temp_dir().join("stophammer_dead_schema_idem.db");
    let _ = std::fs::remove_file(&tmp); // clean slate
    let conn1 = stophammer::db::open_db(&tmp);
    drop(conn1);
    let conn2 = stophammer::db::open_db(&tmp);
    drop(conn2);
    let _ = std::fs::remove_file(&tmp);
}
