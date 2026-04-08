// Issue-SSE-EXHAUSTION — 2026-03-15
//
// Tests for the SSE registry exhaustion fix and artist existence guardrails.

mod common;

use rusqlite::params;

// ---------------------------------------------------------------------------
// Test: db::artist_exists returns false for nonexistent, true for real
// ---------------------------------------------------------------------------
#[test]
fn artist_exists_db_function() {
    let conn = common::test_db();

    assert!(
        !stophammer::db::artist_exists(&conn, "nonexistent").expect("query"),
        "nonexistent artist should return false"
    );

    let now = stophammer::db::unix_now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES ('exists-1', 'Name', 'name', ?1, ?2)",
        params![now, now],
    )
    .expect("insert");

    assert!(
        stophammer::db::artist_exists(&conn, "exists-1").expect("query"),
        "inserted artist should return true"
    );
}
