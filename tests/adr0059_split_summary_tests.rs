// ADR 0059 §7: a row of `include=value_time_splits` gives the summary of the
// feed and of the track that the split names, read by GUID only.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0059-split-summary-crawl-token";
const FEED_GUID: &str = "adr0059-host-feed";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0059-split-summary-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0059-split-summary-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

/// Sends one ingest of a feed with `extra` fields merged into `feed_data`.
async fn ingest(st: &Arc<stophammer::api::AppState>, guid: &str, url: &str, extra: Value) {
    let mut feed_data = json!({
        "feed_guid": guid,
        "title": format!("Feed {guid}"),
        "raw_medium": "music",
        "explicit": false,
        "author_name": "Some Artist",
    });
    for (key, value) in extra.as_object().expect("extra is an object") {
        feed_data[key] = value.clone();
    }
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": feed_data,
    });
    let req = Request::builder()
        .method("POST")
        .uri("/ingest/feed")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
        .expect("build request");
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(req)
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert!(status.is_success(), "ingest failed with {status}: {body}");
    assert_eq!(
        body["accepted"], true,
        "the ingest must be accepted: {body}"
    );
}

fn track(guid: &str, title: &str, duration: Option<i64>, image: Option<&str>) -> Value {
    json!({
        "track_guid": guid,
        "title": title,
        "pub_date": 1_700_000_000,
        "duration_secs": duration,
        "image_url": image,
        "explicit": false,
        "payment_routes": [],
        "value_time_splits": [],
    })
}

/// Reads the host track with `include=value_time_splits`.
async fn get_track(st: &Arc<stophammer::api::AppState>, track_guid: &str) -> Value {
    let uri = format!("/v1/feeds/{FEED_GUID}/tracks/{track_guid}?include=value_time_splits");
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("build request");
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(req)
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert!(
        status.is_success(),
        "GET track failed with {status}: {body}"
    );
    body["data"].clone()
}

const FEED_KEYS: [&str; 5] = [
    "remote_feed_title",
    "remote_feed_image_url",
    "remote_release_artist",
    "remote_release_artist_source",
    "remote_episode_count",
];
const TRACK_KEYS: [&str; 4] = [
    "remote_track_guid",
    "remote_track_title",
    "remote_track_duration_secs",
    "remote_track_image_url",
];

fn split(feed: &str, item: Option<&str>) -> Value {
    let mut v = json!({
        "form": "remote",
        "start_time_secs": 10,
        "duration_secs": 30,
        "remote_feed_guid": feed,
    });
    if let Some(item) = item {
        v["remote_item_guid"] = json!(item);
    }
    v
}

/// Stores the remote album and a host feed with one track that has `splits`.
/// Returns the one split row of the host track.
async fn row_for(album_image: &str, splits: Value) -> Value {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        "album-r",
        "https://a.example/r.xml",
        json!({
            "image_url": album_image,
            "tracks": [
                track("t-full", "Full Track", Some(245), Some("https://img.example/t.jpg")),
                track("t-js", "Script Track", Some(100), Some("javascript:alert(1)")),
            ],
        }),
    )
    .await;
    let mut host = track("host-track", "Host Track", Some(400), None);
    host["value_time_splits"] = splits;
    ingest(
        &st,
        FEED_GUID,
        "https://a.example/host.xml",
        json!({ "tracks": [host] }),
    )
    .await;
    let data = get_track(&st, "host-track").await;
    let rows = data["value_time_splits"].as_array().expect("an array");
    assert_eq!(rows.len(), 1, "one remote row: {rows:?}");
    rows[0].clone()
}

fn assert_all_null(row: &Value, keys: &[&str]) {
    for key in keys {
        assert!(
            row.get(*key).is_some_and(Value::is_null),
            "ADR 0059 §7: {key} must be present with null, got {row}"
        );
    }
}

#[tokio::test]
async fn a_split_gives_the_feed_fields_and_the_track_fields() {
    let row = row_for(
        "https://img.example/album.jpg",
        json!([split("album-r", Some("t-full"))]),
    )
    .await;
    assert_eq!(row["remote_feed_title"], "Feed album-r");
    assert_eq!(
        row["remote_feed_image_url"],
        "https://img.example/album.jpg"
    );
    assert_eq!(row["remote_release_artist"], "Some Artist");
    assert!(
        row["remote_release_artist_source"].is_string(),
        "ADR 0059 §1: the source is a string, got {row}"
    );
    assert!(
        row.get("remote_episode_count").is_some(),
        "ADR 0059 §1: the key is present, got {row}"
    );
    assert_eq!(row["remote_track_guid"], "t-full");
    assert_eq!(row["remote_track_title"], "Full Track");
    assert_eq!(row["remote_track_duration_secs"], 245);
    assert_eq!(row["remote_track_image_url"], "https://img.example/t.jpg");
    assert_eq!(row["remote_feed_guid"], "album-r", "ADR 0073 §4 stays");
    assert_eq!(row["remote_item_guid"], "t-full", "ADR 0073 §4 stays");
}

#[tokio::test]
async fn an_empty_item_guid_gives_the_feed_fields_and_null_track_fields() {
    let row = row_for(
        "https://img.example/album.jpg",
        json!([split("album-r", None)]),
    )
    .await;
    assert_eq!(row["remote_item_guid"], "");
    assert_eq!(row["remote_feed_title"], "Feed album-r");
    assert_all_null(&row, &TRACK_KEYS);
}

#[tokio::test]
async fn a_track_that_is_not_indexed_gives_null_track_fields() {
    let row = row_for(
        "https://img.example/album.jpg",
        json!([split("album-r", Some("t-missing"))]),
    )
    .await;
    assert_eq!(row["remote_feed_title"], "Feed album-r");
    assert_all_null(&row, &TRACK_KEYS);
}

#[tokio::test]
async fn a_feed_that_is_not_indexed_gives_null_in_the_nine_fields() {
    let row = row_for(
        "https://img.example/album.jpg",
        json!([split("album-missing", Some("t-full"))]),
    )
    .await;
    assert_eq!(row["remote_feed_guid"], "album-missing");
    assert_all_null(&row, &FEED_KEYS);
    assert_all_null(&row, &TRACK_KEYS);
}

#[tokio::test]
async fn a_javascript_image_gives_a_null_image() {
    let row = row_for(
        "javascript:alert(1)",
        json!([split("album-r", Some("t-js"))]),
    )
    .await;
    assert_eq!(row["remote_track_guid"], "t-js");
    assert!(
        row["remote_feed_image_url"].is_null(),
        "ADR 0054 §4: a script image gives null, got {row}"
    );
    assert!(
        row["remote_track_image_url"].is_null(),
        "ADR 0054 §4: a script image gives null, got {row}"
    );
}
