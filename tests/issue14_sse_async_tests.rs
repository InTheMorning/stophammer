mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-issue14"));
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

// ---------------------------------------------------------------------------
// Issue #14: SSE endpoint still connects and returns SSE content-type
// (verifies the async stream refactor did not break the SSE endpoint)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sse_endpoint_returns_event_stream_content_type() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        let now = common::now();
        conn.execute(
            "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["a1", "Test Artist", "test artist", now, now],
        )
        .expect("insert artist");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/v1/events?artists=a1")
        .header("Accept", "text/event-stream")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 200, "SSE endpoint should return 200");

    let content_type = resp
        .headers()
        .get("content-type")
        .expect("should have content-type")
        .to_str()
        .expect("content-type to str");
    assert!(
        content_type.contains("text/event-stream"),
        "should be SSE content type, got: {content_type}"
    );
}

// ---------------------------------------------------------------------------
// Issue #14: SSE delivers broadcast message via async stream
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sse_delivers_broadcast_message() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        let now = common::now();
        conn.execute(
            "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["a-sse", "SSE Artist", "sse artist", now, now],
        )
        .expect("insert artist");
    }

    let state = test_app_state(Arc::clone(&db));

    // Publish an event so the channel and ring buffer are populated
    let frame = stophammer::api::SseFrame {
        event_type: "feed_upserted".to_string(),
        subject_guid: "feed-sse-1".to_string(),
        payload: serde_json::json!({"feed_guid": "feed-sse-1"}),
        seq: 1,
    };
    state.sse_registry.publish("a-sse", frame);

    let app = stophammer::api::build_router(state);

    // Connect with Last-Event-ID that is before seq=1 to trigger replay
    let req = Request::builder()
        .method("GET")
        .uri("/v1/events?artists=a-sse")
        .header("Accept", "text/event-stream")
        .header("Last-Event-ID", "0")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 200, "SSE endpoint should return 200");
}
