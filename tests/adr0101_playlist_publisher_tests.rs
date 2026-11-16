mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn publisher_with_musicl_items_is_ingested_and_read_back() {
    let crawl_token = "adr0101-crawl-token";
    let db = common::test_db_arc();
    let signer = Arc::new(common::temp_signer("adr0101-publisher"));
    let node_pubkey_hex = signer.pubkey_hex().to_string();
    let chain = stophammer::verify::build_chain(
        &stophammer::verify::ChainSpec {
            names: vec!["medium_music".into(), "v4v_payment".into()],
        },
        crawl_token.to_string(),
    );
    let state = Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(Arc::clone(&db)),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex,
        admin_token: "test-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    });
    let app = stophammer::api::build_router(state);
    let guid = "a778e463-ecc3-5219-8f05-f33b3983e173";
    let payload = serde_json::json!({
        "canonical_url": "https://example.com/chadf-publisher.xml",
        "source_url": "https://example.com/chadf-publisher.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "adr0101-publisher-hash",
        "feed_data": {
            "feed_guid": guid,
            "title": "ChadF MusicL Playlists",
            "description": null,
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "publisher",
            "author_name": "ChadF",
            "owner_name": null,
            "pub_date": 1_763_280_000,
            "remote_items": [
                {"position": 0, "medium": "musicL", "remote_feed_guid": "playlist-iam", "remote_feed_url": "https://example.com/iam.xml", "rel": "curator"},
                {"position": 1, "medium": "musicL", "remote_feed_guid": "playlist-mmm", "remote_feed_url": "https://example.com/mmm.xml", "rel": "curator"}
            ],
            "persons": [], "entity_ids": [], "links": [],
            "feed_payment_routes": [], "live_items": [], "tracks": []
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri("/ingest/feed")
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize payload"),
        ))
        .expect("build ingest request");
    let response = app.clone().oneshot(req).await.expect("ingest request");
    assert_eq!(response.status(), 200);

    let request = Request::builder()
        .uri(format!("/v1/feeds/{guid}?include=remote_items"))
        .body(Body::empty())
        .expect("build feed request");
    let response = app.oneshot(request).await.expect("feed request");
    assert_eq!(response.status(), 200);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("read feed response")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&body).expect("parse feed response");
    assert_eq!(body["data"]["raw_medium"], "publisher");
    assert_eq!(
        body["data"]["remote_items"]
            .as_array()
            .expect("remote items")
            .len(),
        2
    );
    assert!(
        body["data"]["remote_items"]
            .as_array()
            .expect("remote items")
            .iter()
            .all(|item| item["medium"] == "musicL")
    );
}
