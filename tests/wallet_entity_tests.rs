mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use http_body_util::BodyExt;
use rusqlite::params;
use stophammer::db;
use tower::ServiceExt;

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
        params![
            "feed-w",
            "https://example.com/feed.xml",
            "Wallet Feed",
            "wallet feed",
            credit_id,
            now,
            now
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         explicit, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7)",
        params![
            "track-w",
            "feed-w",
            credit_id,
            "Wallet Track",
            "wallet track",
            now,
            now
        ],
    )
    .unwrap();
    now
}

fn insert_track_route(
    conn: &rusqlite::Connection,
    track_guid: &str,
    name: &str,
    address: &str,
) -> i64 {
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES (?1, 'feed-w', ?2, 'keysend', ?3, 100, 0)",
        params![track_guid, name, address],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn insert_feed_route(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    name: &str,
    address: &str,
) -> i64 {
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES (?1, ?2, 'keysend', ?3, 100, 0)",
        params![feed_guid, name, address],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn endpoint_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0))
        .unwrap()
}

fn alias_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM wallet_aliases", [], |r| r.get(0))
        .unwrap()
}

// ---------------------------------------------------------------------------
// normalize_wallet_address
// ---------------------------------------------------------------------------

#[test]
fn normalize_trims_and_lowercases() {
    assert_eq!(
        db::normalize_wallet_address("lnaddress", " Alice@Example.COM "),
        "alice@example.com"
    );
    assert_eq!(
        db::normalize_wallet_address("keysend", "  0xABCDEF  "),
        "0xabcdef"
    );
    assert_eq!(
        db::normalize_wallet_address("node", " PubKey123 "),
        "pubkey123"
    );
    assert_eq!(
        db::normalize_wallet_address("wallet", " WALLET_ADDR "),
        "wallet_addr"
    );
}

// ---------------------------------------------------------------------------
// Endpoint facts (Pass 1)
// ---------------------------------------------------------------------------

#[test]
fn same_address_different_labels_one_endpoint() {
    let conn = common::test_db();
    let now = common::now();

    let id1 =
        db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
    let id2 =
        db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("ALICE"), now + 1)
            .unwrap();

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

    let id1 = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "node123",
        "7629169",
        "podcast1",
        Some("Alice"),
        now,
    )
    .unwrap();
    let id2 = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "node123",
        "7629169",
        "podcast2",
        Some("Bob"),
        now,
    )
    .unwrap();

    assert_ne!(id1, id2);
    assert_eq!(endpoint_count(&conn), 2);
}

#[test]
fn pass_1_creates_no_wallets() {
    let conn = common::test_db();
    let now = common::now();

    db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
    db::get_or_create_endpoint(
        &conn,
        "lnaddress",
        "alice@example.com",
        "",
        "",
        Some("Alice"),
        now,
    )
    .unwrap();

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
    assert_eq!(
        null_wallet_count, 2,
        "all endpoints should have NULL wallet_id"
    );
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
    let ep =
        db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();

    db::map_track_route_to_endpoint(&conn, route_id, ep, now).unwrap();
    db::map_track_route_to_endpoint(&conn, route_id, ep, now).unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallet_track_route_map", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn address_normalization_deduplicates_case_variants() {
    let conn = common::test_db();
    let now = common::now();

    let id1 = db::get_or_create_endpoint(
        &conn,
        "lnaddress",
        "Alice@Example.COM",
        "",
        "",
        Some("Alice"),
        now,
    )
    .unwrap();
    let id2 = db::get_or_create_endpoint(
        &conn,
        "lnaddress",
        "alice@example.com",
        "",
        "",
        Some("Alice"),
        now,
    )
    .unwrap();

    assert_eq!(
        id1, id2,
        "case-insensitive address should match same endpoint"
    );
    assert_eq!(endpoint_count(&conn), 1);
}

// ---------------------------------------------------------------------------
// Owner creation (Pass 2)
// ---------------------------------------------------------------------------

#[test]
fn provisional_wallet_per_endpoint() {
    let conn = common::test_db();
    let now = common::now();

    let ep1 =
        db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
    let ep2 = db::get_or_create_endpoint(
        &conn,
        "lnaddress",
        "bob@example.com",
        "",
        "",
        Some("Bob"),
        now,
    )
    .unwrap();

    let w1 = db::create_provisional_wallet(&conn, ep1, now).unwrap();
    let w2 = db::create_provisional_wallet(&conn, ep2, now).unwrap();

    assert_ne!(w1, w2, "distinct endpoints should get distinct wallets");

    let (class, confidence): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![w1],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(class, "unknown");
    assert_eq!(confidence, "provisional");
}

#[test]
fn provisional_wallet_uses_first_alias_as_display_name() {
    let conn = common::test_db();

    let ep = db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), 1000)
        .unwrap();
    db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Bob"), 2000).unwrap();

    let wid = db::create_provisional_wallet(&conn, ep, 3000).unwrap();
    let name: String = conn
        .query_row(
            "SELECT display_name FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        name, "Alice",
        "display_name should be the earliest-seen alias"
    );
}

#[test]
fn endpoint_without_alias_gets_placeholder_name() {
    let conn = common::test_db();
    let now = common::now();

    let ep = db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", None, now).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();

    let name: String = conn
        .query_row(
            "SELECT display_name FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        name.starts_with("endpoint-"),
        "should use placeholder: {name}"
    );
}

#[test]
fn fee_true_classifies_as_bot_service_high_confidence() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    // Insert a fee=true track route
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-w', 'feed-w', 'App Fee', 'keysend', 'feenode123', 1, 1)",
        [],
    )
    .unwrap();
    let route_id = conn.last_insert_rowid();

    let ep =
        db::get_or_create_endpoint(&conn, "keysend", "feenode123", "", "", Some("App Fee"), now)
            .unwrap();
    db::map_track_route_to_endpoint(&conn, route_id, ep, now).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();
    db::classify_wallet_hard_signals(&conn, &wid).unwrap();

    let (class, confidence): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(class, "bot_service");
    assert_eq!(confidence, "high_confidence");
}

#[test]
fn operator_override_takes_precedence() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    // Insert a fee=true route
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-w', 'feed-w', 'Overridden', 'keysend', 'overnode', 1, 1)",
        [],
    )
    .unwrap();
    let route_id = conn.last_insert_rowid();

    let ep = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "overnode",
        "",
        "",
        Some("Overridden"),
        now,
    )
    .unwrap();
    db::map_track_route_to_endpoint(&conn, route_id, ep, now).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();

    // Add an operator override
    conn.execute(
        "INSERT INTO wallet_identity_override (override_type, wallet_id, value, created_at) \
         VALUES ('force_class', ?1, 'person_artist', ?2)",
        params![wid, now],
    )
    .unwrap();

    db::classify_wallet_hard_signals(&conn, &wid).unwrap();

    let (class, confidence): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(class, "person_artist");
    assert_eq!(confidence, "reviewed");
}

#[test]
fn label_alone_stays_unknown_provisional() {
    let conn = common::test_db();
    let now = common::now();

    let ep = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "fountain123",
        "",
        "",
        Some("Fountain"),
        now,
    )
    .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();
    db::classify_wallet_hard_signals(&conn, &wid).unwrap();

    let (class, confidence): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        class, "unknown",
        "name alone should not drive classification"
    );
    assert_eq!(confidence, "provisional");
}

// ---------------------------------------------------------------------------
// Artist links (Pass 3)
// ---------------------------------------------------------------------------

