use rusqlite::Connection;
use std::sync::{Arc, Mutex};

/// Opens an in-memory `SQLite` database with the stophammer schema applied.
/// Uses `open_db(":memory:")` so that the migration system is exercised
/// identically to production.
pub fn test_db() -> Connection {
    stophammer::db::open_db(":memory:")
}

/// Returns the DB as an `Arc<Mutex<Connection>>` matching the legacy `db::Db` type.
#[allow(dead_code)]
pub fn test_db_arc() -> Arc<Mutex<Connection>> {
    Arc::new(Mutex::new(test_db()))
}

/// Returns a `DbPool` backed by a temporary file.
///
/// The returned `TempDir` must be kept alive for the lifetime of the pool —
/// dropping it removes the underlying database file.
// Issue-WAL-POOL — 2026-03-14
#[expect(dead_code, reason = "used conditionally across test files")]
pub fn test_db_pool() -> (stophammer::db_pool::DbPool, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = dir.path().join("test.db");
    let pool = stophammer::db_pool::DbPool::open(&db_path).expect("failed to open test db pool");
    (pool, dir)
}

/// Returns a `DbPool` wrapping an in-memory writer connection (test-util mode).
///
/// Both reads and writes go through the single writer. Use this when tests
/// need to seed data via raw SQL on a `Connection` reference.
// Issue-WAL-POOL — 2026-03-14
#[expect(dead_code, reason = "used conditionally across test files")]
pub fn test_db_pool_memory() -> stophammer::db_pool::DbPool {
    stophammer::db_pool::DbPool::from_writer_only(test_db_arc())
}

/// Wraps an `Arc<Mutex<Connection>>` into a `DbPool` for test compatibility.
// Issue-WAL-POOL — 2026-03-14
#[expect(dead_code, reason = "used conditionally across test files")]
pub fn wrap_pool(db: Arc<Mutex<rusqlite::Connection>>) -> stophammer::db_pool::DbPool {
    stophammer::db_pool::DbPool::from_writer_only(db)
}

// SP-05 epoch guard — 2026-03-12
/// Returns current unix timestamp as i64.
#[expect(dead_code, reason = "used conditionally across test files")]
pub fn now() -> i64 {
    stophammer::db::unix_now()
}

/// Builds a signed `POST /sync/register` request body for test callers.
#[expect(dead_code, reason = "used conditionally across test files")]
pub fn signed_sync_register_body(
    signer: &stophammer::signing::NodeSigner,
    node_url: &str,
) -> serde_json::Value {
    let node_pubkey = signer.pubkey_hex().to_string();
    let signed_at = stophammer::db::unix_now();
    let payload = stophammer::sync::RegisterSigningPayload {
        node_pubkey: &node_pubkey,
        node_url,
        signed_at,
    };
    let signature = signer
        .sign_json(&payload)
        .expect("failed to sign sync/register payload");

    serde_json::json!({
        "node_pubkey": node_pubkey,
        "node_url": node_url,
        "signed_at": signed_at,
        "signature": signature
    })
}
