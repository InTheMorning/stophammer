// Issue-ARTIST-IDENTITY — 2026-03-14
//
// Tests for feed-scoped artist identity resolution.
// Two feeds with the same `owner_name` must get distinct artist and artist
// credit records. Re-ingesting the same feed must reuse the existing records.

mod common;

// ---------------------------------------------------------------------------
// 1. Two feeds with same `owner_name` get distinct `artist_ids`
// ---------------------------------------------------------------------------

/// Two feeds with the same name on different `feed_guids` get distinct artists.
#[test]
fn different_feeds_same_name_get_distinct_artists() {
    let conn = common::test_db();

    let artist_a = stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-aaa"))
        .expect("resolve artist for feed-aaa");
    let artist_b = stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-bbb"))
        .expect("resolve artist for feed-bbb");

    assert_ne!(
        artist_a.artist_id, artist_b.artist_id,
        "same name on different feeds must produce distinct artist_ids"
    );
    assert_eq!(artist_a.name, "John Smith");
    assert_eq!(artist_b.name, "John Smith");
}

// ---------------------------------------------------------------------------
// 2. Same feed re-ingested reuses artist
// ---------------------------------------------------------------------------

/// Resolving the same name within the same feed twice must return the same
/// artist record (idempotent).
#[test]
fn same_feed_reingested_reuses_artist() {
    let conn = common::test_db();

    let first = stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-ccc"))
        .expect("first resolve");
    let second = stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-ccc"))
        .expect("second resolve");

    assert_eq!(
        first.artist_id, second.artist_id,
        "re-resolving same name on same feed must return same artist"
    );
}

// ---------------------------------------------------------------------------
// 3. Same artist in two tracks of same feed reuses artist
// ---------------------------------------------------------------------------

/// When two tracks of the same feed both have the same author name, they must
/// resolve to the same artist record (via the feed-scoped alias).
#[test]
fn same_artist_two_tracks_same_feed() {
    let conn = common::test_db();

    let track1_artist = stophammer::db::resolve_artist(&conn, "Jane Doe", Some("feed-ddd"))
        .expect("track 1 artist");
    let track2_artist = stophammer::db::resolve_artist(&conn, "Jane Doe", Some("feed-ddd"))
        .expect("track 2 artist");

    assert_eq!(
        track1_artist.artist_id, track2_artist.artist_id,
        "same author within same feed must resolve to same artist"
    );
}

// ---------------------------------------------------------------------------
// 4. Artist credits are feed-scoped
// ---------------------------------------------------------------------------

/// Same `display_name` on different feeds gets separate credit records.
#[test]
fn artist_credits_are_feed_scoped() {
    let conn = common::test_db();

    let artist_a = stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-eee"))
        .expect("artist a");
    let artist_b = stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-fff"))
        .expect("artist b");

    let credit_a = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_a.name,
        &[(artist_a.artist_id.clone(), artist_a.name.clone(), String::new())],
        Some("feed-eee"),
    )
    .expect("credit a");

    let credit_b = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_b.name,
        &[(artist_b.artist_id.clone(), artist_b.name.clone(), String::new())],
        Some("feed-fff"),
    )
    .expect("credit b");

    assert_ne!(
        credit_a.id, credit_b.id,
        "same display_name on different feeds must produce distinct artist_credit_ids"
    );

    assert_eq!(credit_a.feed_guid.as_deref(), Some("feed-eee"));
    assert_eq!(credit_b.feed_guid.as_deref(), Some("feed-fff"));
}

// ---------------------------------------------------------------------------
// 5. Artist credit idempotent within same feed
// ---------------------------------------------------------------------------

/// Re-requesting the same credit for the same feed must return the existing one.
#[test]
fn artist_credit_idempotent_within_feed() {
    let conn = common::test_db();

    let artist = stophammer::db::resolve_artist(&conn, "Alice", Some("feed-ggg"))
        .expect("resolve alice");

    let credit1 = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist.name,
        &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
        Some("feed-ggg"),
    )
    .expect("credit 1");

    let credit2 = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist.name,
        &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
        Some("feed-ggg"),
    )
    .expect("credit 2");

    assert_eq!(
        credit1.id, credit2.id,
        "same feed + same display_name must return the same credit"
    );
}

// ---------------------------------------------------------------------------
// 6. Case-insensitive scoping
// ---------------------------------------------------------------------------

/// Artist resolution is case-insensitive within a feed scope.
#[test]
fn case_insensitive_within_feed() {
    let conn = common::test_db();

    let lower = stophammer::db::resolve_artist(&conn, "bob jones", Some("feed-hhh"))
        .expect("resolve lower");
    let upper = stophammer::db::resolve_artist(&conn, "Bob Jones", Some("feed-hhh"))
        .expect("resolve upper");

    assert_eq!(
        lower.artist_id, upper.artist_id,
        "case-insensitive match within same feed must return same artist"
    );
}

