// Issue-NEGATIVE-LIMIT — 2026-03-15
//
// Verifies that GET /sync/events?limit=-1 does not bypass the 1000-event cap.
// SQLite treats LIMIT -1 as "no limit", so negative values must be clamped.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-negative-limit.key")
            .expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(vec![])),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token: Some("test-sync-token".into()),
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

// ── Test: GET /sync/events?limit=-1 returns 200 with bounded results ────────

#[tokio::test]
async fn sync_events_negative_limit_is_clamped() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/sync/events?limit=-1")
        .header("X-Sync-Token", "test-sync-token")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        200,
        "GET /sync/events?limit=-1 must return 200"
    );

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");

    let events = json["events"].as_array().expect("events must be an array");
    assert!(
        events.len() <= 1000,
        "negative limit must not bypass the 1000-event cap, got {} events",
        events.len()
    );
}

// ── Test: GET /sync/events?limit=0 also returns 200 with bounded results ────

#[tokio::test]
async fn sync_events_zero_limit_is_clamped() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/sync/events?limit=0")
        .header("X-Sync-Token", "test-sync-token")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        200,
        "GET /sync/events?limit=0 must return 200"
    );

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");

    let events = json["events"].as_array().expect("events must be an array");
    assert!(
        events.len() <= 1000,
        "zero limit must not bypass the 1000-event cap, got {} events",
        events.len()
    );
}

#[tokio::test]
async fn sync_events_requires_auth() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/sync/events?limit=1")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "GET /sync/events without sync auth must return 403"
    );
}

// ── Test: DB-level defense — get_events_since floors negative limit ─────────

#[test]
fn db_get_events_since_negative_limit_returns_bounded() {
    let conn = common::test_db();

    // Insert a handful of minimal events with valid JSON payloads.
    // The payload must be valid for EventPayload deserialization, so we use
    // a feed_retired event which has the simplest payload shape.
    for i in 1..=5_i64 {
        let event_id = format!("evt-neg-{i}");
        let payload = serde_json::json!({
            "feed_guid": format!("guid-{i}")
        });
        conn.execute(
            "INSERT INTO events (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at, warnings_json) \
             VALUES (?1, 'feed_retired', ?2, ?3, 'node', 'sig', ?4, strftime('%s','now'), '[]')",
            rusqlite::params![event_id, payload.to_string(), format!("guid-{i}"), i],
        )
        .expect("insert event");
    }

    // Call with limit = -1 — should be floored to 1 by the defense-in-depth guard.
    let events = stophammer::db::get_events_since(&conn, 0, -1).expect("query should succeed");
    assert!(
        events.len() <= 1,
        "get_events_since with limit=-1 should return at most 1 row, got {}",
        events.len()
    );
}
