// Issue-ARTIST-IDENTITY — 2026-03-14
//
// Tests for feed-scoped artist identity resolution.
// Two feeds with the same `owner_name` must get distinct artist and artist
// credit records. Re-ingesting the same feed must reuse the existing records.

#![allow(
    clippy::similar_names,
    reason = "artist identity tests intentionally compare near-identical fixture names and credits"
)]

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

    let artist_a =
        stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-eee")).expect("artist a");
    let artist_b =
        stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-fff")).expect("artist b");

    let credit_a = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_a.name,
        &[(
            artist_a.artist_id.clone(),
            artist_a.name.clone(),
            String::new(),
        )],
        Some("feed-eee"),
    )
    .expect("credit a");

    let credit_b = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_b.name,
        &[(
            artist_b.artist_id.clone(),
            artist_b.name.clone(),
            String::new(),
        )],
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

    let artist =
        stophammer::db::resolve_artist(&conn, "Alice", Some("feed-ggg")).expect("resolve alice");

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

    let artist =
        stophammer::db::resolve_artist(&conn, "Global Artist", None).expect("unscoped resolve");
    let again = stophammer::db::resolve_artist(&conn, "Global Artist", None)
        .expect("unscoped resolve again");

    assert_eq!(
        artist.artist_id, again.artist_id,
        "unscoped resolve must be idempotent"
    );
}

#[test]
fn feed_artist_resolution_reuses_existing_artist_by_npub() {
    let conn = common::test_db();
    let now = common::now();

    let existing = stophammer::db::resolve_artist(&conn, "Signal Artist", Some("feed-existing"))
        .expect("existing artist");
    conn.execute(
        "INSERT INTO external_ids (entity_type, entity_id, scheme, value, created_at) \
         VALUES ('artist', ?1, 'nostr_npub', 'npub1signalartist', ?2)",
        rusqlite::params![existing.artist_id, now],
    )
    .expect("insert external id");

    let source_entity_ids = vec![stophammer::model::SourceEntityIdClaim {
        id: None,
        feed_guid: "feed-new".into(),
        entity_type: "feed".into(),
        entity_id: "feed-new".into(),
        position: 0,
        scheme: "nostr_npub".into(),
        value: "npub1signalartist".into(),
        source: "podcast_txt".into(),
        extraction_path: "feed.podcast:txt[@purpose='npub']".into(),
        observed_at: now,
    }];

    let resolved = stophammer::db::resolve_feed_artist_from_source_claims(
        &conn,
        "Signal Artist",
        "feed-new",
        &source_entity_ids,
        &[],
    )
    .expect("resolve via npub");

    assert_eq!(resolved.artist_id, existing.artist_id);
}

#[test]
fn feed_artist_resolution_does_not_reuse_artist_by_publisher_guid() {
    // publisher_guid is no longer treated as artist identity evidence.
    // A feed sharing a publisher_guid with an existing feed must NOT reuse
    // the existing artist — it should create a new, independent artist row.
    let conn = common::test_db();
    let now = common::now();

    let existing = stophammer::db::resolve_artist(&conn, "Publisher Artist", Some("feed-existing"))
        .expect("existing artist");
    let credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &existing.name,
        &[(
            existing.artist_id.clone(),
            existing.name.clone(),
            String::new(),
        )],
        Some("feed-existing"),
    )
    .expect("artist credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        rusqlite::params![
            "feed-existing",
            "https://example.com/feed-existing.xml",
            "Publisher Artist Release",
            "publisher artist release",
            credit.id,
            now,
        ],
    )
    .expect("insert feed");
    conn.execute(
        "INSERT INTO feed_remote_items_raw \
         (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
         VALUES (?1, 0, 'publisher', ?2, ?3, 'podcast_remote_item')",
        rusqlite::params![
            "feed-existing",
            "publisher-guid-1",
            "https://wavlake.com/publisher-artist",
        ],
    )
    .expect("insert remote item");

    // Resolve from a new feed that shares the publisher_guid but provides no
    // explicit npub or website identity evidence.
    let resolved = stophammer::db::resolve_feed_artist_from_source_claims(
        &conn,
        "Publisher Artist",
        "feed-new",
        &[],
        &[],
    )
    .expect("resolve without publisher guid identity");

    // Must NOT reuse the existing artist — publisher_guid is not identity
    // evidence, so a fresh artist row should be created.
    assert_ne!(resolved.artist_id, existing.artist_id);
}

#[test]
fn feed_artist_resolution_reuses_existing_artist_by_website_url() {
    let conn = common::test_db();
    let now = common::now();

    let existing = stophammer::db::resolve_artist(&conn, "Website Artist", Some("feed-existing"))
        .expect("existing artist");
    let credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &existing.name,
        &[(
            existing.artist_id.clone(),
            existing.name.clone(),
            String::new(),
        )],
        Some("feed-existing"),
    )
    .expect("artist credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        rusqlite::params![
            "feed-existing",
            "https://example.com/feed-existing.xml",
            "Website Artist Release",
            "website artist release",
            credit.id,
            now,
        ],
    )
    .expect("insert feed");
    conn.execute(
        "INSERT INTO source_entity_links \
         (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
         VALUES (?1, 'feed', ?1, 0, 'website', ?2, 'rss_link', 'feed.link', ?3)",
        rusqlite::params!["feed-existing", "https://wavlake.com/website-artist", now],
    )
    .expect("insert website link");

    let source_entity_links = vec![stophammer::model::SourceEntityLink {
        id: None,
        feed_guid: "feed-new".into(),
        entity_type: "feed".into(),
        entity_id: "feed-new".into(),
        position: 0,
        link_type: "website".into(),
        url: "https://wavlake.com/website-artist".into(),
        source: "rss_link".into(),
        extraction_path: "feed.link".into(),
        observed_at: now,
    }];

    let resolved = stophammer::db::resolve_feed_artist_from_source_claims(
        &conn,
        "Website Artist",
        "feed-new",
        &[],
        &source_entity_links,
    )
    .expect("resolve via website url");

    assert_eq!(resolved.artist_id, existing.artist_id);
}

#[test]
fn feed_artist_resolution_prefers_canonical_artist_when_source_claim_is_split() {
    let conn = common::test_db();
    let now = common::now();

    let artist_a =
        stophammer::db::resolve_artist(&conn, "Split Artist", Some("feed-a1")).expect("artist a");
    let credit_a1 = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_a.name,
        &[(
            artist_a.artist_id.clone(),
            artist_a.name.clone(),
            String::new(),
        )],
        Some("feed-a1"),
    )
    .expect("credit a1");
    let credit_a2 = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_a.name,
        &[(
            artist_a.artist_id.clone(),
            artist_a.name.clone(),
            String::new(),
        )],
        Some("feed-a2"),
    )
    .expect("credit a2");
    let artist_b =
        stophammer::db::resolve_artist(&conn, "Split Artist", Some("feed-b1")).expect("artist b");
    let credit_b1 = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_b.name,
        &[(
            artist_b.artist_id.clone(),
            artist_b.name.clone(),
            String::new(),
        )],
        Some("feed-b1"),
    )
    .expect("credit b1");

    for (feed_guid, credit_id) in [
        ("feed-a1", credit_a1.id),
        ("feed-a2", credit_a2.id),
        ("feed-b1", credit_b1.id),
    ] {
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![
                feed_guid,
                format!("https://example.com/{feed_guid}.xml"),
                format!("Release {feed_guid}"),
                format!("release {feed_guid}"),
                credit_id,
                now,
            ],
        )
        .expect("insert feed");
        conn.execute(
            "INSERT INTO source_entity_links \
             (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
             VALUES (?1, 'feed', ?1, 0, 'website', ?2, 'rss_link', 'feed.link', ?3)",
            rusqlite::params![feed_guid, "https://wavlake.com/split-artist", now],
        )
        .expect("insert website link");
    }

    let source_entity_links = vec![stophammer::model::SourceEntityLink {
        id: None,
        feed_guid: "feed-new".into(),
        entity_type: "feed".into(),
        entity_id: "feed-new".into(),
        position: 0,
        link_type: "website".into(),
        url: "https://wavlake.com/split-artist".into(),
        source: "rss_link".into(),
        extraction_path: "feed.link".into(),
        observed_at: now,
    }];

    let resolved = stophammer::db::resolve_feed_artist_from_source_claims(
        &conn,
        "Split Artist",
        "feed-new",
        &[],
        &source_entity_links,
    )
    .expect("resolve split website url");

    assert_eq!(resolved.artist_id, artist_a.artist_id);
}

// ---------------------------------------------------------------------------
// 8. Full `ingest_transaction` with feed-scoped credits
// ---------------------------------------------------------------------------

/// Two `ingest_transaction` calls with same name but different `feed_guids`.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "integration test constructing full model structs"
)]
fn ingest_transaction_feeds_get_distinct_artists() {
    let mut conn = common::test_db();
    let now = common::now();

    let signer = common::temp_signer("artist-identity-test");

    // Feed A: owner = "John Smith"
    let artist_a =
        stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-x1")).expect("resolve a");
    let credit_a = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_a.name,
        &[(
            artist_a.artist_id.clone(),
            artist_a.name.clone(),
            String::new(),
        )],
        Some("feed-x1"),
    )
    .expect("credit a");

    let feed_a = stophammer::model::Feed {
        feed_guid: "feed-x1".into(),
        feed_url: "https://a.example.com/feed.xml".into(),
        title: "Feed A".into(),
        title_lower: "feed a".into(),
        artist_credit_id: credit_a.id,
        description: None,
        image_url: None,
        language: None,
        explicit: false,
        itunes_type: None,
        episode_count: 0,
        newest_item_at: None,
        oldest_item_at: None,
        created_at: now,
        updated_at: now,
        raw_medium: None,
    };

    let result_a = stophammer::db::ingest_transaction(
        &mut conn,
        artist_a.clone(),
        credit_a.clone(),
        feed_a,
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        &signer,
    );
    assert!(result_a.is_ok(), "ingest feed A should succeed");

    // Feed B: also owner = "John Smith" but different feed_guid
    let artist_b =
        stophammer::db::resolve_artist(&conn, "John Smith", Some("feed-x2")).expect("resolve b");
    let credit_b = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_b.name,
        &[(
            artist_b.artist_id.clone(),
            artist_b.name.clone(),
            String::new(),
        )],
        Some("feed-x2"),
    )
    .expect("credit b");

    let feed_b = stophammer::model::Feed {
        feed_guid: "feed-x2".into(),
        feed_url: "https://b.example.com/feed.xml".into(),
        title: "Feed B".into(),
        title_lower: "feed b".into(),
        artist_credit_id: credit_b.id,
        description: None,
        image_url: None,
        language: None,
        explicit: false,
        itunes_type: None,
        episode_count: 0,
        newest_item_at: None,
        oldest_item_at: None,
        created_at: now,
        updated_at: now,
        raw_medium: None,
    };

    let result_b = stophammer::db::ingest_transaction(
        &mut conn,
        artist_b.clone(),
        credit_b.clone(),
        feed_b,
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
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

#[test]
fn artist_identity_backfill_merges_split_artists_by_website_and_repoints_external_ids() {
    let mut conn = common::test_db();
    let now = common::now();

    let artist_a =
        stophammer::db::resolve_artist(&conn, "Unified Artist", Some("feed-a")).expect("artist a");
    let credit_a = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_a.name,
        &[(
            artist_a.artist_id.clone(),
            artist_a.name.clone(),
            String::new(),
        )],
        Some("feed-a"),
    )
    .expect("credit a");
    let artist_b =
        stophammer::db::resolve_artist(&conn, "Unified Artist", Some("feed-b")).expect("artist b");
    let credit_b = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_b.name,
        &[(
            artist_b.artist_id.clone(),
            artist_b.name.clone(),
            String::new(),
        )],
        Some("feed-b"),
    )
    .expect("credit b");

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-a', 'https://wavlake.com/feed/music/a', 'A', 'a', ?1, ?2, ?2)",
        rusqlite::params![credit_a.id, now],
    )
    .expect("feed a");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-b', 'https://feeds.fountain.fm/b', 'B', 'b', ?1, ?2, ?2)",
        rusqlite::params![credit_b.id, now],
    )
    .expect("feed b");
    for feed_guid in ["feed-a", "feed-b"] {
        conn.execute(
            "INSERT INTO source_entity_links \
             (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
             VALUES (?1, 'feed', ?1, 0, 'website', 'https://wavlake.com/unified-artist', 'rss_link', 'feed.link', ?2)",
            rusqlite::params![feed_guid, now],
        )
        .expect("website link");
    }
    conn.execute(
        "INSERT INTO external_ids (entity_type, entity_id, scheme, value, created_at) \
         VALUES ('artist', ?1, 'musicbrainz_artist', 'mbid-unified', ?2)",
        rusqlite::params![artist_b.artist_id, now],
    )
    .expect("external id");

    let stats = stophammer::db::backfill_artist_identity(&mut conn).expect("artist backfill");
    assert_eq!(stats.merges_applied, 1);

    let artist_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'unified artist'",
            [],
            |row| row.get(0),
        )
        .expect("artist count");
    assert_eq!(artist_count, 1);

    let external_owner: String = conn
        .query_row(
            "SELECT entity_id FROM external_ids \
             WHERE entity_type = 'artist' AND scheme = 'musicbrainz_artist' AND value = 'mbid-unified'",
            [],
            |row| row.get(0),
        )
        .expect("external owner");
    let surviving_name: String = conn
        .query_row(
            "SELECT name FROM artists WHERE artist_id = ?1",
            rusqlite::params![external_owner],
            |row| row.get(0),
        )
        .expect("surviving artist name");
    assert_eq!(surviving_name, "Unified Artist");
}

