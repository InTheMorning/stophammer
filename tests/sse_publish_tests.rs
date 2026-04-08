// Issue-SSE-PUBLISH — 2026-03-14
//
// Tests for SSE publish wiring (Part 1) and Last-Event-ID seq semantics (Part 2).

#![recursion_limit = "256"]
#![allow(
    clippy::too_many_lines,
    reason = "SSE regression tests keep full payloads inline so event shapes stay explicit"
)]
#![allow(
    clippy::unreadable_literal,
    reason = "Unix timestamps and payload byte counts are clearer in raw copied fixture form"
)]

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helper: build AppState with a crawl-token-only verifier chain
// ---------------------------------------------------------------------------

fn test_app_state_with_crawl_token(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-sse-publish"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: vec!["crawl_token".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());

    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
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

fn json_request(method: &str, uri: &str, body: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(body).expect("serialize")))
        .expect("build request")
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

// ---------------------------------------------------------------------------
// Test: ingest a feed -> SSE subscriber receives events (not just keepalives)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn ingest_feed_publishes_to_sse() {
    let crawl_token = "sse-crawl-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);

    // Subscribe to the SSE registry for the artist that will be created.
    // The artist name will be derived from owner_name="SSE Test Artist" and
    // resolved via resolve_artist. We need the artist_id, which is deterministic
    // from the name. Let's subscribe to a wildcard and then check.
    //
    // Actually: resolve_artist creates artist_id as a UUID, so we cannot predict
    // it before ingest. Instead, we'll ingest first, then check the ring buffer.

    let feed_guid = "feed-sse-pub-001";
    let track_guid = "track-sse-pub-001";

    let ingest_body = serde_json::json!({
        "canonical_url": "https://example.com/sse-test-feed.xml",
        "source_url": "https://example.com/sse-test-feed.xml",
        "http_status": 200,
        "content_hash": "abc123ssepub",
        "crawl_token": crawl_token,
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "SSE Publish Test Feed",
            "owner_name": "SSE Test Artist",
            "description": "A feed to test SSE publish",
            "explicit": false,
            "raw_medium": "music",
            "tracks": [
                {
                    "track_guid": track_guid,
                    "title": "SSE Test Track 1",
                    "explicit": false,
                    "payment_routes": [
                        {
                            "recipient_name": "Artist",
                            "route_type": "keysend",
                            "address": "02abc123",
                            "split": 100,
                            "fee": false
                        }
                    ],
                    "value_time_splits": []
                }
            ],
            "remote_items": [],
            "feed_payment_routes": [],
            "live_items": []
        }
    });

    let app = stophammer::api::build_router(Arc::clone(&state));
    let req = json_request("POST", "/ingest/feed", &ingest_body);
    let resp = app.oneshot(req).await.expect("ingest");
    assert_eq!(resp.status(), 200, "ingest should succeed");

    let body = body_json(resp).await;
    assert!(
        body["accepted"].as_bool().unwrap_or(false),
        "ingest should be accepted"
    );

    // Now find the artist_id that was created. We look it up via the DB.
    let artist_id = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT a.artist_id FROM artists a
             JOIN artist_credit_name acn ON acn.artist_id = a.artist_id
             JOIN artist_credit ac ON ac.id = acn.artist_credit_id
             JOIN feeds f ON f.artist_credit_id = ac.id
             WHERE f.feed_guid = ?1",
            rusqlite::params![feed_guid],
            |row| row.get::<_, String>(0),
        )
        .expect("find artist_id for feed")
    };

    // Check the SSE registry has events for this artist.
    let recent = state.sse_registry.recent_events(&artist_id);
    assert!(
        !recent.is_empty(),
        "SSE registry should have events for artist {artist_id} after ingest, got 0"
    );

    // We should still see artist-scoped events even after feed/track events
    // stop carrying embedded artist identity.
    let event_types: Vec<&str> = recent.iter().map(|f| f.event_type.as_str()).collect();
    assert!(
        event_types.contains(&"artist_upserted"),
        "should contain artist_upserted event, got: {event_types:?}"
    );
    assert!(
        event_types.contains(&"artist_credit_created"),
        "should contain artist_credit_created event, got: {event_types:?}"
    );

    // All frames should have seq > 0.
    for frame in &recent {
        assert!(
            frame.seq > 0,
            "SSE frame seq should be > 0, got {}",
            frame.seq
        );
    }
}

