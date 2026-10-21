// ADR 0076 §2: the named feed filter of the release lists.
//
// `GET /v1/feeds/by-name` and `GET /v1/feeds/recent` take `named_feed_guid`.
// With it, each route gives only the music releases with a `named_feeds`
// entry for that GUID, at each rank, confirmed or not.

mod common;

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const ROUTES: [&str; 2] = ["/v1/feeds/recent", "/v1/feeds/by-name"];

const TOKEN: &str = "adr0076-named-feed-filter-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0076-named-feed-filter-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0076-named-feed-filter-admin-token".into(),
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
fn names(position: i64, publisher: &str) -> Value {
    json!({
        "position": position,
        "medium": "publisher",
        "remote_feed_guid": publisher,
        "remote_feed_url": format!("https://example.com/{publisher}.xml"),
        "publisher_reference": true,
    })
}

/// An item of a publisher feed that lists `album`.
fn lists(position: i64, album: &str) -> Value {
    json!({
        "position": position,
        "medium": "music",
        "remote_feed_guid": album,
        "remote_feed_url": format!("https://example.com/{album}.xml"),
    })
}

/// Stores these feeds:
///
/// - `album-two` names `label-a` at rank 1, then `artist-b` at rank 2. Both
///   publisher feeds list it back, so each link is confirmed.
/// - `album-ghost` names `ghost`, a feed that the node does not hold, at
///   rank 1, then `label-a` at rank 2.
/// - `album-none` names no feed.
/// - `album-oneway` names `label-c`, a held feed that does not list
///   `album-oneway` back.
async fn stored() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    ingest_as(
        &st,
        "album-two",
        "music",
        "Artist B",
        json!([names(0, "label-a"), names(1, "artist-b")]),
    )
    .await;
    ingest_as(
        &st,
        "album-ghost",
        "music",
        "Artist C",
        json!([names(0, "ghost"), names(1, "label-a")]),
    )
    .await;
    ingest_as(&st, "album-none", "music", "Artist D", json!([])).await;
    ingest_as(
        &st,
        "album-oneway",
        "music",
        "Artist E",
        json!([names(0, "label-c")]),
    )
    .await;
    ingest_as(
        &st,
        "label-a",
        "publisher",
        "Label A",
        json!([lists(0, "album-two"), lists(1, "album-ghost")]),
    )
    .await;
    ingest_as(
        &st,
        "artist-b",
        "publisher",
        "Artist B",
        json!([lists(0, "album-two")]),
    )
    .await;
    ingest_as(&st, "label-c", "publisher", "Label C", json!([])).await;
    st
}

/// Stores a held publisher feed `label-a`, and `album-url-match`: an album
/// whose one publisher item declares a `feedGuid` the node does not index,
/// with a `feedUrl` equal to the stored `feed_url` of `label-a`. This fixture
/// is separate from `stored()` so that this album does not change the exact
/// sets the other tests assert for `named_feed_guid=label-a` (ADR 0076 §2).
async fn stored_url_match() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    ingest_as(&st, "label-a", "publisher", "Label A", json!([])).await;
    ingest_as(
        &st,
        "album-url-match",
        "music",
        "Artist F",
        json!([{
            "position": 0,
            "medium": "publisher",
            "remote_feed_guid": "label-a-not-indexed",
            "remote_feed_url": "https://example.com/label-a.xml",
            "publisher_reference": true,
        }]),
    )
    .await;
    st
}

/// The row of `uri` with `feed_guid` equal to `guid`.
async fn row(st: &Arc<stophammer::api::AppState>, uri: &str, guid: &str) -> Value {
    let body = send(st, "GET", uri, None).await;
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|r| r["feed_guid"] == guid)
        .unwrap_or_else(|| panic!("{uri}: no row for {guid} in {body}"))
        .clone()
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

fn guid_set(body: &Value) -> BTreeSet<String> {
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .map(|r| r["feed_guid"].as_str().expect("feed_guid").to_string())
        .collect()
}

#[tokio::test]
async fn the_filter_gives_each_release_that_names_the_feed_at_each_rank() {
    let st = stored().await;
    for route in ROUTES {
        let body = send(
            &st,
            "GET",
            &format!("{route}?named_feed_guid=label-a&limit=200"),
            None,
        )
        .await;
        assert_eq!(
            guid_set(&body),
            ["album-two", "album-ghost"]
                .into_iter()
                .map(str::to_string)
                .collect::<BTreeSet<_>>(),
            "{route}: ADR 0076 §2 gives each release that names label-a, at rank 1 or rank 2: {body}"
        );
    }
}

#[tokio::test]
async fn a_release_that_names_a_feed_the_feed_does_not_list_is_in_the_result() {
    let st = stored().await;
    for route in ROUTES {
        let body = send(
            &st,
            "GET",
            &format!("{route}?named_feed_guid=label-c&limit=200"),
            None,
        )
        .await;
        assert_eq!(
            guid_set(&body),
            ["album-oneway"].into_iter().map(str::to_string).collect(),
            "{route}: ADR 0076 §2: album-oneway names label-c, and label-c does not list it \
             back, but the release is still in the result: {body}"
        );
    }
}

