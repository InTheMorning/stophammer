// ADR 0064 task 003: the relay setting and the feed read.
//
// docs/tasks/adr-0064-task-003-node-feed-read.md
//
// `GET /v1/feeds/{guid}` gives `live_items`, one row for each stored live
// event of the feed, sorted by `live_item_guid`. These tests exercise the
// read side of `src/live.rs` through the ingest route and the feed route.
// No test here calls `live::set_confirming_relay_hosts`, so the host list
// stays empty and `confirming_relay` reads `false` throughout.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0064-feed-read-crawl-token";
const ADMIN: &str = "test-adr0064-feed-read-admin-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0064-feed-read-signer"));
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
        skip_ssrf_validation: true,
    })
}

/// Posts one ingest and returns its status and JSON body.
async fn post_ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    url: &str,
    feed_extra: &Value,
    version: u32,
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
        "content_hash": format!("hash-{guid}-{version}"),
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
    let (status, body) = post_ingest(st, guid, url, feed_extra, 1).await;
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

/// Calls `GET /v1/feeds/recent` and returns its JSON body.
async fn get_recent_feeds(st: &Arc<stophammer::api::AppState>) -> Value {
    let req = Request::builder()
        .method("GET")
        .uri("/v1/feeds/recent")
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
    assert!(status.is_success(), "GET /v1/feeds/recent failed: {body}");
    body
}

// ---------------------------------------------------------------------------
// `live_items` gives a pending, a live and an ended row, sorted by
// `live_item_guid`, with the relay link and `confirming_relay: false` when
// `CONFIRMING_RELAY_HOSTS` is empty.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn feed_read_gives_pending_live_and_ended_rows_sorted_by_guid() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-live-items-read";

    // Ingested out of alphabetical order, so a passing sort assertion proves
    // the read sorts the rows rather than keeping ingest order.
    let mut live_row = live_item("mm-live", "live", Some(1_700_000_000), Some(1_700_003_600));
    live_row["live_value_uri"] = json!("https://relay.example.com/events/abc");
    live_row["live_value_protocol"] = json!("socket.io");

    let ended_row = live_item(
        "zz-ended",
        "ended",
        Some(1_699_000_000),
        Some(1_699_003_600),
    );
    let pending_row = live_item("aa-pending", "pending", Some(1_800_000_000), None);

    ingest_ok(
        &st,
        guid,
        "https://l.example/live-items-read.xml",
        &json!({ "live_items": [ended_row, live_row, pending_row] }),
    )
    .await;

    let body = get_feed(&st, guid).await;
    let live_items = body["data"]["live_items"]
        .as_array()
        .expect("live_items is an array");
    assert_eq!(
        live_items.len(),
        3,
        "the feed read gives one row for each stored live item, got {live_items:?}"
    );

    let guids: Vec<&str> = live_items
        .iter()
        .map(|row| row["live_item_guid"].as_str().expect("live_item_guid"))
        .collect();
    assert_eq!(
        guids,
        vec!["aa-pending", "mm-live", "zz-ended"],
        "live_items is sorted by live_item_guid"
    );

    let pending = &live_items[0];
    assert_eq!(
        pending["status"], "pending",
        "the pending row keeps its status"
    );
    assert_eq!(
        pending["confirming_relay"], false,
        "a row with no relay link has no confirming relay"
    );

    let live = &live_items[1];
    assert_eq!(live["status"], "live", "the live row keeps its status");
    assert_eq!(
        live["live_value_uri"], "https://relay.example.com/events/abc",
        "the relay link is given as it was ingested"
    );
    assert_eq!(
        live["live_value_protocol"], "socket.io",
        "the relay protocol is given as it was ingested"
    );
    assert_eq!(
        live["confirming_relay"], false,
        "confirming_relay is false when CONFIRMING_RELAY_HOSTS is empty"
    );
    assert_eq!(
        live["scheduled_start"], 1_700_000_000,
        "the live row gives its scheduled_start"
    );
    assert_eq!(
        live["scheduled_end"], 1_700_003_600,
        "the live row gives its scheduled_end"
    );

    let ended = &live_items[2];
    assert_eq!(ended["status"], "ended", "the ended row keeps its status");
    assert_eq!(
        ended["confirming_relay"], false,
        "an ended row with no relay link has no confirming relay"
    );
}

// ---------------------------------------------------------------------------
// A `javascript:` content_link warns at ingest, and a read gives no value
// for it (ADR 0064 section 4, ADR 0054 section 4).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_javascript_content_link_gives_null_in_the_read() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-live-bad-content-link";

    let mut item = live_item("live-1", "pending", None, None);
    item["content_link"] = json!("javascript:alert(1)");

    ingest_ok(
        &st,
        guid,
        "https://l.example/live-bad-content-link.xml",
        &json!({ "live_items": [item] }),
    )
    .await;

    let body = get_feed(&st, guid).await;
    let live_items = body["data"]["live_items"]
        .as_array()
        .expect("live_items is an array");
    assert_eq!(
        live_items.len(),
        1,
        "the bad content_link still keeps the row"
    );
    assert_eq!(
        live_items[0]["content_link"],
        Value::Null,
        "a javascript: content_link must read as null, got {:?}",
        live_items[0]["content_link"]
    );
}

// ---------------------------------------------------------------------------
// A feed with no live item still gives an empty, but present, live_items
// array on the single-feed read.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_feed_with_no_live_item_gives_an_empty_live_items_array() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-no-live-items";

    ingest_ok(&st, guid, "https://l.example/no-live-items.xml", &json!({})).await;

    let body = get_feed(&st, guid).await;
    assert_eq!(
        body["data"]["live_items"],
        json!([]),
        "a feed with no live item gives an empty live_items array, not a missing field"
    );
}

// ---------------------------------------------------------------------------
// `GET /v1/feeds/recent` does not read live items (ADR 0064 section 6 scopes
// the field to the single-feed read). An empty array on that list route
// would wrongly claim the feed has no live item, so the key must be absent,
// even for a feed that has a live row.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn recent_feeds_gives_no_live_items_key_even_for_a_feed_with_a_live_row() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-recent-has-live-row";

    ingest_ok(
        &st,
        guid,
        "https://l.example/recent-has-live-row.xml",
        &json!({ "live_items": [live_item("live-1", "pending", Some(1_800_000_000), None)] }),
    )
    .await;

    // The single-feed read keeps the key.
    let feed = get_feed(&st, guid).await;
    assert!(
        feed["data"].get("live_items").is_some(),
        "GET /v1/feeds/{{guid}} must keep the live_items key, got {feed}"
    );

    // The list route gives no live_items key at all, on the same feed.
    let recent = get_recent_feeds(&st).await;
    let rows = recent["data"].as_array().expect("data is an array");
    let row = rows
        .iter()
        .find(|row| row["feed_guid"] == guid)
        .unwrap_or_else(|| panic!("the ingested feed must be in the recent list, got {rows:?}"));
    assert!(
        row.get("live_items").is_none(),
        "GET /v1/feeds/recent must give no live_items key, even for a feed with a live row, \
         got {row:?}"
    );
}