#[test]
fn same_feed_artist_credit_match_creates_link() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    // The feed's artist credit name is "Wallet Artist" with artist_id "artist-w"
    let ep = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "abc123",
        "",
        "",
        Some("Wallet Artist"),
        now,
    )
    .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();

    let linked = db::link_wallet_to_artist_if_confident(&conn, &wid, "feed-w").unwrap();
    assert!(
        linked,
        "should create link when alias matches feed artist credit"
    );

    let link_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wallet_artist_links WHERE wallet_id = ?1",
            params![wid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(link_count, 1);
}

#[test]
fn bot_service_high_confidence_skipped_for_artist_linking() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-w', 'feed-w', 'Wallet Artist', 'keysend', 'feebot', 1, 1)",
        [],
    )
    .unwrap();
    let route_id = conn.last_insert_rowid();

    let ep = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "feebot",
        "",
        "",
        Some("Wallet Artist"),
        now,
    )
    .unwrap();
    db::map_track_route_to_endpoint(&conn, route_id, ep, now).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();
    db::classify_wallet_hard_signals(&conn, &wid).unwrap();

    let linked = db::link_wallet_to_artist_if_confident(&conn, &wid, "feed-w").unwrap();
    assert!(!linked, "bot_service/high_confidence should be skipped");
}

// ---------------------------------------------------------------------------
// Merge + cleanup
// ---------------------------------------------------------------------------

#[test]
fn wallet_merge_repoints_endpoints_and_creates_redirect() {
    let conn = common::test_db();
    let now = common::now();

    let ep1 =
        db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
    let ep2 =
        db::get_or_create_endpoint(&conn, "keysend", "def456", "", "", Some("Alice Alt"), now)
            .unwrap();

    let w1 = db::create_provisional_wallet(&conn, ep1, now).unwrap();
    let w2 = db::create_provisional_wallet(&conn, ep2, now).unwrap();

    db::merge_wallets(&conn, &w2, &w1).unwrap();

    // Both endpoints should now point to w1
    let ep2_wallet: String = conn
        .query_row(
            "SELECT wallet_id FROM wallet_endpoints WHERE id = ?1",
            params![ep2],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ep2_wallet, w1);

    // Redirect should exist
    let redirect_target: String = conn
        .query_row(
            "SELECT new_wallet_id FROM wallet_id_redirect WHERE old_wallet_id = ?1",
            params![w2],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(redirect_target, w1);

    // Old wallet should be deleted
    let old_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM wallets WHERE wallet_id = ?1)",
            params![w2],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!old_exists);
}

#[test]
fn redirect_chains_repointed() {
    let conn = common::test_db();
    let now = common::now();

    let ep1 = db::get_or_create_endpoint(&conn, "keysend", "a", "", "", Some("A"), now).unwrap();
    let ep2 = db::get_or_create_endpoint(&conn, "keysend", "b", "", "", Some("B"), now).unwrap();
    let ep3 = db::get_or_create_endpoint(&conn, "keysend", "c", "", "", Some("C"), now).unwrap();

    let w1 = db::create_provisional_wallet(&conn, ep1, now).unwrap();
    let w2 = db::create_provisional_wallet(&conn, ep2, now).unwrap();
    let w3 = db::create_provisional_wallet(&conn, ep3, now).unwrap();

    // Merge w2 into w1 (creates redirect w2 → w1)
    db::merge_wallets(&conn, &w2, &w1).unwrap();
    // Merge w1 into w3 (should repoint w2 redirect to w3 as well)
    db::merge_wallets(&conn, &w1, &w3).unwrap();

    let w2_target: String = conn
        .query_row(
            "SELECT new_wallet_id FROM wallet_id_redirect WHERE old_wallet_id = ?1",
            params![w2],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(w2_target, w3, "redirect chain should be flattened");
}

#[test]
fn cleanup_removes_orphaned_wallets() {
    let conn = common::test_db();
    let now = common::now();

    let ep =
        db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();

    // Unassign the endpoint (simulates the endpoint being repointed elsewhere)
    conn.execute(
        "UPDATE wallet_endpoints SET wallet_id = NULL WHERE id = ?1",
        params![ep],
    )
    .unwrap();

    let stats = db::cleanup_orphaned_wallets(&conn).unwrap();
    assert_eq!(stats.wallets_deleted, 1);

    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM wallets WHERE wallet_id = ?1)",
            params![wid],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!exists);
}

#[test]
fn display_name_rederived_after_merge() {
    let conn = common::test_db();

    // ep1 has alias "Bob" at t=2000, ep2 has alias "Alice" at t=1000
    let ep1 =
        db::get_or_create_endpoint(&conn, "keysend", "abc", "", "", Some("Bob"), 2000).unwrap();
    let ep2 =
        db::get_or_create_endpoint(&conn, "keysend", "def", "", "", Some("Alice"), 1000).unwrap();

    let w1 = db::create_provisional_wallet(&conn, ep1, 3000).unwrap();
    let w2 = db::create_provisional_wallet(&conn, ep2, 3000).unwrap();

    // Before merge, w1 display_name is "Bob"
    let name: String = conn
        .query_row(
            "SELECT display_name FROM wallets WHERE wallet_id = ?1",
            params![w1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name, "Bob");

    // Merge w2 into w1 — "Alice" is earlier, so display_name should become "Alice"
    db::merge_wallets(&conn, &w2, &w1).unwrap();

    let name: String = conn
        .query_row(
            "SELECT display_name FROM wallets WHERE wallet_id = ?1",
            params![w1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        name, "Alice",
        "display name should be re-derived from first-seen alias"
    );
}

// ---------------------------------------------------------------------------
// Backfill integration
// ---------------------------------------------------------------------------

#[test]
fn backfill_is_idempotent() {
    let conn = common::test_db();
    let now = common::now();

    // Set up a feed + track + routes
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES ('art-bf', 'BF Artist', 'bf artist', ?1, ?2)",
        params![now, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES ('BF Artist', ?1)",
        params![now],
    )
    .unwrap();
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, 'art-bf', 0, 'BF Artist', '')",
        params![credit_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         explicit, episode_count, created_at, updated_at) \
         VALUES ('feed-bf', 'https://example.com/bf.xml', 'BF Feed', 'bf feed', ?1, 0, 0, ?2, ?3)",
        params![credit_id, now, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         explicit, created_at, updated_at) \
         VALUES ('track-bf', 'feed-bf', ?1, 'BF Track', 'bf track', 0, ?2, ?3)",
        params![credit_id, now, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-bf', 'feed-bf', 'BF Artist', 'keysend', 'bfaddr', 95, 0)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-bf', 'feed-bf', 'App Fee', 'keysend', 'appfee', 5, 1)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('feed-bf', 'BF Artist', 'keysend', 'bfaddr', 100, 0)",
        [],
    ).unwrap();

    // Run passes 1-3
    let s1 = db::backfill_wallet_pass1(&conn).unwrap();
    assert!(
        s1.endpoints_created >= 2,
        "should create at least 2 endpoints"
    );

    let s2 = db::backfill_wallet_pass2(&conn).unwrap();
    assert!(s2.wallets_created >= 2);

    let _s3 = db::backfill_wallet_pass3(&conn).unwrap();

    // Capture counts
    let ep_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0))
        .unwrap();
    let wallet_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0))
        .unwrap();
    let link_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallet_artist_links", [], |r| r.get(0))
        .unwrap();

    // Run again — should be idempotent
    let s1b = db::backfill_wallet_pass1(&conn).unwrap();
    assert_eq!(
        s1b.endpoints_created, 0,
        "second run should create no new endpoints"
    );

    let s2b = db::backfill_wallet_pass2(&conn).unwrap();
    assert_eq!(
        s2b.wallets_created, 0,
        "second run should create no new wallets"
    );

    db::backfill_wallet_pass3(&conn).unwrap();

    let ep2: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0))
        .unwrap();
    let w2: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0))
        .unwrap();
    let l2: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallet_artist_links", [], |r| r.get(0))
        .unwrap();

    assert_eq!(ep_count, ep2, "endpoint count should not change");
    assert_eq!(wallet_count, w2, "wallet count should not change");
    assert_eq!(link_count, l2, "artist link count should not change");
}

