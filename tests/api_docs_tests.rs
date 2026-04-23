mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state() -> Arc<stophammer::api::AppState> {
    let db = common::test_db_arc();
    let key_path = format!("/tmp/test-openapi-{}.key", uuid::Uuid::new_v4());
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

async fn body_text(resp: axum::response::Response) -> String {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    String::from_utf8(bytes.to_vec()).expect("utf8 body")
}

#[tokio::test]
async fn primary_router_serves_api_explorer_html() {
    let app = stophammer::api::build_router(test_app_state());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), 200);
    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .expect("content-type")
        .to_str()
        .expect("content-type str");
    assert!(content_type.starts_with("text/html"));

    let body = body_text(response).await;
    assert!(body.contains("Stophammer API"));
    assert!(body.contains("/openapi.json"));
}

#[tokio::test]
async fn primary_router_serves_primary_openapi_json() {
    let app = stophammer::api::build_router(test_app_state());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), 200);
    let body = body_text(response).await;
    let json: serde_json::Value = serde_json::from_str(&body).expect("parse openapi json");

    assert_eq!(json["info"]["title"], "Stophammer API");
    assert!(json["paths"]["/ingest/feed"]["post"].is_object());
    assert!(json["paths"]["/sync/push"].is_null());
    assert!(json["paths"]["/v1/feeds/{guid}/tracks/{track_guid}"]["get"].is_object());
    assert!(json["paths"]["/v1/feeds/{guid}/tracks/{track_guid}"]["patch"].is_object());
    assert_eq!(
        json["components"]["securitySchemes"]["AdminToken"]["name"],
        "X-Admin-Token"
    );
}

#[tokio::test]
async fn readonly_router_serves_readonly_openapi_json_without_primary_mutations() {
    let app = stophammer::api::build_readonly_router(test_app_state());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), 200);
    let body = body_text(response).await;
    let json: serde_json::Value = serde_json::from_str(&body).expect("parse openapi json");

    assert!(json["paths"]["/sync/register"].is_null());
    assert!(json["paths"]["/v1/proofs/challenge"].is_null());
    assert!(json["paths"]["/v1/feeds/{guid}"]["patch"].is_null());
    assert!(json["paths"]["/v1/feeds/{guid}/tracks/{track_guid}"]["get"].is_object());
    assert!(json["paths"]["/v1/feeds/{guid}/tracks/{track_guid}"]["patch"].is_null());
    assert_eq!(json["paths"]["/v1/search"]["get"]["tags"][0], "Search");
}