#[test]
fn targeted_artist_identity_resolver_merges_split_artists_by_website() {
    let mut conn = common::test_db();
    let now = common::now();

    let artist_a = stophammer::db::resolve_artist(&conn, "Focused Artist", Some("feed-focus-a"))
        .expect("artist a");
    let credit_a = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_a.name,
        &[(
            artist_a.artist_id.clone(),
            artist_a.name.clone(),
            String::new(),
        )],
        Some("feed-focus-a"),
    )
    .expect("credit a");
    let artist_b = stophammer::db::resolve_artist(&conn, "Focused Artist", Some("feed-focus-b"))
        .expect("artist b");
    let credit_b = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_b.name,
        &[(
            artist_b.artist_id.clone(),
            artist_b.name.clone(),
            String::new(),
        )],
        Some("feed-focus-b"),
    )
    .expect("credit b");

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-focus-a', 'https://wavlake.com/feed/music/focus-a', 'A', 'a', ?1, ?2, ?2)",
        rusqlite::params![credit_a.id, now],
    )
    .expect("feed a");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-focus-b', 'https://feeds.fountain.fm/focus-b', 'B', 'b', ?1, ?2, ?2)",
        rusqlite::params![credit_b.id, now],
    )
    .expect("feed b");
    for feed_guid in ["feed-focus-a", "feed-focus-b"] {
        conn.execute(
            "INSERT INTO source_entity_links \
             (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
             VALUES (?1, 'feed', ?1, 0, 'website', 'https://wavlake.com/focused-artist', 'rss_link', 'feed.link', ?2)",
            rusqlite::params![feed_guid, now],
        )
        .expect("website link");
    }

    let stats = stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-focus-b")
        .expect("targeted feed identity");
    assert_eq!(stats.seed_artists, 1);
    assert_eq!(stats.candidate_groups, 1);
    assert_eq!(stats.groups_processed, 1);
    assert_eq!(stats.merges_applied, 1);

    let artist_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'focused artist'",
            [],
            |row| row.get(0),
        )
        .expect("artist count");
    assert_eq!(artist_count, 1);
}

#[test]
fn explain_artist_identity_for_feed_reports_seed_artists_and_candidate_groups() {
    let conn = common::test_db();
    let now = common::now();

    let artist_a =
        stophammer::db::resolve_artist(&conn, "Explained Artist", Some("feed-explain-a"))
            .expect("artist a");
    let credit_a = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_a.name,
        &[(
            artist_a.artist_id.clone(),
            artist_a.name.clone(),
            String::new(),
        )],
        Some("feed-explain-a"),
    )
    .expect("credit a");
    let artist_b =
        stophammer::db::resolve_artist(&conn, "Explained Artist", Some("feed-explain-b"))
            .expect("artist b");
    let credit_b = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_b.name,
        &[(
            artist_b.artist_id.clone(),
            artist_b.name.clone(),
            String::new(),
        )],
        Some("feed-explain-b"),
    )
    .expect("credit b");

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-explain-a', 'https://wavlake.com/feed/music/explain-a', 'Explain A', 'explain a', ?1, ?2, ?2)",
        rusqlite::params![credit_a.id, now],
    )
    .expect("feed a");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-explain-b', 'https://feeds.fountain.fm/explain-b', 'Explain B', 'explain b', ?1, ?2, ?2)",
        rusqlite::params![credit_b.id, now],
    )
    .expect("feed b");
    for feed_guid in ["feed-explain-a", "feed-explain-b"] {
        conn.execute(
            "INSERT INTO source_entity_links \
             (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
             VALUES (?1, 'feed', ?1, 0, 'website', 'https://wavlake.com/explained-artist', 'rss_link', 'feed.link', ?2)",
            rusqlite::params![feed_guid, now],
        )
        .expect("website link");
    }

    let plan = stophammer::db::explain_artist_identity_for_feed(&conn, "feed-explain-b")
        .expect("feed plan");
    assert_eq!(plan.feed_guid, "feed-explain-b");
    assert_eq!(plan.seed_artists.len(), 1);
    assert_eq!(plan.seed_artists[0].name, "Explained Artist");
    assert_eq!(plan.candidate_groups.len(), 1);
    assert!(
        plan.candidate_groups
            .iter()
            .any(|group| group.source == "normalized_website")
    );
}

#[test]
fn pending_artist_identity_feed_report_lists_unresolved_feeds() {
    let conn = common::test_db();
    let now = common::now();

    let artist_a = stophammer::db::resolve_artist(&conn, "Pending Artist", Some("feed-pending-a"))
        .expect("artist a");
    let credit_a = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_a.name,
        &[(
            artist_a.artist_id.clone(),
            artist_a.name.clone(),
            String::new(),
        )],
        Some("feed-pending-a"),
    )
    .expect("credit a");
    let artist_b = stophammer::db::resolve_artist(&conn, "Pending Artist", Some("feed-pending-b"))
        .expect("artist b");
    let credit_b = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_b.name,
        &[(
            artist_b.artist_id.clone(),
            artist_b.name.clone(),
            String::new(),
        )],
        Some("feed-pending-b"),
    )
    .expect("credit b");
    let stable_artist = stophammer::db::resolve_artist(&conn, "Stable Artist", Some("feed-stable"))
        .expect("stable artist");
    let stable_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &stable_artist.name,
        &[(
            stable_artist.artist_id.clone(),
            stable_artist.name.clone(),
            String::new(),
        )],
        Some("feed-stable"),
    )
    .expect("stable credit");

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-pending-a', 'https://wavlake.com/feed/music/pending-a', 'Pending A', 'pending a', ?1, ?2, ?2)",
        rusqlite::params![credit_a.id, now],
    )
    .expect("feed pending a");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-pending-b', 'https://feeds.fountain.fm/pending-b', 'Pending B', 'pending b', ?1, ?2, ?2)",
        rusqlite::params![credit_b.id, now],
    )
    .expect("feed pending b");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-stable', 'https://example.com/stable.xml', 'Stable', 'stable', ?1, ?2, ?2)",
        rusqlite::params![stable_credit.id, now],
    )
    .expect("feed stable");

    for feed_guid in ["feed-pending-a", "feed-pending-b"] {
        conn.execute(
            "INSERT INTO source_entity_links \
             (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
             VALUES (?1, 'feed', ?1, 0, 'website', 'https://wavlake.com/pending-artist', 'rss_link', 'feed.link', ?2)",
            rusqlite::params![feed_guid, now],
        )
        .expect("pending website link");
    }

    let pending =
        stophammer::db::list_pending_artist_identity_feeds(&conn, 10).expect("pending feeds");
    let guids = pending
        .iter()
        .map(|feed| feed.feed_guid.as_str())
        .collect::<Vec<_>>();
    assert_eq!(guids, vec!["feed-pending-a", "feed-pending-b"]);
    assert!(pending.iter().all(|feed| feed.candidate_groups == 1));
}

#[test]
fn artist_identity_backfill_merges_split_artists_connected_by_release_cluster() {
    let mut conn = common::test_db();
    let now = common::now();

    for (feed_guid, feed_url) in [
        ("feed-cluster-a", "https://feeds.fountain.fm/cluster-a"),
        ("feed-cluster-b", "https://feeds.rssblue.com/cluster-b"),
    ] {
        let artist = stophammer::db::resolve_artist(&conn, "Cluster Artist", Some(feed_guid))
            .expect("artist");
        let credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &artist.name,
            &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
            Some(feed_guid),
        )
        .expect("credit");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, episode_count, created_at, updated_at) \
             VALUES (?1, ?2, 'Cluster Single', 'cluster single', ?3, 1, ?4, ?4)",
            rusqlite::params![feed_guid, feed_url, credit.id, now],
        )
        .expect("insert feed");
        conn.execute(
            "INSERT INTO tracks \
             (track_guid, feed_guid, artist_credit_id, title, title_lower, duration_secs, track_number, created_at, updated_at) \
             VALUES (?1, ?2, ?3, 'Cluster Single', 'cluster single', 200, 1, ?4, ?4)",
            rusqlite::params![format!("track-{feed_guid}"), feed_guid, credit.id, now],
        )
        .expect("insert track");
        stophammer::db::sync_canonical_state_for_feed(&conn, feed_guid).expect("sync canonical");
    }

    let pre_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'cluster artist'",
            [],
            |row| row.get(0),
        )
        .expect("pre artist count");
    assert_eq!(pre_count, 2);

    let stats = stophammer::db::backfill_artist_identity(&mut conn).expect("artist backfill");
    assert_eq!(stats.merges_applied, 1);

    let post_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'cluster artist'",
            [],
            |row| row.get(0),
        )
        .expect("post artist count");
    assert_eq!(post_count, 1);
}