#[test]
fn fee_true_wallet_gets_no_artist_link_in_backfill() {
    let conn = common::test_db();
    let _now = seed_feed_and_track(&conn);

    // Insert a fee=true route with name matching the feed's artist credit
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-w', 'feed-w', 'Wallet Artist', 'keysend', 'feeaddr', 5, 1)",
        [],
    ).unwrap();

    db::backfill_wallet_pass1(&conn).unwrap();
    db::backfill_wallet_pass2(&conn).unwrap();
    db::backfill_wallet_pass3(&conn).unwrap();

    let link_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallet_artist_links", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        link_count, 0,
        "fee=true wallet should get no artist link even if name matches"
    );
}

#[test]
fn per_feed_resolver_creates_endpoints_and_wallets() {
    let conn = common::test_db();
    let _now = seed_feed_and_track(&conn);

    // Insert routes for feed-w
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-w', 'feed-w', 'Artist', 'keysend', 'artistaddr', 95, 0)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-w', 'feed-w', 'App Fee', 'keysend', 'feeaddr', 5, 1)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('feed-w', 'Artist', 'keysend', 'artistaddr', 100, 0)",
        [],
    ).unwrap();

    let stats = db::resolve_wallet_identity_for_feed(&conn, "feed-w").unwrap();

    assert_eq!(
        stats.endpoints_created, 2,
        "should create 2 distinct endpoints"
    );
    assert_eq!(stats.wallets_created, 2, "should create 2 wallets");
    assert_eq!(stats.track_maps_created, 2, "2 track routes mapped");
    assert_eq!(stats.feed_maps_created, 1, "1 feed route mapped");

    // The fee route should have been classified
    let bot_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wallets WHERE wallet_class = 'bot_service'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bot_count, 1);

    // Running again is idempotent
    let stats2 = db::resolve_wallet_identity_for_feed(&conn, "feed-w").unwrap();
    assert_eq!(stats2.endpoints_created, 0);
    assert_eq!(stats2.wallets_created, 0);
}

#[test]
fn wallet_dirty_bit_is_in_default_mask() {
    use stophammer::resolver::queue;
    const _: () = assert!(
        (queue::DEFAULT_DIRTY_MASK & queue::DIRTY_WALLET_IDENTITY) != 0,
        // "DIRTY_WALLET_IDENTITY must be in DEFAULT_DIRTY_MASK"
    );
}

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-wallet"));
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(vec![])),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

#[tokio::test]
async fn get_wallet_endpoint_returns_wallet_details() {
    let db = common::test_db_arc();
    let wid;
    {
        let conn = db.lock().unwrap();
        let now = common::now();
        let ep = db::get_or_create_endpoint(
            &conn,
            "keysend",
            "abc123",
            "7629169",
            "pod1",
            Some("Alice"),
            now,
        )
        .unwrap();
        wid = db::create_provisional_wallet(&conn, ep, now).unwrap();
    }
    let state = test_app_state(db);
    let app = stophammer::api::build_router(state);
    let req = Request::builder()
        .uri(format!("/v1/wallets/{wid}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = &json["data"];
    assert_eq!(data["wallet_id"], wid);
    assert_eq!(data["display_name"], "Alice");
    assert_eq!(data["wallet_class"], "unknown");
    assert_eq!(data["class_confidence"], "provisional");
    assert_eq!(data["endpoints"].as_array().unwrap().len(), 1);
    assert_eq!(data["endpoints"][0]["route_type"], "keysend");
    assert_eq!(data["endpoints"][0]["custom_value"], "pod1");
    assert_eq!(data["aliases"].as_array().unwrap().len(), 1);
    assert_eq!(data["aliases"][0]["alias"], "Alice");
}

#[tokio::test]
async fn get_wallet_follows_redirect() {
    let db = common::test_db_arc();
    let new_wid;
    let old_wid;
    {
        let conn = db.lock().unwrap();
        let now = common::now();
        let ep1 =
            db::get_or_create_endpoint(&conn, "keysend", "abc", "", "", Some("A"), now).unwrap();
        let ep2 =
            db::get_or_create_endpoint(&conn, "keysend", "def", "", "", Some("B"), now).unwrap();
        old_wid = db::create_provisional_wallet(&conn, ep1, now).unwrap();
        new_wid = db::create_provisional_wallet(&conn, ep2, now).unwrap();
        db::merge_wallets(&conn, &old_wid, &new_wid).unwrap();
    }
    let state = test_app_state(db);
    let app = stophammer::api::build_router(state);
    let req = Request::builder()
        .uri(format!("/v1/wallets/{old_wid}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"]["wallet_id"], new_wid, "should follow redirect");
}

#[tokio::test]
async fn get_wallet_not_found() {
    let db = common::test_db_arc();
    let state = test_app_state(db);
    let app = stophammer::api::build_router(state);
    let req = Request::builder()
        .uri("/v1/wallets/nonexistent")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 404);
}

// ---------------------------------------------------------------------------
// Owner grouping (Pass 5)
// ---------------------------------------------------------------------------

#[test]
fn same_feed_same_name_endpoints_grouped() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    // Two different keysend addresses, same name, same feed
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-w', 'feed-w', 'Alice', 'keysend', 'addr1', 50, 0)",
        [],
    ).unwrap();
    let r1 = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-w', 'feed-w', 'Alice', 'keysend', 'addr2', 50, 0)",
        [],
    ).unwrap();
    let r2 = conn.last_insert_rowid();

    let ep1 =
        db::get_or_create_endpoint(&conn, "keysend", "addr1", "", "", Some("Alice"), now).unwrap();
    let ep2 =
        db::get_or_create_endpoint(&conn, "keysend", "addr2", "", "", Some("Alice"), now).unwrap();
    db::map_track_route_to_endpoint(&conn, r1, ep1, now).unwrap();
    db::map_track_route_to_endpoint(&conn, r2, ep2, now).unwrap();
    db::create_provisional_wallet(&conn, ep1, now).unwrap();
    db::create_provisional_wallet(&conn, ep2, now).unwrap();

    let merges = db::group_same_feed_endpoints(&conn, "feed-w").unwrap();
    assert_eq!(merges, 1, "should merge same-name endpoints");

    let wallet_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(wallet_count, 1, "should have 1 wallet after grouping");
}

