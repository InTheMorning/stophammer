// Sprint 1B — Issue #17 & Issue #5 atomicity tests — 2026-03-13

#![recursion_limit = "256"]
#![expect(clippy::significant_drop_tightening, reason = "MutexGuard<Connection> must be held for the full scope in test assertions")]

mod common;

use rusqlite::params;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Helpers: build events for apply_single_event
// ---------------------------------------------------------------------------

fn make_artist_event(
    artist_id: &str,
    name: &str,
    now: i64,
) -> stophammer::event::Event {
    let inner = stophammer::event::ArtistUpsertedPayload {
        artist: stophammer::model::Artist {
            artist_id:  artist_id.into(),
            name:       name.into(),
            name_lower: name.to_lowercase(),
            sort_name:  None,
            type_id:    None,
            area:       None,
            img_url:    None,
            url:        None,
            begin_year: None,
            end_year:   None,
            created_at: now,
            updated_at: now,
        },
    };
    // payload_json must contain only the inner struct (not the tagged enum),
    // matching production format where the DB stores the inner payload.
    let payload_json = serde_json::to_string(&inner).expect("serialize inner");
    let payload = stophammer::event::EventPayload::ArtistUpserted(inner);
    stophammer::event::Event {
        event_id:     format!("evt-{artist_id}"),
        event_type:   stophammer::event::EventType::ArtistUpserted,
        payload,
        subject_guid: artist_id.into(),
        signed_by:    "deadbeef".into(),
        signature:    "cafebabe".into(),
        seq:          1,
        created_at:   now,
        warnings:     vec![],
        payload_json,
    }
}

// ---------------------------------------------------------------------------
// Issue #17 — Test 1: apply_single_event is atomic (all-or-nothing)
//
// Strategy: Verify that after a successful apply_single_event, both the entity
// row AND the event row AND the search index entry AND the quality score all
// exist. This confirms they were written together atomically.
// ---------------------------------------------------------------------------

#[test]
fn apply_single_event_writes_entity_event_search_quality_atomically() {
    let db: Arc<Mutex<rusqlite::Connection>> = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let now = common::now();

    let ev = make_artist_event("atom-artist-1", "Atomic Artist", now);

    let result = stophammer::apply::apply_single_event(&pool, &ev);
    assert!(result.is_ok(), "apply should succeed");

    // All four artifacts must exist: entity, event, search index, quality score
    let conn = db.lock().expect("lock");

    // 1. Artist row
    let artist_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM artists WHERE artist_id = 'atom-artist-1'",
            [],
            |r| r.get(0),
        )
        .expect("artist query");
    assert!(artist_exists, "artist row must exist after atomic apply");

    // 2. Event row
    let event_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM events WHERE event_id = 'evt-atom-artist-1'",
            [],
            |r| r.get(0),
        )
        .expect("event query");
    assert!(event_exists, "event row must exist after atomic apply");

    // 3. Search index entry
    let rowid = stophammer::search::rowid_for("artist", "atom-artist-1");
    let search_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM search_index WHERE rowid = ?1",
            params![rowid],
            |r| r.get(0),
        )
        .expect("search query");
    assert!(search_exists, "search index entry must exist after atomic apply");

    // 4. Quality score
    let quality_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM entity_quality WHERE entity_type = 'artist' AND entity_id = 'atom-artist-1'",
            [],
            |r| r.get(0),
        )
        .expect("quality query");
    assert!(quality_exists, "quality score must exist after atomic apply");
}

// ---------------------------------------------------------------------------
// Issue #17 — Test 2: apply_single_event rolls back on failure
//
// Strategy: We verify that if the event insert step would fail (e.g., due to
// a constraint), none of the preceding writes (entity, search, quality)
// persist. We do this by calling apply_single_event twice with the same
// event_id but different payloads — but apply uses INSERT OR IGNORE for
// events, so duplicates don't fail.
//
// Instead, we verify atomicity by corrupting a table that is written AFTER
// the entity upsert. We drop the events table temporarily, call apply, and
// verify the artist was NOT inserted either (proving the transaction rolled
// back).
// ---------------------------------------------------------------------------

