// Issue-WRITE-AMP — 2026-03-14
//
// Tests for per-entity field-level diffing during re-ingest.
// When a feed is re-crawled, only entities whose fields actually changed
// should produce events. Unchanged tracks must NOT emit TrackUpserted.

mod common;

// ---------------------------------------------------------------------------
// Helper: builds model objects and calls ingest_transaction for a feed.
// Unlike the stale_track_removal helper, this one allows customising track
// titles so we can test field-level diffs, and it only emits events for
// entities that actually changed (using the new diff-aware event building).
// ---------------------------------------------------------------------------

/// Track descriptor used by the test helper.
struct TestTrack {
    guid: &'static str,
    title: &'static str,
}

/// Feed descriptor used by the test helper.
struct TestFeed {
    feed_guid: &'static str,
    title: &'static str,
    description: Option<&'static str>,
}

impl Default for TestFeed {
    fn default() -> Self {
        Self {
            feed_guid: "feed-wa-1",
            title: "Test Feed",
            description: Some("A test feed"),
        }
    }
}

/// Calls `ingest_transaction` directly, then queries the events table to
/// count how many `track_upserted` events were produced by this particular
/// call. Returns `(total_events, track_upserted_count)`.
#[expect(
    clippy::too_many_lines,
    reason = "test helper building full model objects"
)]
fn ingest_and_count_track_events(
    conn: &mut rusqlite::Connection,
    feed: &TestFeed,
    tracks: &[TestTrack],
    signer: &stophammer::signing::NodeSigner,
) -> (i64, i64) {
    // Snapshot event count before this ingest
    let events_before: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'track_upserted'",
            [],
            |r| r.get(0),
        )
        .expect("count events");

    let now = common::now();
    // Use a fixed pub_date for tracks so repeated ingests don't
    // trigger false-positive diffs due to timestamp drift.
    let fixed_pub_date: i64 = 1_700_000_000;

    let artist = stophammer::model::Artist {
        artist_id: format!("art-{}", feed.feed_guid),
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
            artist_id: format!("art-{}", feed.feed_guid),
            position: 0,
            name: "Test Artist".into(),
            join_phrase: String::new(),
        }],
    };

    #[expect(
        clippy::cast_possible_wrap,
        reason = "test: track counts never approach i64::MAX"
    )]
    let feed_model = stophammer::model::Feed {
        feed_guid: feed.feed_guid.into(),
        feed_url: format!("https://example.com/{}.xml", feed.feed_guid),
        title: feed.title.into(),
        title_lower: feed.title.to_lowercase(),
        artist_credit_id: 0,
        description: feed.description.map(String::from),
        image_url: None,
        language: Some("en".into()),
        explicit: false,
        itunes_type: None,
        episode_count: tracks.len() as i64,
        newest_item_at: Some(now),
        oldest_item_at: None,
        created_at: now,
        updated_at: now,
        raw_medium: Some("music".into()),
    };

    let track_tuples: Vec<(
        stophammer::model::Track,
        Vec<stophammer::model::PaymentRoute>,
        Vec<stophammer::model::ValueTimeSplit>,
    )> = tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let track = stophammer::model::Track {
                track_guid: t.guid.into(),
                feed_guid: feed.feed_guid.into(),
                artist_credit_id: 0,
                title: t.title.into(),
                title_lower: t.title.to_lowercase(),
                pub_date: Some(fixed_pub_date),
                duration_secs: Some(180),
                enclosure_url: Some(format!("https://cdn.example.com/{}.mp3", t.guid)),
                enclosure_type: Some("audio/mpeg".into()),
                enclosure_bytes: Some(3_000_000),
                track_number: Some(i64::try_from(i + 1).unwrap()),
                season: None,
                explicit: false,
                description: Some(format!("Description for {}", t.guid)),
                created_at: now,
                updated_at: now,
            };
            (track, vec![], vec![])
        })
        .collect();

    // Issue-WRITE-AMP — 2026-03-14: use diff-aware event building.
    // Query existing state and only emit events for changed entities.
    let event_rows = stophammer::db::build_diff_events(
        conn,
        &artist,
        &artist_credit,
        &feed_model,
        &[], // no remote items
        &[], // no source contributor claims
        &[], // no source entity IDs
        &[], // no source links
        &[], // no source release claims
        &[], // no feed routes
        &[], // no live events
        &track_tuples,
        &[], // no track credits override — use the same one
        now,
        &[], // no warnings
    )
    .expect("build_diff_events");

    stophammer::db::ingest_transaction(
        conn,
        artist,
        artist_credit,
        feed_model,
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        track_tuples,
        event_rows,
        signer,
    )
    .expect("ingest_transaction should succeed");

    // Count track_upserted events emitted by this ingest
    let events_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'track_upserted'",
            [],
            |r| r.get(0),
        )
        .expect("count events");

    let total_events_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .expect("count total events");

    (total_events_after, events_after - events_before)
}

// ---------------------------------------------------------------------------
// Test 1: Re-ingest with all tracks unchanged → 0 TrackUpserted events
// ---------------------------------------------------------------------------

