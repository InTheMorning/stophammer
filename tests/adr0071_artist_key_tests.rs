// ADR 0071 §4: the releases of a name row.
//
// docs/tasks/adr-0071-task-003-artist-key-filter.md
//
// `GET /v1/feeds/by-name?artist_key=<key>` gives the releases that the `name`
// row with that `key` counts in `release_count`.

mod common;

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0071-artist-key-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0071-artist-key-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0071-artist-key-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
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

async fn ingest_as(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    medium: &str,
    author: &str,
    items: Value,
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
            "author_name": author,
            "remote_items": items,
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

/// An item of an album inside `<podcast:publisher>` that names `publisher`.
fn names(position: i64, publisher: &str, rel: Option<&str>) -> Value {
    let mut item = json!({
        "position": position,
        "medium": "publisher",
        "remote_feed_guid": publisher,
        "remote_feed_url": format!("https://example.com/{publisher}.xml"),
        "publisher_reference": true,
    });
    if let Some(rel) = rel {
        item["rel"] = json!(rel);
    }
    item
}

/// An item of a publisher feed that lists `album`.
fn lists(position: i64, album: &str, rel: Option<&str>) -> Value {
    let mut item = json!({
        "position": position,
        "medium": "music",
        "remote_feed_guid": album,
        "remote_feed_url": format!("https://example.com/{album}.xml"),
    });
    if let Some(rel) = rel {
        item["rel"] = json!(rel);
    }
    item
}

/// Stores three name-level releases of "Luna Vale", and one release of the
/// same text whose link to `pub-luna` agrees on the role `artist`. The last
/// release opens the publisher level, so the name row does not count it.
async fn stored() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    for guid in ["luna-1", "luna-2", "luna-3"] {
        ingest_as(&st, guid, "music", "Luna Vale", json!([])).await;
    }
    ingest_as(
        &st,
        "luna-artist",
        "music",
        "Luna Vale",
        json!([names(0, "pub-luna", Some("artist"))]),
    )
    .await;
    ingest_as(&st, "other-1", "music", "Other Artist", json!([])).await;
    ingest_as(
        &st,
        "pub-luna",
        "publisher",
        "Luna Vale",
        json!([lists(0, "luna-artist", Some("artist"))]),
    )
    .await;
    st
}

async fn status_of(st: &Arc<stophammer::api::AppState>, uri: &str) -> u16 {
    stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response")
        .status()
        .as_u16()
}

/// Each feed GUID of each page of `uri`, with a page size of 1.
async fn all_pages(st: &Arc<stophammer::api::AppState>, uri: &str) -> Vec<String> {
    let mut guids = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page_uri = match &cursor {
            Some(c) => format!("{uri}&limit=1&cursor={c}"),
            None => format!("{uri}&limit=1"),
        };
        let body = send(st, "GET", &page_uri, None).await;
        for row in body["data"].as_array().expect("data is an array") {
            guids.push(row["feed_guid"].as_str().expect("feed_guid").to_string());
        }
        match body["pagination"]["cursor"].as_str() {
            Some(c) if body["pagination"]["has_more"] == true => cursor = Some(c.to_string()),
            _ => break,
        }
    }
    guids
}

#[tokio::test]
async fn the_filter_gives_the_releases_that_the_name_row_counts() {
    let st = stored().await;
    let names = send(&st, "GET", "/v1/artist-names?limit=200", None).await;
    let row = names["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|r| r["kind"] == "name" && r["key"] == "luna vale")
        .unwrap_or_else(|| panic!("no name row for luna vale in {names}"))
        .clone();
    assert_eq!(
        row["release_count"], 3,
        "three releases are of the name level: {row}"
    );

    let guids = all_pages(&st, "/v1/feeds/by-name?artist_key=luna%20vale").await;
    let unique: BTreeSet<&String> = guids.iter().collect();
    assert_eq!(
        unique.len(),
        guids.len(),
        "ADR 0071 §4: a cursor continues with no repeat: {guids:?}"
    );
    assert_eq!(
        unique,
        ["luna-1", "luna-2", "luna-3"]
            .iter()
            .map(|g| (*g).to_string())
            .collect::<Vec<_>>()
            .iter()
            .collect(),
        "ADR 0071 §4: the filter gives each release that release_count counts, and no other"
    );
}

#[tokio::test]
async fn a_release_with_an_agreed_artist_link_is_not_in_the_list() {
    let st = stored().await;
    let body = send(
        &st,
        "GET",
        "/v1/feeds/by-name?artist_key=luna%20vale&limit=200",
        None,
    )
    .await;
    let rows = body["data"].as_array().expect("data is an array");
    assert!(
        rows.iter().all(|r| r["feed_guid"] != "luna-artist"),
        "ADR 0070 §4: a release with an agreed artist link opens the publisher level: {body}"
    );
}

#[tokio::test]
async fn a_key_with_no_name_row_gives_an_empty_list() {
    let st = stored().await;
    let body = send(
        &st,
        "GET",
        "/v1/feeds/by-name?artist_key=nobody%20here",
        None,
    )
    .await;
    assert_eq!(
        body["data"],
        json!([]),
        "ADR 0071 §4: a key with no name row gives an empty list: {body}"
    );
}

#[tokio::test]
async fn the_filter_with_another_medium_gives_400() {
    let st = stored().await;
    assert_eq!(
        status_of(
            &st,
            "/v1/feeds/by-name?artist_key=luna%20vale&medium=publisher"
        )
        .await,
        400,
        "ADR 0071 §4: artist_key needs medium=music"
    );
}

#[tokio::test]
async fn the_list_without_the_filter_does_not_change() {
    let st = stored().await;
    let guids = all_pages(&st, "/v1/feeds/by-name?medium=music").await;
    assert_eq!(
        guids.len(),
        5,
        "without artist_key, the release list gives each music feed: {guids:?}"
    );
}