#[test]
fn artist_identity_backfill_merges_single_feed_platform_stragglers_into_anchored_artist() {
    let mut conn = common::test_db();
    let now = common::now();

    let anchored_artist =
        stophammer::db::resolve_artist(&conn, "Anchored Artist", Some("feed-wavlake-a"))
            .expect("anchored artist");
    let anchored_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &anchored_artist.name,
        &[(
            anchored_artist.artist_id.clone(),
            anchored_artist.name.clone(),
            String::new(),
        )],
        Some("feed-wavlake-a"),
    )
    .expect("anchored credit");

    for feed_guid in ["feed-wavlake-a", "feed-wavlake-b"] {
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES (?1, ?2, 'Anchored Release', 'anchored release', ?3, ?4, ?4)",
            rusqlite::params![
                feed_guid,
                format!("https://wavlake.com/feed/music/{feed_guid}"),
                anchored_credit.id,
                now,
            ],
        )
        .expect("insert anchored feed");
        conn.execute(
            "INSERT INTO source_entity_links \
             (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
             VALUES (?1, 'feed', ?1, 0, 'website', 'https://wavlake.com/anchored-artist', 'rss_link', 'feed.link', ?2)",
            rusqlite::params![feed_guid, now],
        )
        .expect("insert anchored website");
        conn.execute(
            "INSERT INTO source_platform_claims \
             (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
             VALUES (?1, 'wavlake', ?2, 'Wavlake', 'derived', 'feed.link', ?3)",
            rusqlite::params![
                feed_guid,
                format!("https://wavlake.com/feed/music/{feed_guid}"),
                now
            ],
        )
        .expect("insert anchored platform");
    }

    for feed_guid in ["feed-fountain-a", "feed-rssblue-b"] {
        let split_artist =
            stophammer::db::resolve_artist(&conn, "Anchored Artist", Some(feed_guid))
                .expect("split artist");
        let split_credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &split_artist.name,
            &[(
                split_artist.artist_id.clone(),
                split_artist.name.clone(),
                String::new(),
            )],
            Some(feed_guid),
        )
        .expect("split credit");
        let (feed_url, platform_key) = if feed_guid.contains("fountain") {
            ("https://feeds.fountain.fm/anchored-a", "fountain")
        } else {
            ("https://feeds.rssblue.com/anchored-b", "rss_blue")
        };
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES (?1, ?2, 'Straggler Release', 'straggler release', ?3, ?4, ?4)",
            rusqlite::params![feed_guid, feed_url, split_credit.id, now],
        )
        .expect("insert split feed");
        conn.execute(
            "INSERT INTO source_platform_claims \
             (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, NULL, 'derived', 'request.canonical_url', ?4)",
            rusqlite::params![feed_guid, platform_key, feed_url, now],
        )
        .expect("insert split platform");
    }

    let pre_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'anchored artist'",
            [],
            |row| row.get(0),
        )
        .expect("pre count");
    assert_eq!(pre_count, 3);

    let stats = stophammer::db::backfill_artist_identity(&mut conn).expect("artist backfill");
    assert!(stats.merges_applied >= 2);

    let post_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'anchored artist'",
            [],
            |row| row.get(0),
        )
        .expect("post count");
    assert_eq!(post_count, 1);
}

#[test]
fn artist_identity_backfill_merges_same_bandcamp_subdomain() {
    let mut conn = common::test_db();
    let now = common::now();

    for (feed_guid, website) in [
        (
            "feed-bandcamp-a",
            "https://johnson-city.bandcamp.com/album/crazy-cloud",
        ),
        (
            "feed-bandcamp-b",
            "https://johnson-city.bandcamp.com/album/early-demos-i",
        ),
    ] {
        let artist =
            stophammer::db::resolve_artist(&conn, "Johnson City", Some(feed_guid)).expect("artist");
        let credit = stophammer::db::get_or_create_artist_credit(
            &conn,
            &artist.name,
            &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
            Some(feed_guid),
        )
        .expect("credit");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES (?1, ?2, 'Bandcamp Release', 'bandcamp release', ?3, ?4, ?4)",
            rusqlite::params![feed_guid, website, credit.id, now],
        )
        .expect("insert feed");
        conn.execute(
            "INSERT INTO source_entity_links \
             (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
             VALUES (?1, 'feed', ?1, 0, 'website', ?2, 'rss_link', 'feed.link', ?3)",
            rusqlite::params![feed_guid, website, now],
        )
        .expect("insert website");
    }

    let pre_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'johnson city'",
            [],
            |row| row.get(0),
        )
        .expect("pre count");
    assert_eq!(pre_count, 2);

    let stats = stophammer::db::backfill_artist_identity(&mut conn).expect("artist backfill");
    assert_eq!(stats.merges_applied, 1);

    let post_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'johnson city'",
            [],
            |row| row.get(0),
        )
        .expect("post count");
    assert_eq!(post_count, 1);
}

#[test]
fn merge_artists_repoints_existing_redirect_chains() {
    let mut conn = common::test_db();

    let artist_a =
        stophammer::db::resolve_artist(&conn, "Chain Artist", Some("feed-a")).expect("artist a");
    let artist_b =
        stophammer::db::resolve_artist(&conn, "Chain Artist", Some("feed-b")).expect("artist b");
    let artist_c =
        stophammer::db::resolve_artist(&conn, "Chain Artist", Some("feed-c")).expect("artist c");

    stophammer::db::merge_artists(&mut conn, &artist_c.artist_id, &artist_b.artist_id)
        .expect("merge c into b");
    stophammer::db::merge_artists(&mut conn, &artist_b.artist_id, &artist_a.artist_id)
        .expect("merge b into a");

    let redirected_target: String = conn
        .query_row(
            "SELECT new_artist_id FROM artist_id_redirect WHERE old_artist_id = ?1",
            rusqlite::params![artist_c.artist_id],
            |row| row.get(0),
        )
        .expect("redirect for c");
    assert_eq!(redirected_target, artist_a.artist_id);
}

// ---------------------------------------------------------------------------
// Required resolver correctness tests
// ---------------------------------------------------------------------------

/// `publisher_guid` is no longer a backfill evidence source.
/// Two artists sharing a `publisher_guid` but no other evidence must NOT be
/// merged by `backfill_artist_identity`.
#[test]
fn backfill_does_not_merge_artists_by_publisher_guid() {
    let mut conn = common::test_db();
    let now = common::now();

    let artist_a =
        stophammer::db::resolve_artist(&conn, "Pguid Artist", Some("feed-pguid-a")).expect("a");
    let credit_a = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_a.name,
        &[(
            artist_a.artist_id.clone(),
            artist_a.name.clone(),
            String::new(),
        )],
        Some("feed-pguid-a"),
    )
    .expect("credit a");
    let artist_b =
        stophammer::db::resolve_artist(&conn, "Pguid Artist", Some("feed-pguid-b")).expect("b");
    let credit_b = stophammer::db::get_or_create_artist_credit(
        &conn,
        &artist_b.name,
        &[(
            artist_b.artist_id.clone(),
            artist_b.name.clone(),
            String::new(),
        )],
        Some("feed-pguid-b"),
    )
    .expect("credit b");

    for (feed_guid, credit_id) in [("feed-pguid-a", credit_a.id), ("feed-pguid-b", credit_b.id)] {
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![
                feed_guid,
                format!("https://example.com/{feed_guid}.xml"),
                "Pguid Release",
                "pguid release",
                credit_id,
                now
            ],
        )
        .expect("insert feed");
        conn.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES (?1, 0, 'publisher', 'shared-publisher-guid', 'https://wavlake.com/pguid-artist', 'podcast_remote_item')",
            rusqlite::params![feed_guid],
        )
        .expect("insert publisher remote item");
    }

    let pre_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'pguid artist'",
            [],
            |row| row.get(0),
        )
        .expect("pre count");
    assert_eq!(pre_count, 2);

    stophammer::db::backfill_artist_identity(&mut conn).expect("backfill");

    // publisher_guid is no longer identity evidence — both artists must survive.
    let post_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'pguid artist'",
            [],
            |row| row.get(0),
        )
        .expect("post count");
    assert_eq!(
        post_count, 2,
        "backfill must not merge artists based solely on shared publisher_guid"
    );
}

/// A single-feed artist with one website link anchors same-name weak
/// single-feed duplicates on aggregator-only platforms.
#[test]
fn single_feed_artist_with_website_anchors_weak_duplicates() {
    let mut conn = common::test_db();
    let now = common::now();

    // Anchor: 1 feed with a website link.
    let anchor =
        stophammer::db::resolve_artist(&conn, "Single Anchor Artist", Some("feed-anchor-site"))
            .expect("anchor");
    let anchor_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &anchor.name,
        &[(anchor.artist_id.clone(), anchor.name.clone(), String::new())],
        Some("feed-anchor-site"),
    )
    .expect("anchor credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-anchor-site', 'https://example.com/anchor.xml', 'Anchor', 'anchor', ?1, ?2, ?2)",
        rusqlite::params![anchor_credit.id, now],
    )
    .expect("anchor feed");
    conn.execute(
        "INSERT INTO source_entity_links \
         (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
         VALUES ('feed-anchor-site', 'feed', 'feed-anchor-site', 0, 'website', 'https://anchor-artist.example.com', 'rss_link', 'feed.link', ?1)",
        rusqlite::params![now],
    )
    .expect("anchor website");
    conn.execute(
        "INSERT INTO source_platform_claims \
         (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
         VALUES ('feed-anchor-site', 'wavlake', 'https://example.com/anchor.xml', NULL, 'derived', 'request.canonical_url', ?1)",
        rusqlite::params![now],
    )
    .expect("anchor platform");

    // Weak: 1 fountain-only feed, no identity evidence, same name.
    let weak =
        stophammer::db::resolve_artist(&conn, "Single Anchor Artist", Some("feed-weak-fountain"))
            .expect("weak");
    let weak_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &weak.name,
        &[(weak.artist_id.clone(), weak.name.clone(), String::new())],
        Some("feed-weak-fountain"),
    )
    .expect("weak credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-weak-fountain', 'https://feeds.fountain.fm/single-anchor', 'Weak', 'weak', ?1, ?2, ?2)",
        rusqlite::params![weak_credit.id, now],
    )
    .expect("weak feed");
    conn.execute(
        "INSERT INTO source_platform_claims \
         (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
         VALUES ('feed-weak-fountain', 'fountain', 'https://feeds.fountain.fm/single-anchor', NULL, 'derived', 'request.canonical_url', ?1)",
        rusqlite::params![now],
    )
    .expect("weak platform");

    let pre_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'single anchor artist'",
            [],
            |row| row.get(0),
        )
        .expect("pre count");
    assert_eq!(pre_count, 2);

    let stats = stophammer::db::backfill_artist_identity(&mut conn).expect("backfill");
    assert!(stats.merges_applied >= 1, "expected at least one merge");

    let post_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'single anchor artist'",
            [],
            |row| row.get(0),
        )
        .expect("post count");
    assert_eq!(
        post_count, 1,
        "single-feed artist with website evidence must anchor weak duplicate"
    );
}

