// Issue-STALE-TRACKS — 2026-03-14
//
// Tests for stale track removal during feed re-ingest.
// When a feed is re-crawled, tracks that were in the previous crawl but are
// missing from the new one must be deleted and produce TrackRemoved events.

mod common;

use rusqlite::params;

// ---------------------------------------------------------------------------
// Helper: builds model objects and calls ingest_transaction for a feed with
// the given track GUIDs. Returns the seqs produced by the transaction.
// ---------------------------------------------------------------------------

#[expect(
    clippy::too_many_lines,
    reason = "test helper building full model objects for ingest_transaction"
)]
fn ingest_feed_with_tracks(
    conn: &mut rusqlite::Connection,
    feed_guid: &str,
    track_guids: &[&str],
    signer: &stophammer::signing::NodeSigner,
) -> Vec<(i64, String, String)> {
    let now = common::now();

    let artist = stophammer::model::Artist {
        artist_id: format!("art-{feed_guid}"),
        name: "Test Artist".into(),
        name_lower: "test artist".into(),
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

    let artist_credit = stophammer::model::ArtistCredit {
        id: 0,
        display_name: "Test Artist".into(),
        feed_guid: None,
        created_at: now,
        names: vec![stophammer::model::ArtistCreditName {
            id: 0,
            artist_credit_id: 0,
            artist_id: format!("art-{feed_guid}"),
            position: 0,
            name: "Test Artist".into(),
            join_phrase: String::new(),
        }],
    };

    #[expect(
        clippy::cast_possible_wrap,
        reason = "test: track counts never approach i64::MAX"
    )]
    let feed = stophammer::model::Feed {
        feed_guid: feed_guid.into(),
        feed_url: format!("https://example.com/{feed_guid}.xml"),
        title: "Test Feed".into(),
        title_lower: "test feed".into(),
        artist_credit_id: 0,
        description: Some("Test feed description".into()),
        image_url: None,
        publisher: None,
        language: Some("en".into()),
        explicit: false,
        itunes_type: None,
        release_artist: None,
        release_artist_sort: None,
        release_date: None,
        release_kind: None,
        episode_count: track_guids.len() as i64,
        newest_item_at: Some(now),
        oldest_item_at: None,
        created_at: now,
        updated_at: now,
        raw_medium: Some("music".into()),
    };

    let tracks: Vec<(
        stophammer::model::Track,
        Vec<stophammer::model::PaymentRoute>,
        Vec<stophammer::model::ValueTimeSplit>,
    )> = track_guids
        .iter()
        .enumerate()
        .map(|(i, tg)| {
            let track = stophammer::model::Track {
                track_guid: (*tg).into(),
                feed_guid: feed_guid.into(),
                artist_credit_id: 0,
                title: format!("Track {i}"),
                title_lower: format!("track {i}"),
                pub_date: Some(now),
                duration_secs: Some(180),
                enclosure_url: Some(format!("https://cdn.example.com/{tg}.mp3")),
                enclosure_type: Some("audio/mpeg".into()),
                enclosure_bytes: Some(3_000_000),
                track_number: Some(i64::try_from(i + 1).unwrap()),
                season: None,
                image_url: None,
                publisher: None,
                language: None,
                explicit: false,
                description: Some(format!("Description for track {i}")),
                track_artist: None,
                track_artist_sort: None,
                created_at: now,
                updated_at: now,
            };
            (track, vec![], vec![])
        })
        .collect();

    // Build minimal event rows: one FeedUpserted + one TrackUpserted per track
    let mut event_rows = vec![stophammer::db::EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: stophammer::event::EventType::FeedUpserted,
        payload_json: "{}".into(),
        subject_guid: feed_guid.into(),
        created_at: now,
        warnings: vec![],
    }];

    for tg in track_guids {
        event_rows.push(stophammer::db::EventRow {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: stophammer::event::EventType::TrackUpserted,
            payload_json: "{}".into(),
            subject_guid: (*tg).into(),
            created_at: now,
            warnings: vec![],
        });
    }

    stophammer::db::ingest_transaction(
        conn,
        artist,
        artist_credit,
        feed,
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        tracks,
        event_rows,
        signer,
    )
    .expect("ingest_transaction should succeed")
}

// ---------------------------------------------------------------------------
// Test 1: Re-ingest with one track removed -> that track is deleted and a
// TrackRemoved event is emitted.
// ---------------------------------------------------------------------------

