// FG-02 SSE artist follow — 2026-03-13
//
// Tests for the `GET /v1/events?artists=id1,id2` Server-Sent Events endpoint.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-fg02-sse.key")
            .expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(vec![])),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: String::new(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

// ---------------------------------------------------------------------------
// Test: GET /v1/events?artists=foo returns text/event-stream content type
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_endpoint_returns_event_stream_content_type() {
    let db = common::test_db_arc();
    let state = test_app_state(db);
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/v1/events?artists=artist-1,artist-2")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 200, "SSE endpoint should return 200 OK");

    let content_type = resp
        .headers()
        .get("content-type")
        .expect("content-type header must be present")
        .to_str()
        .expect("header to str");
    assert!(
        content_type.contains("text/event-stream"),
        "content-type must be text/event-stream, got: {content_type}"
    );

    let cache_control = resp
        .headers()
        .get("cache-control")
        .expect("cache-control header must be present")
        .to_str()
        .expect("header to str");
    assert!(
        cache_control.contains("no-cache"),
        "cache-control must be no-cache, got: {cache_control}"
    );
}

// ---------------------------------------------------------------------------
// Test: SSE registry can publish and receive events
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_registry_publish_and_subscribe() {
    let registry = stophammer::api::SseRegistry::new();

    // Subscribe to artist-1
    let mut rx = registry
        .subscribe("artist-1")
        .expect("subscribe should succeed");

    // Publish an event for artist-1
    let frame = stophammer::api::SseFrame {
        event_type: "track_upserted".to_string(),
        subject_guid: "track-abc".to_string(),
        payload: serde_json::json!({"title": "New Song"}),
        seq: 1,
    };
    registry.publish("artist-1", frame.clone());

    // Should receive the event
    let received = rx.recv().await.expect("should receive event");
    assert_eq!(received.event_type, "track_upserted");
    assert_eq!(received.subject_guid, "track-abc");
}

// ---------------------------------------------------------------------------
// Test: SSE registry does not cross-pollinate between artists
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_registry_no_cross_pollination() {
    let registry = stophammer::api::SseRegistry::new();

    let mut rx_a = registry
        .subscribe("artist-a")
        .expect("subscribe should succeed");
    let _rx_b = registry
        .subscribe("artist-b")
        .expect("subscribe should succeed");

    // Publish to artist-b only
    let frame = stophammer::api::SseFrame {
        event_type: "feed_upserted".to_string(),
        subject_guid: "feed-xyz".to_string(),
        payload: serde_json::json!({}),
        seq: 1,
    };
    registry.publish("artist-b", frame);

    // artist-a should NOT receive the event (try_recv should error)
    let result = rx_a.try_recv();
    assert!(
        result.is_err(),
        "artist-a should not receive artist-b events"
    );
}

// ---------------------------------------------------------------------------
// Test: SSE registry ring buffer stores recent events for replay
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_registry_ring_buffer_replay() {
    let registry = stophammer::api::SseRegistry::new();

    // Publish 5 events before anyone subscribes
    for i in 0..5 {
        let frame = stophammer::api::SseFrame {
            event_type: "track_upserted".to_string(),
            subject_guid: format!("track-{i}"),
            payload: serde_json::json!({"n": i}),
            seq: i + 1,
        };
        registry.publish("artist-replay", frame);
    }

    // Get recent events for replay
    let recent = registry.recent_events("artist-replay");
    assert_eq!(recent.len(), 5, "should have 5 recent events");
    assert_eq!(recent[0].subject_guid, "track-0");
    assert_eq!(recent[4].subject_guid, "track-4");
}

// ---------------------------------------------------------------------------
// Test: Ring buffer is bounded to 100 events
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_registry_ring_buffer_bounded() {
    let registry = stophammer::api::SseRegistry::new();

    // Publish 150 events
    for i in 0..150 {
        let frame = stophammer::api::SseFrame {
            event_type: "track_upserted".to_string(),
            subject_guid: format!("track-{i}"),
            payload: serde_json::json!({"n": i}),
            seq: i + 1,
        };
        registry.publish("artist-bounded", frame);
    }

    let recent = registry.recent_events("artist-bounded");
    assert_eq!(recent.len(), 100, "ring buffer must be bounded to 100");
    // Oldest event in buffer should be track-50 (first 50 were evicted)
    assert_eq!(recent[0].subject_guid, "track-50");
    assert_eq!(recent[99].subject_guid, "track-149");
}

// ---------------------------------------------------------------------------
// Test: GET /v1/events is also present on the readonly router
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_endpoint_on_readonly_router() {
    let db = common::test_db_arc();
    let state = test_app_state(db);
    let app = stophammer::api::build_readonly_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/v1/events?artists=artist-1")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        200,
        "SSE endpoint should return 200 on readonly router"
    );
}

// ---------------------------------------------------------------------------
// Test: GET /v1/events with empty artists param returns 200
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sse_endpoint_empty_artists() {
    let db = common::test_db_arc();
    let state = test_app_state(db);
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/v1/events?artists=")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    // Should return 200 OK (just no events to subscribe to)
    assert_eq!(resp.status(), 200);
}