#[test]
fn fee_vs_nonfee_same_name_not_grouped() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-w', 'feed-w', 'Alice', 'keysend', 'addr1', 95, 0)",
        [],
    ).unwrap();
    let r1 = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-w', 'feed-w', 'Alice', 'keysend', 'addr2', 5, 1)",
        [],
    ).unwrap();
    let r2 = conn.last_insert_rowid();

    let ep1 =
        db::get_or_create_endpoint(&conn, "keysend", "addr1", "", "", Some("Alice"), now).unwrap();
    let ep2 =
        db::get_or_create_endpoint(&conn, "keysend", "addr2", "", "", Some("Alice"), now).unwrap();
    db::map_track_route_to_endpoint(&conn, r1, ep1, now).unwrap();
    db::map_track_route_to_endpoint(&conn, r2, ep2, now).unwrap();
    let w1 = db::create_provisional_wallet(&conn, ep1, now).unwrap();
    let w2 = db::create_provisional_wallet(&conn, ep2, now).unwrap();
    // Classify so fee endpoint becomes bot_service (different wallet_class)
    db::classify_wallet_hard_signals(&conn, &w1).unwrap();
    db::classify_wallet_hard_signals(&conn, &w2).unwrap();

    let merges = db::group_same_feed_endpoints(&conn, "feed-w").unwrap();
    assert_eq!(merges, 0, "fee vs non-fee should NOT be grouped");

    let wallet_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(wallet_count, 2);
}

#[test]
fn review_items_created_for_cross_wallet_aliases() {
    let conn = common::test_db();
    let now = common::now();

    // Two endpoints with same alias but different wallets
    let ep1 =
        db::get_or_create_endpoint(&conn, "keysend", "addr1", "", "", Some("Alice"), now).unwrap();
    let ep2 =
        db::get_or_create_endpoint(&conn, "keysend", "addr2", "", "", Some("Alice"), now).unwrap();
    db::create_provisional_wallet(&conn, ep1, now).unwrap();
    db::create_provisional_wallet(&conn, ep2, now).unwrap();

    let created = db::generate_wallet_review_items(&conn).unwrap();
    assert!(
        created >= 2,
        "should create review items for both wallets sharing 'alice'"
    );

    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wallet_identity_review WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(pending >= 2);

    // Idempotent
    let created2 = db::generate_wallet_review_items(&conn).unwrap();
    assert_eq!(created2, 0, "should not create duplicate review items");
}

#[test]
fn likely_wallet_owner_match_reviews_created_for_same_alias_on_same_feed() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    let route_a = insert_feed_route(&conn, "feed-w", "Alice", "addr-owner-a");
    let route_b = insert_feed_route(&conn, "feed-w", "Alice", "addr-owner-b");

    let ep_a = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "addr-owner-a",
        "",
        "",
        Some("Alice"),
        now,
    )
    .unwrap();
    let ep_b = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "addr-owner-b",
        "",
        "",
        Some("Alice"),
        now,
    )
    .unwrap();
    db::map_feed_route_to_endpoint(&conn, route_a, ep_a, now).unwrap();
    db::map_feed_route_to_endpoint(&conn, route_b, ep_b, now).unwrap();

    let _wallet_a = db::create_provisional_wallet(&conn, ep_a, now).unwrap();
    let _wallet_b = db::create_provisional_wallet(&conn, ep_b, now).unwrap();

    let created = db::generate_wallet_review_items(&conn).unwrap();
    assert!(
        created >= 4,
        "two same-alias wallets on one feed should produce both cross_wallet_alias and likely_wallet_owner_match reviews"
    );

    let sources = db::list_pending_wallet_reviews(&conn, 10)
        .unwrap()
        .into_iter()
        .map(|review| (review.source, review.confidence, review.explanation))
        .collect::<Vec<_>>();
    assert!(
        sources.iter().any(|(source, confidence, explanation)| {
            source == "likely_wallet_owner_match"
                && confidence == "high_confidence"
                && explanation.contains("appear on the same feed")
        }),
        "same-feed alias overlap should create a high-confidence likely_wallet_owner_match review"
    );
    let likely_review = db::list_pending_wallet_reviews(&conn, 10)
        .unwrap()
        .into_iter()
        .find(|review| review.source == "likely_wallet_owner_match")
        .expect("likely wallet review");
    assert_eq!(likely_review.score, Some(65));
}

#[test]
fn likely_wallet_owner_match_includes_shared_artist_link_support() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    let route_a = insert_feed_route(&conn, "feed-w", "Alice", "addr-owner-a");
    let route_b = insert_feed_route(&conn, "feed-w", "Alice", "addr-owner-b");

    let ep_a = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "addr-owner-a",
        "",
        "",
        Some("Alice"),
        now,
    )
    .unwrap();
    let ep_b = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "addr-owner-b",
        "",
        "",
        Some("Alice"),
        now,
    )
    .unwrap();
    db::map_feed_route_to_endpoint(&conn, route_a, ep_a, now).unwrap();
    db::map_feed_route_to_endpoint(&conn, route_b, ep_b, now).unwrap();

    let wallet_a = db::create_provisional_wallet(&conn, ep_a, now).unwrap();
    let wallet_b = db::create_provisional_wallet(&conn, ep_b, now).unwrap();

    let artist = stophammer::model::Artist {
        artist_id: "artist-alice-wallet-link".into(),
        name: "Alice".into(),
        name_lower: "alice".into(),
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: now,
        updated_at: now,
    };
    db::upsert_artist_if_absent(&conn, &artist).unwrap();
    conn.execute(
        "INSERT INTO wallet_artist_links \
         (wallet_id, artist_id, confidence, evidence_entity_type, evidence_entity_id, created_at) \
         VALUES (?1, ?2, 'high_confidence', 'feed', 'feed-w', ?3)",
        rusqlite::params![wallet_a, artist.artist_id, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO wallet_artist_links \
         (wallet_id, artist_id, confidence, evidence_entity_type, evidence_entity_id, created_at) \
         VALUES (?1, ?2, 'high_confidence', 'feed', 'feed-w', ?3)",
        rusqlite::params![wallet_b, artist.artist_id, now],
    )
    .unwrap();

    db::generate_wallet_review_items(&conn).unwrap();

    let likely_review = db::list_pending_wallet_reviews(&conn, 10)
        .unwrap()
        .into_iter()
        .find(|review| review.source == "likely_wallet_owner_match")
        .expect("likely wallet review");
    assert_eq!(likely_review.score, Some(85));
    assert!(
        likely_review
            .supporting_sources
            .contains(&"shared_artist_link".to_string()),
        "shared linked-artist evidence should strengthen likely owner matches"
    );
}

