mod common;

use rusqlite::params;
use stophammer::db;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn seed_feed_and_track(conn: &rusqlite::Connection) -> i64 {
    let now = common::now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["artist-w", "Wallet Artist", "wallet artist", now, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        params!["Wallet Artist", now],
    )
    .unwrap();
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, ?2, 0, ?3, '')",
        params![credit_id, "artist-w", "Wallet Artist"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, ?7)",
        params!["feed-w", "https://example.com/feed.xml", "Wallet Feed", "wallet feed", credit_id, now, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         explicit, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7)",
        params!["track-w", "feed-w", credit_id, "Wallet Track", "wallet track", now, now],
    )
    .unwrap();
    now
}

fn insert_track_route(conn: &rusqlite::Connection, track_guid: &str, name: &str, address: &str) -> i64 {
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES (?1, 'feed-w', ?2, 'keysend', ?3, 100, 0)",
        params![track_guid, name, address],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn insert_feed_route(conn: &rusqlite::Connection, feed_guid: &str, name: &str, address: &str) -> i64 {
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES (?1, ?2, 'keysend', ?3, 100, 0)",
        params![feed_guid, name, address],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn endpoint_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0)).unwrap()
}

fn alias_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM wallet_aliases", [], |r| r.get(0)).unwrap()
}

// ---------------------------------------------------------------------------
// normalize_wallet_address
// ---------------------------------------------------------------------------

#[test]
fn normalize_trims_and_lowercases() {
    assert_eq!(db::normalize_wallet_address("lnaddress", " Alice@Example.COM "), "alice@example.com");
    assert_eq!(db::normalize_wallet_address("keysend", "  0xABCDEF  "), "0xabcdef");
    assert_eq!(db::normalize_wallet_address("node", " PubKey123 "), "pubkey123");
    assert_eq!(db::normalize_wallet_address("wallet", " WALLET_ADDR "), "wallet_addr");
}

// ---------------------------------------------------------------------------
// Endpoint facts (Pass 1)
// ---------------------------------------------------------------------------

#[test]
fn same_address_different_labels_one_endpoint() {
    let conn = common::test_db();
    let now = common::now();

    let id1 = db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
    let id2 = db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("ALICE"), now + 1).unwrap();

    assert_eq!(id1, id2, "same normalized address should reuse endpoint");
    assert_eq!(endpoint_count(&conn), 1);
    // Two aliases: "Alice" and "ALICE" have the same alias_lower so only one alias row
    assert_eq!(alias_count(&conn), 1);
}

#[test]
fn same_address_distinct_labels_create_separate_aliases() {
    let conn = common::test_db();
    let now = common::now();

    db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
    db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Bob"), now + 1).unwrap();

    assert_eq!(endpoint_count(&conn), 1);
    assert_eq!(alias_count(&conn), 2);
}

#[test]
fn keysend_with_distinct_custom_values_create_separate_endpoints() {
    let conn = common::test_db();
    let now = common::now();

    let id1 = db::get_or_create_endpoint(&conn, "keysend", "node123", "7629169", "podcast1", Some("Alice"), now).unwrap();
    let id2 = db::get_or_create_endpoint(&conn, "keysend", "node123", "7629169", "podcast2", Some("Bob"), now).unwrap();

    assert_ne!(id1, id2);
    assert_eq!(endpoint_count(&conn), 2);
}

#[test]
fn pass_1_creates_no_wallets() {
    let conn = common::test_db();
    let now = common::now();

    db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
    db::get_or_create_endpoint(&conn, "lnaddress", "alice@example.com", "", "", Some("Alice"), now).unwrap();

    let wallet_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(wallet_count, 0, "Pass 1 should not create wallets");

    let null_wallet_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wallet_endpoints WHERE wallet_id IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(null_wallet_count, 2, "all endpoints should have NULL wallet_id");
}

#[test]
fn route_maps_point_to_endpoint() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    let track_route_id = insert_track_route(&conn, "track-w", "Alice", "abc123");
    let feed_route_id = insert_feed_route(&conn, "feed-w", "Alice", "abc123");

    let endpoint_id =
        db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();

    db::map_track_route_to_endpoint(&conn, track_route_id, endpoint_id, now).unwrap();
    db::map_feed_route_to_endpoint(&conn, feed_route_id, endpoint_id, now).unwrap();

    let mapped_track_ep: i64 = conn
        .query_row(
            "SELECT endpoint_id FROM wallet_track_route_map WHERE route_id = ?1",
            params![track_route_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mapped_track_ep, endpoint_id);

    let mapped_feed_ep: i64 = conn
        .query_row(
            "SELECT endpoint_id FROM wallet_feed_route_map WHERE route_id = ?1",
            params![feed_route_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mapped_feed_ep, endpoint_id);
}

#[test]
fn empty_recipient_name_creates_no_alias() {
    let conn = common::test_db();
    let now = common::now();

    db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", None, now).unwrap();
    assert_eq!(alias_count(&conn), 0);

    db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some(""), now).unwrap();
    assert_eq!(alias_count(&conn), 0);

    db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("  "), now).unwrap();
    assert_eq!(alias_count(&conn), 0);
}

#[test]
fn alias_last_seen_updated_on_repeat() {
    let conn = common::test_db();
    let t1 = 1000;
    let t2 = 2000;

    db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), t1).unwrap();
    db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), t2).unwrap();

    let (first, last): (i64, i64) = conn
        .query_row(
            "SELECT first_seen_at, last_seen_at FROM wallet_aliases WHERE alias_lower = 'alice'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(first, t1, "first_seen_at should not change");
    assert_eq!(last, t2, "last_seen_at should be updated");
}

#[test]
fn route_map_insert_is_idempotent() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);
    let route_id = insert_track_route(&conn, "track-w", "Alice", "abc123");
    let ep = db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();

    db::map_track_route_to_endpoint(&conn, route_id, ep, now).unwrap();
    db::map_track_route_to_endpoint(&conn, route_id, ep, now).unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallet_track_route_map", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn address_normalization_deduplicates_case_variants() {
    let conn = common::test_db();
    let now = common::now();

    let id1 = db::get_or_create_endpoint(&conn, "lnaddress", "Alice@Example.COM", "", "", Some("Alice"), now).unwrap();
    let id2 = db::get_or_create_endpoint(&conn, "lnaddress", "alice@example.com", "", "", Some("Alice"), now).unwrap();

    assert_eq!(id1, id2, "case-insensitive address should match same endpoint");
    assert_eq!(endpoint_count(&conn), 1);
}