#[tokio::test]
async fn a_release_that_names_a_feed_the_node_does_not_hold_is_in_the_result() {
    let st = stored().await;
    for route in ROUTES {
        let body = send(
            &st,
            "GET",
            &format!("{route}?named_feed_guid=ghost&limit=200"),
            None,
        )
        .await;
        assert_eq!(
            guid_set(&body),
            ["album-ghost"].into_iter().map(str::to_string).collect(),
            "{route}: ADR 0076 §2: the node does not hold ghost, but album-ghost names it, \
             so it is in the result for that GUID: {body}"
        );
    }
}

#[tokio::test]
async fn pages_of_one_give_each_release_one_time_with_no_gap() {
    let st = stored().await;
    for route in ROUTES {
        let guids = all_pages(&st, &format!("{route}?named_feed_guid=label-a")).await;
        let unique: BTreeSet<&String> = guids.iter().collect();
        assert_eq!(
            unique.len(),
            guids.len(),
            "{route}: ADR 0076 §2: a cursor continues with no repeat: {guids:?}"
        );
        assert_eq!(
            unique,
            ["album-two", "album-ghost"]
                .iter()
                .map(|g| (*g).to_string())
                .collect::<Vec<_>>()
                .iter()
                .collect(),
            "{route}: ADR 0076 §2: a page of limit=1 gives each release that names \
             label-a, with no gap: {guids:?}"
        );
    }
}

#[tokio::test]
async fn a_guid_that_no_release_names_gives_an_empty_list() {
    let st = stored().await;
    for route in ROUTES {
        let body = send(
            &st,
            "GET",
            &format!("{route}?named_feed_guid=nobody-names-this"),
            None,
        )
        .await;
        assert_eq!(
            body["data"],
            json!([]),
            "{route}: ADR 0076 §2: a GUID that no release names gives an empty list: {body}"
        );
    }
}

#[tokio::test]
async fn the_filter_with_another_medium_gives_400() {
    let st = stored().await;
    for route in ROUTES {
        assert_eq!(
            status_of(
                &st,
                &format!("{route}?named_feed_guid=label-a&medium=publisher")
            )
            .await,
            400,
            "{route}: ADR 0076 §2: named_feed_guid needs medium=music"
        );
    }
}

#[tokio::test]
async fn the_filter_with_artist_key_gives_400() {
    let st = stored().await;
    for route in ROUTES {
        assert_eq!(
            status_of(
                &st,
                &format!("{route}?named_feed_guid=label-a&artist_key=artist%20b")
            )
            .await,
            400,
            "{route}: ADR 0076 §2: named_feed_guid conflicts with artist_key"
        );
    }
}

#[tokio::test]
async fn the_list_without_the_filter_does_not_change() {
    let st = stored().await;
    for route in ROUTES {
        let body = send(&st, "GET", &format!("{route}?medium=music&limit=200"), None).await;
        assert_eq!(
            guid_set(&body),
            ["album-two", "album-ghost", "album-none", "album-oneway"]
                .into_iter()
                .map(str::to_string)
                .collect::<BTreeSet<_>>(),
            "{route}: without named_feed_guid, the release list gives each music feed: {body}"
        );
    }
}

#[tokio::test]
async fn the_url_step_finds_an_album_with_an_unindexed_feed_guid() {
    let st = stored_url_match().await;
    for route in ROUTES {
        let body = send(
            &st,
            "GET",
            &format!("{route}?named_feed_guid=label-a&limit=200"),
            None,
        )
        .await;
        assert_eq!(
            guid_set(&body),
            ["album-url-match"]
                .into_iter()
                .map(str::to_string)
                .collect::<BTreeSet<_>>(),
            "{route}: ADR 0076 §2: the album's item declares a feedGuid the node \
             does not index, but its feedUrl is the stored feed_url of label-a, so \
             the URL step of get_feeds_naming_feed finds it: {body}"
        );

        let r = row(
            &st,
            &format!("{route}?limit=200&include=named_feeds"),
            "album-url-match",
        )
        .await;
        let entries = r["named_feeds"]
            .as_array()
            .expect("named_feeds is an array");
        assert_eq!(
            entries[0]["publisher_feed_guid"], "label-a",
            "{route}: the named_feeds entry resolves the unindexed feedGuid to \
             label-a through the matching feedUrl: {r}"
        );
    }
}

#[tokio::test]
async fn the_document_declares_named_feed_guid_on_both_routes() {
    let doc = stophammer::openapi::primary_document();
    let value = serde_json::to_value(&doc).expect("serialize OpenAPI document");
    for route in ROUTES {
        let parameters = value["paths"][route]["get"]["parameters"]
            .as_array()
            .unwrap_or_else(|| panic!("ADR 0076 §2: GET {route} must list parameters"));
        assert!(
            parameters.iter().any(|p| p["name"] == "named_feed_guid"),
            "ADR 0076 §2: GET {route} must declare named_feed_guid"
        );
    }
}
