// ADR 0064 task 003b: the confirming relay, across both reads.
//
// docs/tasks/adr-0064-task-003b-node-list-route.md
//
// `live::set_confirming_relay_hosts` takes effect only on its first call
// (it is a `OnceLock`), so every test that needs a non-empty host list must
// share one binary and set the list exactly once, before the first read or
// ingest. This file is that separate binary. `tests/adr0064_live_list_tests.rs`
// covers every other case, with an empty host list throughout.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0064-confirming-relay-crawl-token";
const ADMIN: &str = "test-adr0064-confirming-relay-admin-token";
const RELAY_HOST: &str = "relay.example.com";

static INIT_HOSTS: Once = Once::new();

/// Sets `CONFIRMING_RELAY_HOSTS` to `[RELAY_HOST]` one time for this test
/// binary. Every test calls this before it ingests or reads anything.
fn init_hosts() {
    INIT_HOSTS.call_once(|| {
        stophammer::live::set_confirming_relay_hosts(vec![RELAY_HOST.to_string()]);
    });
}

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0064-confirming-relay-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: ADMIN.into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
    })
}

/// Posts one ingest and returns its status and JSON body.
async fn post_ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    url: &str,
    feed_extra: &Value,
) -> (http::StatusCode, Value) {
    let mut feed_data = json!({
        "feed_guid": guid,
        "title": format!("Feed {guid}"),
        "raw_medium": "music",
        "explicit": false,
        "author_name": "Some Artist",
        "tracks": [],
        "live_items": [],
    });
    for (key, value) in feed_extra.as_object().expect("feed_extra is an object") {
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
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// Ingests one feed and asserts it was accepted.
async fn ingest_ok(st: &Arc<stophammer::api::AppState>, guid: &str, url: &str, feed_extra: &Value) {
    let (status, body) = post_ingest(st, guid, url, feed_extra).await;
    assert!(
        status.is_success() && body["accepted"] == true,
        "ingest of {guid} must be accepted, got {status}: {body}"
    );
}

/// One `<podcast:liveItem>` for an ingest payload.
fn live_item(guid: &str, status: &str, start_at: Option<i64>, end_at: Option<i64>) -> Value {
    json!({
        "live_item_guid": guid,
        "title": format!("Live {guid}"),
        "status": status,
        "start_at": start_at,
        "end_at": end_at,
        "explicit": false,
    })
}

/// Gets a feed by GUID and returns its JSON body.
async fn get_feed(st: &Arc<stophammer::api::AppState>, guid: &str) -> Value {
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/feeds/{guid}"))
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
    let body: Value = serde_json::from_slice(&bytes).expect("the response is JSON");
    assert!(status.is_success(), "GET /v1/feeds/{guid} failed: {body}");
    body
}

/// Calls `GET /v1/live-items` with the given raw query string (no leading
/// `?`) and returns its status and JSON body.
async fn get_live_items(
    st: &Arc<stophammer::api::AppState>,
    query: &str,
) -> (http::StatusCode, Value) {
    let uri = if query.is_empty() {
        "/v1/live-items".to_string()
    } else {
        format!("/v1/live-items?{query}")
    };
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
    (status, body)
}

// ---------------------------------------------------------------------------
// A `live` row with a listed `https` host gives `confirming_relay: true` in
// both the single-feed read and the list route.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_confirming_relay_gives_true_in_both_reads() {
    init_hosts();
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-confirming-relay-both-reads";
    let now = common::now();

    let mut item = live_item("live-confirmed", "live", Some(now - 100), Some(now + 100));
    item["live_value_uri"] = json!(format!("https://{RELAY_HOST}/events/abc"));
    item["live_value_protocol"] = json!("socket.io");

    ingest_ok(
        &st,
        guid,
        "https://l.example/confirming-relay-both-reads.xml",
        &json!({ "live_items": [item] }),
    )
    .await;

    let feed = get_feed(&st, guid).await;
    let feed_row = feed["data"]["live_items"]
        .as_array()
        .expect("live_items is an array")
        .iter()
        .find(|row| row["live_item_guid"] == "live-confirmed")
        .unwrap_or_else(|| panic!("the row must be in the feed read, got {feed}"));
    assert_eq!(
        feed_row["confirming_relay"], true,
        "the single-feed read must give confirming_relay: true, got {feed_row:?}"
    );

    let (status, body) = get_live_items(&st, "view=all").await;
    assert!(status.is_success(), "the list route failed: {body}");
    let list_row = body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|row| row["live_item_guid"] == "live-confirmed")
        .unwrap_or_else(|| panic!("the row must be in the list route, got {body}"));
    assert_eq!(
        list_row["confirming_relay"], true,
        "the list route must give confirming_relay: true, got {list_row:?}"
    );
    assert_eq!(
        list_row["feed_guid"], guid,
        "the list row must name its feed_guid, got {list_row:?}"
    );
}

// ---------------------------------------------------------------------------
// `now` gives a confirming-relay row after its `end` (ADR 0064 section 6,
// case L1).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn now_gives_a_confirming_relay_row_long_after_its_end() {
    init_hosts();
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-confirming-relay-after-end";
    let now = common::now();

    // scheduled_end is a full day in the past, far past the one-hour margin
    // that a non-confirming row gets (ADR 0064 section 6).
    let mut item = live_item(
        "live-overtime",
        "live",
        Some(now - 100_000),
        Some(now - 90_000),
    );
    item["live_value_uri"] = json!(format!("https://{RELAY_HOST}/events/xyz"));

    ingest_ok(
        &st,
        guid,
        "https://l.example/confirming-relay-after-end.xml",
        &json!({ "live_items": [item] }),
    )
    .await;

    let (status, body) = get_live_items(&st, "").await;
    assert!(
        status.is_success(),
        "the default-view request failed: {body}"
    );
    let guids: Vec<&str> = body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .map(|row| row["live_item_guid"].as_str().expect("live_item_guid"))
        .collect();
    assert!(
        guids.contains(&"live-overtime"),
        "a confirming relay keeps a live row in the now view long past its \
         scheduled_end, got {guids:?}"
    );
}
