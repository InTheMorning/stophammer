// ADR 0069 §3a: each `publisher` row gives `album_names_rank`.
//
// An album can name more than one publisher feed inside
// `<podcast:publisher>`. The rank is the one-based place of the matching
// item among the album's own publisher-link items, in `position` order.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0069-rank-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0069-rank-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0069-rank-admin-token".into(),
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

async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    url: &str,
    medium: &str,
    author: &str,
    remote_items: Value,
) {
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
            "remote_items": remote_items,
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

/// A `remoteItem` that an album gives for a publisher feed. `publisher` is
/// the parser mark of ADR 0069 §3: true for an item inside
/// `<podcast:publisher>`.
fn names(position: i64, publisher_guid: &str, publisher: bool) -> Value {
    json!({
        "position": position,
        "medium": "publisher",
        "remote_feed_guid": publisher_guid,
        "remote_feed_url": format!("https://p.example/{publisher_guid}.xml"),
        "publisher_reference": publisher,
    })
}

/// A feed-level `remoteItem` outside `<podcast:publisher>`, with a medium
/// that is neither `"publisher"` nor `"music"`, so it builds no `publisher`
/// row and does not take a `position` slot among the publisher-link items.
fn unrelated_item(position: i64) -> Value {
    json!({
        "position": position,
        "medium": "soundbite",
        "remote_feed_guid": "unrelated-feed",
        "remote_feed_url": "https://u.example/unrelated.xml",
    })
}

/// A `remoteItem` that a publisher feed gives for an album.
fn lists(position: i64, album_guid: &str) -> Value {
    json!({
        "position": position,
        "medium": "music",
        "remote_feed_guid": album_guid,
        "remote_feed_url": format!("https://a.example/{album_guid}.xml"),
    })
}

async fn album(st: &Arc<stophammer::api::AppState>, guid: &str, artist: &str, items: Value) {
    ingest(
        st,
        guid,
        &format!("https://a.example/{guid}.xml"),
        "music",
        artist,
        items,
    )
    .await;
}

async fn publisher(st: &Arc<stophammer::api::AppState>, guid: &str, name: &str, items: Value) {
    ingest(
        st,
        guid,
        &format!("https://p.example/{guid}.xml"),
        "publisher",
        name,
        items,
    )
    .await;
}

async fn read(st: &Arc<stophammer::api::AppState>, guid: &str) -> Value {
    send(
        st,
        "GET",
        &format!("/v1/feeds/{guid}?include=publisher"),
        None,
    )
    .await["data"]
        .clone()
}

fn row<'a>(data: &'a Value, other_guid: &str) -> &'a Value {
    data["publisher"]
        .as_array()
        .expect("publisher is an array")
        .iter()
        .find(|row| {
            row["publisher_feed_guid"] == other_guid || row["music_feed_guid"] == other_guid
        })
        .unwrap_or_else(|| panic!("no publisher row for {other_guid} in {data}"))
}

#[tokio::test]
async fn an_album_ranks_two_named_publishers_in_position_order() {
    let st = state(common::test_db_arc());
    album(
        &st,
        "album-1",
        "Artist One",
        json!([names(0, "label-a", true), names(1, "label-b", true)]),
    )
    .await;
    publisher(&st, "label-a", "Label A", json!([lists(0, "album-1")])).await;
    publisher(&st, "label-b", "Label B", json!([lists(0, "album-1")])).await;

    let data = read(&st, "album-1").await;
    let a = row(&data, "label-a");
    assert_eq!(a["album_names_as"], "publisher", "{a}");
    assert_eq!(a["album_names_rank"], 1, "{a}");
    let b = row(&data, "label-b");
    assert_eq!(b["album_names_as"], "publisher", "{b}");
    assert_eq!(b["album_names_rank"], 2, "{b}");

    // ADR 0069 §3a: both directions give the same rank for the same link.
    let label_b_read = read(&st, "label-b").await;
    let from_publisher_side = row(&label_b_read, "album-1");
    assert_eq!(
        from_publisher_side["album_names_rank"], 2,
        "{from_publisher_side}"
    );
}

#[tokio::test]
async fn a_feed_the_node_does_not_hold_still_counts_in_the_rank() {
    let st = state(common::test_db_arc());
    album(
        &st,
        "album-2",
        "Artist Two",
        json!([names(0, "label-missing", true), names(1, "label-c", true)]),
    )
    .await;
    publisher(&st, "label-c", "Label C", json!([lists(0, "album-2")])).await;

    let data = read(&st, "album-2").await;
    let missing = row(&data, "label-missing");
    assert_eq!(
        missing["remote_feed_title"],
        Value::Null,
        "the node holds no feed for label-missing: {missing}"
    );
    assert_eq!(missing["album_names_rank"], 1, "{missing}");

    let c = row(&data, "label-c");
    assert_eq!(
        c["album_names_rank"], 2,
        "label-c is the second named feed, even though the node does not hold label-missing: {c}"
    );
}

#[tokio::test]
async fn a_feed_level_item_outside_publisher_before_the_links_does_not_change_the_rank() {
    let st = state(common::test_db_arc());
    album(
        &st,
        "album-3",
        "Artist Three",
        json!([unrelated_item(0), names(1, "label-d", true)]),
    )
    .await;
    publisher(&st, "label-d", "Label D", json!([lists(0, "album-3")])).await;

    let data = read(&st, "album-3").await;
    let d = row(&data, "label-d");
    assert_eq!(
        d["album_names_rank"], 1,
        "the soundbite item at position 0 is not a publisher-link item, so label-d is rank 1: {d}"
    );
}

#[tokio::test]
async fn a_publisher_that_lists_an_album_not_naming_it_gives_a_null_rank() {
    let st = state(common::test_db_arc());
    album(&st, "album-4", "Artist Four", json!([])).await;
    publisher(&st, "claimer", "Claimer", json!([lists(0, "album-4")])).await;

    let data = read(&st, "claimer").await;
    let listed = row(&data, "album-4");
    assert_eq!(listed["album_names_as"], Value::Null, "{listed}");
    assert_eq!(
        listed["album_names_rank"],
        Value::Null,
        "the rank is null when album_names_as is null: {listed}"
    );
}
