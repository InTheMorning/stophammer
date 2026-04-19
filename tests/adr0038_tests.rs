mod common;

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tower::ServiceExt;

fn test_app_state_with_crawl_token(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0038-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    // Build a verifier chain with only crawl_token (skip medium_music so we can test the listing filter)
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

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "ADR0038 end-to-end regression keeps related ingest and query assertions together"
)]
async fn test_adr0038_track_remote_items_and_whitelist() {
    let crawl_token = "adr0038-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    // 1. Ingest a music feed with track-level remoteItem
    let music_feed_guid = "music-feed-1";
    let track_guid = "track-1";
    let pub_guid = "publisher-feed-1";

    let ingest_payload = serde_json::json!({
        "canonical_url": "https://music.example/rss",
        "source_url": "https://music.example/rss",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "hash-1",
        "feed_data": {
            "feed_guid": music_feed_guid,
            "title": "Music Feed",
            "raw_medium": "music",
            "image_url": "https://example.com/art.jpg",
            "language": "en",
            "explicit": false,
            "tracks": [{
                "track_guid": track_guid,
                "title": "Music Track",
                "remote_items": [{
                    "position": 0,
                    "medium": "publisher",
                    "remote_feed_guid": pub_guid,
                    "remote_feed_url": "https://pub.example/rss"
                }],
                "enclosure_url": "https://example.com/ep1.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 123,
                "explicit": false
            }]
        }
    });

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&ingest_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 2. Ingest a non-music feed
    let podcast_feed_guid = "podcast-feed-1";
    let ingest_payload2 = serde_json::json!({
        "canonical_url": "https://podcast.example/rss",
        "source_url": "https://podcast.example/rss",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "hash-2",
        "feed_data": {
            "feed_guid": podcast_feed_guid,
            "title": "Podcast Feed",
            "raw_medium": "podcast",
            "image_url": "https://example.com/art.jpg",
            "language": "en",
            "explicit": false,
            "tracks": []
        }
    });

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&ingest_payload2).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 3. Verify track-level remote_items and publisher include
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/v1/tracks/{track_guid}?include=remote_items,publisher"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let track_resp = body_json(resp).await;
    let track_data = &track_resp["data"];

    assert_eq!(track_data["track_guid"], track_guid);
    assert_eq!(track_data["remote_items"].as_array().unwrap().len(), 1);
    assert_eq!(track_data["remote_items"][0]["remote_feed_guid"], pub_guid);

    // publisher include should show the direction (music_to_publisher)
    assert_eq!(track_data["publisher"].as_array().unwrap().len(), 1);
    assert_eq!(
        track_data["publisher"][0]["direction"],
        "music_to_publisher"
    );
    assert_eq!(track_data["publisher"][0]["remote_feed_guid"], pub_guid);

    // 4. Verify /v1/feeds/recent whitelist (should only show music feed)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/feeds/recent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let recent_resp = body_json(resp).await;
    let recent_feeds = recent_resp["data"].as_array().unwrap();
    assert!(
        recent_feeds
            .iter()
            .any(|f| f["feed_guid"] == music_feed_guid)
    );
    assert!(
        !recent_feeds
            .iter()
            .any(|f| f["feed_guid"] == podcast_feed_guid)
    );

    // 5. Verify /v1/search whitelist
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/search?q=Music")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let search_resp = body_json(resp).await;
    let search_results = search_resp["data"].as_array().unwrap();
    assert!(
        search_results
            .iter()
            .any(|r| r["entity_id"] == music_feed_guid)
    );

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/search?q=Podcast")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let search_resp2 = body_json(resp).await;
    let search_results2 = search_resp2["data"].as_array().unwrap();
    assert!(search_results2.is_empty());
}
