// CORS tests.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use http::Request;
use tower::ServiceExt;

fn test_app_state() -> Arc<stophammer::api::AppState> {
    let db = common::test_db_arc();
    let key_path = format!("/tmp/test-sp08-signer-{}.key", uuid::Uuid::new_v4());
    let signer = Arc::new(stophammer::signing::NodeSigner::load_or_create(&key_path).unwrap());
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

/// OPTIONS preflight request should return 200 with CORS headers.
#[tokio::test]
async fn cors_preflight_returns_200_with_headers() {
    let state = test_app_state();
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/health")
                .header("Origin", "https://example.com")
                .header("Access-Control-Request-Method", "GET")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "OPTIONS preflight should return 200");
    assert!(
        resp.headers().contains_key("access-control-allow-origin"),
        "response should contain access-control-allow-origin header"
    );
}

/// GET with Origin header should include CORS headers in response.
#[tokio::test]
async fn cors_get_includes_allow_origin() {
    let state = test_app_state();
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health")
                .header("Origin", "https://example.com")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let allow_origin = resp
        .headers()
        .get("access-control-allow-origin")
        .expect("should have access-control-allow-origin header");
    assert_eq!(allow_origin, "*", "allow-origin should be wildcard");
}

/// Authorization header must be listed as an allowed header.
#[tokio::test]
async fn cors_allows_authorization_header() {
    let state = test_app_state();
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/v1/feeds/some-guid")
                .header("Origin", "https://example.com")
                .header("Access-Control-Request-Method", "PATCH")
                .header("Access-Control-Request-Headers", "authorization")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "preflight should return 200");
    let allow_headers = resp
        .headers()
        .get("access-control-allow-headers")
        .expect("should have access-control-allow-headers")
        .to_str()
        .unwrap()
        .to_lowercase();
    assert!(
        allow_headers.contains("authorization"),
        "allowed headers should include authorization, got: {allow_headers}"
    );
}

/// x-admin-token header must be listed as an allowed header.
#[tokio::test]
async fn cors_allows_x_admin_token_header() {
    let state = test_app_state();
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/v1/feeds/test-feed-guid")
                .header("Origin", "https://example.com")
                .header("Access-Control-Request-Method", "DELETE")
                .header("Access-Control-Request-Headers", "x-admin-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "preflight should return 200");
    let allow_headers = resp
        .headers()
        .get("access-control-allow-headers")
        .expect("should have access-control-allow-headers")
        .to_str()
        .unwrap()
        .to_lowercase();
    assert!(
        allow_headers.contains("x-admin-token"),
        "allowed headers should include x-admin-token, got: {allow_headers}"
    );
}

/// Readonly router should also have CORS headers.
#[tokio::test]
async fn cors_readonly_router_has_headers() {
    let state = test_app_state();
    let app = stophammer::api::build_readonly_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health")
                .header("Origin", "https://example.com")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let allow_origin = resp
        .headers()
        .get("access-control-allow-origin")
        .expect("readonly router should have access-control-allow-origin header");
    assert_eq!(allow_origin, "*", "allow-origin should be wildcard");
}

/// max-age should be set for caching preflight results.
#[tokio::test]
async fn cors_max_age_is_set() {
    let state = test_app_state();
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/health")
                .header("Origin", "https://example.com")
                .header("Access-Control-Request-Method", "GET")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        resp.headers().contains_key("access-control-max-age"),
        "preflight response should contain access-control-max-age header"
    );
}
