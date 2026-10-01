// ADR 0059 §5: an entry that names a track gives its summary.
//
// docs/tasks/adr-0059-task-002-track-summary.md
//
// A `remote_items` entry with a `remote_track_guid` gives `remote_track_title`,
// `remote_track_duration_secs` and `remote_track_image_url` of that track. Each
// is null when the index holds no such track.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0059-track-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0059-track-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0059-track-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
    })
}

async fn send(
    st: &Arc<stophammer::api::AppState>,
    method: &str,
    uri: &str,
    body: Option<&Value>,
) -> Value {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(value) => {
            req = req.header("Content-Type", "application/json");
            Body::from(serde_json::to_vec(value).expect("serialize"))
        }
        None => Body::empty(),
    };
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(req.body(body).expect("build request"))
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert!(
        status.is_success(),
        "{method} {uri} failed with {status}: {json}"
    );
    json
}

async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    url: &str,
    medium: &str,
    extra: Value,
) {
    let mut feed_data = json!({
        "feed_guid": guid,
        "title": format!("Feed {guid}"),
        "raw_medium": medium,
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
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
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

fn entry(position: i64, item_guid: &str) -> Value {
    json!({
        "position": position,
        "medium": "music",
        "remote_feed_guid": "album-s",
        "remote_feed_url": "https://a.example/s.xml",
        "item_guid": item_guid,
    })
}

/// Stores one album with three tracks and a playlist that names them, and
/// one track that is not indexed. Returns the playlist entries by item GUID.
async fn playlist_entries() -> HashMap<String, Value> {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        "album-s",
        "https://a.example/s.xml",
        "music",
        json!({
            "image_url": "https://img.example/album.jpg",
            "tracks": [
                track("t-full", "Full Track", Some(245), Some("https://img.example/t.jpg")),
                track("t-bare", "Bare Track", None, None),
                track("t-js", "Script Track", Some(100), Some("javascript:alert(1)")),
            ],
        }),
    )
    .await;
    ingest(
        &st,
        "list-s",
        "https://l.example/s.xml",
        "musicL",
        json!({ "remote_items": [
            entry(0, "t-full"),
            entry(1, "t-bare"),
            entry(2, "t-js"),
            entry(3, "t-missing"),
        ]}),
    )
    .await;
    let body = send(&st, "GET", "/v1/feeds/list-s?include=remote_items", None).await;
    body["data"]["remote_items"]
        .as_array()
        .expect("remote_items")
        .iter()
        .map(|e| {
            (
                e["remote_item_guid"]
                    .as_str()
                    .expect("item guid")
                    .to_string(),
                e.clone(),
            )
        })
        .collect()
}

#[tokio::test]
async fn an_entry_for_an_indexed_track_gives_its_title_duration_and_image() {
    let entries = playlist_entries().await;
    let full = &entries["t-full"];
    assert_eq!(full["remote_track_guid"], "t-full", "the track is indexed");
    assert_eq!(
        full["remote_track_title"], "Full Track",
        "ADR 0059 §5: the entry gives the stored title of the track"
    );
    assert_eq!(
        full["remote_track_duration_secs"], 245,
        "ADR 0059 §5: the entry gives the stored duration of the track"
    );
    assert_eq!(
        full["remote_track_image_url"], "https://img.example/t.jpg",
        "ADR 0059 §5: the entry gives the stored image of the item"
    );
}

#[tokio::test]
async fn a_track_with_no_image_and_no_duration_gives_only_its_title() {
    let entries = playlist_entries().await;
    let bare = &entries["t-bare"];
    assert_eq!(
        bare["remote_track_title"], "Bare Track",
        "ADR 0059 §5: the entry gives the title"
    );
    assert!(
        bare.get("remote_track_duration_secs")
            .is_some_and(Value::is_null),
        "ADR 0059 §5: a track with no duration gives the key with null"
    );
    assert!(
        bare.get("remote_track_image_url")
            .is_some_and(Value::is_null),
        "ADR 0059 §5: the image of the item, not the album image, so null"
    );
}

#[tokio::test]
async fn an_entry_for_a_track_that_is_not_indexed_gives_null() {
    let entries = playlist_entries().await;
    let missing = &entries["t-missing"];
    assert!(
        missing["remote_track_guid"].is_null(),
        "the track is not indexed"
    );
    for key in [
        "remote_track_title",
        "remote_track_duration_secs",
        "remote_track_image_url",
    ] {
        assert!(
            missing.get(key).is_some_and(Value::is_null),
            "ADR 0059 §5: an entry for a track that is not indexed gives {key} with null"
        );
    }
}

#[tokio::test]
async fn a_javascript_image_gives_a_null_image() {
    let entries = playlist_entries().await;
    let js = &entries["t-js"];
    assert_eq!(
        js["remote_track_title"], "Script Track",
        "the track is indexed"
    );
    assert!(
        js["remote_track_image_url"].is_null(),
        "ADR 0059 §5 and ADR 0054 §4: a non-web image gives null"
    );
}