#[test]
fn wallet_claim_feeds_include_routes_and_source_claims() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    let route_id = insert_feed_route(&conn, "feed-w", "Alice", "addr-claims");
    let endpoint =
        db::get_or_create_endpoint(&conn, "keysend", "addr-claims", "", "", Some("Alice"), now)
            .unwrap();
    db::map_feed_route_to_endpoint(&conn, route_id, endpoint, now).unwrap();
    let wallet_id = db::create_provisional_wallet(&conn, endpoint, now).unwrap();

    db::replace_source_contributor_claims_for_feed(
        &conn,
        "feed-w",
        &[stophammer::model::SourceContributorClaim {
            id: None,
            feed_guid: "feed-w".into(),
            entity_type: "feed".into(),
            entity_id: "feed-w".into(),
            position: 0,
            name: "Wallet Artist".into(),
            role: Some("artist".into()),
            role_norm: Some("artist".into()),
            group_name: None,
            href: Some("https://example.com/artist".into()),
            img: None,
            source: "test".into(),
            extraction_path: "feed.person".into(),
            observed_at: now,
        }],
    )
    .unwrap();
    db::replace_source_entity_ids_for_feed(
        &conn,
        "feed-w",
        &[stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: "feed-w".into(),
            entity_type: "feed".into(),
            entity_id: "feed-w".into(),
            position: 0,
            scheme: "nostr_npub".into(),
            value: "npub1walletclaim".into(),
            source: "test".into(),
            extraction_path: "feed.value".into(),
            observed_at: now,
        }],
    )
    .unwrap();
    db::replace_source_entity_links_for_feed(
        &conn,
        "feed-w",
        &[stophammer::model::SourceEntityLink {
            id: None,
            feed_guid: "feed-w".into(),
            entity_type: "feed".into(),
            entity_id: "feed-w".into(),
            position: 0,
            link_type: "website".into(),
            url: "https://example.com/feed".into(),
            source: "test".into(),
            extraction_path: "feed.link".into(),
            observed_at: now,
        }],
    )
    .unwrap();
    db::replace_source_release_claims_for_feed(
        &conn,
        "feed-w",
        &[stophammer::model::SourceReleaseClaim {
            id: None,
            feed_guid: "feed-w".into(),
            entity_type: "feed".into(),
            entity_id: "feed-w".into(),
            position: 0,
            claim_type: "description".into(),
            claim_value: "Feed description".into(),
            source: "test".into(),
            extraction_path: "feed.description".into(),
            observed_at: now,
        }],
    )
    .unwrap();
    db::replace_source_platform_claims_for_feed(
        &conn,
        "feed-w",
        &[stophammer::model::SourcePlatformClaim {
            id: None,
            feed_guid: "feed-w".into(),
            platform_key: "wavlake".into(),
            url: Some("https://wavlake.com/feed-w".into()),
            owner_name: Some("Wavlake".into()),
            source: "test".into(),
            extraction_path: "feed.platform".into(),
            observed_at: now,
        }],
    )
    .unwrap();

    let claim_feeds = db::get_wallet_claim_feeds(&conn, &wallet_id).unwrap();
    assert_eq!(claim_feeds.len(), 1, "wallet should touch one feed");
    assert_eq!(claim_feeds[0].feed_guid, "feed-w");
    assert_eq!(
        claim_feeds[0].routes.len(),
        1,
        "route evidence should be included"
    );
    assert_eq!(claim_feeds[0].contributor_claims.len(), 1);
    assert_eq!(claim_feeds[0].entity_id_claims.len(), 1);
    assert_eq!(claim_feeds[0].link_claims.len(), 1);
    assert_eq!(claim_feeds[0].release_claims.len(), 1);
    assert_eq!(claim_feeds[0].platform_claims.len(), 1);
}

// ---- Soft-signal classification tests ----

#[test]
fn soft_signal_fountain_alias() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);

    let ep_id = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "abc123",
        "7629169",
        "fountain_pod",
        Some("Fountain"),
        1000,
    )
    .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();

    // Should still be unknown/provisional after hard signals (no fee, no override)
    let (cls, cnf): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(cls, "unknown");
    assert_eq!(cnf, "provisional");

    let classified = db::classify_wallet_soft_signals(&conn, &wid).unwrap();
    assert!(classified);

    let (cls, cnf): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(cls, "organization_platform");
    assert_eq!(cnf, "provisional");
}

#[test]
fn soft_signal_no_override_high_confidence() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);

    // Create endpoint and map route BEFORE creating wallet (so hard signals can find fee=true)
    let route_id = insert_track_route(&conn, "track-w", "Fountain", "abc123");
    conn.execute(
        "UPDATE payment_routes SET fee = 1 WHERE id = ?1",
        params![route_id],
    )
    .unwrap();
    let ep_id =
        db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Fountain"), 1000)
            .unwrap();
    db::map_track_route_to_endpoint(&conn, route_id, ep_id, 1000).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();
    db::classify_wallet_hard_signals(&conn, &wid).unwrap();

    let (cls, cnf): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(cls, "bot_service");
    assert_eq!(cnf, "high_confidence");

    // Soft signals must not override
    let classified = db::classify_wallet_soft_signals(&conn, &wid).unwrap();
    assert!(!classified);

    let (cls2, cnf2): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(cls2, "bot_service");
    assert_eq!(cnf2, "high_confidence");
}

#[test]
fn soft_signal_no_override_reviewed() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);

    let ep_id =
        db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Fountain"), 1000)
            .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();

    // Force to reviewed via override
    conn.execute(
        "INSERT INTO wallet_identity_override (override_type, wallet_id, value, created_at) VALUES ('force_class', ?1, 'person_artist', 1000)",
        params![wid],
    ).unwrap();
    db::classify_wallet_hard_signals(&conn, &wid).unwrap();

    let classified = db::classify_wallet_soft_signals(&conn, &wid).unwrap();
    assert!(!classified);
}

#[test]
fn soft_signal_lnaddress_domain() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);

    let ep_id = db::get_or_create_endpoint(
        &conn,
        "lnaddress",
        "user@getalby.com",
        "",
        "",
        Some("SomeUser"),
        1000,
    )
    .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();

    let classified = db::classify_wallet_soft_signals(&conn, &wid).unwrap();
    assert!(classified);

    let class: String = conn
        .query_row(
            "SELECT wallet_class FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(class, "organization_platform");
}

#[test]
fn soft_signal_unknown_alias_no_change() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);

    let ep_id = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "abc123",
        "",
        "",
        Some("RandomPerson"),
        1000,
    )
    .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();

    let classified = db::classify_wallet_soft_signals(&conn, &wid).unwrap();
    assert!(!classified);

    let class: String = conn
        .query_row(
            "SELECT wallet_class FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(class, "unknown");
}

#[test]
fn soft_signal_partial_name_no_match() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);

    let ep_id = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "abc123",
        "",
        "",
        Some("Fountain Valley Podcast"),
        1000,
    )
    .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();

    let classified = db::classify_wallet_soft_signals(&conn, &wid).unwrap();
    assert!(!classified);

    let class: String = conn
        .query_row(
            "SELECT wallet_class FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(class, "unknown");
}

// ---- Split-shape heuristic tests ----

#[test]
fn split_small_across_many_feeds() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);
    let credit_id: i64 = conn
        .query_row(
            "SELECT artist_credit_id FROM feeds WHERE feed_guid = 'feed-w'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    // Create endpoint + wallet
    let ep_id = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "platform_node",
        "7629169",
        "platform_val",
        Some("PlatformApp"),
        1000,
    )
    .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();

    // Create feed-level routes in 4 different feeds with small non-fee splits
    for i in 0..4 {
        let fg = format!("feed-guid-split-{i}");
        conn.execute(
            "INSERT OR IGNORE INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, explicit, episode_count, created_at, updated_at) \
             VALUES (?1, ?2, 'F', 'f', ?3, 0, 0, 1000, 1000)",
            params![fg, format!("http://example.com/{i}"), credit_id],
        ).unwrap();
        let route_id: i64 = conn.query_row(
            "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
             VALUES (?1, 'PlatformApp', 'keysend', 'platform_node', '7629169', 'platform_val', 3, 0) RETURNING id",
            params![fg],
            |r| r.get(0),
        ).unwrap();
        db::map_feed_route_to_endpoint(&conn, route_id, ep_id, 1000).unwrap();
    }

    let classified = db::classify_wallet_split_heuristics(&conn, &wid).unwrap();
    assert!(classified);

    let class: String = conn
        .query_row(
            "SELECT wallet_class FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(class, "organization_platform");
}

