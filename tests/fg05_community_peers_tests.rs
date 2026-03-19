// FG-05 community peers — 2026-03-13

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state() -> Arc<stophammer::api::AppState> {
    let db = common::test_db_arc();
    let signer = Arc::new(common::temp_signer("test-fg05-signer"));
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

/// GET /sync/peers on the readonly router should require auth.
#[tokio::test]
async fn readonly_router_sync_peers_requires_auth() {
    let state = test_app_state();
    let app = stophammer::api::build_readonly_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/sync/peers")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        403,
        "GET /sync/peers without sync auth must return 403"
    );
}

/// GET /sync/peers on the readonly router should return 200 with valid sync auth.
#[tokio::test]
async fn readonly_router_serves_sync_peers() {
    let state = test_app_state();
    let app = stophammer::api::build_readonly_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/sync/peers")
                .header("X-Sync-Token", "test-sync-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "GET /sync/peers on readonly router should return 200, got {}",
        resp.status()
    );

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        body.get("nodes").is_some(),
        "response should contain 'nodes' field"
    );
}

/// GET /node/info on the readonly router should return 200 with `node_pubkey`.
#[tokio::test]
async fn readonly_router_serves_node_info() {
    let state = test_app_state();
    let pubkey = state.node_pubkey_hex.clone();
    let app = stophammer::api::build_readonly_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/node/info")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "GET /node/info on readonly router should return 200, got {}",
        resp.status()
    );

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let returned_pubkey = body["node_pubkey"]
        .as_str()
        .expect("should have node_pubkey");
    assert_eq!(
        returned_pubkey, pubkey,
        "node_pubkey should match the state's pubkey"
    );
}

/// Verify /sync/peers still works on the primary router too.
#[tokio::test]
async fn primary_router_still_serves_sync_peers() {
    let state = test_app_state();
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/sync/peers")
                .header("X-Sync-Token", "test-sync-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
}

/// Verify /node/info still works on the primary router too.
#[tokio::test]
async fn primary_router_still_serves_node_info() {
    let state = test_app_state();
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/node/info")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
}
