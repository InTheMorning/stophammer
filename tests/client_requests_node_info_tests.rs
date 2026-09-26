// Client requests task 003: node info revision.
//
// docs/tasks/client-requests-task-003-node-info-revision.md
//
// GET /node/info gives git_revision and built_at fields.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-node-info-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: vec!["content_hash".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());

    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-node-info-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

async fn get(app: axum::Router, uri: &str) -> (http::StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = resp.status();
    (status, body_json(resp).await)
}

#[tokio::test]
async fn node_info_has_required_fields() {
    let crawl_token = "test-crawl-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let (status, body) = get(app, "/node/info").await;

    assert_eq!(
        status.as_u16(),
        200,
        "v4vmm request 2: /node/info must respond with 200"
    );

    assert!(
        body.get("node_pubkey").is_some(),
        "response must have node_pubkey field"
    );
    assert!(
        body.get("git_revision").is_some(),
        "v4vmm request 2: response must have git_revision field"
    );
    assert!(
        body.get("built_at").is_some(),
        "v4vmm request 2: response must have built_at field"
    );
}