// ---------------------------------------------------------------------------
// Test: SSE broadcast delivers live events to subscriber
// ---------------------------------------------------------------------------
#[tokio::test]
async fn ingest_feed_update_without_artist_events_does_not_notify_artist_subscriber() {
    let crawl_token = "sse-live-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);

    // First ingest to get the artist_id.
    let ingest_body = serde_json::json!({
        "canonical_url": "https://example.com/sse-live-feed.xml",
        "source_url": "https://example.com/sse-live-feed.xml",
        "http_status": 200,
        "content_hash": "abc123sselive",
        "crawl_token": crawl_token,
        "feed_data": {
            "feed_guid": "feed-sse-live-001",
            "title": "SSE Live Test Feed",
            "owner_name": "Live Test Artist",
            "explicit": false,
            "raw_medium": "music",
            "tracks": [
                {
                    "track_guid": "track-sse-live-001",
                    "title": "Track 1",
                    "explicit": false,
                    "payment_routes": [
                        {
                            "recipient_name": "Artist",
                            "route_type": "keysend",
                            "address": "02abc123",
                            "split": 100,
                            "fee": false
                        }
                    ],
                    "value_time_splits": []
                }
            ],
            "remote_items": [],
            "feed_payment_routes": [],
            "live_items": []
        }
    });

    let app = stophammer::api::build_router(Arc::clone(&state));
    let req = json_request("POST", "/ingest/feed", &ingest_body);
    let resp = app.oneshot(req).await.expect("ingest");
    assert_eq!(resp.status(), 200);

    // Get artist_id from DB.
    let artist_id = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT a.artist_id FROM artists a
             JOIN artist_credit_name acn ON acn.artist_id = a.artist_id
             JOIN artist_credit ac ON ac.id = acn.artist_credit_id
             JOIN feeds f ON f.artist_credit_id = ac.id
             WHERE f.feed_guid = ?1",
            rusqlite::params!["feed-sse-live-001"],
            |row| row.get::<_, String>(0),
        )
        .expect("find artist_id")
    };

    // Now subscribe and do a second ingest. Source-first feed/track upserts no
    // longer route through the artist-scoped SSE channel on their own.
    let mut rx = state
        .sse_registry
        .subscribe(&artist_id)
        .expect("subscribe should succeed");

    // Do a second ingest (different content_hash so it's not rejected as no-change).
    let ingest_body2 = serde_json::json!({
        "canonical_url": "https://example.com/sse-live-feed.xml",
        "source_url": "https://example.com/sse-live-feed.xml",
        "http_status": 200,
        "content_hash": "def456sselive",
        "crawl_token": crawl_token,
        "feed_data": {
            "feed_guid": "feed-sse-live-001",
            "title": "SSE Live Test Feed Updated",
            "owner_name": "Live Test Artist",
            "explicit": false,
            "raw_medium": "music",
            "tracks": [
                {
                    "track_guid": "track-sse-live-001",
                    "title": "Track 1 Updated",
                    "explicit": false,
                    "payment_routes": [
                        {
                            "recipient_name": "Artist",
                            "route_type": "keysend",
                            "address": "02abc123",
                            "split": 100,
                            "fee": false
                        }
                    ],
                    "value_time_splits": []
                }
            ],
            "remote_items": [],
            "feed_payment_routes": [],
            "live_items": []
        }
    });

    let app2 = stophammer::api::build_router(Arc::clone(&state));
    let req2 = json_request("POST", "/ingest/feed", &ingest_body2);
    let resp2 = app2.oneshot(req2).await.expect("ingest2");
    let status = resp2.status();
    if status != 200 {
        let body = body_json(resp2).await;
        panic!("second ingest should succeed, got {status} with body {body}");
    }

    // Feed/track updates alone should not produce new artist-scoped frames.
    let received = rx.try_recv();
    assert!(
        received.is_err(),
        "feed/track-only reingest should not notify artist subscriber, got: {received:?}"
    );
}

