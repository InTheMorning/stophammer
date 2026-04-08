#![recursion_limit = "256"]

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state_with_chain(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
    signer_path: &std::path::Path,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create(signer_path).expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec {
        names: vec![
            "crawl_token".to_string(),
            "medium_music".to_string(),
            "v4v_payment".to_string(),
        ],
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

fn json_request(method: &str, uri: &str, body: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(body).expect("serialize")))
        .expect("build request")
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
async fn ingest_musicl_discards_tracks_and_skips_resolver_queue() {
    let crawl_token = "musicl-crawl-token";
    let db = common::test_db_arc();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("musicl-api.key");
    let state = test_app_state_with_chain(Arc::clone(&db), crawl_token, &signer_path);
    let app = stophammer::api::build_router(state);

    let payload = serde_json::json!({
        "canonical_url": "https://example.com/playlist.xml",
        "source_url": "https://example.com/playlist.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "musicl-hash-001",
        "feed_data": {
            "feed_guid": "feed-musicl-api",
            "title": "Playlist Feed",
            "description": "Curated remote playlist",
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "musicL",
            "author_name": "Playlist Curator",
            "owner_name": null,
            "pub_date": 1700000000,
            "remote_items": [{
                "position": 0,
                "medium": "music",
                "remote_feed_guid": "remote-feed-guid",
                "remote_feed_url": "https://example.com/remote.xml"
            }],
            "persons": [],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [],
            "live_items": [],
            "tracks": [{
                "track_guid": "track-musicl-api",
                "title": "Should Be Discarded",
                "pub_date": 1700000000,
                "duration_secs": 120,
                "enclosure_url": "https://cdn.example.com/discarded.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 12345,
                "alternate_enclosures": [],
                "track_number": 1,
                "season": null,
                "explicit": false,
                "description": null,
                "author_name": null,
                "persons": [],
                "entity_ids": [],
                "links": [],
                "payment_routes": [],
                "value_time_splits": []
            }]
        }
    });

    let resp = app
        .clone()
        .oneshot(json_request("POST", "/ingest/feed", &payload))
        .await
        .expect("ingest");
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["accepted"], true);
    assert_eq!(body["no_change"], false);

    let conn = db.lock().expect("lock db");
    let (raw_medium, episode_count): (String, i64) = conn
        .query_row(
            "SELECT raw_medium, episode_count FROM feeds WHERE feed_guid = ?1",
            ["feed-musicl-api"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("musicL feed");
    assert_eq!(raw_medium, "musicL");
    assert_eq!(
        episode_count, 0,
        "musicL feeds should not materialize tracks"
    );

    let track_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE feed_guid = ?1",
            ["feed-musicl-api"],
            |row| row.get(0),
        )
        .expect("track count");
    assert_eq!(track_count, 0, "musicL tracks should be discarded");

    let remote_item_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feed_remote_items_raw WHERE feed_guid = ?1",
            ["feed-musicl-api"],
            |row| row.get(0),
        )
        .expect("remote items");
    assert_eq!(
        remote_item_count, 1,
        "musicL remote items should be preserved"
    );
}

#[tokio::test]
async fn recent_feeds_default_to_music_and_allow_explicit_musicl_filter() {
    let crawl_token = "musicl-recent-crawl-token";
    let db = common::test_db_arc();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("musicl-recent.key");
    let state = test_app_state_with_chain(Arc::clone(&db), crawl_token, &signer_path);
    let app = stophammer::api::build_router(state);

    let music_payload = serde_json::json!({
        "canonical_url": "https://example.com/music.xml",
        "source_url": "https://example.com/music.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "music-hash-001",
        "feed_data": {
            "feed_guid": "feed-music-api",
            "title": "Music Feed",
            "description": null,
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "music",
            "author_name": "Music Artist",
            "owner_name": null,
            "pub_date": 1700000000,
            "remote_items": [],
            "persons": [],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [{
                "recipient_name": "Artist",
                "route_type": "keysend",
                "address": "03e7156ae33b0a208d0744199163177e909e80176e55d97a2f221ede0f934dd9ad",
                "custom_key": null,
                "custom_value": null,
                "split": 100,
                "fee": false
            }],
            "live_items": [],
            "tracks": [{
                "track_guid": "track-music-api",
                "title": "Music Track",
                "pub_date": 1700000000,
                "duration_secs": 180,
                "enclosure_url": "https://cdn.example.com/music.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 424242,
                "alternate_enclosures": [],
                "track_number": 1,
                "season": null,
                "explicit": false,
                "description": null,
                "author_name": null,
                "persons": [],
                "entity_ids": [],
                "links": [],
                "payment_routes": [],
                "value_time_splits": []
            }]
        }
    });

    let musicl_payload = serde_json::json!({
        "canonical_url": "https://example.com/musicl.xml",
        "source_url": "https://example.com/musicl.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "musicl-hash-002",
        "feed_data": {
            "feed_guid": "feed-musicl-recent",
            "title": "Playlist Feed",
            "description": null,
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "musicL",
            "author_name": "Playlist Curator",
            "owner_name": null,
            "pub_date": 1700000001,
            "remote_items": [{
                "position": 0,
                "medium": "music",
                "remote_feed_guid": "remote-feed-guid-2",
                "remote_feed_url": "https://example.com/remote-2.xml"
            }],
            "persons": [],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [],
            "live_items": [],
            "tracks": []
        }
    });

    for payload in [&music_payload, &musicl_payload] {
        let resp = app
            .clone()
            .oneshot(json_request("POST", "/ingest/feed", payload))
            .await
            .expect("ingest");
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        assert_eq!(body["accepted"], true);
    }

    let default_recent = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/feeds/recent")
                .body(Body::empty())
                .expect("recent request"),
        )
        .await
        .expect("recent response");
    assert_eq!(default_recent.status(), 200);
    let default_json = body_json(default_recent).await;
    assert_eq!(default_json["data"].as_array().map_or(0, Vec::len), 1);
    assert_eq!(default_json["data"][0]["feed_guid"], "feed-music-api");
    assert_eq!(default_json["data"][0]["raw_medium"], "music");

    let musicl_recent = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/feeds/recent?medium=musicL")
                .body(Body::empty())
                .expect("recent request"),
        )
        .await
        .expect("recent response");
    assert_eq!(musicl_recent.status(), 200);
    let musicl_json = body_json(musicl_recent).await;
    assert_eq!(musicl_json["data"].as_array().map_or(0, Vec::len), 1);
    assert_eq!(musicl_json["data"][0]["feed_guid"], "feed-musicl-recent");
    assert_eq!(musicl_json["data"][0]["raw_medium"], "musicL");
}

