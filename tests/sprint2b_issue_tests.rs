// Sprint 2B issue tests: Issue #7, #11, #3 — 2026-03-13

mod common;

use rusqlite::params;

// ===========================================================================
// Issue #7: Missing DB indexes
// ===========================================================================

#[test]
fn test_issue7_idx_ac_display_lower_exists() {
    let conn = common::test_db();
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='idx_ac_display_lower'",
            [],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert!(
        exists,
        "missing index idx_ac_display_lower on artist_credit(LOWER(display_name))"
    );
}

#[test]
fn test_issue7_idx_proof_challenges_feed_state_exists() {
    let conn = common::test_db();
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='idx_proof_challenges_feed_state'",
            [],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert!(
        exists,
        "missing composite index idx_proof_challenges_feed_state on proof_challenges(feed_guid, state)"
    );
}

#[test]
fn test_issue7_old_idx_proof_challenges_feed_dropped() {
    let conn = common::test_db();
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='idx_proof_challenges_feed'",
            [],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert!(
        !exists,
        "old index idx_proof_challenges_feed should have been dropped"
    );
}

// ===========================================================================
// Issue #11: RouteType enum mismatches schema
// ===========================================================================

#[test]
fn test_issue11_route_type_wallet_deserializes() {
    let rt: stophammer::model::RouteType = serde_json::from_str("\"wallet\"")
        .expect("'wallet' should deserialize to RouteType::Wallet");
    assert_eq!(rt, stophammer::model::RouteType::Wallet);
}

#[test]
fn test_issue11_route_type_keysend_deserializes() {
    let rt: stophammer::model::RouteType = serde_json::from_str("\"keysend\"")
        .expect("'keysend' should deserialize to RouteType::Keysend");
    assert_eq!(rt, stophammer::model::RouteType::Keysend);
}

#[test]
fn test_issue11_route_type_wallet_serializes() {
    let rt = stophammer::model::RouteType::Wallet;
    let json = serde_json::to_string(&rt).expect("serialize wallet");
    assert_eq!(json, "\"wallet\"");
}

#[test]
fn test_issue11_route_type_keysend_serializes() {
    let rt = stophammer::model::RouteType::Keysend;
    let json = serde_json::to_string(&rt).expect("serialize keysend");
    assert_eq!(json, "\"keysend\"");
}

#[test]
fn test_issue11_all_route_types_roundtrip() {
    for val in &["node", "wallet", "keysend", "lnaddress"] {
        let json = format!("\"{val}\"");
        let rt: stophammer::model::RouteType = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("'{val}' should deserialize: {e}"));
        let back =
            serde_json::to_string(&rt).unwrap_or_else(|e| panic!("'{val}' should serialize: {e}"));
        assert_eq!(back, json, "roundtrip mismatch for '{val}'");
    }
}

// ===========================================================================
// Issue #3: insert_event_idempotent uses RETURNING seq
// ===========================================================================

fn seed_for_event(conn: &rusqlite::Connection) {
    let now = stophammer::db::unix_now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["artist-i3", "I3 Artist", "i3 artist", now, now],
    )
    .expect("insert artist");
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        params!["I3 Artist", now],
    )
    .expect("insert artist_credit");
}

#[test]
fn test_issue3_insert_event_idempotent_returns_seq() {
    let conn = common::test_db();
    seed_for_event(&conn);

    let result = stophammer::db::insert_event_idempotent(
        &conn,
        "evt-i3-001",
        &stophammer::event::EventType::ArtistUpserted,
        r#"{"artist":{"artist_id":"artist-i3","name":"I3 Artist","name_lower":"i3 artist","created_at":0,"updated_at":0}}"#,
        "artist-i3",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "sig-placeholder-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        stophammer::db::unix_now(),
        &[],
    );
    let seq = result
        .expect("insert_event_idempotent should succeed")
        .expect("should return Some(seq)");
    assert!(seq > 0, "seq should be positive, got {seq}");
}

#[test]
fn test_issue3_insert_event_idempotent_duplicate_returns_none() {
    let conn = common::test_db();
    seed_for_event(&conn);

    let now = stophammer::db::unix_now();
    let event_id = "evt-i3-dup";

    // First insert: should succeed.
    let first = stophammer::db::insert_event_idempotent(
        &conn,
        event_id,
        &stophammer::event::EventType::ArtistUpserted,
        r#"{"artist":{"artist_id":"artist-i3","name":"I3 Artist","name_lower":"i3 artist","created_at":0,"updated_at":0}}"#,
        "artist-i3",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "sig-placeholder-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        now,
        &[],
    )
    .expect("first insert should succeed");
    assert!(first.is_some(), "first insert should return Some(seq)");

    // Second insert (same event_id): should return None (idempotent skip).
    let second = stophammer::db::insert_event_idempotent(
        &conn,
        event_id,
        &stophammer::event::EventType::ArtistUpserted,
        r#"{"artist":{"artist_id":"artist-i3","name":"I3 Artist","name_lower":"i3 artist","created_at":0,"updated_at":0}}"#,
        "artist-i3",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "sig-placeholder-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        now,
        &[],
    )
    .expect("second insert should succeed (idempotent)");
    assert!(second.is_none(), "duplicate event_id should return None");
}

#[test]
fn test_issue3_insert_event_idempotent_seq_increments() {
    let conn = common::test_db();
    seed_for_event(&conn);

    let now = stophammer::db::unix_now();
    let seq1 = stophammer::db::insert_event_idempotent(
        &conn,
        "evt-i3-seq1",
        &stophammer::event::EventType::ArtistUpserted,
        r#"{"artist":{"artist_id":"artist-i3","name":"I3 Artist","name_lower":"i3 artist","created_at":0,"updated_at":0}}"#,
        "artist-i3",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "sig-placeholder-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        now,
        &[],
    )
    .expect("first insert")
    .expect("should return Some(seq)");

    let seq2 = stophammer::db::insert_event_idempotent(
        &conn,
        "evt-i3-seq2",
        &stophammer::event::EventType::ArtistUpserted,
        r#"{"artist":{"artist_id":"artist-i3","name":"I3 Artist","name_lower":"i3 artist","created_at":0,"updated_at":0}}"#,
        "artist-i3",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "sig-placeholder-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        now,
        &[],
    )
    .expect("second insert")
    .expect("should return Some(seq)");

    assert_eq!(
        seq2,
        seq1 + 1,
        "seq should increment: got seq1={seq1}, seq2={seq2}"
    );
}