// ---------------------------------------------------------------------------
// 7. Unscoped fallback (no `feed_guid`) still works
// ---------------------------------------------------------------------------

/// Unscoped `resolve_artist` (backward compat) remains idempotent.
#[test]
fn unscoped_fallback_works() {
    let conn = common::test_db();

    let artist = stophammer::db::resolve_artist(&conn, "Global Artist", None)
        .expect("unscoped resolve");
    let again = stophammer::db::resolve_artist(&conn, "Global Artist", None)
        .expect("unscoped resolve again");

    assert_eq!(
        artist.artist_id, again.artist_id,
        "unscoped resolve must be idempotent"
    );
}

// ---------------------------------------------------------------------------
// 8. Full `ingest_transaction` with feed-scoped credits
// ---------------------------------------------------------------------------

/// Two `ingest_transaction` calls with same name but different `feed_guids`.
#[test]
#[expect(clippy::too_many_lines, reason = "integration test constructing full model structs")]
fn ingest_transaction_feeds_get_distinct_artists() {
    let mut conn = common::test_db();
    let now = common::now();

    let signer = stophammer::signing::NodeSigner::load_or_create(
        "/tmp/artist-identity-test.key",
    )
    .expect("signer");

    // Feed A: owner = "John Smith"
    let artist_a = stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-x1"))
        .expect("resolve a");
    let credit_a = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_a.name,
        &[(artist_a.artist_id.clone(), artist_a.name.clone(), String::new())],
        Some("feed-x1"),
    )
    .expect("credit a");

    let feed_a = stophammer::model::Feed {
        feed_guid:        "feed-x1".into(),
        feed_url:         "https://a.example.com/feed.xml".into(),
        title:            "Feed A".into(),
        title_lower:      "feed a".into(),
        artist_credit_id: credit_a.id,
        description:      None,
        image_url:        None,
        language:         None,
        explicit:         false,
        itunes_type:      None,
        episode_count:    0,
        newest_item_at:   None,
        oldest_item_at:   None,
        created_at:       now,
        updated_at:       now,
        raw_medium:       None,
    };

    let result_a = stophammer::db::ingest_transaction(
        &mut conn,
        artist_a.clone(),
        credit_a.clone(),
        feed_a,
        vec![],
        vec![],
        vec![],
        &signer,
    );
    assert!(result_a.is_ok(), "ingest feed A should succeed");

    // Feed B: also owner = "John Smith" but different feed_guid
    let artist_b = stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-x2"))
        .expect("resolve b");
    let credit_b = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_b.name,
        &[(artist_b.artist_id.clone(), artist_b.name.clone(), String::new())],
        Some("feed-x2"),
    )
    .expect("credit b");

    let feed_b = stophammer::model::Feed {
        feed_guid:        "feed-x2".into(),
        feed_url:         "https://b.example.com/feed.xml".into(),
        title:            "Feed B".into(),
        title_lower:      "feed b".into(),
        artist_credit_id: credit_b.id,
        description:      None,
        image_url:        None,
        language:         None,
        explicit:         false,
        itunes_type:      None,
        episode_count:    0,
        newest_item_at:   None,
        oldest_item_at:   None,
        created_at:       now,
        updated_at:       now,
        raw_medium:       None,
    };

    let result_b = stophammer::db::ingest_transaction(
        &mut conn,
        artist_b.clone(),
        credit_b.clone(),
        feed_b,
        vec![],
        vec![],
        vec![],
        &signer,
    );
    assert!(result_b.is_ok(), "ingest feed B should succeed");

    // Verify distinct artist IDs
    assert_ne!(
        artist_a.artist_id, artist_b.artist_id,
        "feed A and feed B must have distinct artist_ids even with same name"
    );

    // Verify distinct credit IDs
    assert_ne!(
        credit_a.id, credit_b.id,
        "feed A and feed B must have distinct credit_ids even with same display_name"
    );

    // Verify both feeds exist in DB with their respective credit IDs
    let stored_a: i64 = conn
        .query_row(
            "SELECT artist_credit_id FROM feeds WHERE feed_guid = 'feed-x1'",
            [],
            |r| r.get(0),
        )
        .expect("feed A should exist");
    assert_eq!(stored_a, credit_a.id);

    let stored_b: i64 = conn
        .query_row(
            "SELECT artist_credit_id FROM feeds WHERE feed_guid = 'feed-x2'",
            [],
            |r| r.get(0),
        )
        .expect("feed B should exist");
    assert_eq!(stored_b, credit_b.id);
}
