// SP-03 rate limiting — 2026-03-13

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Verify that the governor rate limiter correctly throttles after burst.
#[test]
fn governor_rejects_after_burst() {
    let limiter = stophammer::api::build_rate_limiter(2, 2);
    let key = "127.0.0.1".to_string();

    // First 2 should succeed (burst=2).
    assert!(limiter.check_key(&key).is_ok(), "request 1 should pass");
    assert!(limiter.check_key(&key).is_ok(), "request 2 should pass");

    // 3rd should fail (burst exhausted, no time for replenish).
    assert!(
        limiter.check_key(&key).is_err(),
        "request 3 should be rate-limited"
    );
}

/// Different IPs should have independent rate limit buckets.
#[test]
fn governor_per_ip_isolation() {
    let limiter = stophammer::api::build_rate_limiter(1, 1);

    let ip_a = "10.0.0.1".to_string();
    let ip_b = "10.0.0.2".to_string();

    assert!(
        limiter.check_key(&ip_a).is_ok(),
        "ip_a request 1 should pass"
    );
    assert!(
        limiter.check_key(&ip_a).is_err(),
        "ip_a request 2 should be limited"
    );

    // ip_b is independent — should still pass.
    assert!(
        limiter.check_key(&ip_b).is_ok(),
        "ip_b request 1 should pass"
    );
}

/// `rate_limit_config()` returns defaults when env vars are absent.
/// We cannot safely set/unset env vars in the 2024 edition without `unsafe`,
/// so this test verifies the function returns reasonable values and does not panic.
#[test]
fn rate_limit_config_does_not_panic() {
    let (rps, burst) = stophammer::api::rate_limit_config();
    assert!(rps > 0, "rps should be positive");
    assert!(burst > 0, "burst should be positive");
    assert!(burst >= rps, "burst should be >= rps");
}

/// The `/health` endpoint must not be behind the rate limiter layer in production.
/// Since rate limiting is applied in main.rs (not inside `build_router`), the router
/// itself should let /health through regardless of request count.
#[tokio::test]
async fn health_endpoint_not_rate_limited_in_router() {
    use http::Request;
    use tower::ServiceExt;

    let db = common::test_db_arc();
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-sp03-signer.key").unwrap(),
    );
    let pubkey = signer.pubkey_hex().to_string();
    let state = Arc::new(stophammer::api::AppState {
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
    });

    let app = stophammer::api::build_router(state);

    // Hit /health 200 times — all should return 200 (no rate limiter in build_router).
    for i in 0..200 {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            200,
            "health check #{i} failed with {}",
            resp.status()
        );
    }
}