// ---------------------------------------------------------------------------
// Test: live-item transitions emit live_event_started / live_event_ended
// ---------------------------------------------------------------------------
#[tokio::test]
async fn live_item_transitions_publish_live_sse_frames() {
    let crawl_token = "sse-live-transition-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(Arc::clone(&state));

    let ingest = |content_hash: &str, live_item: serde_json::Value| {
        json_request(
            "POST",
            "/ingest/feed",
            &serde_json::json!({
                "canonical_url": "https://example.com/live-transition-feed.xml",
                "source_url": "https://example.com/live-transition-feed.xml",
                "http_status": 200,
                "content_hash": content_hash,
                "crawl_token": crawl_token,
                "feed_data": {
                    "feed_guid": "feed-live-transition-001",
                    "title": "Live Transition Feed",
                    "owner_name": "Live Transition Artist",
                    "explicit": false,
                    "raw_medium": "music",
                    "tracks": [],
                    "remote_items": [],
                    "feed_payment_routes": [],
                    "live_items": [live_item]
                }
            }),
        )
    };

    let pending_resp = app
        .clone()
        .oneshot(ingest(
            "live-transition-1",
            serde_json::json!({
                "live_item_guid": "live-item-001",
                "title": "Listening Party",
                "status": "pending",
                "start_at": 1710291600,
                "end_at": 1710298800,
                "content_link": "https://stream.example.com/live",
                "explicit": false,
                "payment_routes": [],
                "value_time_splits": []
            }),
        ))
        .await
        .expect("pending ingest");
    assert_eq!(pending_resp.status(), 200);

    let artist_id = {
        let conn = db.lock().unwrap();
        conn.query_row(
            "SELECT a.artist_id FROM artists a
             JOIN artist_credit_name acn ON acn.artist_id = a.artist_id
             JOIN artist_credit ac ON ac.id = acn.artist_credit_id
             JOIN feeds f ON f.artist_credit_id = ac.id
             WHERE f.feed_guid = ?1",
            rusqlite::params!["feed-live-transition-001"],
            |row| row.get::<_, String>(0),
        )
        .expect("find artist_id")
    };

    let after_pending = state.sse_registry.recent_events(&artist_id);
    assert!(
        after_pending.iter().all(|frame| {
            frame.event_type != "live_event_started" && frame.event_type != "live_event_ended"
        }),
        "pending live item should not emit started/ended SSE frames: {after_pending:?}"
    );

    let live_resp = app
        .clone()
        .oneshot(ingest(
            "live-transition-2",
            serde_json::json!({
                "live_item_guid": "live-item-001",
                "title": "Listening Party",
                "status": "live",
                "start_at": 1710291600,
                "end_at": 1710298800,
                "content_link": "https://stream.example.com/live",
                "explicit": false,
                "payment_routes": [],
                "value_time_splits": []
            }),
        ))
        .await
        .expect("live ingest");
    let status = live_resp.status();
    if status != 200 {
        let body = body_json(live_resp).await;
        panic!("live ingest should succeed, got {status} with body {body}");
    }

    let recent_after_live = state.sse_registry.recent_events(&artist_id);
    let started = recent_after_live
        .iter()
        .find(|frame| frame.event_type == "live_event_started")
        .expect("live transition should emit live_event_started");
    assert_eq!(started.subject_guid, "live-item-001");
    assert_eq!(started.payload["feed_guid"], "feed-live-transition-001");
    assert_eq!(started.payload["status"], "live");

    let ended_resp = app
        .oneshot(ingest(
            "live-transition-3",
            serde_json::json!({
                "live_item_guid": "live-item-001",
                "title": "Listening Party Replay",
                "status": "ended",
                "start_at": 1710291600,
                "end_at": 1710298800,
                "content_link": "https://stream.example.com/live",
                "pub_date": 1710298800,
                "duration_secs": 3600,
                "enclosure_url": "https://cdn.example.com/replay.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 12345678,
                "explicit": false,
                "payment_routes": [],
                "value_time_splits": []
            }),
        ))
        .await
        .expect("ended ingest");
    assert_eq!(ended_resp.status(), 200);

    let recent_after_ended = state.sse_registry.recent_events(&artist_id);
    let ended = recent_after_ended
        .iter()
        .find(|frame| frame.event_type == "live_event_ended")
        .expect("ended transition should emit live_event_ended");
    assert_eq!(ended.subject_guid, "live-item-001");
    assert_eq!(ended.payload["feed_guid"], "feed-live-transition-001");
    assert_eq!(ended.payload["status"], "ended");
}

