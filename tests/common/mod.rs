use rusqlite::Connection;
use std::sync::{Arc, Mutex};

/// Opens an in-memory `SQLite` database with the stophammer schema applied.
/// Uses `open_db(":memory:")` so that the migration system is exercised
/// identically to production.
pub fn test_db() -> Connection {
    stophammer::db::open_db(":memory:")
}

/// Returns the DB as an `Arc<Mutex<Connection>>` matching the `db::Db` type.
#[allow(dead_code, reason = "used conditionally across test files")]
pub fn test_db_arc() -> Arc<Mutex<Connection>> {
    Arc::new(Mutex::new(test_db()))
}

// SP-05 epoch guard — 2026-03-12
/// Returns current unix timestamp as i64.
#[allow(dead_code, reason = "used conditionally across test files")]
pub fn now() -> i64 {
    stophammer::db::unix_now()
}
