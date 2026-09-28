// ADR 0064: a live item is an RSS fact.
//
// docs/tasks/adr-0064-task-002b-node-ingest-rules.md
//
// The ingest keeps a pending, live and ended row alike, applies the ADR 0064
// section 4 ban and the two row caps, and makes no track, payment route or
// value time split from a live item. These tests exercise `src/live.rs`
// through the `/ingest/feed` route.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0064-ingest-crawl-token";
const ADMIN: &str = "test-adr0064-ingest-admin-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0064-ingest-signer"));
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

/// Ingests one feed and asserts it was accepted. Returns the response body.
async fn ingest_ok(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    url: &str,
    feed_extra: &Value,
    version: u32,
) -> Value {
    let (status, body) = post_ingest(st, guid, url, feed_extra, version).await;
    assert!(
        status.is_success() && body["accepted"] == true,
        "ingest of {guid} must be accepted, got {status}: {body}"
    );
    body
}

fn live_rows(
    db: &Arc<Mutex<rusqlite::Connection>>,
    feed_guid: &str,
) -> Vec<stophammer::model::LiveEvent> {
    let conn = db.lock().expect("lock");
    stophammer::db::get_live_events_for_feed(&conn, feed_guid).expect("read live events")
}

fn warnings_of(body: &Value) -> Vec<String> {
    body["warnings"]
        .as_array()
        .expect("warnings is an array")
        .iter()
        .map(|w| w.as_str().expect("warning is a string").to_string())
        .collect()
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

// ---------------------------------------------------------------------------
// A live item with a status the live_events table does not accept is
// dropped with a warning, and the rest of the feed still ingests. Before
// this rule, the item reached the INSERT and its CHECK constraint failed the
// whole feed.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unknown_status_is_dropped_and_the_rest_of_the_feed_still_ingests() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-unknown-status";
    let body = ingest_ok(
        &st,
        guid,
        "https://l.example/unknown-status.xml",
        &json!({
            "live_items": [
                live_item("live-cancelled", "cancelled", None, None),
                live_item("live-pending", "pending", None, None),
            ]
        }),
        1,
    )
    .await;

    let rows = live_rows(&db, guid);
    assert_eq!(
        rows.len(),
        1,
        "an unknown status is dropped, and the valid item is still stored"
    );
    assert_eq!(
        rows[0].live_item_guid, "live-pending",
        "the stored row is the one with a known status"
    );
    assert_eq!(
        warnings_of(&body).len(),
        1,
        "the dropped item gives one warning"
    );
    assert!(
        warnings_of(&body).iter().any(|w| w.contains("cancelled")),
        "the warning names the unknown status value, got {:?}",
        warnings_of(&body)
    );
}

// ---------------------------------------------------------------------------
// An ended item makes an ended row, and no track, payment route or value
// time split, even when the payload supplies them. A normal item of the same
// feed still becomes a track.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ended_live_item_makes_no_track_route_or_vts_and_a_normal_item_still_does() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-ended-1";
    let url = "https://l.example/ended1.xml";

    let mut ended = live_item(
        "live-ended-1",
        "ended",
        Some(1_700_000_000),
        Some(1_700_003_600),
    );
    ended["enclosure_url"] = json!("https://cdn.example.com/replay.mp3");
    ended["enclosure_type"] = json!("audio/mpeg");
    ended["enclosure_bytes"] = json!(12345);
    ended["pub_date"] = json!(1_700_003_600);
    ended["payment_routes"] = json!([{
        "recipient_name": "Artist",
        "route_type": "keysend",
        "address": "02abc123",
        "split": 100,
        "fee": false
    }]);
    ended["value_time_splits"] = json!([{
        "start_time_secs": 0,
        "duration_secs": 30,
        "remote_feed_guid": "remote-feed",
        "remote_item_guid": "remote-item",
        "split": 100
    }]);

    ingest_ok(
        &st,
        guid,
        url,
        &json!({
            "live_items": [ended],
            "tracks": [{
                "track_guid": "track-normal-1",
                "title": "A Normal Track",
                "explicit": false
            }]
        }),
        1,
    )
    .await;

    let rows = live_rows(&db, guid);
    assert_eq!(rows.len(), 1, "the ended item makes one live row");
    assert_eq!(
        rows[0].status, "ended",
        "the stored row keeps the ended status"
    );

    let conn = db.lock().expect("lock");
    let live_track = stophammer::db::get_track_for_feed(&conn, guid, "live-ended-1")
        .expect("query track for live item guid");
    assert!(
        live_track.is_none(),
        "an ended live item must make no track, got {live_track:?}"
    );
    let routes = stophammer::db::get_payment_routes_for_feed_track(&conn, guid, "live-ended-1")
        .expect("query payment routes for live item guid");
    assert!(
        routes.is_empty(),
        "an ended live item must make no payment route, got {routes:?}"
    );
    let vts = stophammer::db::get_value_time_splits_for_feed_track(&conn, guid, "live-ended-1")
        .expect("query value time splits for live item guid");
    assert!(
        vts.is_empty(),
        "an ended live item must make no value time split, got {vts:?}"
    );

    let normal_track = stophammer::db::get_track_for_feed(&conn, guid, "track-normal-1")
        .expect("query normal track");
    assert!(
        normal_track.is_some(),
        "a normal item of the same feed must still become a track"
    );
}

