//! ADR 0023 "Startup order": `open_db` applies the PRAGMAs (WAL, foreign
//! keys, synchronous) before it calls `run_migrations`, so WAL mode and
//! foreign key enforcement are active while a migration runs.

use std::path::Path;

/// The source text of `try_open_db` in `src/db.rs`.
fn try_open_db_body() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/db.rs");
    let source = std::fs::read_to_string(&path).expect("read src/db.rs");
    let start = source
        .find("pub fn try_open_db(")
        .expect("src/db.rs defines try_open_db");
    let rest = &source[start..];
    let end = rest.find("\n}\n").expect("the end of try_open_db");
    rest[..end].to_string()
}

#[test]
fn the_pragmas_come_before_the_repairs_and_the_migrations() {
    let body = try_open_db_body();
    let pragmas = body
        .find("PRAGMA journal_mode = WAL")
        .expect("ADR 0023: try_open_db sets journal_mode = WAL");
    for pragma in ["PRAGMA foreign_keys = ON", "PRAGMA synchronous = NORMAL"] {
        let at = body
            .find(pragma)
            .unwrap_or_else(|| panic!("ADR 0023: try_open_db sets {pragma}"));
        assert!(
            at < body
                .find("run_migrations(")
                .expect("try_open_db runs the migrations"),
            "ADR 0023: {pragma} must come before run_migrations in try_open_db"
        );
    }
    let migrations = body
        .find("run_migrations(")
        .expect("try_open_db runs the migrations");
    assert!(
        pragmas < migrations,
        "ADR 0023: open_db applies the PRAGMAs before run_migrations"
    );
    if let Some(first_repair) = body.find("ensure_") {
        assert!(
            pragmas < first_repair,
            "ADR 0023: open_db applies the PRAGMAs before the first repair writes"
        );
    }
}

#[test]
fn an_opened_database_has_wal_foreign_keys_and_normal_sync() {
    let dir = tempfile::tempdir().expect("temp dir");
    let conn = stophammer::db::open_db(dir.path().join("adr0023-order.db"));
    let journal: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("read journal_mode");
    let foreign_keys: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("read foreign_keys");
    let synchronous: i64 = conn
        .query_row("PRAGMA synchronous", [], |row| row.get(0))
        .expect("read synchronous");
    assert_eq!(journal, "wal", "ADR 0023: the database is in WAL mode");
    assert_eq!(foreign_keys, 1, "ADR 0023: foreign keys are enforced");
    assert_eq!(synchronous, 1, "ADR 0023: synchronous is NORMAL");
}
