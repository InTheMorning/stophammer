//! ADR 0094 §1: `medium` selects feed rows in each search path.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0094-search-medium-crawl-token";
const MUSIC_GUID: &str = "adr0094-search-medium-music";
const PLAYLIST_GUID: &str = "adr0094-search-medium-playlist";

type Db = Arc<Mutex<rusqlite::Connection>>;

fn state(db: Db) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0094-search-medium-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let chain = stophammer::verify::build_chain(
        &stophammer::verify::ChainSpec { names: vec![] },
        TOKEN.to_owned(),
    );
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "adr0094-search-medium-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn send(st: &Arc<stophammer::api::AppState>, uri: &str) -> (u16, Value) {
    let response = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = response.status().as_u16();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("read response")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&body).expect("parse response JSON"),
    )
}

async fn ingest(st: &Arc<stophammer::api::AppState>, guid: &str, medium: &str, tracks: &[&str]) {
    let tracks: Vec<Value> = tracks
        .iter()
        .map(|title| {
            json!({
                "track_guid": format!("{guid}-track"),
                "title": title,
                "pub_date": 1_700_000_000,
                "explicit": false,
            })
        })
        .collect();
    let url = format!("https://search-medium.example/{guid}.xml");
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": "Deck Darlings",
            "raw_medium": medium,
            "author_name": "Search Medium Artist",
            "explicit": false,
            "tracks": tracks,
        }
    });
    let response = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&payload).expect("encode payload"),
                ))
                .expect("build ingest request"),
        )
        .await
        .expect("send ingest request");
    assert!(
        response.status().is_success(),
        "ingest gave {}",
        response.status()
    );
}

fn rows(body: &Value) -> &[Value] {
    body["data"].as_array().expect("search data is an array")
}

fn has_id(body: &Value, guid: &str) -> bool {
    rows(body).iter().any(|row| row["entity_id"] == guid)
}

async fn fixture() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    ingest(&st, MUSIC_GUID, "music", &["Deck Track"]).await;
    ingest(&st, PLAYLIST_GUID, "musicL", &[]).await;
    st
}

#[tokio::test]
async fn medium_selects_feed_rows_in_rank_filter_and_sorted_searches() {
    let st = fixture().await;
    for query in ["q=Deck", "q=Darli"] {
        let (status, both) = send(
            &st,
            &format!("/v1/search?type=feed&medium=music,musicL&{query}"),
        )
        .await;
        assert_eq!(status, 200, "{both}");
        assert!(has_id(&both, MUSIC_GUID), "music feed hit: {both}");
        assert!(has_id(&both, PLAYLIST_GUID), "playlist hit: {both}");

        let (status, playlist) =
            send(&st, &format!("/v1/search?type=feed&medium=musicL&{query}")).await;
        assert_eq!(status, 200, "{playlist}");
        assert!(
            !has_id(&playlist, MUSIC_GUID),
            "music feed excluded: {playlist}"
        );
        assert!(has_id(&playlist, PLAYLIST_GUID), "playlist hit: {playlist}");
    }

    let (status, filtered) = send(&st, "/v1/search?type=feed&medium=musicL&title=Deck").await;
    assert_eq!(status, 200, "{filtered}");
    assert!(
        has_id(&filtered, PLAYLIST_GUID),
        "playlist filter hit: {filtered}"
    );

    for query in ["q=Deck", "q=Darli"] {
        let (status, sorted) = send(
            &st,
            &format!("/v1/search?type=feed&medium=musicL&sort=name&{query}"),
        )
        .await;
        assert_eq!(status, 200, "{sorted}");
        assert!(
            has_id(&sorted, PLAYLIST_GUID),
            "sorted playlist hit: {sorted}"
        );
    }
}

#[tokio::test]
async fn playlist_hit_maps_to_feed_and_keeps_its_medium() {
    let st = fixture().await;
    let (status, body) = send(&st, "/v1/search?type=feed&medium=musicL&q=Deck").await;
    assert_eq!(status, 200, "{body}");
    let row = rows(&body)
        .iter()
        .find(|row| row["entity_id"] == PLAYLIST_GUID)
        .expect("playlist row is present");
    assert_eq!(row["entity_type"], "feed");
    assert_eq!(row["medium"], "musicL");
}

#[tokio::test]
async fn no_type_playlist_medium_keeps_tracks_and_filters_music_feeds() {
    let st = fixture().await;
    let (status, body) = send(&st, "/v1/search?medium=musicL&q=Deck").await;
    assert_eq!(status, 200, "{body}");
    assert!(has_id(&body, PLAYLIST_GUID), "playlist hit: {body}");
    assert!(
        rows(&body)
            .iter()
            .any(|row| row["entity_type"] == "track" && row["feed_guid"] == MUSIC_GUID),
        "track hit: {body}"
    );
    assert!(!has_id(&body, MUSIC_GUID), "music feed excluded: {body}");

    let (status, body) = send(
        &st,
        "/v1/search?type=feed&medium=music,musicL&min_tracks=1&q=Deck",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(
        !has_id(&body, PLAYLIST_GUID),
        "playlist has no tracks: {body}"
    );
}

#[tokio::test]
async fn medium_errors_and_repeated_values_follow_search_contract() {
    let st = fixture().await;
    for path in [
        "/v1/search?type=track&medium=music&q=Deck",
        "/v1/search?type=publisher&medium=musicL&q=Deck",
    ] {
        let (status, body) = send(&st, path).await;
        assert_eq!(status, 400, "{path}: {body}");
        assert_eq!(body["code"], "unsupported_filter", "{path}: {body}");
        assert_eq!(body["filter"], "medium", "{path}: {body}");
    }
    for path in [
        "/v1/search?type=feed&medium=publisher&q=Deck",
        "/v1/search?type=feed&medium=&q=Deck",
        "/v1/search?type=feed&medium=music,,musicL&q=Deck",
    ] {
        let (status, body) = send(&st, path).await;
        assert_eq!(status, 400, "{path}: {body}");
        assert_eq!(body["code"], "invalid_filter", "{path}: {body}");
        assert_eq!(body["filter"], "medium", "{path}: {body}");
    }
    let (status, body) = send(
        &st,
        "/v1/search?type=feed&medium=music&medium=musicL&q=Deck",
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "duplicate parameter `medium`");
}

#[tokio::test]
async fn medium_cursor_pages_return_each_feed_once_and_capability_is_listed() {
    let st = fixture().await;
    let mut cursor: Option<String> = None;
    let mut ids = Vec::new();
    loop {
        let mut path = "/v1/search?type=feed&medium=music,musicL&q=Deck&limit=1".to_owned();
        if let Some(value) = cursor {
            path.push_str("&cursor=");
            path.push_str(&value);
        }
        let (status, body) = send(&st, &path).await;
        assert_eq!(status, 200, "{body}");
        ids.extend(
            rows(&body)
                .iter()
                .map(|row| row["entity_id"].as_str().unwrap().to_owned()),
        );
        cursor = body["pagination"]["cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    ids.sort();
    assert_eq!(ids, [MUSIC_GUID, PLAYLIST_GUID]);

    let (status, capabilities) = send(&st, "/v1/node/capabilities").await;
    assert_eq!(status, 200, "{capabilities}");
    assert_eq!(
        capabilities["capabilities"].as_array().unwrap().last(),
        Some(&json!("search_medium"))
    );
}
