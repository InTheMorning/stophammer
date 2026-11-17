// ADR 0102 §3 and §5: playlist link facts and confirmed publisher fields.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0102-playlist-count-token";
const PUBLISHER: &str = "pub-list";
const PLAYLIST: &str = "playlist-one";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0102-playlist-signer"));
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

async fn ingest(st: &Arc<stophammer::api::AppState>, guid: &str, medium: &str, items: Value) {
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
            "remote_items": items,
        }
    });
    send(st, "/ingest/feed", Some(payload)).await;
}

async fn setup() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        "album-one",
        "music",
        json!([{
            "position": 0, "medium": "publisher", "remote_feed_guid": PUBLISHER,
            "remote_feed_url": "https://example.com/pub-list.xml", "publisher_reference": true
        }]),
    )
    .await;
    ingest(
        &st,
        PLAYLIST,
        "musicL",
        json!([{
            "position": 0, "medium": "publisher", "remote_feed_guid": PUBLISHER,
            "remote_feed_url": "https://example.com/pub-list.xml", "publisher_reference": true
        }]),
    )
    .await;
    ingest(&st, "playlist-one-way", "musicL", json!([])).await;
    ingest(&st, PUBLISHER, "publisher", json!([
        {"position": 0, "medium": "music", "remote_feed_guid": "album-one", "remote_feed_url": "https://example.com/album-one.xml", "rel": "label"},
        {"position": 1, "medium": "musicL", "remote_feed_guid": PLAYLIST, "remote_feed_url": "https://example.com/playlist-one.xml", "rel": "curator"},
        {"position": 2, "medium": "musicL", "remote_feed_guid": "playlist-one-way", "remote_feed_url": "https://example.com/playlist-one-way.xml"}
    ])).await;
    st
}

fn find(rows: &Value, guid: &str) -> Value {
    rows["data"]
        .as_array()
        .expect("data array")
        .iter()
        .find(|row| row["feed_guid"] == guid)
        .expect("feed row")
        .clone()
}

#[tokio::test]
async fn publisher_playlist_count_and_curator_fields_follow_adr_0102() {
    let st = setup().await;
    let recent = send(
        &st,
        "/v1/feeds/recent?medium=publisher&include=link_facts",
        None,
    )
    .await;
    let publisher = find(&recent, PUBLISHER);
    assert_eq!(publisher["two_way_link_count"], 1);
    assert_eq!(publisher["two_way_playlist_link_count"], 1);
    assert_eq!(publisher["stated_rels"], json!(["label"]));
    assert_eq!(
        publisher["confirmed_release_artists"],
        json!(["Artist album-one"])
    );

    let detail = send(&st, &format!("/v1/feeds/{PUBLISHER}"), None).await;
    assert_eq!(detail["data"]["two_way_playlist_link_count"], 1);
    assert!(detail["data"].get("two_way_link_count").is_none());

    let by_name = send(
        &st,
        "/v1/feeds/by-name?medium=publisher&include=link_facts",
        None,
    )
    .await;
    assert_eq!(find(&by_name, PUBLISHER)["two_way_playlist_link_count"], 1);
    let artist = send(&st, "/v1/artist-names?include=link_facts", None).await;
    assert_eq!(find(&artist, PUBLISHER)["two_way_playlist_link_count"], 1);
    let search = send(
        &st,
        "/v1/search?type=publisher&q=pub-list&include=link_facts",
        None,
    )
    .await;
    assert_eq!(search["data"][0]["two_way_playlist_link_count"], 1);

    let recent_playlist = send(&st, "/v1/feeds/recent?medium=musicL", None).await;
    let playlist = find(&recent_playlist, PLAYLIST);
    assert_eq!(playlist["confirmed_publisher"]["feed_guid"], PUBLISHER);
    assert_eq!(playlist["confirmed_publisher"]["title"], "Feed pub-list");
    assert_eq!(playlist["confirmed_publisher"]["role"], "curator");
    assert_eq!(
        playlist["confirmed_publisher"]["role_agreement"],
        "one_side"
    );
    assert_eq!(
        find(&recent_playlist, "playlist-one-way").get("confirmed_publisher"),
        Some(&Value::Null)
    );
    assert!(
        find(
            &send(&st, "/v1/feeds/recent?medium=music", None).await,
            "album-one"
        )
        .get("confirmed_publisher")
        .is_none()
    );
    let by_name_playlist = send(&st, "/v1/feeds/by-name?medium=musicL", None).await;
    assert_eq!(
        find(&by_name_playlist, PLAYLIST)["confirmed_publisher"]["feed_guid"],
        PUBLISHER
    );
}