#[test]
fn apply_single_event_rolls_back_entity_on_event_insert_failure() {
    let db: Arc<Mutex<rusqlite::Connection>> = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let now = common::now();

    // Corrupt the events table by renaming it so INSERT fails
    {
        let conn = db.lock().expect("lock");
        conn.execute_batch("ALTER TABLE events RENAME TO events_backup")
            .expect("rename events table");
    }

    let ev = make_artist_event("rollback-artist-1", "Rollback Artist", now);
    let result = stophammer::apply::apply_single_event(&pool, &ev);

    // The apply should fail because the events table is missing
    assert!(result.is_err(), "apply should fail when events table is missing");

    // Restore the events table
    {
        let conn = db.lock().expect("lock");
        conn.execute_batch("ALTER TABLE events_backup RENAME TO events")
            .expect("restore events table");
    }

    // Verify the artist was NOT inserted (transaction rolled back)
    let conn = db.lock().expect("lock");
    let artist_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM artists WHERE artist_id = 'rollback-artist-1'",
            [],
            |r| r.get(0),
        )
        .expect("artist query");
    assert!(
        !artist_exists,
        "artist should NOT exist when event insert failed — transaction must roll back"
    );

    // Verify the search index was NOT populated
    let rowid = stophammer::search::rowid_for("artist", "rollback-artist-1");
    let search_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM search_index WHERE rowid = ?1",
            params![rowid],
            |r| r.get(0),
        )
        .expect("search query");
    assert!(
        !search_exists,
        "search index should NOT contain entry when transaction rolled back"
    );

    // Verify the quality score was NOT stored
    let quality_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM entity_quality WHERE entity_type = 'artist' AND entity_id = 'rollback-artist-1'",
            [],
            |r| r.get(0),
        )
        .expect("quality query");
    assert!(
        !quality_exists,
        "quality score should NOT exist when transaction rolled back"
    );
}

// ---------------------------------------------------------------------------
// Issue #17 — Test 3: apply_single_event search index failure rolls back entity
//
// Strategy: Drop the search_index table, attempt apply, verify entity is not
// persisted either.
// ---------------------------------------------------------------------------

#[test]
fn apply_single_event_rolls_back_entity_on_search_failure() {
    let db: Arc<Mutex<rusqlite::Connection>> = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let now = common::now();

    // Drop the search_index FTS5 table so search writes fail
    {
        let conn = db.lock().expect("lock");
        conn.execute_batch("DROP TABLE IF EXISTS search_index")
            .expect("drop search_index");
    }

    let ev = make_artist_event("search-fail-artist", "Search Fail Artist", now);
    let result = stophammer::apply::apply_single_event(&pool, &ev);

    // The apply should fail because the search_index table is missing
    assert!(result.is_err(), "apply should fail when search_index is missing");

    // Rebuild the search_index table for cleanup (not needed for assertion)
    // The key assertion: the artist must NOT have been persisted
    let conn = db.lock().expect("lock");
    let artist_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM artists WHERE artist_id = 'search-fail-artist'",
            [],
            |r| r.get(0),
        )
        .expect("artist query");
    assert!(
        !artist_exists,
        "artist should NOT exist when search index write failed — transaction must roll back"
    );
}