#[test]
fn split_dominant_few_feeds() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);

    let ep_id = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "artist_node",
        "",
        "",
        Some("ArtistName"),
        1000,
    )
    .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();

    // One feed, dominant split (use feed-level route to avoid track FK)
    let route_id: i64 = conn.query_row(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('feed-w', 'ArtistName', 'keysend', 'artist_node', 95, 0) RETURNING id",
        [],
        |r| r.get(0),
    ).unwrap();
    db::map_feed_route_to_endpoint(&conn, route_id, ep_id, 1000).unwrap();

    let classified = db::classify_wallet_split_heuristics(&conn, &wid).unwrap();
    assert!(classified);

    let class: String = conn
        .query_row(
            "SELECT wallet_class FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(class, "person_artist");
}

#[test]
fn split_no_override_soft_signal() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);

    let ep_id =
        db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Wavlake"), 1000)
            .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();

    // Soft-classify first
    db::classify_wallet_soft_signals(&conn, &wid).unwrap();
    let class: String = conn
        .query_row(
            "SELECT wallet_class FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(class, "organization_platform");

    // Split heuristics should not override
    let classified = db::classify_wallet_split_heuristics(&conn, &wid).unwrap();
    assert!(!classified);
}

#[test]
fn split_no_override_high_confidence() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);

    let route_id = insert_track_route(&conn, "track-w", "SomeBot", "botnode");
    conn.execute(
        "UPDATE payment_routes SET fee = 1 WHERE id = ?1",
        params![route_id],
    )
    .unwrap();
    let ep_id =
        db::get_or_create_endpoint(&conn, "keysend", "botnode", "", "", Some("SomeBot"), 1000)
            .unwrap();
    db::map_track_route_to_endpoint(&conn, route_id, ep_id, 1000).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();
    db::classify_wallet_hard_signals(&conn, &wid).unwrap();

    let classified = db::classify_wallet_split_heuristics(&conn, &wid).unwrap();
    assert!(!classified);
}

#[test]
fn split_fee_routes_excluded() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);
    let credit_id: i64 = conn
        .query_row(
            "SELECT artist_credit_id FROM feeds WHERE feed_guid = 'feed-w'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let ep_id =
        db::get_or_create_endpoint(&conn, "keysend", "feenode", "", "", Some("FeeBot"), 1000)
            .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();

    // Create 4 feeds but all routes are fee=1 — should not count toward small-share signal
    for i in 0..4 {
        let fg = format!("feed-guid-fee-{i}");
        conn.execute(
            "INSERT OR IGNORE INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, explicit, episode_count, created_at, updated_at) \
             VALUES (?1, ?2, 'F', 'f', ?3, 0, 0, 1000, 1000)",
            params![fg, format!("http://example.com/fee/{i}"), credit_id],
        ).unwrap();
        let route_id: i64 = conn.query_row(
            "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split, fee) \
             VALUES (?1, 'FeeBot', 'keysend', 'feenode', 3, 1) RETURNING id",
            params![fg],
            |r| r.get(0),
        ).unwrap();
        db::map_feed_route_to_endpoint(&conn, route_id, ep_id, 1000).unwrap();
    }

    let classified = db::classify_wallet_split_heuristics(&conn, &wid).unwrap();
    assert!(!classified);
}

#[test]
fn split_below_feed_threshold() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);
    let credit_id: i64 = conn
        .query_row(
            "SELECT artist_credit_id FROM feeds WHERE feed_guid = 'feed-w'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let ep_id = db::get_or_create_endpoint(
        &conn,
        "keysend",
        "twofeeds",
        "",
        "",
        Some("TwoFeedApp"),
        1000,
    )
    .unwrap();
    let wid = db::create_provisional_wallet(&conn, ep_id, 1000).unwrap();

    // Only 2 feeds — below the threshold of 3
    for i in 0..2 {
        let fg = format!("feed-guid-two-{i}");
        conn.execute(
            "INSERT OR IGNORE INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, explicit, episode_count, created_at, updated_at) \
             VALUES (?1, ?2, 'F', 'f', ?3, 0, 0, 1000, 1000)",
            params![fg, format!("http://example.com/two/{i}"), credit_id],
        ).unwrap();
        let route_id: i64 = conn.query_row(
            "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split, fee) \
             VALUES (?1, 'TwoFeedApp', 'keysend', 'twofeeds', 3, 0) RETURNING id",
            params![fg],
            |r| r.get(0),
        ).unwrap();
        db::map_feed_route_to_endpoint(&conn, route_id, ep_id, 1000).unwrap();
    }

    let classified = db::classify_wallet_split_heuristics(&conn, &wid).unwrap();
    assert!(!classified);

    let class: String = conn
        .query_row(
            "SELECT wallet_class FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(class, "unknown");
}

// ---- Wallet review CRUD tests ----

#[test]
fn list_pending_reviews_filters_resolved() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    let ep1 =
        db::get_or_create_endpoint(&conn, "keysend", "addr1", "", "", Some("Alice"), now).unwrap();
    let wid1 = db::create_provisional_wallet(&conn, ep1, now).unwrap();
    let ep2 =
        db::get_or_create_endpoint(&conn, "keysend", "addr2", "", "", Some("Alice"), now).unwrap();
    let wid2 = db::create_provisional_wallet(&conn, ep2, now).unwrap();

    // Create two reviews — one pending, one resolved
    conn.execute(
        "INSERT INTO wallet_identity_review \
         (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
         VALUES (?1, 'cross_wallet_alias', 'alice', json_array(?1), '[]', 'pending', ?2, ?2)",
        params![wid1, now],
    ).unwrap();
    conn.execute(
        "INSERT INTO wallet_identity_review \
         (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
         VALUES (?1, 'cross_wallet_alias', 'alice', json_array(?1), '[]', 'resolved', ?2, ?3)",
        params![wid2, now, now],
    ).unwrap();

    let reviews = db::list_pending_wallet_reviews(&conn, 50).unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].wallet_id, wid1);
}