#[test]
fn reingest_unchanged_tracks_emits_zero_track_events() {
    let mut conn = common::test_db();
    let signer = stophammer::signing::NodeSigner::load_or_create("/tmp/write-amp-test-1.key")
        .expect("signer");

    let feed = TestFeed::default();
    let tracks = [
        TestTrack {
            guid: "wa-t1",
            title: "Song One",
        },
        TestTrack {
            guid: "wa-t2",
            title: "Song Two",
        },
        TestTrack {
            guid: "wa-t3",
            title: "Song Three",
        },
    ];

    // First ingest: all 3 tracks are new → 3 TrackUpserted
    let (_, first_track_events) = ingest_and_count_track_events(&mut conn, &feed, &tracks, &signer);
    assert_eq!(
        first_track_events, 3,
        "first ingest should emit 3 TrackUpserted events"
    );

    // Second ingest: identical data → 0 TrackUpserted
    let (_, second_track_events) =
        ingest_and_count_track_events(&mut conn, &feed, &tracks, &signer);
    assert_eq!(
        second_track_events, 0,
        "re-ingest with unchanged tracks should emit 0 TrackUpserted events"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Re-ingest with 1 track title changed → exactly 1 TrackUpserted
// ---------------------------------------------------------------------------

#[test]
fn reingest_one_changed_track_emits_one_event() {
    let mut conn = common::test_db();
    let signer = stophammer::signing::NodeSigner::load_or_create("/tmp/write-amp-test-2.key")
        .expect("signer");

    let feed = TestFeed {
        feed_guid: "feed-wa-2",
        ..TestFeed::default()
    };
    let tracks = [
        TestTrack {
            guid: "wa2-t1",
            title: "Song Alpha",
        },
        TestTrack {
            guid: "wa2-t2",
            title: "Song Beta",
        },
        TestTrack {
            guid: "wa2-t3",
            title: "Song Gamma",
        },
    ];

    // First ingest
    let _ = ingest_and_count_track_events(&mut conn, &feed, &tracks, &signer);

    // Second ingest: change title of track 2 only
    let tracks_v2 = [
        TestTrack {
            guid: "wa2-t1",
            title: "Song Alpha",
        },
        TestTrack {
            guid: "wa2-t2",
            title: "Song Beta REMIX",
        },
        TestTrack {
            guid: "wa2-t3",
            title: "Song Gamma",
        },
    ];

    let (_, track_events) = ingest_and_count_track_events(&mut conn, &feed, &tracks_v2, &signer);
    assert_eq!(
        track_events, 1,
        "re-ingest with 1 changed track title should emit exactly 1 TrackUpserted"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Re-ingest with 1 new track added → exactly 1 TrackUpserted
// ---------------------------------------------------------------------------

#[test]
fn reingest_one_new_track_emits_one_event() {
    let mut conn = common::test_db();
    let signer = stophammer::signing::NodeSigner::load_or_create("/tmp/write-amp-test-3.key")
        .expect("signer");

    let feed = TestFeed {
        feed_guid: "feed-wa-3",
        ..TestFeed::default()
    };
    let tracks = [
        TestTrack {
            guid: "wa3-t1",
            title: "Song One",
        },
        TestTrack {
            guid: "wa3-t2",
            title: "Song Two",
        },
    ];

    // First ingest
    let _ = ingest_and_count_track_events(&mut conn, &feed, &tracks, &signer);

    // Second ingest: add 1 new track, keep existing 2 unchanged
    let tracks_v2 = [
        TestTrack {
            guid: "wa3-t1",
            title: "Song One",
        },
        TestTrack {
            guid: "wa3-t2",
            title: "Song Two",
        },
        TestTrack {
            guid: "wa3-t3",
            title: "Song Three NEW",
        },
    ];

    let (_, track_events) = ingest_and_count_track_events(&mut conn, &feed, &tracks_v2, &signer);
    assert_eq!(
        track_events, 1,
        "re-ingest with 1 new track should emit exactly 1 TrackUpserted for the new track"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Feed-level fields unchanged → no FeedUpserted emitted on re-ingest
// ---------------------------------------------------------------------------

#[test]
fn reingest_unchanged_feed_emits_no_feed_event() {
    let mut conn = common::test_db();
    let signer = stophammer::signing::NodeSigner::load_or_create("/tmp/write-amp-test-4.key")
        .expect("signer");

    let feed = TestFeed {
        feed_guid: "feed-wa-4",
        ..TestFeed::default()
    };
    let tracks = [TestTrack {
        guid: "wa4-t1",
        title: "Song",
    }];

    // First ingest
    let _ = ingest_and_count_track_events(&mut conn, &feed, &tracks, &signer);

    let feed_events_before: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'feed_upserted'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    // Second ingest: same feed data
    let _ = ingest_and_count_track_events(&mut conn, &feed, &tracks, &signer);

    let feed_events_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'feed_upserted'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(
        feed_events_after - feed_events_before,
        0,
        "re-ingest with unchanged feed should emit 0 FeedUpserted events"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Feed title changes → FeedUpserted emitted
// ---------------------------------------------------------------------------

#[test]
fn reingest_changed_feed_title_emits_feed_event() {
    let mut conn = common::test_db();
    let signer = stophammer::signing::NodeSigner::load_or_create("/tmp/write-amp-test-5.key")
        .expect("signer");

    let feed = TestFeed {
        feed_guid: "feed-wa-5",
        title: "Original Title",
        ..TestFeed::default()
    };
    let tracks = [TestTrack {
        guid: "wa5-t1",
        title: "Song",
    }];

    // First ingest
    let _ = ingest_and_count_track_events(&mut conn, &feed, &tracks, &signer);

    let feed_events_before: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'feed_upserted'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    // Second ingest: changed title
    let feed_v2 = TestFeed {
        feed_guid: "feed-wa-5",
        title: "Updated Title",
        ..TestFeed::default()
    };
    let _ = ingest_and_count_track_events(&mut conn, &feed_v2, &tracks, &signer);

    let feed_events_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'feed_upserted'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(
        feed_events_after - feed_events_before,
        1,
        "re-ingest with changed feed title should emit exactly 1 FeedUpserted"
    );
}
