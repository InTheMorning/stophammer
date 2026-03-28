// Issue-MIGRATIONS — 2026-03-14

mod common;

const ALLOWED_DROP_TABLE_LINES: &[&str] = &["DROP TABLE wallet_identity_override;"];

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
        count, 21,
        "exactly twenty-one migrations should be recorded"
    );
}

// ---------------------------------------------------------------------------
// no_drop_table_in_migrations: scan every migration SQL for DROP TABLE to
// guard against accidental data destruction.
// ---------------------------------------------------------------------------

#[test]
fn no_drop_table_in_migrations() {
    let baseline = include_str!("../migrations/0001_baseline.sql");
    let feed_scope = include_str!("../migrations/0002_artist_credit_feed_scope.sql");
    let search_unique = include_str!("../migrations/0003_search_entities_unique.sql");
    let proof_level = include_str!("../migrations/0004_proof_level.sql");
    let live_events = include_str!("../migrations/0005_live_events_and_remote_items.sql");
    let source_claims = include_str!("../migrations/0006_source_claim_staging.sql");
    let source_links_release =
        include_str!("../migrations/0007_source_link_and_release_claims.sql");
    let source_role_norm = include_str!("../migrations/0008_source_contributor_role_norm.sql");
    let source_item_enclosures = include_str!("../migrations/0009_source_item_enclosures.sql");
    let source_platform_claims = include_str!("../migrations/0010_source_platform_claims.sql");
    let canonical_release_recording =
        include_str!("../migrations/0011_canonical_release_recording.sql");
    let resolver_queue = include_str!("../migrations/0012_resolver_queue.sql");
    let artist_identity_reviews = include_str!("../migrations/0013_artist_identity_reviews.sql");
    let resolved_overlay_tables = include_str!("../migrations/0014_resolved_overlay_tables.sql");
    let live_events_feed_scoped =
        include_str!("../migrations/0015_live_events_feed_scoped_key.sql");
    let wallet_entities = include_str!("../migrations/0016_wallet_entities.sql");
    let wallet_force_override =
        include_str!("../migrations/0017_wallet_force_confidence_override.sql");
    let wallet_merge_batches = include_str!("../migrations/0018_wallet_merge_apply_batches.sql");
    let feed_delete_cleanup = include_str!("../migrations/0019_feed_delete_cleanup_triggers.sql");
    let artist_credit_null_scope =
        include_str!("../migrations/0020_artist_credit_null_scope_dedup.sql");
    let route_custom_normalization =
        include_str!("../migrations/0021_route_custom_value_normalization.sql");
    let all_migrations = [
        baseline,
        feed_scope,
        search_unique,
        proof_level,
        live_events,
        source_claims,
        source_links_release,
        source_role_norm,
        source_item_enclosures,
        source_platform_claims,
        canonical_release_recording,
        resolver_queue,
        artist_identity_reviews,
        resolved_overlay_tables,
        live_events_feed_scoped,
        wallet_entities,
        wallet_force_override,
        wallet_merge_batches,
        feed_delete_cleanup,
        artist_credit_null_scope,
        route_custom_normalization,
    ];
    for (i, sql) in all_migrations.iter().enumerate() {
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
                i + 1,
                line_no + 1,
            );
        }
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