// ---------------------------------------------------------------------------
// The ban: a live item with no relay link is dropped when it has no end, no
// start, or runs more than six hours. Exactly six hours is kept. A pending
// item with the same times is kept. A live item with a relay link and no end
// is kept.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ban_drops_a_live_item_with_no_end() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-ban-no-end";
    let body = ingest_ok(
        &st,
        guid,
        "https://l.example/ban-no-end.xml",
        &json!({ "live_items": [live_item("live-1", "live", Some(1_700_000_000), None)] }),
        1,
    )
    .await;

    assert!(
        live_rows(&db, guid).is_empty(),
        "a live item with no relay link and no end must be dropped"
    );
    assert_eq!(
        warnings_of(&body).len(),
        1,
        "the dropped item gives one warning"
    );
}

#[tokio::test]
async fn ban_drops_a_live_item_with_no_start() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-ban-no-start";
    let body = ingest_ok(
        &st,
        guid,
        "https://l.example/ban-no-start.xml",
        &json!({ "live_items": [live_item("live-1", "live", None, Some(1_700_003_600))] }),
        1,
    )
    .await;

    assert!(
        live_rows(&db, guid).is_empty(),
        "a live item with no relay link and no start must be dropped"
    );
    assert_eq!(
        warnings_of(&body).len(),
        1,
        "the dropped item gives one warning"
    );
}

#[tokio::test]
async fn ban_drops_a_live_item_that_runs_over_six_hours() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-ban-over-six";
    let start = 1_700_000_000_i64;
    let over_six_hours = start + 6 * 3600 + 1;
    let body = ingest_ok(
        &st,
        guid,
        "https://l.example/ban-over-six.xml",
        &json!({ "live_items": [live_item("live-1", "live", Some(start), Some(over_six_hours))] }),
        1,
    )
    .await;

    assert!(
        live_rows(&db, guid).is_empty(),
        "a live item that runs six hours and one second must be dropped"
    );
    assert_eq!(
        warnings_of(&body).len(),
        1,
        "the dropped item gives one warning"
    );
}

#[tokio::test]
async fn a_live_item_of_exactly_six_hours_is_kept() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-exactly-six";
    let start = 1_700_000_000_i64;
    let exactly_six_hours = start + 6 * 3600;
    let body = ingest_ok(
        &st,
        guid,
        "https://l.example/exactly-six.xml",
        &json!({ "live_items": [live_item("live-1", "live", Some(start), Some(exactly_six_hours))] }),
        1,
    )
    .await;

    assert_eq!(
        live_rows(&db, guid).len(),
        1,
        "a live item of exactly six hours is kept"
    );
    assert!(
        warnings_of(&body).is_empty(),
        "a kept item gives no ban warning"
    );
}

#[tokio::test]
async fn a_pending_item_with_the_same_times_is_kept() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-pending-same-times";
    let body = ingest_ok(
        &st,
        guid,
        "https://l.example/pending-same-times.xml",
        &json!({ "live_items": [live_item("live-1", "pending", None, None)] }),
        1,
    )
    .await;

    assert_eq!(
        live_rows(&db, guid).len(),
        1,
        "the ban does not apply to a pending item, even with no start or end"
    );
    assert!(
        warnings_of(&body).is_empty(),
        "a kept pending item gives no ban warning"
    );
}