#[test]
fn reingest_removes_stale_track() {
    let mut conn = common::test_db();
    let signer = common::temp_signer("stale-track-test-1");

    // First ingest: 3 tracks
    let _ = ingest_feed_with_tracks(
        &mut conn,
        "feed-stale-1",
        &["track-a", "track-b", "track-c"],
        &signer,
    );

    // Verify all 3 tracks exist
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE feed_guid = 'feed-stale-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 3, "should have 3 tracks after first ingest");

    // Re-ingest: only 2 tracks (track-b removed)
    let _ = ingest_feed_with_tracks(&mut conn, "feed-stale-1", &["track-a", "track-c"], &signer);

    // track-b should no longer exist
    let track_b_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM tracks WHERE track_guid = 'track-b'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !track_b_exists,
        "track-b should have been removed on re-ingest"
    );

    // Remaining tracks should still exist
    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE feed_guid = 'feed-stale-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 2, "should have 2 tracks after re-ingest");

    // A TrackRemoved event should have been emitted for track-b
    let removal_event: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM events WHERE event_type = 'track_removed' AND subject_guid = 'track-b'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        removal_event,
        "a TrackRemoved event should be emitted for track-b"
    );

    // episode_count should be 2
    let episode_count: i64 = conn
        .query_row(
            "SELECT episode_count FROM feeds WHERE feed_guid = 'feed-stale-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        episode_count, 2,
        "episode_count should reflect actual track count"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Re-ingest with 0 tracks -> all previous tracks removed.
// ---------------------------------------------------------------------------

#[test]
fn reingest_removes_all_tracks() {
    let mut conn = common::test_db();
    let signer = common::temp_signer("stale-track-test-2");

    // First ingest: 3 tracks
    let _ = ingest_feed_with_tracks(
        &mut conn,
        "feed-stale-2",
        &["track-d", "track-e", "track-f"],
        &signer,
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE feed_guid = 'feed-stale-2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 3);

    // Re-ingest: 0 tracks
    let _ = ingest_feed_with_tracks(&mut conn, "feed-stale-2", &[], &signer);

    // All tracks should be gone
    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE feed_guid = 'feed-stale-2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        remaining, 0,
        "all tracks should be removed on re-ingest with 0 tracks"
    );

    // 3 TrackRemoved events should exist
    let removal_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'track_removed' \
             AND subject_guid IN ('track-d', 'track-e', 'track-f')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(removal_count, 3, "3 TrackRemoved events should be emitted");

    // episode_count should be 0
    let episode_count: i64 = conn
        .query_row(
            "SELECT episode_count FROM feeds WHERE feed_guid = 'feed-stale-2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        episode_count, 0,
        "episode_count should be 0 when all tracks removed"
    );
}

#[test]
fn reingest_replaces_feed_routes_with_wallet_maps() {
    let mut conn = common::test_db();
    let signer = common::temp_signer("stale-track-wallet-feed-route");
    let now = common::now();

    let _ = ingest_feed_with_tracks(&mut conn, "feed-wallet-routes", &["track-w1"], &signer);

    conn.execute(
        "INSERT INTO feed_payment_routes \
         (feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
         VALUES (?1, ?2, 'keysend', ?3, '', '', 100, 0)",
        params!["feed-wallet-routes", "Alice", "alice-node"],
    )
    .unwrap();
    let route_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO wallet_endpoints \
         (route_type, normalized_address, custom_key, custom_value, wallet_id, created_at) \
         VALUES ('keysend', ?1, '', '', NULL, ?2)",
        params!["alice-node", now],
    )
    .unwrap();
    let endpoint_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO wallet_feed_route_map (route_id, endpoint_id, created_at) \
         VALUES (?1, ?2, ?3)",
        params![route_id, endpoint_id, now],
    )
    .unwrap();

    let _: Vec<(i64, String, String)> =
        ingest_feed_with_tracks(&mut conn, "feed-wallet-routes", &["track-w1"], &signer);

    let old_route_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feed_payment_routes WHERE id = ?1",
            params![route_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old_route_exists, 0, "old feed route should be replaced");

    let old_map_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wallet_feed_route_map WHERE route_id = ?1",
            params![route_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        old_map_exists, 0,
        "wallet_feed_route_map should be cleared before deleting feed routes"
    );
}