// ---------------------------------------------------------------------------
// Issue #5 — Test 4: ingest_transaction includes search+quality atomically
//
// Strategy: Perform a full ingest via the API handler (using the E2E
// pattern from TC-05), then verify that search index AND quality scores
// are populated. This confirms they are part of the same transaction.
// ---------------------------------------------------------------------------

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "E2E ingest atomicity test")]
async fn ingest_search_quality_atomic_with_ingest_transaction() {
    use axum::body::Body;
    use http::Request;
    use http_body_util::BodyExt;
    use std::collections::HashMap;
    use std::sync::RwLock;
    use tower::ServiceExt;

    let crawl_token = "sprint1b-crawl-token";
    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-sprint1b-signer.key")
            .expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: vec!["crawl_token".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());

    let state = Arc::new(stophammer::api::AppState {
        db:               stophammer::db_pool::DbPool::from_writer_only(Arc::clone(&db)),
        chain:            Arc::new(chain),
        signer,
        node_pubkey_hex:  pubkey,
        admin_token:      "test-admin".into(),
        sync_token:      None,
        push_client:      reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry:     Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    });
    let app = stophammer::api::build_router(state);

    let feed_guid = "feed-s1b-atomic";
    let track_guid = "track-s1b-atomic-01";

    let ingest_payload = serde_json::json!({
        "canonical_url": "https://example.com/s1b-atomic.xml",
        "source_url": "https://example.com/s1b-atomic.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "s1b-hash-unique-001",
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "Sprint1B Atomic Album",
            "description": "Testing ingest atomicity",
            "image_url": "https://img.example.com/s1b.jpg",
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "music",
            "author_name": "S1B Artist",
            "owner_name": "S1B Artist",
            "pub_date": null,
            "feed_payment_routes": [{
                "recipient_name": "S1B Artist",
                "route_type": "node",
                "address": "03s1bfeedrouteaddress",
                "custom_key": null,
                "custom_value": null,
                "split": 95,
                "fee": false
            }],
            "tracks": [{
                "track_guid": track_guid,
                "title": "S1B Atomic Track",
                "pub_date": 1_700_000_000,
                "duration_secs": 240,
                "enclosure_url": "https://cdn.example.com/s1b-track.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 5_000_000,
                "track_number": 1,
                "season": null,
                "explicit": false,
                "description": "First track for atomicity test",
                "author_name": null,
                "payment_routes": [{
                    "recipient_name": "S1B Artist",
                    "route_type": "node",
                    "address": "03s1btrackrouteaddress",
                    "custom_key": "7629169",
                    "custom_value": "podcast-s1b",
                    "split": 95,
                    "fee": false
                }],
                "value_time_splits": []
            }]
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/ingest/feed")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&ingest_payload).expect("serialize")))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("ingest should not panic");
    let status = resp.status().as_u16();
    let raw_bytes = resp.into_body().collect().await.expect("read body").to_bytes();
    let raw_text = String::from_utf8_lossy(&raw_bytes);
    assert!(
        status == 200 || status == 207,
        "ingest should return 200 or 207, got {status}: {raw_text}"
    );

    // Verify ALL artifacts are present atomically:
    let conn = db.lock().expect("lock");

    // 1. Feed exists
    let feed_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM feeds WHERE feed_guid = ?1",
            params![feed_guid],
            |r| r.get(0),
        )
        .expect("feed query");
    assert!(feed_exists, "feed must exist after ingest");

    // 2. Feed search index
    let feed_rowid = stophammer::search::rowid_for("feed", feed_guid);
    let feed_search: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM search_index WHERE rowid = ?1",
            params![feed_rowid],
            |r| r.get(0),
        )
        .expect("feed search query");
    assert!(feed_search, "feed search index must be populated atomically with ingest");

    // 3. Feed quality score
    let feed_quality: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM entity_quality WHERE entity_type = 'feed' AND entity_id = ?1",
            params![feed_guid],
            |r| r.get(0),
        )
        .expect("feed quality query");
    assert!(feed_quality, "feed quality score must be stored atomically with ingest");

    // 4. Track search index
    let track_rowid = stophammer::search::rowid_for("track", track_guid);
    let track_search: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM search_index WHERE rowid = ?1",
            params![track_rowid],
            |r| r.get(0),
        )
        .expect("track search query");
    assert!(track_search, "track search index must be populated atomically with ingest");

    // 5. Track quality score
    let track_quality: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM entity_quality WHERE entity_type = 'track' AND entity_id = ?1",
            params![track_guid],
            |r| r.get(0),
        )
        .expect("track quality query");
    assert!(track_quality, "track quality score must be stored atomically with ingest");

    // 6. Crawl cache
    let crawl_cached: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM feed_crawl_cache WHERE feed_url = 'https://example.com/s1b-atomic.xml'",
            [],
            |r| r.get(0),
        )
        .expect("crawl cache query");
    assert!(crawl_cached, "crawl cache must be written atomically with ingest");
}