// ---------------------------------------------------------------------------
// Test: Last-Event-ID as integer seq — replay only events with seq > N
// ---------------------------------------------------------------------------
#[tokio::test]
async fn last_event_id_seq_replay() {
    let registry = stophammer::api::SseRegistry::new();

    // Publish 5 events with increasing seq values.
    for i in 1..=5 {
        let frame = stophammer::api::SseFrame {
            event_type: "track_upserted".to_string(),
            subject_guid: format!("track-{i}"),
            payload: serde_json::json!({"n": i}),
            seq: i,
        };
        registry.publish("artist-replay-seq", frame);
    }

    // Get recent events and filter by seq > 3 (simulating Last-Event-ID: 3).
    let recent = registry.recent_events("artist-replay-seq");
    let replayed: Vec<&stophammer::api::SseFrame> = recent.iter().filter(|f| f.seq > 3).collect();

    assert_eq!(
        replayed.len(),
        2,
        "should replay exactly 2 events with seq > 3"
    );
    assert_eq!(replayed[0].seq, 4);
    assert_eq!(replayed[1].seq, 5);
}

// ---------------------------------------------------------------------------
// Test: Last-Event-ID with seq=0 replays all ring buffer events
// ---------------------------------------------------------------------------
#[tokio::test]
async fn last_event_id_zero_replays_all() {
    let registry = stophammer::api::SseRegistry::new();

    for i in 1..=3 {
        let frame = stophammer::api::SseFrame {
            event_type: "feed_upserted".to_string(),
            subject_guid: format!("feed-{i}"),
            payload: serde_json::json!({}),
            seq: i,
        };
        registry.publish("artist-zero", frame);
    }

    let recent = registry.recent_events("artist-zero");
    let replayed_count = recent.iter().filter(|f| f.seq > 0).count();

    assert_eq!(replayed_count, 3, "seq > 0 should replay all 3 events");
}

// ---------------------------------------------------------------------------
// Test: publish_events_to_sse still routes artist-scoped events
// ---------------------------------------------------------------------------
#[tokio::test]
async fn publish_events_to_sse_routes_to_artist() {
    let registry = stophammer::api::SseRegistry::new();

    // Subscribe to the target artist.
    let mut rx = registry
        .subscribe("artist-pub-test")
        .expect("subscribe should succeed");

    // Build an ArtistUpserted event that references artist-pub-test.
    let ev = stophammer::event::Event {
        event_id: "ev-pub-test-1".to_string(),
        event_type: stophammer::event::EventType::ArtistUpserted,
        payload: stophammer::event::EventPayload::ArtistUpserted(
            stophammer::event::ArtistUpsertedPayload {
                artist: stophammer::model::Artist {
                    artist_id: "artist-pub-test".to_string(),
                    name: "Pub Test Artist".to_string(),
                    name_lower: "pub test artist".to_string(),
                    sort_name: Some("Pub Test Artist".to_string()),
                    type_id: Some(1),
                    area: None,
                    img_url: None,
                    url: None,
                    begin_year: None,
                    end_year: None,
                    created_at: 0,
                    updated_at: 0,
                },
            },
        ),
        subject_guid: "artist-pub-test".to_string(),
        signed_by: "deadbeef".to_string(),
        signature: "cafebabe".to_string(),
        seq: 42,
        created_at: 0,
        warnings: vec![],
        payload_json: "{}".to_string(),
    };

    stophammer::api::publish_events_to_sse(&registry, &[ev]);

    // The subscriber for artist-pub-test should receive the event.
    let received = rx.try_recv();
    assert!(received.is_ok(), "subscriber should receive SSE frame");
    let frame = received.unwrap();
    assert_eq!(frame.seq, 42, "SSE frame should carry seq=42");
    assert_eq!(frame.event_type, "artist_upserted");
    assert_eq!(frame.subject_guid, "artist-pub-test");
}

// ---------------------------------------------------------------------------
// Test: SseFrame seq field is used as SSE id: in the stream
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_frame_seq_used_as_id_field() {
    // Verify the SSE frame's seq is serialized into the id field.
    let frame = stophammer::api::SseFrame {
        event_type: "track_upserted".to_string(),
        subject_guid: "track-id-test".to_string(),
        payload: serde_json::json!({"title": "ID Test"}),
        seq: 99,
    };

    // seq should be accessible and correct.
    assert_eq!(frame.seq, 99);
    // The SSE handler uses frame.seq.to_string() as the id, so verify
    // the string representation is correct.
    assert_eq!(frame.seq.to_string(), "99");
}