/// A single-feed artist with one `nostr_npub` anchors same-name weak
/// single-feed duplicates on aggregator-only platforms.
#[test]
fn single_feed_artist_with_npub_anchors_weak_duplicates() {
    let mut conn = common::test_db();
    let now = common::now();

    let anchor =
        stophammer::db::resolve_artist(&conn, "Npub Anchor Artist", Some("feed-npub-anchor"))
            .expect("anchor");
    let anchor_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &anchor.name,
        &[(anchor.artist_id.clone(), anchor.name.clone(), String::new())],
        Some("feed-npub-anchor"),
    )
    .expect("anchor credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-npub-anchor', 'https://example.com/npub-anchor.xml', 'Npub Anchor', 'npub anchor', ?1, ?2, ?2)",
        rusqlite::params![anchor_credit.id, now],
    )
    .expect("anchor feed");
    conn.execute(
        "INSERT INTO source_platform_claims \
         (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
         VALUES ('feed-npub-anchor', 'wavlake', 'https://example.com/npub-anchor.xml', NULL, 'derived', 'request.canonical_url', ?1)",
        rusqlite::params![now],
    )
    .expect("anchor platform");
    stophammer::db::replace_source_entity_ids_for_feed(
        &conn,
        "feed-npub-anchor",
        &[stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: "feed-npub-anchor".into(),
            entity_type: "feed".into(),
            entity_id: "feed-npub-anchor".into(),
            position: 0,
            scheme: "nostr_npub".into(),
            value: "npub1anchortest".into(),
            source: "podcast_txt".into(),
            extraction_path: "feed.podcast:txt".into(),
            observed_at: now,
        }],
    )
    .expect("npub source claim");

    let weak = stophammer::db::resolve_artist(&conn, "Npub Anchor Artist", Some("feed-npub-weak"))
        .expect("weak");
    let weak_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &weak.name,
        &[(weak.artist_id.clone(), weak.name.clone(), String::new())],
        Some("feed-npub-weak"),
    )
    .expect("weak credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-npub-weak', 'https://feeds.rssblue.com/npub-weak', 'Npub Weak', 'npub weak', ?1, ?2, ?2)",
        rusqlite::params![weak_credit.id, now],
    )
    .expect("weak feed");
    conn.execute(
        "INSERT INTO source_platform_claims \
         (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
         VALUES ('feed-npub-weak', 'rss_blue', 'https://feeds.rssblue.com/npub-weak', NULL, 'derived', 'request.canonical_url', ?1)",
        rusqlite::params![now],
    )
    .expect("weak platform");

    let pre_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'npub anchor artist'",
            [],
            |row| row.get(0),
        )
        .expect("pre count");
    assert_eq!(pre_count, 2);

    let stats = stophammer::db::backfill_artist_identity(&mut conn).expect("backfill");
    assert!(stats.merges_applied >= 1, "expected at least one merge");

    let post_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'npub anchor artist'",
            [],
            |row| row.get(0),
        )
        .expect("post count");
    assert_eq!(
        post_count, 1,
        "single-feed artist with npub evidence must anchor weak duplicate"
    );
}

/// `preferred_artist_target` chooses the artist with explicit identity evidence
/// over the one created earlier (which the old tie-break would have preferred).
///
/// Setup: weak artist created first (older `created_at`), strong artist created
/// second (newer, but has explicit website evidence). Under the old ordering
/// (no evidence weight), the older weak artist would win. Under the new
/// ordering (evidence first), the strong artist must win.
#[test]
fn merge_target_prefers_evidence_over_creation_order() {
    let mut conn = common::test_db();
    let earlier = common::now();
    let later = earlier + 1;

    // Weak: created FIRST (older created_at), single fountain feed, no evidence.
    let weak = stophammer::db::resolve_artist(&conn, "Target Pref Artist", Some("feed-tp-weak"))
        .expect("weak");
    let weak_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &weak.name,
        &[(weak.artist_id.clone(), weak.name.clone(), String::new())],
        Some("feed-tp-weak"),
    )
    .expect("weak credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-tp-weak', 'https://feeds.fountain.fm/tp-weak', 'TP Weak', 'tp weak', ?1, ?2, ?2)",
        rusqlite::params![weak_credit.id, earlier],
    )
    .expect("weak feed");
    conn.execute(
        "INSERT INTO source_platform_claims \
         (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
         VALUES ('feed-tp-weak', 'fountain', 'https://feeds.fountain.fm/tp-weak', NULL, 'derived', 'request.canonical_url', ?1)",
        rusqlite::params![earlier],
    )
    .expect("weak platform");

    // Strong: created SECOND (newer created_at), single wavlake feed, has website.
    let strong =
        stophammer::db::resolve_artist(&conn, "Target Pref Artist", Some("feed-tp-strong"))
            .expect("strong");
    let strong_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &strong.name,
        &[(strong.artist_id.clone(), strong.name.clone(), String::new())],
        Some("feed-tp-strong"),
    )
    .expect("strong credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-tp-strong', 'https://wavlake.com/tp-strong.xml', 'TP Strong', 'tp strong', ?1, ?2, ?2)",
        rusqlite::params![strong_credit.id, later],
    )
    .expect("strong feed");
    conn.execute(
        "INSERT INTO source_entity_links \
         (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
         VALUES ('feed-tp-strong', 'feed', 'feed-tp-strong', 0, 'website', 'https://targetpref.example.com', 'rss_link', 'feed.link', ?1)",
        rusqlite::params![later],
    )
    .expect("strong website");
    conn.execute(
        "INSERT INTO source_platform_claims \
         (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
         VALUES ('feed-tp-strong', 'wavlake', 'https://wavlake.com/tp-strong.xml', NULL, 'derived', 'request.canonical_url', ?1)",
        rusqlite::params![later],
    )
    .expect("strong platform");

    // Backfill: anchored_name gate fires — strong is anchor (has website),
    // weak is the weak candidate (fountain, single-feed, no evidence).
    let stats = stophammer::db::backfill_artist_identity(&mut conn).expect("backfill");
    assert!(stats.merges_applied >= 1, "expected at least one merge");

    // The strong artist (newer, but has evidence) must be the merge target.
    // Under the old preferred_artist_target (no evidence weight), the weak
    // artist would win because it has an older created_at.
    let strong_redirected: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artist_id_redirect WHERE old_artist_id = ?1",
            rusqlite::params![strong.artist_id],
            |row| row.get(0),
        )
        .expect("redirect query for strong");
    assert_eq!(
        strong_redirected, 0,
        "the artist with explicit identity evidence must be the merge target, not redirected away"
    );

    let weak_redirected: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artist_id_redirect WHERE old_artist_id = ?1",
            rusqlite::params![weak.artist_id],
            |row| row.get(0),
        )
        .expect("redirect query for weak");
    assert_eq!(
        weak_redirected, 1,
        "the artist with no identity evidence must be merged away, even if it was created first"
    );
}

#[test]
fn wallet_name_variants_raise_review_without_auto_merge() {
    let mut conn = common::test_db();
    let now = common::now();

    let canonical =
        stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-wallet-variant"))
            .expect("canonical artist");
    let canonical_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &canonical.name,
        &[(
            canonical.artist_id.clone(),
            canonical.name.clone(),
            String::new(),
        )],
        Some("feed-wallet-variant"),
    )
    .expect("canonical credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-wallet-variant', 'https://example.com/wallet-variant.xml', 'Wallet Variant', 'wallet variant', ?1, ?2, ?2)",
        rusqlite::params![canonical_credit.id, now],
    )
    .expect("insert feed");

    let variant = stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-wallet-variant"))
        .expect("variant artist");
    let variant_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &variant.name,
        &[(
            variant.artist_id.clone(),
            variant.name.clone(),
            String::new(),
        )],
        Some("feed-wallet-variant"),
    )
    .expect("variant credit");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-wallet-variant', 'feed-wallet-variant', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
        rusqlite::params![variant_credit.id, now],
    )
    .expect("insert track");

    conn.execute(
        "INSERT INTO feed_payment_routes \
         (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
         VALUES ('feed-wallet-variant', 'HeyCitizen', 'lnaddress', 'heycitizen@example.com', NULL, NULL, 100, 0)",
        [],
    )
    .expect("insert feed route");

    let wallet_stats =
        stophammer::db::resolve_wallet_identity_for_feed(&conn, "feed-wallet-variant")
            .expect("resolve wallet identity");
    assert_eq!(
        wallet_stats.wallets_created, 1,
        "feed wallet route should create one provisional wallet"
    );
    assert_eq!(
        wallet_stats.artist_links_created, 1,
        "incremental wallet resolver should create the feed artist link"
    );

    let link_stats = stophammer::db::backfill_wallet_pass3(&conn).expect("wallet pass3");
    assert_eq!(
        link_stats.artist_links_created, 0,
        "global wallet pass3 should be idempotent after incremental linking"
    );

    let stats = stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-wallet-variant")
        .expect("resolve artist identity");
    assert_eq!(
        stats.merges_applied, 0,
        "wallet-based name variants should raise review, not auto-merge"
    );
    assert!(
        stats.pending_reviews >= 1,
        "wallet-based name variants should create at least one pending review"
    );

    let reviews =
        stophammer::db::list_artist_identity_reviews_for_feed(&conn, "feed-wallet-variant")
            .expect("list reviews");
    let review_sources = reviews
        .iter()
        .map(|review| review.source.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        review_sources.contains("wallet_name_variant"),
        "wallet_name_variant review should be present"
    );
    assert!(
        review_sources.contains("likely_same_artist"),
        "likely_same_artist review should appear when multiple same-feed signals agree"
    );
    assert!(
        review_sources.contains("track_feed_name_variant"),
        "track_feed_name_variant review should also be present for the same feed"
    );
    let review = reviews
        .iter()
        .find(|review| review.source == "wallet_name_variant")
        .expect("wallet_name_variant review");
    assert_eq!(review.name_key, "heycitizen");
    let review_artist_ids = review
        .artist_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let expected_artist_ids = [canonical.artist_id.clone(), variant.artist_id.clone()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        review_artist_ids, expected_artist_ids,
        "review should include both canonical and variant artist ids"
    );
    let likely_review = reviews
        .iter()
        .find(|review| review.source == "likely_same_artist")
        .expect("likely_same_artist review");
    assert_eq!(likely_review.confidence, "high_confidence");
    assert_eq!(likely_review.score, Some(65));
    assert!(
        likely_review
            .explanation
            .contains("Multiple same-feed evidence families agree"),
        "combined-signal review should explain why it is stronger than a single heuristic"
    );
}

#[test]
fn track_feed_name_variants_raise_review_without_wallet_evidence() {
    let mut conn = common::test_db();
    let now = common::now();

    let canonical =
        stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-track-feed-variant"))
            .expect("canonical artist");
    let canonical_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &canonical.name,
        &[(
            canonical.artist_id.clone(),
            canonical.name.clone(),
            String::new(),
        )],
        Some("feed-track-feed-variant"),
    )
    .expect("canonical credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-track-feed-variant', 'https://example.com/track-feed-variant.xml', 'Track Feed Variant', 'track feed variant', ?1, ?2, ?2)",
        rusqlite::params![canonical_credit.id, now],
    )
    .expect("insert feed");

    let variant =
        stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-track-feed-variant"))
            .expect("variant artist");
    let variant_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &variant.name,
        &[(
            variant.artist_id.clone(),
            variant.name.clone(),
            String::new(),
        )],
        Some("feed-track-feed-variant"),
    )
    .expect("variant credit");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-track-feed-variant', 'feed-track-feed-variant', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
        rusqlite::params![variant_credit.id, now],
    )
    .expect("insert track");

    let stats =
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-track-feed-variant")
            .expect("resolve artist identity");
    assert_eq!(
        stats.merges_applied, 0,
        "track/feed name variants should raise review, not auto-merge"
    );
    assert_eq!(
        stats.pending_reviews, 1,
        "track/feed name variants should create one pending review"
    );

    let reviews =
        stophammer::db::list_artist_identity_reviews_for_feed(&conn, "feed-track-feed-variant")
            .expect("list reviews");
    let review = reviews
        .iter()
        .find(|review| review.source == "track_feed_name_variant")
        .expect("track_feed_name_variant review");
    assert_eq!(review.name_key, "heycitizen");
    let review_artist_ids = review
        .artist_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let expected_artist_ids = [canonical.artist_id.clone(), variant.artist_id.clone()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        review_artist_ids, expected_artist_ids,
        "review should include both feed and track artist ids"
    );
}