#[test]
fn pending_wallet_reviews_prioritize_high_confidence_sources() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    let ep1 =
        db::get_or_create_endpoint(&conn, "keysend", "addr-priority-1", "", "", Some("Alice"), now)
            .unwrap();
    let wid1 = db::create_provisional_wallet(&conn, ep1, now).unwrap();
    let ep2 =
        db::get_or_create_endpoint(&conn, "keysend", "addr-priority-2", "", "", Some("Alice"), now)
            .unwrap();
    let wid2 = db::create_provisional_wallet(&conn, ep2, now).unwrap();

    conn.execute(
        "INSERT INTO wallet_identity_review \
         (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
         VALUES (?1, 'cross_wallet_alias', 'alice', json_array(?1), '[]', 'pending', ?2, ?2)",
        params![wid1, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO wallet_identity_review \
         (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
         VALUES (?1, 'likely_wallet_owner_match', 'alice', json_array(?1, ?2), '[]', 'pending', ?3, ?3)",
        params![wid2, wid1, now - 60],
    )
    .unwrap();

    let reviews = db::list_pending_wallet_reviews(&conn, 10).unwrap();
    let sources = reviews
        .iter()
        .map(|review| review.source.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        sources,
        vec!["likely_wallet_owner_match", "cross_wallet_alias"],
        "pending wallet reviews should prioritize high-confidence items ahead of review_required items"
    );
}

#[test]
fn set_override_resolves_review() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    let ep =
        db::get_or_create_endpoint(&conn, "keysend", "addr1", "", "", Some("Alice"), now).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();

    let review_id: i64 = conn.query_row(
        "INSERT INTO wallet_identity_review \
         (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
         VALUES (?1, 'cross_wallet_alias', 'alice', json_array(?1), '[]', 'pending', ?2, ?2) RETURNING id",
        params![wid, now],
        |r| r.get(0),
    ).unwrap();

    db::set_wallet_identity_override_for_review(&conn, review_id, "do_not_merge", None, None)
        .unwrap();

    let status: String = conn
        .query_row(
            "SELECT status FROM wallet_identity_review WHERE id = ?1",
            params![review_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "blocked");

    let override_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM wallet_identity_override WHERE wallet_id = ?1 AND override_type = 'do_not_merge'",
        params![wid],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(override_count, 1);
}

#[test]
fn backfill_refresh_applies_wallet_merge_overrides() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    let ep1 = db::get_or_create_endpoint(&conn, "keysend", "addr-left", "", "", Some("Alice"), now)
        .unwrap();
    let ep2 =
        db::get_or_create_endpoint(&conn, "keysend", "addr-right", "", "", Some("Alice"), now)
            .unwrap();

    let w1 = db::create_provisional_wallet(&conn, ep1, now).unwrap();
    let w2 = db::create_provisional_wallet(&conn, ep2, now).unwrap();

    let review_id: i64 = conn
        .query_row(
            "INSERT INTO wallet_identity_review \
             (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
             VALUES (?1, 'cross_wallet_alias', 'alice', json_array(?1, ?3), '[]', 'pending', ?2, ?2) RETURNING id",
            params![w2, now, w1],
            |r| r.get(0),
        )
        .unwrap();

    db::set_wallet_identity_override_for_review(&conn, review_id, "merge", Some(&w1), None)
        .unwrap();

    let stats = db::backfill_wallet_pass5(&conn).unwrap();
    assert_eq!(stats.merges_from_overrides, 1);
    assert!(stats.apply_batch_id.is_some());

    let ep2_wallet: String = conn
        .query_row(
            "SELECT wallet_id FROM wallet_endpoints WHERE id = ?1",
            params![ep2],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ep2_wallet, w1, "override merge should repoint endpoints");

    let redirect_target: String = conn
        .query_row(
            "SELECT new_wallet_id FROM wallet_id_redirect WHERE old_wallet_id = ?1",
            params![w2],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(redirect_target, w1);
}

#[test]
fn undo_last_wallet_merge_batch_restores_wallets() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    let ep1 = db::get_or_create_endpoint(&conn, "keysend", "undo-left", "", "", Some("Alice"), now)
        .unwrap();
    let ep2 =
        db::get_or_create_endpoint(&conn, "keysend", "undo-right", "", "", Some("Alice"), now)
            .unwrap();

    let w1 = db::create_provisional_wallet(&conn, ep1, now).unwrap();
    let w2 = db::create_provisional_wallet(&conn, ep2, now).unwrap();

    let review_id: i64 = conn
        .query_row(
            "INSERT INTO wallet_identity_review \
             (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
             VALUES (?1, 'cross_wallet_alias', 'alice', json_array(?1, ?3), '[]', 'pending', ?2, ?2) RETURNING id",
            params![w2, now, w1],
            |r| r.get(0),
        )
        .unwrap();

    db::set_wallet_identity_override_for_review(&conn, review_id, "merge", Some(&w1), None)
        .unwrap();
    let apply_stats = db::backfill_wallet_pass5(&conn).unwrap();
    assert_eq!(apply_stats.merges_from_overrides, 1);

    let undo_stats = db::undo_last_wallet_merge_batch(&conn)
        .unwrap()
        .expect("expected undo batch");
    assert_eq!(undo_stats.merges_reverted, 1);

    let old_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM wallets WHERE wallet_id = ?1)",
            params![w2],
            |r| r.get(0),
        )
        .unwrap();
    assert!(old_exists, "old wallet should be restored");

    let ep2_wallet: String = conn
        .query_row(
            "SELECT wallet_id FROM wallet_endpoints WHERE id = ?1",
            params![ep2],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        ep2_wallet, w2,
        "endpoint should move back to restored wallet"
    );

    let redirect_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM wallet_id_redirect WHERE old_wallet_id = ?1)",
            params![w2],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!redirect_exists, "undo should remove the applied redirect");
}

#[test]
fn force_class_override_consumed_by_hard_signals() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    let ep =
        db::get_or_create_endpoint(&conn, "keysend", "addr1", "", "", Some("Alice"), now).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();

    let review_id: i64 = conn.query_row(
        "INSERT INTO wallet_identity_review \
         (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
         VALUES (?1, 'cross_wallet_alias', 'alice', json_array(?1), '[]', 'pending', ?2, ?2) RETURNING id",
        params![wid, now],
        |r| r.get(0),
    ).unwrap();

    // Force class via review tool path
    db::set_wallet_identity_override_for_review(
        &conn,
        review_id,
        "force_class",
        None,
        Some("person_artist"),
    )
    .unwrap();

    // Now hard signals should pick up the override
    db::classify_wallet_hard_signals(&conn, &wid).unwrap();

    let (cls, cnf): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(cls, "person_artist");
    assert_eq!(cnf, "reviewed");
}

#[test]
fn get_wallet_detail_returns_full_info() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

    let ep =
        db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();

    let detail = db::get_wallet_detail(&conn, &wid).unwrap().unwrap();
    assert_eq!(detail.wallet_id, wid);
    assert_eq!(detail.display_name, "Alice");
    assert_eq!(detail.endpoints.len(), 1);
    assert_eq!(detail.aliases.len(), 1);
    assert_eq!(detail.aliases[0].alias, "Alice");
}

#[test]
fn wallet_dirty_bit_after_promotions_before_search() {
    use stophammer::resolver::queue;
    // Wallet identity bit should be distinct from all other bits
    assert_eq!(queue::DIRTY_WALLET_IDENTITY, 1 << 5);
    assert_ne!(
        queue::DIRTY_WALLET_IDENTITY,
        queue::DIRTY_CANONICAL_PROMOTIONS
    );
    assert_ne!(queue::DIRTY_WALLET_IDENTITY, queue::DIRTY_CANONICAL_SEARCH);
}

// ---------------------------------------------------------------------------
// Wavlake route exclusion
// ---------------------------------------------------------------------------

/// Create a Wavlake feed with a platform claim and a track, returning `now`.
fn seed_wavlake_feed(conn: &rusqlite::Connection, feed_guid: &str, track_guid: &str) -> i64 {
    let now = common::now();
    // Reuse existing artist if present, otherwise create one.
    let artist_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM artists WHERE artist_id = 'artist-w')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    if !artist_exists {
        conn.execute(
            "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
             VALUES ('artist-w', 'Wallet Artist', 'wallet artist', ?1, ?1)",
            params![now],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT OR IGNORE INTO artist_credit (display_name, created_at) VALUES ('Wallet Artist', ?1)",
        params![now],
    )
    .unwrap();
    let credit_id: i64 = conn
        .query_row(
            "SELECT id FROM artist_credit WHERE display_name = 'Wallet Artist'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, 'artist-w', 0, 'Wallet Artist', '')",
        params![credit_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, 'WL Feed', 'wl feed', ?3, 0, 0, ?4, ?4)",
        params![
            feed_guid,
            format!("https://wavlake.com/feed/music/{feed_guid}"),
            credit_id,
            now
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         explicit, created_at, updated_at) \
         VALUES (?1, ?2, ?3, 'WL Track', 'wl track', 0, ?4, ?4)",
        params![track_guid, feed_guid, credit_id, now],
    )
    .unwrap();
    // Mark this feed as a Wavlake feed via platform claim.
    conn.execute(
        "INSERT INTO source_platform_claims \
         (feed_guid, platform_key, url, source, extraction_path, observed_at) \
         VALUES (?1, 'wavlake', ?2, 'platform_classifier', 'request.canonical_url', ?3)",
        params![
            feed_guid,
            format!("https://wavlake.com/feed/music/{feed_guid}"),
            now
        ],
    )
    .unwrap();
    now
}

