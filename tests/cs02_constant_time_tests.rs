// CS-02 constant-time admin token comparison tests — 2026-03-12

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

/// Build a full `AppState` backed by the given DB with a specific admin token.
fn test_app_state_with_token(
    db: Arc<Mutex<rusqlite::Connection>>,
    admin_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-cs02-signer"));
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

// ---------------------------------------------------------------------------
// CS-02-01: Correct admin token is accepted (constant-time comparison)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn correct_admin_token_accepted() {
    let db = common::test_db_arc();
    let state = test_app_state_with_token(Arc::clone(&db), "secret-admin-token-42");
    let app = stophammer::api::build_router(state);

    // Use the admin merge endpoint which requires X-Admin-Token.
    // The merge will fail with 500 (artists don't exist), but auth should pass.
    // A 403 would mean auth failed; anything else means auth passed.
    let req = Request::builder()
        .method("POST")
        .uri("/admin/artists/merge")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "secret-admin-token-42")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "source_artist_id": "nonexistent-a1",
                "target_artist_id": "nonexistent-a2",
            }))
            .expect("json"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("request");
    // If the admin auth passes, we get 500 (no such artists). If it fails, 403.
    assert_ne!(
        resp.status(),
        403,
        "correct admin token should not be rejected"
    );
}

// ---------------------------------------------------------------------------
// CS-02-02: Wrong admin token is rejected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn wrong_admin_token_rejected() {
    let db = common::test_db_arc();
    let state = test_app_state_with_token(Arc::clone(&db), "correct-token-xyz");
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/admin/artists/merge")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "wrong-token-abc")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "source_artist_id": "a1",
                "target_artist_id": "a2",
            }))
            .expect("json"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("request");
    assert_eq!(resp.status(), 403, "wrong admin token should be rejected");
}

// ---------------------------------------------------------------------------
// CS-02-03: Prefix of correct token is rejected (timing-safe)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prefix_admin_token_rejected() {
    let db = common::test_db_arc();
    let state = test_app_state_with_token(Arc::clone(&db), "correct-token-xyz");
    let app = stophammer::api::build_router(state);

    // Send only a prefix of the correct token.
    let req = Request::builder()
        .method("POST")
        .uri("/admin/artists/merge")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "correct-token")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "source_artist_id": "a1",
                "target_artist_id": "a2",
            }))
            .expect("json"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("request");
    assert_eq!(
        resp.status(),
        403,
        "prefix of correct admin token must be rejected"
    );
}

// ---------------------------------------------------------------------------
// CS-02-04: Empty admin token header is rejected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_admin_token_rejected() {
    let db = common::test_db_arc();
    let state = test_app_state_with_token(Arc::clone(&db), "correct-token-xyz");
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/admin/artists/merge")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "source_artist_id": "a1",
                "target_artist_id": "a2",
            }))
            .expect("json"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("request");
    assert_eq!(resp.status(), 403, "empty admin token should be rejected");
}