#[test]
fn likely_same_artist_includes_shared_external_id_support() {
    let mut conn = common::test_db();
    let now = common::now();

    let canonical =
        stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-shared-extid-review"))
            .expect("canonical artist");
    let canonical_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &canonical.name,
        &[(
            canonical.artist_id.clone(),
            canonical.name.clone(),
            String::new(),
        )],
        Some("feed-shared-extid-review"),
    )
    .expect("canonical credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-shared-extid-review', 'https://example.com/shared-extid.xml', 'Shared External Id Review', 'shared external id review', ?1, ?2, ?2)",
        rusqlite::params![canonical_credit.id, now],
    )
    .expect("insert feed");

    let variant =
        stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-shared-extid-review"))
            .expect("variant artist");
    let variant_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &variant.name,
        &[(
            variant.artist_id.clone(),
            variant.name.clone(),
            String::new(),
        )],
        Some("feed-shared-extid-review"),
    )
    .expect("variant credit");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-shared-extid-review', 'feed-shared-extid-review', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
        rusqlite::params![variant_credit.id, now],
    )
    .expect("insert track");
    conn.execute(
        "INSERT INTO feed_payment_routes \
         (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
         VALUES ('feed-shared-extid-review', 'HeyCitizen', 'lnaddress', 'heycitizen@example.com', NULL, NULL, 100, 0)",
        [],
    )
    .expect("insert feed route");
    for artist_id in [&canonical.artist_id, &variant.artist_id] {
        conn.execute(
            "INSERT INTO external_ids (entity_type, entity_id, scheme, value, created_at) \
             VALUES ('artist', ?1, 'musicbrainz_artist', 'mbid-shared-extid-review', ?2)",
            rusqlite::params![artist_id, now],
        )
        .expect("insert shared artist external id");
    }

    let wallet_stats =
        stophammer::db::resolve_wallet_identity_for_feed(&conn, "feed-shared-extid-review")
            .expect("resolve wallet identity");
    assert_eq!(wallet_stats.wallets_created, 1);
    assert_eq!(wallet_stats.artist_links_created, 1);

    let stats =
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-shared-extid-review")
            .expect("resolve artist identity");
    assert_eq!(stats.merges_applied, 0, "scored review should remain review-only");

    let likely_review = stophammer::db::list_artist_identity_reviews_for_feed(
        &conn,
        "feed-shared-extid-review",
    )
    .expect("list reviews")
    .into_iter()
    .find(|review| review.source == "likely_same_artist")
    .expect("likely_same_artist review");
    assert_eq!(likely_review.confidence, "high_confidence");
    assert_eq!(likely_review.score, Some(100));
    let supporting = likely_review
        .supporting_sources
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(supporting.contains("track_feed_name_variant"));
    assert!(supporting.contains("wallet_name_variant"));
    assert!(
        supporting.contains("shared_external_id"),
        "shared artist external ids should strengthen likely_same_artist"
    );
}

#[test]
fn likely_same_artist_skips_conflicting_external_ids() {
    let mut conn = common::test_db();
    let now = common::now();

    let canonical = stophammer::db::resolve_artist(
        &conn,
        "HeyCitizen",
        Some("feed-conflicting-extid-review"),
    )
    .expect("canonical artist");
    let canonical_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &canonical.name,
        &[(
            canonical.artist_id.clone(),
            canonical.name.clone(),
            String::new(),
        )],
        Some("feed-conflicting-extid-review"),
    )
    .expect("canonical credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-conflicting-extid-review', 'https://example.com/conflicting-extid.xml', 'Conflicting External Id Review', 'conflicting external id review', ?1, ?2, ?2)",
        rusqlite::params![canonical_credit.id, now],
    )
    .expect("insert feed");

    let variant = stophammer::db::resolve_artist(
        &conn,
        "Hey Citizen",
        Some("feed-conflicting-extid-review"),
    )
    .expect("variant artist");
    let variant_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &variant.name,
        &[(
            variant.artist_id.clone(),
            variant.name.clone(),
            String::new(),
        )],
        Some("feed-conflicting-extid-review"),
    )
    .expect("variant credit");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-conflicting-extid-review', 'feed-conflicting-extid-review', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
        rusqlite::params![variant_credit.id, now],
    )
    .expect("insert track");
    conn.execute(
        "INSERT INTO feed_payment_routes \
         (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
         VALUES ('feed-conflicting-extid-review', 'HeyCitizen', 'lnaddress', 'heycitizen@example.com', NULL, NULL, 100, 0)",
        [],
    )
    .expect("insert feed route");
    conn.execute(
        "INSERT INTO external_ids (entity_type, entity_id, scheme, value, created_at) \
         VALUES ('artist', ?1, 'musicbrainz_artist', 'mbid-conflict-a', ?2)",
        rusqlite::params![canonical.artist_id, now],
    )
    .expect("insert canonical external id");
    conn.execute(
        "INSERT INTO external_ids (entity_type, entity_id, scheme, value, created_at) \
         VALUES ('artist', ?1, 'musicbrainz_artist', 'mbid-conflict-b', ?2)",
        rusqlite::params![variant.artist_id, now],
    )
    .expect("insert variant external id");

    let wallet_stats =
        stophammer::db::resolve_wallet_identity_for_feed(&conn, "feed-conflicting-extid-review")
            .expect("resolve wallet identity");
    assert_eq!(wallet_stats.wallets_created, 1);
    assert_eq!(wallet_stats.artist_links_created, 1);

    let stats = stophammer::db::resolve_artist_identity_for_feed(
        &mut conn,
        "feed-conflicting-extid-review",
    )
    .expect("resolve artist identity");
    assert_eq!(stats.merges_applied, 0, "conflicting ext ids should not auto-merge");

    let reviews = stophammer::db::list_artist_identity_reviews_for_feed(
        &conn,
        "feed-conflicting-extid-review",
    )
    .expect("list reviews");
    let review_sources = reviews
        .iter()
        .map(|review| review.source.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(review_sources.contains("track_feed_name_variant"));
    assert!(review_sources.contains("wallet_name_variant"));
    let likely_review = reviews
        .iter()
        .find(|review| review.source == "likely_same_artist")
        .expect("blocked likely_same_artist review");
    assert_eq!(likely_review.confidence, "blocked");
    assert!(
        likely_review
            .conflict_reasons
            .contains(&"conflicting_external_id".to_string()),
        "conflicting artist external ids should be surfaced explicitly"
    );
    assert!(
        likely_review
            .explanation
            .contains("conflicting external IDs"),
        "blocked likely_same_artist review should explain the conflict"
    );
}

#[test]
fn likely_same_artist_includes_normalized_website_support() {
    let mut conn = common::test_db();
    let now = common::now();

    let canonical =
        stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-shared-website-review"))
            .expect("canonical artist");
    let canonical_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &canonical.name,
        &[(
            canonical.artist_id.clone(),
            canonical.name.clone(),
            String::new(),
        )],
        Some("feed-shared-website-review"),
    )
    .expect("canonical credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-shared-website-review', 'https://example.com/shared-website.xml', 'Shared Website Review', 'shared website review', ?1, ?2, ?2)",
        rusqlite::params![canonical_credit.id, now],
    )
    .expect("insert canonical feed");
    conn.execute(
        "INSERT INTO source_entity_links \
         (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
         VALUES ('feed-shared-website-review', 'feed', 'feed-shared-website-review', 0, 'website', 'https://artist.example.com/heycitizen', 'rss_link', 'feed.link', ?1)",
        rusqlite::params![now],
    )
    .expect("insert canonical website");

    let variant =
        stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-shared-website-review"))
            .expect("variant artist");
    let variant_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &variant.name,
        &[(
            variant.artist_id.clone(),
            variant.name.clone(),
            String::new(),
        )],
        Some("feed-shared-website-review"),
    )
    .expect("variant credit");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-shared-website-review', 'feed-shared-website-review', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
        rusqlite::params![variant_credit.id, now],
    )
    .expect("insert track");
    conn.execute(
        "INSERT INTO feed_payment_routes \
         (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
         VALUES ('feed-shared-website-review', 'HeyCitizen', 'lnaddress', 'heycitizen@example.com', NULL, NULL, 100, 0)",
        [],
    )
    .expect("insert feed route");

    let variant_feed_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &variant.name,
        &[(variant.artist_id.clone(), variant.name.clone(), String::new())],
        Some("feed-shared-website-variant"),
    )
    .expect("variant feed credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-shared-website-variant', 'https://example.com/shared-website-variant.xml', 'Shared Website Variant', 'shared website variant', ?1, ?2, ?2)",
        rusqlite::params![variant_feed_credit.id, now],
    )
    .expect("insert variant feed");
    conn.execute(
        "INSERT INTO source_entity_links \
         (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
         VALUES ('feed-shared-website-variant', 'feed', 'feed-shared-website-variant', 0, 'website', 'https://artist.example.com/heycitizen', 'rss_link', 'feed.link', ?1)",
        rusqlite::params![now],
    )
    .expect("insert variant website");

    let wallet_stats =
        stophammer::db::resolve_wallet_identity_for_feed(&conn, "feed-shared-website-review")
            .expect("resolve wallet identity");
    assert_eq!(wallet_stats.wallets_created, 1);
    assert_eq!(wallet_stats.artist_links_created, 1);

    let stats =
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-shared-website-review")
            .expect("resolve artist identity");
    assert_eq!(stats.merges_applied, 0, "scored review should remain review-only");

    let likely_review = stophammer::db::list_artist_identity_reviews_for_feed(
        &conn,
        "feed-shared-website-review",
    )
    .expect("list reviews")
    .into_iter()
    .find(|review| review.source == "likely_same_artist")
    .expect("likely_same_artist review");
    assert_eq!(likely_review.confidence, "high_confidence");
    assert_eq!(likely_review.score, Some(95));
    let supporting = likely_review
        .supporting_sources
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(supporting.contains("track_feed_name_variant"));
    assert!(supporting.contains("wallet_name_variant"));
    assert!(
        supporting.contains("normalized_website"),
        "shared normalized website should strengthen likely_same_artist"
    );
}

