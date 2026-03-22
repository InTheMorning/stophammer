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

// ---------------------------------------------------------------------------
// Owner creation (Pass 2)
// ---------------------------------------------------------------------------

#[test]
fn provisional_wallet_per_endpoint() {
    let conn = common::test_db();
    let now = common::now();

    let ep1 = db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
    let ep2 = db::get_or_create_endpoint(&conn, "lnaddress", "bob@example.com", "", "", Some("Bob"), now).unwrap();

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

    let ep = db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), 1000).unwrap();
    db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Bob"), 2000).unwrap();

    let wid = db::create_provisional_wallet(&conn, ep, 3000).unwrap();
    let name: String = conn
        .query_row("SELECT display_name FROM wallets WHERE wallet_id = ?1", params![wid], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "Alice", "display_name should be the earliest-seen alias");
}

#[test]
fn endpoint_without_alias_gets_placeholder_name() {
    let conn = common::test_db();
    let now = common::now();

    let ep = db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", None, now).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();

    let name: String = conn
        .query_row("SELECT display_name FROM wallets WHERE wallet_id = ?1", params![wid], |r| r.get(0))
        .unwrap();
    assert!(name.starts_with("endpoint-"), "should use placeholder: {name}");
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

    let ep = db::get_or_create_endpoint(&conn, "keysend", "feenode123", "", "", Some("App Fee"), now).unwrap();
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

    let ep = db::get_or_create_endpoint(&conn, "keysend", "overnode", "", "", Some("Overridden"), now).unwrap();
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

    let ep = db::get_or_create_endpoint(&conn, "keysend", "fountain123", "", "", Some("Fountain"), now).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();
    db::classify_wallet_hard_signals(&conn, &wid).unwrap();

    let (class, confidence): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(class, "unknown", "name alone should not drive classification");
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
    let ep = db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Wallet Artist"), now).unwrap();
    let wid = db::create_provisional_wallet(&conn, ep, now).unwrap();

    let linked = db::link_wallet_to_artist_if_confident(&conn, &wid, "feed-w").unwrap();
    assert!(linked, "should create link when alias matches feed artist credit");

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

    let ep = db::get_or_create_endpoint(&conn, "keysend", "feebot", "", "", Some("Wallet Artist"), now).unwrap();
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

    let ep1 = db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
    let ep2 = db::get_or_create_endpoint(&conn, "keysend", "def456", "", "", Some("Alice Alt"), now).unwrap();

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

    let ep = db::get_or_create_endpoint(&conn, "keysend", "abc123", "", "", Some("Alice"), now).unwrap();
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
    let ep1 = db::get_or_create_endpoint(&conn, "keysend", "abc", "", "", Some("Bob"), 2000).unwrap();
    let ep2 = db::get_or_create_endpoint(&conn, "keysend", "def", "", "", Some("Alice"), 1000).unwrap();

    let w1 = db::create_provisional_wallet(&conn, ep1, 3000).unwrap();
    let w2 = db::create_provisional_wallet(&conn, ep2, 3000).unwrap();

    // Before merge, w1 display_name is "Bob"
    let name: String = conn
        .query_row("SELECT display_name FROM wallets WHERE wallet_id = ?1", params![w1], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "Bob");

    // Merge w2 into w1 — "Alice" is earlier, so display_name should become "Alice"
    db::merge_wallets(&conn, &w2, &w1).unwrap();

    let name: String = conn
        .query_row("SELECT display_name FROM wallets WHERE wallet_id = ?1", params![w1], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "Alice", "display name should be re-derived from first-seen alias");
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
    ).unwrap();
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES ('BF Artist', ?1)",
        params![now],
    ).unwrap();
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, 'art-bf', 0, 'BF Artist', '')",
        params![credit_id],
    ).unwrap();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         explicit, episode_count, created_at, updated_at) \
         VALUES ('feed-bf', 'https://example.com/bf.xml', 'BF Feed', 'bf feed', ?1, 0, 0, ?2, ?3)",
        params![credit_id, now, now],
    ).unwrap();
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         explicit, created_at, updated_at) \
         VALUES ('track-bf', 'feed-bf', ?1, 'BF Track', 'bf track', 0, ?2, ?3)",
        params![credit_id, now, now],
    ).unwrap();
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
    assert!(s1.endpoints_created >= 2, "should create at least 2 endpoints");

    let s2 = db::backfill_wallet_pass2(&conn).unwrap();
    assert!(s2.wallets_created >= 2);

    let s3 = db::backfill_wallet_pass3(&conn).unwrap();

    // Capture counts
    let ep_count: i64 = conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0)).unwrap();
    let wallet_count: i64 = conn.query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0)).unwrap();
    let link_count: i64 = conn.query_row("SELECT COUNT(*) FROM wallet_artist_links", [], |r| r.get(0)).unwrap();

    // Run again — should be idempotent
    let s1b = db::backfill_wallet_pass1(&conn).unwrap();
    assert_eq!(s1b.endpoints_created, 0, "second run should create no new endpoints");

    let s2b = db::backfill_wallet_pass2(&conn).unwrap();
    assert_eq!(s2b.wallets_created, 0, "second run should create no new wallets");

    db::backfill_wallet_pass3(&conn).unwrap();

    let ep2: i64 = conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0)).unwrap();
    let w2: i64 = conn.query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0)).unwrap();
    let l2: i64 = conn.query_row("SELECT COUNT(*) FROM wallet_artist_links", [], |r| r.get(0)).unwrap();

    assert_eq!(ep_count, ep2, "endpoint count should not change");
    assert_eq!(wallet_count, w2, "wallet count should not change");
    assert_eq!(link_count, l2, "artist link count should not change");
}

#[test]
fn fee_true_wallet_gets_no_artist_link_in_backfill() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

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
    assert_eq!(link_count, 0, "fee=true wallet should get no artist link even if name matches");
}

#[test]
fn per_feed_resolver_creates_endpoints_and_wallets() {
    let conn = common::test_db();
    let now = seed_feed_and_track(&conn);

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

    assert_eq!(stats.endpoints_created, 2, "should create 2 distinct endpoints");
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
    assert!(
        queue::DEFAULT_DIRTY_MASK & queue::DIRTY_WALLET_IDENTITY != 0,
        "DIRTY_WALLET_IDENTITY must be in DEFAULT_DIRTY_MASK"
    );
}

#[test]
fn wallet_dirty_bit_after_promotions_before_search() {
    use stophammer::resolver::queue;
    // Wallet identity bit should be distinct from all other bits
    assert_eq!(queue::DIRTY_WALLET_IDENTITY, 1 << 5);
    assert_ne!(queue::DIRTY_WALLET_IDENTITY, queue::DIRTY_CANONICAL_PROMOTIONS);
    assert_ne!(queue::DIRTY_WALLET_IDENTITY, queue::DIRTY_CANONICAL_SEARCH);
}
