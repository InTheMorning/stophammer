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
        &[],
    )
    .expect("resolve via npub");

    assert_eq!(resolved.artist_id, existing.artist_id);
}

#[test]
fn feed_artist_resolution_reuses_existing_artist_by_publisher_guid() {
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

    let remote_items = vec![stophammer::model::FeedRemoteItemRaw {
        id: None,
        feed_guid: "feed-new".into(),
        position: 0,
        medium: Some("publisher".into()),
        remote_feed_guid: "publisher-guid-1".into(),
        remote_feed_url: Some("https://wavlake.com/publisher-artist".into()),
        source: "podcast_remote_item".into(),
    }];

    let resolved = stophammer::db::resolve_feed_artist_from_source_claims(
        &conn,
        "Publisher Artist",
        "feed-new",
        &[],
        &remote_items,
        &[],
    )
    .expect("resolve via publisher guid");

    assert_eq!(resolved.artist_id, existing.artist_id);
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