#[test]
fn wavlake_routes_skipped_in_pass1() {
    let conn = common::test_db();
    seed_wavlake_feed(&conn, "feed-wl", "track-wl");

    // Insert a track route and a feed route on the Wavlake feed.
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-wl', 'feed-wl', 'Wavlake', 'keysend', 'wl-node-abc', 5, 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('feed-wl', 'Wavlake', 'keysend', 'wl-node-def', 3, 0)",
        [],
    )
    .unwrap();

    let stats = db::backfill_wallet_pass1(&conn).unwrap();
    assert_eq!(
        stats.endpoints_created, 0,
        "Wavlake routes should produce no endpoints"
    );
    assert_eq!(stats.track_maps_created, 0);
    assert_eq!(stats.feed_maps_created, 0);
}

#[test]
fn non_wavlake_routes_unaffected() {
    let conn = common::test_db();
    seed_feed_and_track(&conn);
    insert_track_route(&conn, "track-w", "Alice", "alice-node-123");

    let stats = db::backfill_wallet_pass1(&conn).unwrap();
    assert_eq!(
        stats.endpoints_created, 1,
        "non-Wavlake route should create an endpoint"
    );
    assert_eq!(stats.track_maps_created, 1);
}

#[test]
fn incremental_resolver_skips_wavlake_feed() {
    let conn = common::test_db();
    seed_wavlake_feed(&conn, "feed-wl2", "track-wl2");

    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-wl2', 'feed-wl2', 'Wavlake', 'keysend', 'wl-node-ghi', 5, 0)",
        [],
    )
    .unwrap();

    let stats = db::resolve_wallet_identity_for_feed(&conn, "feed-wl2").unwrap();
    assert_eq!(stats.endpoints_created, 0);
    assert_eq!(stats.wallets_created, 0);
}

#[test]
fn purge_removes_wavlake_wallets() {
    let conn = common::test_db();
    seed_wavlake_feed(&conn, "feed-wl3", "track-wl3");

    // Manually create a route + endpoint + wallet for this Wavlake feed
    // (simulating state before the filter was added).
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-wl3', 'feed-wl3', 'Wavlake', 'keysend', 'wl-node-jkl', 5, 0)",
        [],
    )
    .unwrap();
    let route_id = conn.last_insert_rowid();

    let now = common::now();
    conn.execute(
        "INSERT INTO wallet_endpoints (route_type, normalized_address, custom_key, custom_value, created_at) \
         VALUES ('keysend', 'wl-node-jkl', '', '', ?1)",
        params![now],
    )
    .unwrap();
    let ep_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO wallet_track_route_map (route_id, endpoint_id, created_at) VALUES (?1, ?2, ?3)",
        params![route_id, ep_id, now],
    )
    .unwrap();

    // Create a wallet and assign the endpoint.
    conn.execute(
        "INSERT INTO wallets (wallet_id, display_name, display_name_lower, wallet_class, class_confidence, created_at, updated_at) \
         VALUES ('wl-wallet-1', 'Wavlake', 'wavlake', 'unknown', 'provisional', ?1, ?1)",
        params![now],
    )
    .unwrap();
    conn.execute(
        "UPDATE wallet_endpoints SET wallet_id = 'wl-wallet-1' WHERE id = ?1",
        params![ep_id],
    )
    .unwrap();

    assert_eq!(endpoint_count(&conn), 1);
    let wallet_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(wallet_count, 1);

    // Purge + orphan cleanup should remove everything.
    let removed = db::purge_wavlake_wallet_route_maps(&conn).unwrap();
    assert!(removed > 0, "should have removed route map entries");
    let cleanup = db::cleanup_orphaned_wallets(&conn).unwrap();
    assert_eq!(cleanup.wallets_deleted, 1);
    assert_eq!(endpoint_count(&conn), 0);
}

#[test]
fn purge_preserves_shared_endpoints() {
    let conn = common::test_db();

    // Create both a Wavlake feed and a non-Wavlake feed.
    seed_wavlake_feed(&conn, "feed-wl4", "track-wl4");
    // Non-Wavlake feed reuses the same artist credit.
    let now = common::now();
    let credit_id: i64 = conn
        .query_row(
            "SELECT id FROM artist_credit WHERE display_name = 'Wallet Artist'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         explicit, episode_count, created_at, updated_at) \
         VALUES ('feed-normal', 'https://example.com/normal.xml', 'Normal', 'normal', ?1, 0, 0, ?2, ?2)",
        params![credit_id, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         explicit, created_at, updated_at) \
         VALUES ('track-normal', 'feed-normal', ?1, 'Normal Track', 'normal track', 0, ?2, ?2)",
        params![credit_id, now],
    )
    .unwrap();

    // Both feeds have a route pointing to the same address.
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-wl4', 'feed-wl4', 'SharedAddr', 'keysend', 'shared-node-xyz', 50, 0)",
        [],
    )
    .unwrap();
    let wl_route_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES ('track-normal', 'feed-normal', 'SharedAddr', 'keysend', 'shared-node-xyz', 50, 0)",
        [],
    )
    .unwrap();
    let normal_route_id = conn.last_insert_rowid();

    // Create one shared endpoint.
    conn.execute(
        "INSERT INTO wallet_endpoints (route_type, normalized_address, custom_key, custom_value, created_at) \
         VALUES ('keysend', 'shared-node-xyz', '', '', ?1)",
        params![now],
    )
    .unwrap();
    let ep_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO wallets (wallet_id, display_name, display_name_lower, wallet_class, class_confidence, created_at, updated_at) \
         VALUES ('shared-wallet', 'SharedAddr', 'sharedaddr', 'unknown', 'provisional', ?1, ?1)",
        params![now],
    )
    .unwrap();
    conn.execute(
        "UPDATE wallet_endpoints SET wallet_id = 'shared-wallet' WHERE id = ?1",
        params![ep_id],
    )
    .unwrap();

    // Both routes map to the same endpoint.
    conn.execute(
        "INSERT INTO wallet_track_route_map (route_id, endpoint_id, created_at) VALUES (?1, ?2, ?3)",
        params![wl_route_id, ep_id, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO wallet_track_route_map (route_id, endpoint_id, created_at) VALUES (?1, ?2, ?3)",
        params![normal_route_id, ep_id, now],
    )
    .unwrap();

    // Purge Wavlake route maps — the shared endpoint should survive
    // because the non-Wavlake route map still references it.
    let removed = db::purge_wavlake_wallet_route_maps(&conn).unwrap();
    assert_eq!(
        removed, 1,
        "only the Wavlake route map entry should be removed"
    );

    let cleanup = db::cleanup_orphaned_wallets(&conn).unwrap();
    assert_eq!(
        cleanup.wallets_deleted, 0,
        "wallet should survive — endpoint still has a route map"
    );
    assert_eq!(endpoint_count(&conn), 1, "endpoint should survive");
}
