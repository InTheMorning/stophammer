// Finding-3 separate sync token — 2026-03-13
//
// POST /sync/register should use a separate SYNC_TOKEN, not ADMIN_TOKEN.
// The sync token must NOT grant access to admin endpoints.
// Backward compatibility: if SYNC_TOKEN is not set, fall back to ADMIN_TOKEN.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    admin_token: &str,
    sync_token: Option<&str>,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-finding3-sync-token.key")
            .expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(vec![])),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: admin_token.into(),
        sync_token: sync_token.map(String::from),
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

fn register_request_body() -> serde_json::Value {
    serde_json::json!({
        "node_pubkey": "deadbeef01234567890abcdef01234567890abcdef01234567890abcdef012345",
        "node_url":    "http://community.example.com:8008/sync/push"
    })
}

// ── Test: POST /sync/register with valid X-Sync-Token returns 200 ───────────

#[tokio::test]
async fn sync_register_with_valid_sync_token_returns_200() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), "admin-secret", Some("sync-secret"));
    let app = stophammer::api::build_router(state);

    let body = register_request_body();
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Sync-Token", "sync-secret")
        .body(axum::body::Body::from(serde_json::to_vec(&body).expect("serialize")))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        200,
        "POST /sync/register with valid X-Sync-Token must return 200"
    );

    let bytes = resp.into_body().collect().await.expect("read body").to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    assert_eq!(json["ok"], true);
}

// ── Test: POST /sync/register with X-Admin-Token (legacy fallback) → 200 ────

#[tokio::test]
async fn sync_register_with_admin_token_legacy_returns_200() {
    let db = common::test_db_arc();
    // SYNC_TOKEN is not set (None) — should fall back to ADMIN_TOKEN
    let state = test_app_state(Arc::clone(&db), "admin-secret", None);
    let app = stophammer::api::build_router(state);

    let body = register_request_body();
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "admin-secret")
        .body(axum::body::Body::from(serde_json::to_vec(&body).expect("serialize")))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        200,
        "POST /sync/register with X-Admin-Token (legacy fallback) must return 200"
    );
}

// ── Test: POST /sync/register with invalid X-Sync-Token returns 403 ─────────

#[tokio::test]
async fn sync_register_with_invalid_sync_token_returns_403() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), "admin-secret", Some("sync-secret"));
    let app = stophammer::api::build_router(state);

    let body = register_request_body();
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Sync-Token", "wrong-token")
        .body(axum::body::Body::from(serde_json::to_vec(&body).expect("serialize")))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "POST /sync/register with invalid X-Sync-Token must return 403"
    );
}

// ── Test: Admin endpoint with X-Sync-Token must be rejected (403) ───────────

#[tokio::test]
async fn admin_endpoint_with_sync_token_returns_403() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), "admin-secret", Some("sync-secret"));
    let app = stophammer::api::build_router(state);

    let body = serde_json::json!({
        "artist_id": "artist-1",
        "alias":     "Some Alias"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/admin/artists/alias")
        .header("Content-Type", "application/json")
        .header("X-Sync-Token", "sync-secret")
        .body(axum::body::Body::from(serde_json::to_vec(&body).expect("serialize")))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "Admin endpoint must reject X-Sync-Token (sync token must NOT grant admin access)"
    );
}

// ── Test: Admin endpoint with X-Admin-Token still works ─────────────────────

#[tokio::test]
async fn admin_endpoint_with_admin_token_still_works() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), "admin-secret", Some("sync-secret"));
    let app = stophammer::api::build_router(state);

    // Use /admin/artists/merge which calls spawn_blocking. With nonexistent
    // artists the merge returns 500 (spawn_blocking behaviour in single-thread
    // test runtime), but the auth check runs first — a 403 would mean the
    // admin token was rejected. Anything else proves auth passed.
    let body = serde_json::json!({
        "source_artist_id": "nonexistent-a1",
        "target_artist_id": "nonexistent-a2"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/admin/artists/merge")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "admin-secret")
        .body(axum::body::Body::from(serde_json::to_vec(&body).expect("serialize")))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_ne!(
        resp.status(),
        403,
        "Admin endpoint must still accept valid X-Admin-Token (non-403 proves auth passed)"
    );
}

// ── Test: Neither SYNC_TOKEN nor ADMIN_TOKEN set → registration rejected ────

#[tokio::test]
async fn sync_register_no_tokens_configured_returns_403() {
    let db = common::test_db_arc();
    // Both tokens empty/unset
    let state = test_app_state(Arc::clone(&db), "", None);
    let app = stophammer::api::build_router(state);

    let body = register_request_body();
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Sync-Token", "any-token")
        .body(axum::body::Body::from(serde_json::to_vec(&body).expect("serialize")))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "POST /sync/register must return 403 when neither SYNC_TOKEN nor ADMIN_TOKEN is configured"
    );
}

// ── Test: When SYNC_TOKEN is set, X-Admin-Token should NOT work for /sync/register ──

#[tokio::test]
async fn sync_register_admin_token_rejected_when_sync_token_configured() {
    let db = common::test_db_arc();
    // SYNC_TOKEN is explicitly set — admin token should not be accepted for registration
    let state = test_app_state(Arc::clone(&db), "admin-secret", Some("sync-secret"));
    let app = stophammer::api::build_router(state);

    let body = register_request_body();
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "admin-secret")
        .body(axum::body::Body::from(serde_json::to_vec(&body).expect("serialize")))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "POST /sync/register with X-Admin-Token must return 403 when SYNC_TOKEN is configured (use X-Sync-Token instead)"
    );
}
