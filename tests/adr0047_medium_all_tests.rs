// ADR 0047: a corrective pass takes its corpus from the node's feed list, so
// that list must be able to name every medium the index holds. Without `all`,
// a pass silently covers the music feeds and omits the rest.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0047-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec {
        names: vec!["crawl_token".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

async fn ingest(app: axum::Router, token: &str, guid: &str, medium: &str) {
    let payload = serde_json::json!({
        "canonical_url": format!("https://example.com/{guid}.xml"),
        "source_url": format!("https://example.com/{guid}.xml"),
        "crawl_token": token,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": format!("Feed {guid}"),
            "raw_medium": medium,
            "explicit": false,
            "tracks": []
        }
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert!(
        resp.status().is_success(),
        "seeding {medium} feed {guid} must succeed, got {}",
        resp.status()
    );
}

async fn recent_guids(app: axum::Router, query: &str) -> Vec<String> {
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/feeds/recent{query}"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert!(resp.status().is_success(), "recent feeds must succeed");
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|f| f["feed_guid"].as_str().expect("feed_guid").to_string())
        .collect()
}

#[tokio::test]
async fn medium_all_returns_every_medium_the_index_holds() {
    let token = "adr0047-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);

    for (guid, medium) in [
        ("adr0047-music", "music"),
        ("adr0047-publisher", "publisher"),
        ("adr0047-musicl", "musicL"),
    ] {
        ingest(
            stophammer::api::build_router(Arc::clone(&st)),
            token,
            guid,
            medium,
        )
        .await;
    }

    let default = recent_guids(stophammer::api::build_router(Arc::clone(&st)), "?limit=50").await;
    assert!(
        !default.contains(&"adr0047-publisher".to_string()),
        "the default is music, so a publisher feed must be absent"
    );

    let all = recent_guids(
        stophammer::api::build_router(Arc::clone(&st)),
        "?limit=50&medium=all",
    )
    .await;
    for guid in ["adr0047-music", "adr0047-publisher", "adr0047-musicl"] {
        assert!(
            all.contains(&guid.to_string()),
            "medium=all must return {guid}, got {all:?}"
        );
    }
}

#[tokio::test]
async fn medium_all_is_case_insensitive_and_named_mediums_still_filter() {
    let token = "adr0047-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);

    for (guid, medium) in [
        ("adr0047-case-music", "music"),
        ("adr0047-case-publisher", "publisher"),
    ] {
        ingest(
            stophammer::api::build_router(Arc::clone(&st)),
            token,
            guid,
            medium,
        )
        .await;
    }

    let upper = recent_guids(
        stophammer::api::build_router(Arc::clone(&st)),
        "?limit=50&medium=ALL",
    )
    .await;
    assert!(
        upper.contains(&"adr0047-case-publisher".to_string()),
        "medium=ALL must behave as medium=all"
    );

    let only_publisher = recent_guids(
        stophammer::api::build_router(st),
        "?limit=50&medium=publisher",
    )
    .await;
    assert_eq!(
        only_publisher,
        vec!["adr0047-case-publisher".to_string()],
        "a named medium must still filter to that medium alone"
    );
}