#[test]
fn likely_same_artist_includes_shared_npub_support() {
    let mut conn = common::test_db();
    let now = common::now();

    let canonical =
        stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-shared-npub-review"))
            .expect("canonical artist");
    let canonical_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &canonical.name,
        &[(
            canonical.artist_id.clone(),
            canonical.name.clone(),
            String::new(),
        )],
        Some("feed-shared-npub-review"),
    )
    .expect("canonical credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-shared-npub-review', 'https://example.com/shared-npub.xml', 'Shared Npub Review', 'shared npub review', ?1, ?2, ?2)",
        rusqlite::params![canonical_credit.id, now],
    )
    .expect("insert canonical feed");
    stophammer::db::replace_source_entity_ids_for_feed(
        &conn,
        "feed-shared-npub-review",
        &[stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: "feed-shared-npub-review".into(),
            entity_type: "feed".into(),
            entity_id: "feed-shared-npub-review".into(),
            position: 0,
            scheme: "nostr_npub".into(),
            value: "npub1sharedreview".into(),
            source: "rss_txt".into(),
            extraction_path: "feed.podcast:txt[@purpose='npub']".into(),
            observed_at: now,
        }],
    )
    .expect("insert canonical npub");

    let variant =
        stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-shared-npub-review"))
            .expect("variant artist");
    let variant_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &variant.name,
        &[(
            variant.artist_id.clone(),
            variant.name.clone(),
            String::new(),
        )],
        Some("feed-shared-npub-review"),
    )
    .expect("variant credit");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-shared-npub-review', 'feed-shared-npub-review', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
        rusqlite::params![variant_credit.id, now],
    )
    .expect("insert track");
    conn.execute(
        "INSERT INTO feed_payment_routes \
         (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
         VALUES ('feed-shared-npub-review', 'HeyCitizen', 'lnaddress', 'heycitizen@example.com', NULL, NULL, 100, 0)",
        [],
    )
    .expect("insert feed route");

    let variant_feed_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &variant.name,
        &[(variant.artist_id.clone(), variant.name.clone(), String::new())],
        Some("feed-shared-npub-variant"),
    )
    .expect("variant feed credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-shared-npub-variant', 'https://example.com/shared-npub-variant.xml', 'Shared Npub Variant', 'shared npub variant', ?1, ?2, ?2)",
        rusqlite::params![variant_feed_credit.id, now],
    )
    .expect("insert variant feed");
    stophammer::db::replace_source_entity_ids_for_feed(
        &conn,
        "feed-shared-npub-variant",
        &[stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: "feed-shared-npub-variant".into(),
            entity_type: "feed".into(),
            entity_id: "feed-shared-npub-variant".into(),
            position: 0,
            scheme: "nostr_npub".into(),
            value: "npub1sharedreview".into(),
            source: "rss_txt".into(),
            extraction_path: "feed.podcast:txt[@purpose='npub']".into(),
            observed_at: now,
        }],
    )
    .expect("insert variant npub");

    let wallet_stats =
        stophammer::db::resolve_wallet_identity_for_feed(&conn, "feed-shared-npub-review")
            .expect("resolve wallet identity");
    assert_eq!(wallet_stats.wallets_created, 1);
    assert_eq!(wallet_stats.artist_links_created, 1);

    let stats =
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-shared-npub-review")
            .expect("resolve artist identity");
    assert_eq!(stats.merges_applied, 0, "scored review should remain review-only");

    let likely_review = stophammer::db::list_artist_identity_reviews_for_feed(
        &conn,
        "feed-shared-npub-review",
    )
    .expect("list reviews")
    .into_iter()
    .find(|review| review.source == "likely_same_artist")
    .expect("likely_same_artist review");
    assert_eq!(likely_review.confidence, "high_confidence");
    assert_eq!(likely_review.score, Some(100));
    let supporting = likely_review
        .supporting_sources
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(supporting.contains("track_feed_name_variant"));
    assert!(supporting.contains("wallet_name_variant"));
    assert!(
        supporting.contains("shared_npub"),
        "shared feed-level npub claims should strengthen likely_same_artist"
    );
}

#[test]
fn collaboration_credit_raises_review_without_auto_merge() {
    let mut conn = common::test_db();
    let now = common::now();

    let canonical =
        stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-collaboration-credit"))
            .expect("canonical artist");
    let canonical_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &canonical.name,
        &[(
            canonical.artist_id.clone(),
            canonical.name.clone(),
            String::new(),
        )],
        Some("feed-collaboration-credit"),
    )
    .expect("canonical credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-collaboration-credit', 'https://example.com/collaboration-credit.xml', 'Collaboration Credit', 'collaboration credit', ?1, ?2, ?2)",
        rusqlite::params![canonical_credit.id, now],
    )
    .expect("insert feed");

    let collaboration = stophammer::db::resolve_artist(
        &conn,
        "HeyCitizen and Fletcher",
        Some("feed-collaboration-credit"),
    )
    .expect("collaboration artist");
    let collaboration_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &collaboration.name,
        &[(
            collaboration.artist_id.clone(),
            collaboration.name.clone(),
            String::new(),
        )],
        Some("feed-collaboration-credit"),
    )
    .expect("collaboration credit");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-collaboration-credit', 'feed-collaboration-credit', ?1, 'Hardware Store Lady (Screw and Bolt Mix)', 'hardware store lady (screw and bolt mix)', 0, ?2, ?2)",
        rusqlite::params![collaboration_credit.id, now],
    )
    .expect("insert track");

    let stats =
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-collaboration-credit")
            .expect("resolve artist identity");
    assert_eq!(
        stats.merges_applied, 0,
        "collaboration credits should raise review, not auto-merge"
    );
    assert_eq!(
        stats.pending_reviews, 1,
        "collaboration credits should create one pending review"
    );

    let reviews =
        stophammer::db::list_artist_identity_reviews_for_feed(&conn, "feed-collaboration-credit")
            .expect("list reviews");
    let review = reviews
        .iter()
        .find(|review| review.source == "collaboration_credit")
        .expect("collaboration_credit review");
    assert_eq!(review.name_key, "heycitizen");
    assert_eq!(review.evidence_key, collaboration.artist_id);
    let review_artist_ids = review
        .artist_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let expected_artist_ids = [canonical.artist_id.clone(), collaboration.artist_id.clone()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        review_artist_ids, expected_artist_ids,
        "review should include both the feed artist and the collaboration artist ids"
    );
}

#[test]
fn pending_artist_reviews_prioritize_scored_high_confidence_items() {
    let mut conn = common::test_db();
    let now = common::now();

    let canonical =
        stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-priority-artist"))
            .expect("canonical artist");
    let canonical_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &canonical.name,
        &[(
            canonical.artist_id.clone(),
            canonical.name.clone(),
            String::new(),
        )],
        Some("feed-priority-artist"),
    )
    .expect("canonical credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-priority-artist', 'https://example.com/feed-priority-artist.xml', 'Priority Artist Feed', 'priority artist feed', ?1, ?2, ?2)",
        rusqlite::params![canonical_credit.id, now],
    )
    .expect("insert feed");

    let variant =
        stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-priority-artist"))
            .expect("variant artist");
    let variant_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &variant.name,
        &[(
            variant.artist_id.clone(),
            variant.name.clone(),
            String::new(),
        )],
        Some("feed-priority-artist"),
    )
    .expect("variant credit");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-priority-artist', 'feed-priority-artist', ?1, 'Priority Track', 'priority track', 0, ?2, ?2)",
        rusqlite::params![variant_credit.id, now],
    )
    .expect("insert track");
    conn.execute(
        "INSERT INTO feed_payment_routes \
         (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
         VALUES ('feed-priority-artist', 'HeyCitizen', 'lnaddress', 'priority@example.com', NULL, NULL, 100, 0)",
        [],
    )
    .expect("insert feed route");

    let wallet_stats =
        stophammer::db::resolve_wallet_identity_for_feed(&conn, "feed-priority-artist")
            .expect("resolve wallet identity");
    assert_eq!(wallet_stats.artist_links_created, 1);

    let stats = stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-priority-artist")
        .expect("resolve artist identity");
    assert_eq!(stats.merges_applied, 0);

    let reviews =
        stophammer::db::list_pending_artist_identity_reviews(&conn, 10).expect("pending reviews");
    let likely_index = reviews
        .iter()
        .position(|review| review.source == "likely_same_artist")
        .expect("likely_same_artist review");
    let wallet_index = reviews
        .iter()
        .position(|review| review.source == "wallet_name_variant")
        .expect("wallet_name_variant review");
    assert!(
        likely_index < wallet_index,
        "scored high-confidence review should sort ahead of an unscored high-confidence review"
    );
    assert_eq!(reviews[likely_index].score, Some(65));
    assert_eq!(reviews[wallet_index].score, None);
}

#[test]
fn contributor_name_variants_raise_review_without_auto_merge() {
    let mut conn = common::test_db();
    let now = common::now();

    let feed_artist = stophammer::db::resolve_artist(
        &conn,
        "Compilation Host",
        Some("feed-contributor-name-variant"),
    )
    .expect("feed artist");
    let feed_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &feed_artist.name,
        &[(
            feed_artist.artist_id.clone(),
            feed_artist.name.clone(),
            String::new(),
        )],
        Some("feed-contributor-name-variant"),
    )
    .expect("feed credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-contributor-name-variant', 'https://example.com/contributor-name-variant.xml', 'Contributor Name Variant', 'contributor name variant', ?1, ?2, ?2)",
        rusqlite::params![feed_credit.id, now],
    )
    .expect("insert feed");

    let first_variant =
        stophammer::db::resolve_artist(&conn, "HeyCitizen", Some("feed-contributor-name-variant"))
            .expect("first variant");
    let first_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &first_variant.name,
        &[(
            first_variant.artist_id.clone(),
            first_variant.name.clone(),
            String::new(),
        )],
        Some("feed-contributor-name-variant"),
    )
    .expect("first credit");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-contributor-name-variant-a', 'feed-contributor-name-variant', ?1, 'Hardware Store Lady', 'hardware store lady', 0, ?2, ?2)",
        rusqlite::params![first_credit.id, now],
    )
    .expect("insert first track");

    let second_variant =
        stophammer::db::resolve_artist(&conn, "Hey Citizen", Some("feed-contributor-name-variant"))
            .expect("second variant");
    let second_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &second_variant.name,
        &[(
            second_variant.artist_id.clone(),
            second_variant.name.clone(),
            String::new(),
        )],
        Some("feed-contributor-name-variant"),
    )
    .expect("second credit");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-contributor-name-variant-b', 'feed-contributor-name-variant', ?1, 'Autistic Girl', 'autistic girl', 0, ?2, ?2)",
        rusqlite::params![second_credit.id, now],
    )
    .expect("insert second track");

    stophammer::db::replace_source_contributor_claims_for_feed(
        &conn,
        "feed-contributor-name-variant",
        &[
            stophammer::model::SourceContributorClaim {
                id: None,
                feed_guid: "feed-contributor-name-variant".into(),
                entity_type: "track".into(),
                entity_id: "track-contributor-name-variant-a".into(),
                position: 0,
                name: "HeyCitizen".into(),
                role: Some("musician".into()),
                role_norm: Some("musician".into()),
                group_name: None,
                href: None,
                img: None,
                source: "podcast_person".into(),
                extraction_path: "track.podcast:person[0]".into(),
                observed_at: now,
            },
            stophammer::model::SourceContributorClaim {
                id: None,
                feed_guid: "feed-contributor-name-variant".into(),
                entity_type: "track".into(),
                entity_id: "track-contributor-name-variant-b".into(),
                position: 0,
                name: "Hey Citizen".into(),
                role: Some("musician".into()),
                role_norm: Some("musician".into()),
                group_name: None,
                href: None,
                img: None,
                source: "podcast_person".into(),
                extraction_path: "track.podcast:person[0]".into(),
                observed_at: now,
            },
        ],
    )
    .expect("replace contributor claims");

    let stats = stophammer::db::resolve_artist_identity_for_feed(
        &mut conn,
        "feed-contributor-name-variant",
    )
    .expect("resolve artist identity");
    assert_eq!(
        stats.merges_applied, 0,
        "contributor name variants should raise review, not auto-merge"
    );
    assert_eq!(
        stats.pending_reviews, 1,
        "contributor name variants should create one pending review"
    );

    let reviews = stophammer::db::list_artist_identity_reviews_for_feed(
        &conn,
        "feed-contributor-name-variant",
    )
    .expect("list reviews");
    let review = reviews
        .iter()
        .find(|review| review.source == "contributor_name_variant")
        .expect("contributor_name_variant review");
    assert_eq!(review.name_key, "heycitizen");
    assert_eq!(review.evidence_key, "feed-contributor-name-variant");
    let review_artist_ids = review
        .artist_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let expected_artist_ids = [
        first_variant.artist_id.clone(),
        second_variant.artist_id.clone(),
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        review_artist_ids, expected_artist_ids,
        "review should include both track artist ids backed by contributor claims"
    );
}

