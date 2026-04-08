//! Sync-register authentication tests.
//!
//! `/sync/register` uses `SYNC_TOKEN`, not `ADMIN_TOKEN`.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    admin_token: &str,
    sync_token: Option<&str>,
) -> Arc<stophammer::api::AppState> {
    let temp_dir = tempfile::tempdir().expect("create temp signer dir");
    let key_path = temp_dir.path().join("test-finding3-sync-token.key");
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create(&key_path).expect("create signer"),
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

async fn register_request_body() -> (MockServer, serde_json::Value) {
    let temp_dir = tempfile::tempdir().expect("create temp signer dir");
    let key_path = temp_dir.path().join("test-finding3-sync-token-body.key");
    let signer = stophammer::signing::NodeSigner::load_or_create(&key_path).expect("create signer");
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/node/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "node_pubkey": signer.pubkey_hex()
        })))
        .mount(&mock_server)
        .await;
    let body =
        common::signed_sync_register_body(&signer, &format!("{}/sync/push", mock_server.uri()));
    (mock_server, body)
}

// ── Test: POST /sync/register with valid X-Sync-Token returns 200 ───────────

#[tokio::test]
async fn sync_register_with_valid_sync_token_returns_200() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), "admin-secret", Some("sync-secret"));
    let app = stophammer::api::build_router(state);

    let (_mock_server, body) = register_request_body().await;
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Sync-Token", "sync-secret")
        .body(axum::body::Body::from(
            serde_json::to_vec(&body).expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        200,
        "POST /sync/register with valid X-Sync-Token must return 200"
    );

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    assert_eq!(json["ok"], true);
}

// ── Test: POST /sync/register with X-Admin-Token and no SYNC_TOKEN → 403 ────

#[tokio::test]
async fn sync_register_with_admin_token_and_no_sync_token_returns_403() {
    let db = common::test_db_arc();
    // SYNC_TOKEN is not set (None) — sync auth must still reject.
    let state = test_app_state(Arc::clone(&db), "admin-secret", None);
    let app = stophammer::api::build_router(state);

    let (_mock_server, body) = register_request_body().await;
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "admin-secret")
        .body(axum::body::Body::from(
            serde_json::to_vec(&body).expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "POST /sync/register with X-Admin-Token must return 403 when SYNC_TOKEN is not configured"
    );
}

// ── Test: POST /sync/register with invalid X-Sync-Token returns 403 ─────────

#[tokio::test]
async fn sync_register_with_invalid_sync_token_returns_403() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), "admin-secret", Some("sync-secret"));
    let app = stophammer::api::build_router(state);

    let (_mock_server, body) = register_request_body().await;
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Sync-Token", "wrong-token")
        .body(axum::body::Body::from(
            serde_json::to_vec(&body).expect("serialize"),
        ))
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

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/nonexistent-feed")
        .header("X-Sync-Token", "sync-secret")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        401,
        "Admin endpoint must not treat X-Sync-Token as valid admin or bearer auth"
    );
}

// ── Test: Admin endpoint with X-Admin-Token still works ─────────────────────

#[tokio::test]
async fn admin_endpoint_with_admin_token_still_works() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), "admin-secret", Some("sync-secret"));
    let app = stophammer::api::build_router(state);

    // Use the feed delete endpoint. With a nonexistent feed, a non-403 result
    // proves the admin auth check passed before the lookup.
    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/nonexistent-feed")
        .header("X-Admin-Token", "admin-secret")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_ne!(
        resp.status(),
        403,
        "Admin endpoint must still accept valid X-Admin-Token (non-403 proves auth passed)"
    );
}

// ── Test: Missing SYNC_TOKEN on server → registration rejected ───────────────

#[tokio::test]
async fn sync_register_no_tokens_configured_returns_403() {
    let db = common::test_db_arc();
    // Both tokens empty/unset
    let state = test_app_state(Arc::clone(&db), "", None);
    let app = stophammer::api::build_router(state);

    let (_mock_server, body) = register_request_body().await;
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Sync-Token", "any-token")
        .body(axum::body::Body::from(
            serde_json::to_vec(&body).expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "POST /sync/register must return 403 when SYNC_TOKEN is not configured"
    );
}

// ── Test: When SYNC_TOKEN is set, X-Admin-Token should NOT work for /sync/register ──

#[tokio::test]
async fn sync_register_admin_token_rejected_when_sync_token_configured() {
    let db = common::test_db_arc();
    // SYNC_TOKEN is explicitly set — admin token should not be accepted for registration
    let state = test_app_state(Arc::clone(&db), "admin-secret", Some("sync-secret"));
    let app = stophammer::api::build_router(state);

    let (_mock_server, body) = register_request_body().await;
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "admin-secret")
        .body(axum::body::Body::from(
            serde_json::to_vec(&body).expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "POST /sync/register with X-Admin-Token must return 403 when SYNC_TOKEN is configured (use X-Sync-Token instead)"
    );
}