#[tokio::test]
async fn recent_feeds_support_default_music_and_explicit_musicl_filters() {
    let crawl_token = "musicl-artist-crawl-token";
    let db = common::test_db_arc();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("musicl-artist.key");
    let state = test_app_state_with_chain(Arc::clone(&db), crawl_token, &signer_path);
    let app = stophammer::api::build_router(state);

    let music_payload = serde_json::json!({
        "canonical_url": "https://example.com/artist-music.xml",
        "source_url": "https://example.com/artist-music.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "artist-music-hash-001",
        "feed_data": {
            "feed_guid": "feed-artist-music",
            "title": "Artist Music Feed",
            "description": null,
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "music",
            "author_name": "Shared Artist",
            "owner_name": null,
            "pub_date": 1700000000,
            "remote_items": [],
            "persons": [],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [{
                "recipient_name": "Artist",
                "route_type": "keysend",
                "address": "03e7156ae33b0a208d0744199163177e909e80176e55d97a2f221ede0f934dd9ad",
                "custom_key": null,
                "custom_value": null,
                "split": 100,
                "fee": false
            }],
            "live_items": [],
            "tracks": [{
                "track_guid": "track-artist-music",
                "title": "Artist Music Track",
                "pub_date": 1700000000,
                "duration_secs": 180,
                "enclosure_url": "https://cdn.example.com/artist-music.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 424242,
                "alternate_enclosures": [],
                "track_number": 1,
                "season": null,
                "explicit": false,
                "description": null,
                "author_name": null,
                "persons": [],
                "entity_ids": [],
                "links": [],
                "payment_routes": [],
                "value_time_splits": []
            }]
        }
    });

    let musicl_payload = serde_json::json!({
        "canonical_url": "https://example.com/artist-musicl.xml",
        "source_url": "https://example.com/artist-musicl.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "artist-musicl-hash-001",
        "feed_data": {
            "feed_guid": "feed-artist-musicl",
            "title": "Artist Playlist Feed",
            "description": null,
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "musicL",
            "author_name": "Shared Artist",
            "owner_name": null,
            "pub_date": 1700000001,
            "remote_items": [{
                "position": 0,
                "medium": "music",
                "remote_feed_guid": "artist-remote-feed-guid",
                "remote_feed_url": "https://example.com/artist-remote.xml"
            }],
            "persons": [],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [],
            "live_items": [],
            "tracks": []
        }
    });

    for payload in [&music_payload, &musicl_payload] {
        let resp = app
            .clone()
            .oneshot(json_request("POST", "/ingest/feed", payload))
            .await
            .expect("ingest");
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        assert_eq!(body["accepted"], true);
    }

    let default_recent_feeds = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/feeds/recent")
                .body(Body::empty())
                .expect("recent feeds request"),
        )
        .await
        .expect("recent feeds response");
    assert_eq!(default_recent_feeds.status(), 200);
    let default_json = body_json(default_recent_feeds).await;
    assert_eq!(default_json["data"].as_array().map_or(0, Vec::len), 1);
    assert_eq!(default_json["data"][0]["feed_guid"], "feed-artist-music");
    assert_eq!(default_json["data"][0]["raw_medium"], "music");

    let musicl_recent_feeds = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/feeds/recent?medium=musicL")
                .body(Body::empty())
                .expect("recent feeds request"),
        )
        .await
        .expect("recent feeds response");
    assert_eq!(musicl_recent_feeds.status(), 200);
    let musicl_json = body_json(musicl_recent_feeds).await;
    assert_eq!(musicl_json["data"].as_array().map_or(0, Vec::len), 1);
    assert_eq!(musicl_json["data"][0]["feed_guid"], "feed-artist-musicl");
    assert_eq!(musicl_json["data"][0]["raw_medium"], "musicL");
}