/// Canonical promotions computed after an artist-identity merge reference the
/// surviving artist, not the merged-away one.
///
/// This verifies the ordering invariant in `resolve_feed`: the
/// `DIRTY_ARTIST_IDENTITY` phase (which merges artists and repoints
/// `artist_credit_name` rows) must run before the `DIRTY_CANONICAL_PROMOTIONS`
/// phase (which emits external-id promotions scoped to the feed's current
/// artist).  If the order were reversed, `single_artist_id_for_credit` would
/// still return the pre-merge `artist_id`, and the surviving artist would not
/// receive the `nostr_npub` evidence it owns.
#[test]
fn canonical_promotions_after_merge_reference_surviving_artist() {
    let mut conn = common::test_db();
    let now = common::now();

    // Strong: one wavlake feed, one nostr_npub — will be the merge target.
    let strong = stophammer::db::resolve_artist(
        &conn,
        "Promo Order Artist",
        Some("feed-promo-order-strong"),
    )
    .expect("strong artist");
    let strong_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &strong.name,
        &[(strong.artist_id.clone(), strong.name.clone(), String::new())],
        Some("feed-promo-order-strong"),
    )
    .expect("strong credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-promo-order-strong', 'https://wavlake.com/promo-order.xml', 'Promo Order', 'promo order', ?1, ?2, ?2)",
        rusqlite::params![strong_credit.id, now],
    )
    .expect("strong feed");
    stophammer::db::replace_source_entity_ids_for_feed(
        &conn,
        "feed-promo-order-strong",
        &[stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: "feed-promo-order-strong".into(),
            entity_type: "feed".into(),
            entity_id: "feed-promo-order-strong".into(),
            position: 0,
            scheme: "nostr_npub".into(),
            value: "npub1promoordertest".into(),
            source: "podcast_txt".into(),
            extraction_path: "feed.podcast:txt".into(),
            observed_at: now,
        }],
    )
    .expect("strong npub source claim");
    conn.execute(
        "INSERT INTO source_platform_claims \
         (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
         VALUES ('feed-promo-order-strong', 'wavlake', 'https://wavlake.com/promo-order.xml', NULL, 'derived', 'request.canonical_url', ?1)",
        rusqlite::params![now],
    )
    .expect("strong platform");

    // Weak: one fountain feed, no identity evidence — will be merged into strong.
    let weak =
        stophammer::db::resolve_artist(&conn, "Promo Order Artist", Some("feed-promo-order-weak"))
            .expect("weak artist");
    let weak_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &weak.name,
        &[(weak.artist_id.clone(), weak.name.clone(), String::new())],
        Some("feed-promo-order-weak"),
    )
    .expect("weak credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-promo-order-weak', 'https://feeds.fountain.fm/promo-order-weak', 'Promo Order Weak', 'promo order weak', ?1, ?2, ?2)",
        rusqlite::params![weak_credit.id, now],
    )
    .expect("weak feed");
    conn.execute(
        "INSERT INTO source_platform_claims \
         (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
         VALUES ('feed-promo-order-weak', 'fountain', 'https://feeds.fountain.fm/promo-order-weak', NULL, 'derived', 'request.canonical_url', ?1)",
        rusqlite::params![now],
    )
    .expect("weak platform");

    // Phase 1 (DIRTY_ARTIST_IDENTITY): merge weak into strong.
    let stats =
        stophammer::db::resolve_artist_identity_for_feed(&mut conn, "feed-promo-order-weak")
            .expect("artist identity");
    assert!(
        stats.merges_applied >= 1,
        "expected weak to be merged into strong"
    );

    // Phase 2 (DIRTY_CANONICAL_PROMOTIONS): sync promotions for the strong feed
    // AFTER the merge.  The promotion must reference the surviving artist.
    stophammer::db::sync_canonical_promotions_for_feed(&conn, "feed-promo-order-strong")
        .expect("sync promotions");

    let promo_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM resolved_external_ids_by_feed \
             WHERE feed_guid = 'feed-promo-order-strong' \
               AND scheme = 'nostr_npub' AND value = 'npub1promoordertest'",
            [],
            |row| row.get(0),
        )
        .expect("promo count query");
    assert_eq!(
        promo_count, 1,
        "sync_canonical_promotions_for_feed must emit a resolved external-id for the npub"
    );

    let promo_artist: String = conn
        .query_row(
            "SELECT entity_id FROM resolved_external_ids_by_feed \
             WHERE feed_guid = 'feed-promo-order-strong' \
               AND scheme = 'nostr_npub' AND value = 'npub1promoordertest'",
            [],
            |row| row.get(0),
        )
        .expect("promo entity_id query");
    assert_eq!(
        promo_artist, strong.artist_id,
        "promotion entity_id must be the surviving artist, not the merged-away one"
    );
}

/// `cleanup_orphaned_artists` deletes truly unreferenced artists and their
/// associated rows, and leaves artists that are still referenced untouched.
#[test]
fn orphan_cleanup_deletes_unreferenced_artists_only() {
    let mut conn = common::test_db();
    let now = common::now();

    // Artist A: unreferenced — no feeds, no tracks, no releases, no recordings.
    let orphan =
        stophammer::db::resolve_artist(&conn, "Orphan Artist", None).expect("orphan artist");

    // Artist B: referenced by a live feed — must NOT be deleted.
    let live = stophammer::db::resolve_artist(&conn, "Live Artist", Some("feed-live-ref"))
        .expect("live artist");
    let live_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &live.name,
        &[(live.artist_id.clone(), live.name.clone(), String::new())],
        Some("feed-live-ref"),
    )
    .expect("live credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-live-ref', 'https://example.com/live-ref.xml', 'Live Feed', 'live feed', ?1, ?2, ?2)",
        rusqlite::params![live_credit.id, now],
    )
    .expect("live feed");

    // Add an alias and a tag to the orphan to verify associated row cleanup.
    conn.execute(
        "INSERT OR IGNORE INTO artist_aliases (alias_lower, artist_id, created_at) VALUES ('orphan alias', ?1, ?2)",
        rusqlite::params![orphan.artist_id, now],
    )
    .expect("orphan alias");

    let pre_orphan: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE artist_id = ?1",
            rusqlite::params![orphan.artist_id],
            |row| row.get(0),
        )
        .expect("pre orphan");
    assert_eq!(pre_orphan, 1);

    let stats =
        stophammer::db::cleanup_orphaned_artists(&mut conn).expect("cleanup_orphaned_artists");
    assert!(
        stats.artists_deleted >= 1,
        "expected at least one orphan artist deleted"
    );

    // Orphan must be gone.
    let post_orphan: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE artist_id = ?1",
            rusqlite::params![orphan.artist_id],
            |row| row.get(0),
        )
        .expect("post orphan");
    assert_eq!(post_orphan, 0, "orphan artist must have been deleted");

    // Alias must be cleaned up too.
    let alias_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artist_aliases WHERE artist_id = ?1",
            rusqlite::params![orphan.artist_id],
            |row| row.get(0),
        )
        .expect("alias count");
    assert_eq!(alias_count, 0, "orphan artist aliases must be deleted");

    // Live artist must be untouched.
    let post_live: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE artist_id = ?1",
            rusqlite::params![live.artist_id],
            |row| row.get(0),
        )
        .expect("post live");
    assert_eq!(post_live, 1, "referenced artist must not be deleted");
}

// ---------------------------------------------------------------------------
// Publisher link grouping
// ---------------------------------------------------------------------------

/// Helper: create a feed + artist + credit for publisher link tests.
fn seed_feed_for_publisher(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    feed_url: &str,
    artist_name: &str,
    raw_medium: &str,
) -> (String, i64) {
    let now = common::now();
    let artist =
        stophammer::db::resolve_artist(conn, artist_name, Some(feed_guid)).expect("artist");
    let credit = stophammer::db::get_or_create_artist_credit(
        conn,
        &artist.name,
        &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
        Some(feed_guid),
    )
    .expect("credit");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         raw_medium, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        rusqlite::params![
            feed_guid,
            feed_url,
            artist_name,
            artist_name.to_ascii_lowercase(),
            credit.id,
            raw_medium,
            now,
        ],
    )
    .expect("feed");
    (artist.artist_id, credit.id)
}

#[test]
fn publisher_link_groups_two_way_validated() {
    let mut conn = common::test_db();

    // Create a publisher feed.
    let (_, _) = seed_feed_for_publisher(
        &conn,
        "pub-feed-1",
        "https://wavlake.com/feed/artist/pub-feed-1",
        "Publisher Artist",
        "publisher",
    );

    // Create two child music feeds (separate artist_ids because resolve_artist
    // creates distinct IDs per feed hint).
    let (_, _) = seed_feed_for_publisher(
        &conn,
        "child-feed-a",
        "https://wavlake.com/feed/music/child-a",
        "Publisher Artist",
        "music",
    );
    let (_, _) = seed_feed_for_publisher(
        &conn,
        "child-feed-b",
        "https://wavlake.com/feed/music/child-b",
        "Publisher Artist",
        "music",
    );

    // Publisher → child (medium='music')
    for (pos, child) in (0i64..).zip(["child-feed-a", "child-feed-b"]) {
        conn.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES ('pub-feed-1', ?1, 'music', ?2, '', 'podcast_remote_item')",
            rusqlite::params![pos, child],
        )
        .expect("publisher→child");
    }

    // Child → publisher back-links (medium='publisher')
    for child in ["child-feed-a", "child-feed-b"] {
        conn.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES (?1, 0, 'publisher', 'pub-feed-1', '', 'podcast_remote_item')",
            rusqlite::params![child],
        )
        .expect("child→publisher");
    }

    // Verify both directions exist before backfill.
    let pub_to_child: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feed_remote_items_raw WHERE feed_guid = 'pub-feed-1' AND medium = 'music'",
            [],
            |r| r.get(0),
        )
        .expect("pub→child count");
    assert_eq!(pub_to_child, 2, "two publisher→child links");

    let child_to_pub: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feed_remote_items_raw WHERE medium = 'publisher' AND remote_feed_guid = 'pub-feed-1'",
            [],
            |r| r.get(0),
        )
        .expect("child→pub count");
    assert_eq!(child_to_pub, 2, "two child→publisher back-links");

    let pub_medium: String = conn
        .query_row(
            "SELECT raw_medium FROM feeds WHERE feed_guid = 'pub-feed-1'",
            [],
            |r| r.get(0),
        )
        .expect("pub medium");
    assert_eq!(pub_medium, "publisher");

    // Count distinct artist_ids across child feeds — should be 2 before merge.
    let pre_artist_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'publisher artist'",
            [],
            |row| row.get(0),
        )
        .expect("pre count");
    // 3 artists: publisher feed + child-a + child-b (all share the name "Publisher Artist")
    assert_eq!(
        pre_artist_count, 3,
        "should have 3 distinct artists before merge"
    );

    // Run backfill — validated Wavlake publisher links should confirm the
    // publisher feed's own artist row as well, so all three rows collapse.
    let stats = stophammer::db::backfill_artist_identity(&mut conn).expect("backfill");
    assert!(
        stats.merges_applied >= 2,
        "publisher link should merge artists: {stats:?}"
    );

    let artist_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'publisher artist'",
            [],
            |row| row.get(0),
        )
        .expect("artist count");
    assert_eq!(
        artist_count, 1,
        "validated wavlake publisher feeds must confirm the publisher artist row"
    );
}

