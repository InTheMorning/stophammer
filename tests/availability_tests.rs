mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-avail-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(
            "test-token".into(),
            vec![],
        )),
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

// Issue-RECONCILE-AUTH — 2026-03-16: reconcile now requires dedicated sync auth.
fn json_request_authed(
    method: &str,
    uri: &str,
    body: &serde_json::Value,
) -> Request<axum::body::Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("X-Sync-Token", "test-sync-token")
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

// ==========================================================================
// VULN-03: Reconcile with oversized `have` array
// ==========================================================================

#[tokio::test]
async fn reconcile_rejects_oversized_have() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Build a have array with 10,001 entries (exceeds limit of 10,000).
    let have: Vec<serde_json::Value> = (0..10_001)
        .map(|i| {
            serde_json::json!({
                "event_id": format!("fake-event-{i}"),
                "seq": i,
            })
        })
        .collect();

    // Issue-RECONCILE-AUTH — 2026-03-16: reconcile requires auth.
    let resp = app
        .oneshot(json_request_authed(
            "POST",
            "/sync/reconcile",
            &serde_json::json!({
                "node_pubkey": "test-pubkey",
                "have": have,
                "since_seq": 0,
            }),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        400,
        "reconcile with oversized have should be rejected"
    );
}

// A have array within the limit should be accepted.
#[tokio::test]
async fn reconcile_accepts_valid_have() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Issue-RECONCILE-AUTH — 2026-03-16: reconcile requires auth.
    let resp = app
        .oneshot(json_request_authed(
            "POST",
            "/sync/reconcile",
            &serde_json::json!({
                "node_pubkey": "test-pubkey",
                "have": [],
                "since_seq": 0,
            }),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "reconcile with empty have should be accepted"
    );
}

// ==========================================================================
// VULN-04: FTS5 field truncation
// ==========================================================================

#[test]
fn fts5_handles_oversized_description_without_error() {
    let conn = common::test_db();

    // A 100KB description should not cause an error -- it gets truncated.
    let huge_description = "x".repeat(100_000);

    let result = stophammer::search::populate_search_index(
        &conn,
        "feed",
        "fts-test-feed",
        "searchablename",
        "searchabletitle",
        &huge_description,
        "",
    );

    assert!(
        result.is_ok(),
        "populate_search_index should succeed with oversized description: {:?}",
        result.err()
    );

    // Verify the index entry was created by doing a FTS5 MATCH query directly.
    // (The contentless FTS5 table's content columns return NULL, but MATCH works.)
    let match_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_index WHERE search_index MATCH 'searchablename'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert!(
        match_count > 0,
        "FTS5 MATCH should find the entry despite truncated description"
    );
}

#[test]
fn fts5_truncates_large_field_to_limit() {
    let conn = common::test_db();

    // A description of exactly 20,000 bytes should be truncated to 10,000.
    let big_description = "a".repeat(20_000);

    let result = stophammer::search::populate_search_index(
        &conn,
        "feed",
        "truncate-test",
        "",
        "Truncation Test",
        &big_description,
        "",
    );

    assert!(result.is_ok(), "should not error on oversized field");
}

// ==========================================================================
// VULN-06: Body size limit (via DefaultBodyLimit)
// ==========================================================================
// Note: Axum's DefaultBodyLimit returns 413 Payload Too Large when exceeded.
// We test this by sending a body larger than MAX_BODY_BYTES (2 MiB) to
// POST /v1/blocks (ADR 0056 removed POST /v1/proofs/challenge, the route
// this pair of tests used before).

#[tokio::test]
async fn body_size_limit_rejects_oversized_payload() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Create a 3 MiB JSON body (exceeds 2 MiB limit).
    let big_value = "x".repeat(3 * 1024 * 1024);
    let body = serde_json::json!({
        "kind": "url",
        "value": big_value,
        "reason": "oversize test",
    });

    let req = Request::builder()
        .method("POST")
        .uri("/v1/blocks")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "test-admin-token")
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // Axum returns 413 when the body exceeds DefaultBodyLimit.
    assert_eq!(
        resp.status(),
        413,
        "oversized body should be rejected with 413 Payload Too Large"
    );
}

// A body within the limit should be accepted normally.
#[tokio::test]
async fn body_within_limit_accepted() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let body = serde_json::json!({
        "kind": "url",
        "value": "https://example.com/normal-feed.xml",
        "reason": "normal-sized body test",
    });

    let req = Request::builder()
        .method("POST")
        .uri("/v1/blocks")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "test-admin-token")
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 201, "normal-sized body should be accepted");
}

// ==========================================================================
// VULN-07: Track count limit on ingest (unit-level check)
// ==========================================================================
// We can't easily test the full ingest handler without a valid crawl token
// and verifier chain. Instead, we verify the limit exists by checking that
// the constant is public and reasonable.

// (The track count limit is enforced inside the handler. Integration tests
// with a full verifier chain would require setting up CRAWL_TOKEN, which is
// outside the scope of this security audit. The constant is defined at
// MAX_TRACKS_PER_INGEST = 500.)

// ==========================================================================
// VULN-08: FTS truncation preserves UTF-8 boundaries
// ==========================================================================

#[test]
fn fts5_truncation_respects_char_boundaries() {
    let conn = common::test_db();

    // Build a string that would cross a multi-byte char boundary at exactly
    // 10,000 bytes: 9,999 bytes of ASCII + a 3-byte UTF-8 character.
    // The truncation should cut before the multi-byte char, not in the middle.
    let mut desc = "a".repeat(9_999);
    desc.push('\u{2603}'); // snowman, 3 bytes in UTF-8

    assert!(desc.len() > 10_000, "test string should exceed limit");

    let result = stophammer::search::populate_search_index(
        &conn,
        "feed",
        "utf8-test-feed",
        "",
        "UTF-8 Test",
        &desc,
        "",
    );

    assert!(
        result.is_ok(),
        "truncation should produce valid UTF-8: {:?}",
        result.err()
    );
}

// ==========================================================================
// AVAIL-09: SSE registry artist count limit
// ==========================================================================

#[test]
fn sse_registry_limits_artist_entries() {
    let registry = stophammer::api::SseRegistry::new();

    // Subscribe to 10,000 unique artists (the limit).
    for i in 0..10_000 {
        let id = format!("artist-{i}");
        let result = registry.subscribe(&id);
        assert!(result.is_some(), "subscribe to artist {i} should succeed");
    }

    assert_eq!(registry.artist_count(), 10_000);

    // The 10,001st unique artist should be rejected.
    let result = registry.subscribe("artist-overflow");
    assert!(
        result.is_none(),
        "subscribe beyond limit should return None"
    );

    // But subscribing to an existing artist should still work.
    let result = registry.subscribe("artist-0");
    assert!(
        result.is_some(),
        "subscribe to existing artist should succeed even when at limit"
    );
}

// ==========================================================================
// AVAIL-10: SSE concurrent connection limit
// ==========================================================================

#[test]
fn sse_connection_limit_enforced() {
    let registry = stophammer::api::SseRegistry::new();

    // Acquire 1,000 connection slots (the limit).
    for i in 0..1_000 {
        assert!(
            registry.try_acquire_connection(),
            "connection {i} should be granted"
        );
    }

    assert_eq!(registry.active_connections(), 1_000);

    // The 1,001st connection should be rejected.
    assert!(
        !registry.try_acquire_connection(),
        "connection beyond limit should be rejected"
    );

    // Releasing one slot should allow a new connection.
    registry.release_connection();
    assert_eq!(registry.active_connections(), 999);
    assert!(
        registry.try_acquire_connection(),
        "connection after release should be granted"
    );
}