#[tokio::test]
async fn a_live_item_with_a_relay_link_and_no_end_is_kept() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-relay-no-end";
    let mut with_relay = live_item("live-1", "live", Some(1_700_000_000), None);
    with_relay["live_value_uri"] = json!("https://relay.example/event?event_id=abc");
    let body = ingest_ok(
        &st,
        guid,
        "https://l.example/relay-no-end.xml",
        &json!({ "live_items": [with_relay] }),
        1,
    )
    .await;

    let rows = live_rows(&db, guid);
    assert_eq!(rows.len(), 1, "a relay link keeps a live item with no end");
    assert!(
        warnings_of(&body).is_empty(),
        "a kept item with a relay link gives no ban warning"
    );
}

// ---------------------------------------------------------------------------
// The two caps: 11 live items store 10, and 11 ended items keep the 10
// newest, each dropped item with a warning.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn eleven_live_items_store_ten_with_one_warning() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-cap-active";
    let items: Vec<Value> = (0..11)
        .map(|i| {
            let mut it = live_item(
                &format!("live-active-{i}"),
                "live",
                Some(1_700_000_000),
                None,
            );
            it["live_value_uri"] = json!(format!("https://relay.example/event?event_id={i}"));
            it
        })
        .collect();
    let body = ingest_ok(
        &st,
        guid,
        "https://l.example/cap-active.xml",
        &json!({ "live_items": items }),
        1,
    )
    .await;

    assert_eq!(
        live_rows(&db, guid).len(),
        10,
        "a feed with 11 live items stores only 10"
    );
    assert_eq!(
        warnings_of(&body).len(),
        1,
        "the eleventh live item gives one warning"
    );
}

#[tokio::test]
async fn eleven_ended_items_keep_the_ten_newest_with_one_warning() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-cap-ended";
    let items: Vec<Value> = (0..11)
        .map(|i| {
            live_item(
                &format!("live-ended-{i}"),
                "ended",
                Some(1_700_000_000 + i64::from(i)),
                None,
            )
        })
        .collect();
    let body = ingest_ok(
        &st,
        guid,
        "https://l.example/cap-ended.xml",
        &json!({ "live_items": items }),
        1,
    )
    .await;

    let rows = live_rows(&db, guid);
    assert_eq!(rows.len(), 10, "a feed with 11 ended items keeps only 10");
    assert!(
        !rows.iter().any(|row| row.live_item_guid == "live-ended-0"),
        "the ended item with the oldest start_at is dropped"
    );
    assert_eq!(
        warnings_of(&body).len(),
        1,
        "the dropped ended item gives one warning"
    );
}

// ---------------------------------------------------------------------------
// A javascript: content_link gives a warning.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn javascript_content_link_gives_a_warning() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-bad-content-link";
    let mut item = live_item("live-1", "pending", None, None);
    item["content_link"] = json!("javascript:alert(1)");
    let body = ingest_ok(
        &st,
        guid,
        "https://l.example/bad-content-link.xml",
        &json!({ "live_items": [item] }),
        1,
    )
    .await;

    assert_eq!(
        live_rows(&db, guid).len(),
        1,
        "a bad content_link still keeps the row; it only warns"
    );
    assert!(
        warnings_of(&body)
            .iter()
            .any(|w| w.contains("content_link")),
        "a javascript: content_link must give a warning, got {:?}",
        warnings_of(&body)
    );
}

// ---------------------------------------------------------------------------
// A musicL feed gives no live row.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn musicl_feed_gives_no_live_row() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-musicl-live";
    ingest_ok(
        &st,
        guid,
        "https://l.example/musicl-live.xml",
        &json!({
            "raw_medium": "musicL",
            "live_items": [live_item("live-1", "live", Some(1_700_000_000), Some(1_700_003_600))]
        }),
        1,
    )
    .await;

    assert!(
        live_rows(&db, guid).is_empty(),
        "a musicL feed must give no live row"
    );
}
