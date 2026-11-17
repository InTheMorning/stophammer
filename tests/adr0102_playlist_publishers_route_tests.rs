//! Tests for the playlist publisher endpoint in ADR 0102 §6.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0102-playlist-publishers-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer(
        "test-adr0102-playlist-publishers-signer",
    ));
    let pubkey = signer.pubkey_hex().to_string();
    let chain = stophammer::verify::build_chain(
        &stophammer::verify::ChainSpec { names: vec![] },
        TOKEN.to_string(),
    );
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0102-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn send(st: &Arc<stophammer::api::AppState>, uri: &str, body: Option<Value>) -> Value {
    let request = Request::builder()
        .method(if body.is_some() { "POST" } else { "GET" })
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .expect("build request");
    let response = stophammer::api::build_router(Arc::clone(st))
        .oneshot(request)
        .await
        .expect("send request");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert!(status.is_success(), "{uri} returned {status}: {value}");
    value
}

async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    medium: &str,
    remote_items: Value,
) {
    let url = format!("https://example.com/{guid}.xml");
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": format!("Feed {guid}"),
            "raw_medium": medium,
            "explicit": false,
            "author_name": format!("Artist {guid}"),
            "remote_items": remote_items,
        }
    });
    let response = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(payload.to_string()))
                .expect("build ingest request"),
        )
        .await
        .expect("send ingest request");
    assert!(response.status().is_success(), "ingest of {guid} failed");
}

async fn add_publisher(
    st: &Arc<stophammer::api::AppState>,
    publisher: &str,
    playlist: &str,
    confirmed: bool,
) {
    let playlist_items = if confirmed {
        json!([{
            "position": 0,
            "medium": "publisher",
            "remote_feed_guid": publisher,
            "remote_feed_url": format!("https://example.com/{publisher}.xml"),
            "publisher_reference": true,
        }])
    } else {
        json!([])
    };
    ingest(st, playlist, "musicL", playlist_items).await;
    ingest(
        st,
        publisher,
        "publisher",
        json!([{
            "position": 0,
            "medium": "musicL",
            "remote_feed_guid": playlist,
            "remote_feed_url": format!("https://example.com/{playlist}.xml"),
            "rel": "curator",
        }]),
    )
    .await;
}

async fn setup() -> Arc<stophammer::api::AppState> {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    add_publisher(&st, "publisher-old", "playlist-old", true).await;
    add_publisher(&st, "publisher-new", "playlist-new", true).await;
    add_publisher(&st, "publisher-undated", "playlist-undated", true).await;
    add_publisher(&st, "publisher-one-way", "playlist-one-way", false).await;
    {
        let conn = db.lock().expect("lock test database");
        conn.execute(
            "UPDATE feeds SET release_date = 100 WHERE feed_guid = 'playlist-old'",
            [],
        )
        .expect("set old playlist date");
        conn.execute(
            "UPDATE feeds SET last_build_date = 200 WHERE feed_guid = 'playlist-new'",
            [],
        )
        .expect("set new playlist build date");
    }
    st
}

#[tokio::test]
async fn route_sorts_confirmed_playlists_and_paginates_rows() {
    let st = setup().await;
    let first_page = send(&st, "/v1/feeds/playlist-publishers?limit=1", None).await;
    assert_eq!(first_page["data"].as_array().unwrap().len(), 1);
    assert_eq!(first_page["data"][0]["feed_guid"], "publisher-new");
    assert_eq!(first_page["data"][0]["newest_playlist_at"], 200);
    assert_eq!(first_page["pagination"]["has_more"], true);
    let cursor = first_page["pagination"]["cursor"]
        .as_str()
        .expect("next page cursor");
    let second_page = send(
        &st,
        &format!("/v1/feeds/playlist-publishers?limit=1&cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(second_page["data"].as_array().unwrap().len(), 1);
    assert_eq!(second_page["data"][0]["feed_guid"], "publisher-old");
    assert_eq!(second_page["data"][0]["newest_playlist_at"], 100);

    let all_rows = send(&st, "/v1/feeds/playlist-publishers", None).await;
    assert_eq!(all_rows["data"].as_array().unwrap().len(), 3);
    assert_eq!(all_rows["data"][2]["feed_guid"], "publisher-undated");
    assert_eq!(all_rows["data"][2]["newest_playlist_at"], Value::Null);
    assert!(
        all_rows["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["feed_guid"] != "publisher-one-way")
    );
}