// ---------------------------------------------------------------------------
// Issue #5 — Test 5: ingest_transaction writes search index + quality
//
// Strategy: Call ingest_transaction successfully and verify that search index
// and quality score data are written as part of the same transaction (not
// as a separate post-commit step in api.rs).
//
// Before the fix, ingest_transaction does NOT write search/quality data.
// After the fix, it does — making the writes atomic with the entity data.
// ---------------------------------------------------------------------------

#[test]
#[expect(clippy::too_many_lines, reason = "integration test sets up full ingest data and verifies all search/quality artifacts")]
fn ingest_transaction_writes_search_and_quality_atomically() {
    let mut conn = common::test_db();
    let now = common::now();

    let artist = stophammer::model::Artist {
        artist_id:  "art-atomic-s1b".into(),
        name:       "Atomic S1B Artist".into(),
        name_lower: "atomic s1b artist".into(),
        sort_name:  None,
        type_id:    None,
        area:       None,
        img_url:    None,
        url:        None,
        begin_year: None,
        end_year:   None,
        created_at: now,
        updated_at: now,
    };

    let artist_credit = stophammer::model::ArtistCredit {
        id:           0,
        display_name: "Atomic S1B Artist".into(),
        feed_guid:    None,
        created_at:   now,
        names:        vec![stophammer::model::ArtistCreditName {
            id:               0,
            artist_credit_id: 0,
            artist_id:        "art-atomic-s1b".into(),
            position:         0,
            name:             "Atomic S1B Artist".into(),
            join_phrase:      String::new(),
        }],
    };

    let feed = stophammer::model::Feed {
        feed_guid:        "feed-atomic-s1b".into(),
        feed_url:         "https://example.com/atomic.xml".into(),
        title:            "Atomic S1B Album".into(),
        title_lower:      "atomic s1b album".into(),
        artist_credit_id: 0,
        description:      Some("Test atomicity".into()),
        image_url:        Some("https://img.example.com/at.jpg".into()),
        language:         Some("en".into()),
        explicit:         false,
        itunes_type:      None,
        episode_count:    1,
        newest_item_at:   Some(now),
        oldest_item_at:   None,
        created_at:       now,
        updated_at:       now,
        raw_medium:       Some("music".into()),
    };

    let track = stophammer::model::Track {
        track_guid:       "track-atomic-s1b-01".into(),
        feed_guid:        "feed-atomic-s1b".into(),
        artist_credit_id: 0,
        title:            "Atomic Track One".into(),
        title_lower:      "atomic track one".into(),
        pub_date:         Some(now),
        duration_secs:    Some(240),
        enclosure_url:    Some("https://cdn.example.com/at-01.mp3".into()),
        enclosure_type:   Some("audio/mpeg".into()),
        enclosure_bytes:  Some(5_000_000),
        track_number:     Some(1),
        season:           None,
        explicit:         false,
        description:      Some("First atomic track".into()),
        created_at:       now,
        updated_at:       now,
    };

    let route = stophammer::model::PaymentRoute {
        id:              None,
        track_guid:      "track-atomic-s1b-01".into(),
        feed_guid:       "feed-atomic-s1b".into(),
        recipient_name:  Some("Atomic Artist".into()),
        route_type:      stophammer::model::RouteType::Node,
        address:         "03atomicrouteaddress".into(),
        custom_key:      None,
        custom_value:    None,
        split:           95,
        fee:             false,
    };

    // Issue-SEQ-INTEGRITY — 2026-03-14: pass signer, EventRow no longer has signed_by/signature.
    let signer = stophammer::signing::NodeSigner::load_or_create("/tmp/sprint1b-atom-test.key")
        .expect("signer");
    let result = stophammer::db::ingest_transaction(
        &mut conn,
        artist,
        artist_credit,
        feed,
        vec![],
        vec![(track, vec![route], vec![])],
        // Issue-WRITE-AMP — 2026-03-14: include TrackUpserted event so that
        // search/quality is computed for the track (diff-aware gating).
        vec![
            stophammer::db::EventRow {
                event_id:     "evt-atomic-s1b-1".into(),
                event_type:   stophammer::event::EventType::FeedUpserted,
                payload_json: "{}".into(),
                subject_guid: "feed-atomic-s1b".into(),
                created_at:   now,
                warnings:     vec![],
            },
            stophammer::db::EventRow {
                event_id:     "evt-atomic-s1b-2".into(),
                event_type:   stophammer::event::EventType::TrackUpserted,
                payload_json: "{}".into(),
                subject_guid: "track-atomic-s1b-01".into(),
                created_at:   now,
                warnings:     vec![],
            },
        ],
        &signer,
    );
    assert!(result.is_ok(), "ingest_transaction should succeed");

    // After the fix, ingest_transaction must write search index + quality
    // as part of the same transaction.

    // Feed search index must be populated
    let feed_rowid = stophammer::search::rowid_for("feed", "feed-atomic-s1b");
    let feed_search: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM search_index WHERE rowid = ?1",
            params![feed_rowid],
            |r| r.get(0),
        )
        .expect("feed search query");
    assert!(
        feed_search,
        "feed search index must be written by ingest_transaction (Issue #5 fix)"
    );

    // Feed quality score must be stored
    let feed_quality: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM entity_quality WHERE entity_type = 'feed' AND entity_id = 'feed-atomic-s1b'",
            [],
            |r| r.get(0),
        )
        .expect("feed quality query");
    assert!(
        feed_quality,
        "feed quality score must be written by ingest_transaction (Issue #5 fix)"
    );

    // Artist search index must be populated
    let artist_rowid = stophammer::search::rowid_for("artist", "art-atomic-s1b");
    let artist_search: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM search_index WHERE rowid = ?1",
            params![artist_rowid],
            |r| r.get(0),
        )
        .expect("artist search query");
    assert!(
        artist_search,
        "artist search index must be written by ingest_transaction (Issue #5 fix)"
    );

    // Artist quality score must be stored
    let artist_quality: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM entity_quality WHERE entity_type = 'artist' AND entity_id = 'art-atomic-s1b'",
            [],
            |r| r.get(0),
        )
        .expect("artist quality query");
    assert!(
        artist_quality,
        "artist quality score must be written by ingest_transaction (Issue #5 fix)"
    );

    // Track search index must be populated
    let track_rowid = stophammer::search::rowid_for("track", "track-atomic-s1b-01");
    let track_search: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM search_index WHERE rowid = ?1",
            params![track_rowid],
            |r| r.get(0),
        )
        .expect("track search query");
    assert!(
        track_search,
        "track search index must be written by ingest_transaction (Issue #5 fix)"
    );

    // Track quality score must be stored
    let track_quality: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM entity_quality WHERE entity_type = 'track' AND entity_id = 'track-atomic-s1b-01'",
            [],
            |r| r.get(0),
        )
        .expect("track quality query");
    assert!(
        track_quality,
        "track quality score must be written by ingest_transaction (Issue #5 fix)"
    );
}