#[test]
fn publisher_link_ignores_one_way() {
    let mut conn = common::test_db();

    // Publisher feed.
    let (_, _) = seed_feed_for_publisher(
        &conn,
        "pub-feed-2",
        "https://wavlake.com/feed/artist/pub-feed-2",
        "One Way Artist",
        "publisher",
    );

    // Child feeds.
    let (_, _) = seed_feed_for_publisher(
        &conn,
        "child-feed-c",
        "https://wavlake.com/feed/music/child-c",
        "One Way Artist",
        "music",
    );
    let (_, _) = seed_feed_for_publisher(
        &conn,
        "child-feed-d",
        "https://wavlake.com/feed/music/child-d",
        "One Way Artist",
        "music",
    );

    // Publisher → child only (no back-link from child → publisher).
    for (pos, child) in (0i64..).zip(["child-feed-c", "child-feed-d"]) {
        conn.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES ('pub-feed-2', ?1, 'music', ?2, '', 'podcast_remote_item')",
            rusqlite::params![pos, child],
        )
        .expect("publisher→child");
    }

    let _stats = stophammer::db::backfill_artist_identity(&mut conn).expect("backfill");

    // Without back-links, publisher grouping should not fire.
    // The anchored_name strategy might still merge them (same name, single feed each),
    // but the publisher_link source should NOT appear.
    let publisher_reviews: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artist_identity_review WHERE source = 'publisher_link'",
            [],
            |row| row.get(0),
        )
        .expect("publisher_link reviews");
    assert_eq!(
        publisher_reviews, 0,
        "one-way links should not produce publisher_link groups"
    );
}

#[test]
fn non_wavlake_publisher_link_keeps_publisher_artist_separate() {
    let mut conn = common::test_db();

    let (_, _) = seed_feed_for_publisher(
        &conn,
        "pub-feed-non-wavlake",
        "https://publisher.example.com/artist/pub-feed",
        "Publisher Artist",
        "publisher",
    );
    let (_, _) = seed_feed_for_publisher(
        &conn,
        "child-feed-non-wavlake-a",
        "https://music.example.com/child-a.xml",
        "Publisher Artist",
        "music",
    );
    let (_, _) = seed_feed_for_publisher(
        &conn,
        "child-feed-non-wavlake-b",
        "https://music.example.com/child-b.xml",
        "Publisher Artist",
        "music",
    );

    for (pos, child) in (0i64..).zip(["child-feed-non-wavlake-a", "child-feed-non-wavlake-b"]) {
        conn.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES ('pub-feed-non-wavlake', ?1, 'music', ?2, '', 'podcast_remote_item')",
            rusqlite::params![pos, child],
        )
        .expect("publisher→child");
    }

    for child in ["child-feed-non-wavlake-a", "child-feed-non-wavlake-b"] {
        conn.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES (?1, 0, 'publisher', 'pub-feed-non-wavlake', '', 'podcast_remote_item')",
            rusqlite::params![child],
        )
        .expect("child→publisher");
    }

    let stats = stophammer::db::backfill_artist_identity(&mut conn).expect("backfill");
    assert!(
        stats.merges_applied >= 1,
        "expected child merge from publisher link"
    );

    let artist_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'publisher artist'",
            [],
            |row| row.get(0),
        )
        .expect("artist count");
    assert_eq!(
        artist_count, 2,
        "non-wavlake publisher links should remain a signal, not confirm the publisher artist"
    );
}

#[test]
fn publisher_name_variants_raise_review_without_auto_merge() {
    let mut conn = common::test_db();

    let (_, _) = seed_feed_for_publisher(
        &conn,
        "pub-feed-publisher-variant",
        "https://publisher.example.com/artist/pub-feed-variant",
        "Publisher Curator",
        "publisher",
    );
    let (first_artist_id, _) = seed_feed_for_publisher(
        &conn,
        "child-feed-publisher-variant-a",
        "https://music.example.com/heycitizen-a.xml",
        "HeyCitizen",
        "music",
    );
    let (second_artist_id, _) = seed_feed_for_publisher(
        &conn,
        "child-feed-publisher-variant-b",
        "https://music.example.com/heycitizen-b.xml",
        "Hey Citizen",
        "music",
    );

    for (pos, child) in (0i64..).zip([
        "child-feed-publisher-variant-a",
        "child-feed-publisher-variant-b",
    ]) {
        conn.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES ('pub-feed-publisher-variant', ?1, 'music', ?2, '', 'podcast_remote_item')",
            rusqlite::params![pos, child],
        )
        .expect("publisher→child");
    }

    for child in [
        "child-feed-publisher-variant-a",
        "child-feed-publisher-variant-b",
    ] {
        conn.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES (?1, 0, 'publisher', 'pub-feed-publisher-variant', '', 'podcast_remote_item')",
            rusqlite::params![child],
        )
        .expect("child→publisher");
    }

    let stats = stophammer::db::resolve_artist_identity_for_feed(
        &mut conn,
        "child-feed-publisher-variant-a",
    )
    .expect("resolve artist identity");
    assert_eq!(
        stats.merges_applied, 0,
        "publisher-family name variants should raise review, not auto-merge"
    );
    assert_eq!(
        stats.pending_reviews, 1,
        "publisher-family name variants should create one pending review"
    );

    let reviews = stophammer::db::list_artist_identity_reviews_for_feed(
        &conn,
        "child-feed-publisher-variant-a",
    )
    .expect("list reviews");
    let review = reviews
        .iter()
        .find(|review| review.source == "publisher_name_variant")
        .expect("publisher_name_variant review");
    assert_eq!(review.name_key, "heycitizen");
    assert_eq!(review.evidence_key, "pub-feed-publisher-variant");
    let review_artist_ids = review
        .artist_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let expected_artist_ids = [first_artist_id, second_artist_id]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        review_artist_ids, expected_artist_ids,
        "review should include both child artists from the same publisher family"
    );
}

#[test]
fn likely_same_artist_can_combine_publisher_family_with_track_variant() {
    let mut conn = common::test_db();
    let now = common::now();

    let (_, _) = seed_feed_for_publisher(
        &conn,
        "pub-feed-likely-publisher",
        "https://publisher.example.com/artist/pub-feed-likely",
        "Publisher Curator",
        "publisher",
    );
    let (canonical_artist_id, canonical_credit_id) = seed_feed_for_publisher(
        &conn,
        "child-feed-likely-publisher-a",
        "https://music.example.com/heycitizen-likely-a.xml",
        "HeyCitizen",
        "music",
    );
    let (publisher_variant_artist_id, _) = seed_feed_for_publisher(
        &conn,
        "child-feed-likely-publisher-b",
        "https://music.example.com/heycitizen-likely-b.xml",
        "Hey Citizen",
        "music",
    );

    for (pos, child) in (0i64..).zip([
        "child-feed-likely-publisher-a",
        "child-feed-likely-publisher-b",
    ]) {
        conn.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES ('pub-feed-likely-publisher', ?1, 'music', ?2, '', 'podcast_remote_item')",
            rusqlite::params![pos, child],
        )
        .expect("publisher→child");
    }

    for child in [
        "child-feed-likely-publisher-a",
        "child-feed-likely-publisher-b",
    ] {
        conn.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES (?1, 0, 'publisher', 'pub-feed-likely-publisher', '', 'podcast_remote_item')",
            rusqlite::params![child],
        )
        .expect("child→publisher");
    }

    let track_variant_artist = stophammer::db::resolve_artist(
        &conn,
        "Hey Citizen",
        Some("child-feed-likely-publisher-a"),
    )
    .expect("track variant artist");
    let track_variant_credit = stophammer::db::get_or_create_artist_credit(
        &conn,
        &track_variant_artist.name,
        &[(
            track_variant_artist.artist_id.clone(),
            track_variant_artist.name.clone(),
            String::new(),
        )],
        Some("child-feed-likely-publisher-a"),
    )
    .expect("track variant credit");
    conn.execute(
        "UPDATE feeds SET artist_credit_id = ?1 WHERE feed_guid = 'child-feed-likely-publisher-a'",
        rusqlite::params![canonical_credit_id],
    )
    .expect("reassert canonical feed credit");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, explicit, created_at, updated_at) \
         VALUES ('track-likely-publisher', 'child-feed-likely-publisher-a', ?1, 'Variant Track', 'variant track', 0, ?2, ?2)",
        rusqlite::params![track_variant_credit.id, now],
    )
    .expect("insert variant track");

    let stats = stophammer::db::resolve_artist_identity_for_feed(
        &mut conn,
        "child-feed-likely-publisher-a",
    )
    .expect("resolve artist identity");
    assert_eq!(stats.merges_applied, 0, "scored review should remain review-only");
    assert!(
        stats.pending_reviews >= 2,
        "publisher-family plus track variant evidence should surface multiple review items"
    );

    let reviews = stophammer::db::list_artist_identity_reviews_for_feed(
        &conn,
        "child-feed-likely-publisher-a",
    )
    .expect("list reviews");
    let likely_review = reviews
        .iter()
        .find(|review| review.source == "likely_same_artist")
        .expect("likely_same_artist review");
    assert_eq!(likely_review.confidence, "high_confidence");
    assert_eq!(likely_review.score, Some(50));
    let supporting = likely_review
        .supporting_sources
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        supporting.contains("track_feed_name_variant"),
        "track/feed disagreement should contribute to likely_same_artist"
    );
    assert!(
        supporting.contains("publisher_name_variant"),
        "publisher-family name variant should contribute to likely_same_artist"
    );
    let review_artist_ids = likely_review
        .artist_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let expected_artist_ids = [
        canonical_artist_id,
        publisher_variant_artist_id,
        track_variant_artist.artist_id,
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        review_artist_ids, expected_artist_ids,
        "likely_same_artist should union the relevant same-name artist rows across track and publisher-family evidence"
    );
}