#[test]
fn reingest_removes_stale_track_with_wallet_maps() {
    let mut conn = common::test_db();
    let signer = common::temp_signer("stale-track-wallet-track-route");
    let now = common::now();

    let _ = ingest_feed_with_tracks(
        &mut conn,
        "feed-wallet-track",
        &["track-wa", "track-wb"],
        &signer,
    );

    conn.execute(
        "INSERT INTO payment_routes \
         (track_guid, feed_guid, recipient_name, route_type, address, custom_key, custom_value, split, fee) \
         VALUES (?1, ?2, ?3, 'keysend', ?4, '', '', 100, 0)",
        params!["track-wb", "feed-wallet-track", "Alice", "alice-node-track"],
    )
    .unwrap();
    let route_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO wallet_endpoints \
         (route_type, normalized_address, custom_key, custom_value, wallet_id, created_at) \
         VALUES ('keysend', ?1, '', '', NULL, ?2)",
        params!["alice-node-track", now],
    )
    .unwrap();
    let endpoint_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO wallet_track_route_map (route_id, endpoint_id, created_at) \
         VALUES (?1, ?2, ?3)",
        params![route_id, endpoint_id, now],
    )
    .unwrap();

    let _: Vec<(i64, String, String)> =
        ingest_feed_with_tracks(&mut conn, "feed-wallet-track", &["track-wa"], &signer);

    let removed_track_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE track_guid = 'track-wb'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(removed_track_exists, 0, "stale track should be removed");

    let old_map_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wallet_track_route_map WHERE route_id = ?1",
            params![route_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        old_map_exists, 0,
        "wallet_track_route_map should be cleared before deleting track routes"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Re-ingest with same tracks -> no removals, no extra events.
// ---------------------------------------------------------------------------

#[test]
fn reingest_same_tracks_no_removals() {
    let mut conn = common::test_db();
    let signer = common::temp_signer("stale-track-test-3");

    // First ingest: 2 tracks
    let _ = ingest_feed_with_tracks(&mut conn, "feed-stale-3", &["track-g", "track-h"], &signer);

    let _events_before: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap();

    // Re-ingest: same 2 tracks
    let _ = ingest_feed_with_tracks(&mut conn, "feed-stale-3", &["track-g", "track-h"], &signer);

    // No TrackRemoved events should exist
    let removal_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'track_removed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        removal_count, 0,
        "no TrackRemoved events when same tracks re-ingested"
    );

    // Both tracks should still exist
    let track_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE feed_guid = 'feed-stale-3'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(track_count, 2, "both tracks should still exist");

    // episode_count should still be 2
    let episode_count: i64 = conn
        .query_row(
            "SELECT episode_count FROM feeds WHERE feed_guid = 'feed-stale-3'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(episode_count, 2, "episode_count should still be 2");
}

// ---------------------------------------------------------------------------
// Test 4: Removed track's child rows (routes, quality, search) are cleaned up.
// ---------------------------------------------------------------------------

#[test]
fn reingest_cleans_up_child_rows() {
    let mut conn = common::test_db();
    let signer = common::temp_signer("stale-track-test-4");

    // First ingest: 2 tracks
    let _ = ingest_feed_with_tracks(&mut conn, "feed-stale-4", &["track-i", "track-j"], &signer);
    stophammer::db::sync_source_read_models_for_feed(&conn, "feed-stale-4")
        .expect("sync source read models after first ingest");

    // Verify quality + search rows exist for track-j
    let quality_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM entity_quality WHERE entity_type = 'track' AND entity_id = 'track-j'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        quality_exists,
        "quality row should exist for track-j after first ingest"
    );

    let search_rowid = stophammer::search::rowid_for("track", "track-j");
    let search_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM search_entities WHERE rowid = ?1",
            params![search_rowid],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        search_exists,
        "search index should exist for track-j after first ingest"
    );

    // Re-ingest: only track-i (track-j removed)
    let _ = ingest_feed_with_tracks(&mut conn, "feed-stale-4", &["track-i"], &signer);

    // track-j quality row should be cleaned up
    let quality_after: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM entity_quality WHERE entity_type = 'track' AND entity_id = 'track-j'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !quality_after,
        "quality row for track-j should be cleaned up after removal"
    );

    // track-j search index should be cleaned up
    let search_after: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM search_entities WHERE rowid = ?1",
            params![search_rowid],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !search_after,
        "search index for track-j should be cleaned up after removal"
    );

    // track-i should still have its quality and search rows
    let quality_i: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM entity_quality WHERE entity_type = 'track' AND entity_id = 'track-i'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(quality_i, "quality row for track-i should still exist");
}