// ---------------------------------------------------------------------------
// Issue #5 — Test 6: ingest_transaction rolls back search/quality on failure
//
// Strategy: Call ingest_transaction with a corrupted events table to force
// failure after entity writes. Verify that search index and quality data
// are also rolled back (because they are now inside the same transaction).
// ---------------------------------------------------------------------------

#[test]
#[expect(clippy::too_many_lines, reason = "integration test sets up full ingest data and verifies rollback of all artifacts")]
fn ingest_transaction_rolls_back_search_quality_on_failure() {
    let mut conn = common::test_db();
    let now = common::now();

    let artist = stophammer::model::Artist {
        artist_id:  "art-rollback-s1b".into(),
        name:       "Rollback S1B Artist".into(),
        name_lower: "rollback s1b artist".into(),
        sort_name:  None,
        type_id:    None,
        area:       None,
        img_url:    None,
        url:        None,
        begin_year: None,
        end_year:   None,
        created_at: now,
        updated_at: now,
    };

    let artist_credit = stophammer::model::ArtistCredit {
        id:           0,
        display_name: "Rollback S1B Artist".into(),
        feed_guid:    None,
        created_at:   now,
        names:        vec![stophammer::model::ArtistCreditName {
            id:               0,
            artist_credit_id: 0,
            artist_id:        "art-rollback-s1b".into(),
            position:         0,
            name:             "Rollback S1B Artist".into(),
            join_phrase:      String::new(),
        }],
    };

    let feed = stophammer::model::Feed {
        feed_guid:        "feed-rollback-s1b".into(),
        feed_url:         "https://example.com/rollback.xml".into(),
        title:            "Rollback S1B Album".into(),
        title_lower:      "rollback s1b album".into(),
        artist_credit_id: 0,
        description:      Some("Test rollback".into()),
        image_url:        Some("https://img.example.com/rb.jpg".into()),
        language:         Some("en".into()),
        explicit:         false,
        itunes_type:      None,
        episode_count:    0,
        newest_item_at:   None,
        oldest_item_at:   None,
        created_at:       now,
        updated_at:       now,
        raw_medium:       Some("music".into()),
    };

    // Drop the events table to force a failure during event insertion
    conn.execute_batch("ALTER TABLE events RENAME TO events_backup")
        .expect("rename events table");

    // Issue-SEQ-INTEGRITY — 2026-03-14: pass signer, EventRow no longer has signed_by/signature.
    let signer2 = stophammer::signing::NodeSigner::load_or_create("/tmp/sprint1b-atom-test2.key")
        .expect("signer");
    let result = stophammer::db::ingest_transaction(
        &mut conn,
        artist,
        artist_credit,
        feed,
        vec![],
        vec![],
        vec![stophammer::db::EventRow {
            event_id:     "evt-rollback-s1b".into(),
            event_type:   stophammer::event::EventType::ArtistUpserted,
            payload_json: "{}".into(),
            subject_guid: "art-rollback-s1b".into(),
            created_at:   now,
            warnings:     vec![],
        }],
        &signer2,
    );

    // Restore events table
    conn.execute_batch("ALTER TABLE events_backup RENAME TO events")
        .expect("restore events table");

    // The transaction should have failed
    assert!(result.is_err(), "ingest should fail when events table is missing");

    // Verify the feed was NOT inserted (transaction rolled back)
    let feed_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM feeds WHERE feed_guid = 'feed-rollback-s1b'",
            [],
            |r| r.get(0),
        )
        .expect("feed query");
    assert!(
        !feed_exists,
        "feed should NOT exist when event insert failed — transaction must roll back"
    );

    // Verify no search index entry exists
    let rowid = stophammer::search::rowid_for("feed", "feed-rollback-s1b");
    let search_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM search_index WHERE rowid = ?1",
            params![rowid],
            |r| r.get(0),
        )
        .expect("search query");
    assert!(
        !search_exists,
        "search index should NOT contain entry when transaction rolled back"
    );

    // Verify no quality score exists
    let quality_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM entity_quality WHERE entity_type = 'feed' AND entity_id = 'feed-rollback-s1b'",
            [],
            |r| r.get(0),
        )
        .expect("quality query");
    assert!(
        !quality_exists,
        "quality score should NOT exist when transaction rolled back"
    );
}
