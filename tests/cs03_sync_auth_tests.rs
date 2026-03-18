// CS-03 authenticated register — 2026-03-12
//
// POST /sync/register must require X-Admin-Token authentication.
// Without it, any unauthenticated client can register as a push peer.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state_with_token(
    db: Arc<Mutex<rusqlite::Connection>>,
    admin_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-cs03-sync-auth.key")
            .expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(vec![])),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: admin_token.into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

fn register_request_body() -> serde_json::Value {
    let signer = stophammer::signing::NodeSigner::load_or_create("/tmp/test-cs03-sync-auth-body.key")
        .expect("create signer");
    common::signed_sync_register_body(&signer, "http://community.example.com:8008/sync/push")
}

// ── Test: POST /sync/register without auth returns 403 ──────────────────────

#[tokio::test]
async fn sync_register_without_auth_returns_403() {
    let db = common::test_db_arc();
    let state = test_app_state_with_token(Arc::clone(&db), "my-secret-admin-token");
    let app = stophammer::api::build_router(state);

    let body = register_request_body();
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&body).expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "POST /sync/register without X-Admin-Token must return 403"
    );
}

// ── Test: POST /sync/register with valid token returns 200 ──────────────────

#[tokio::test]
async fn sync_register_with_valid_token_returns_200() {
    let db = common::test_db_arc();
    let state = test_app_state_with_token(Arc::clone(&db), "my-secret-admin-token");
    let app = stophammer::api::build_router(state);

    let body = register_request_body();
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "my-secret-admin-token")
        .body(axum::body::Body::from(
            serde_json::to_vec(&body).expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        200,
        "POST /sync/register with valid X-Admin-Token must return 200"
    );

    // Verify the response body indicates success
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    assert_eq!(json["ok"], true);
}

// ── Test: POST /sync/register with invalid token returns 403 ────────────────

#[tokio::test]
async fn sync_register_with_invalid_token_returns_403() {
    let db = common::test_db_arc();
    let state = test_app_state_with_token(Arc::clone(&db), "my-secret-admin-token");
    let app = stophammer::api::build_router(state);

    let body = register_request_body();
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "wrong-token")
        .body(axum::body::Body::from(
            serde_json::to_vec(&body).expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "POST /sync/register with wrong X-Admin-Token must return 403"
    );
}

// ── Test: POST /sync/register with empty admin_token on server returns 403 ──

#[tokio::test]
async fn sync_register_misconfigured_server_returns_403() {
    let db = common::test_db_arc();
    // Server has no admin_token configured (empty string)
    let state = test_app_state_with_token(Arc::clone(&db), "");
    let app = stophammer::api::build_router(state);

    let body = register_request_body();
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "any-token")
        .body(axum::body::Body::from(
            serde_json::to_vec(&body).expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "POST /sync/register with unconfigured admin_token must return 403 (misconfigured)"
    );
}
