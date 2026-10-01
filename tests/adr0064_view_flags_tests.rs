// ADR 0064 task 005: the view flags on each row.
//
// docs/tasks/adr-0064-task-005-view-flags-on-each-row.md
//
// Each live row gives `in_now_view` and `in_upcoming_view`, in
// `GET /v1/live-items` and in `live_items` of `GET /v1/feeds/{guid}`. Each
// one is true exactly when the view of the same name gives the row at the
// time of the read. These tests check the flags against the case table of
// `tests/adr0064_live_list_tests.rs`, a specific no-relay case, and the feed
// read with row times set relative to the real clock.

mod common;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0064-view-flags-crawl-token";
const ADMIN: &str = "test-adr0064-view-flags-admin-token";

/// A fixed clock for the tests that call `list_live_items` directly. Every
/// timestamp below is relative to this value, so a boundary check does not
/// depend on when the test runs.
const NOW: i64 = 1_800_000_000;

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0064-view-flags-signer"));
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

/// A [`stophammer::query::LiveItemsQuery`] with only `view` set.
fn view_query(view: &str) -> stophammer::query::LiveItemsQuery {
    stophammer::query::LiveItemsQuery {
        view: Some(view.to_string()),
        ..Default::default()
    }
}

/// Reads `live_item_guid`, `in_now_view` and `in_upcoming_view` from each
/// row of a [`stophammer::query::LiveItemsPage`], through `serde_json` since
/// `LiveItemListResponse`'s fields are private.
fn flag_rows(page: &stophammer::query::LiveItemsPage) -> HashMap<String, (bool, bool)> {
    serde_json::to_value(&page.data)
        .expect("serialize page data")
        .as_array()
        .expect("data is an array")
        .iter()
        .map(|row| {
            let guid = row["live_item_guid"]
                .as_str()
                .expect("live_item_guid")
                .to_string();
            let in_now = row["in_now_view"].as_bool().expect("in_now_view");
            let in_upcoming = row["in_upcoming_view"].as_bool().expect("in_upcoming_view");
            (guid, (in_now, in_upcoming))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Through the list function with a fixed `now`, each row of the case table
// of `tests/adr0064_live_list_tests.rs` gives `in_now_view` and
// `in_upcoming_view` that agree with `view=now` and `view=upcoming`
// membership (docs/plans/adr-0064-live-item-cases.md).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn each_case_table_row_gives_flags_that_agree_with_the_views() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-view-flags-case-table";

    let mut l3 = live_item("l3", "live", None, None);
    l3["live_value_uri"] = json!("event-id-only");

    let items = vec![
        live_item("p1", "pending", Some(NOW + 1000), None),
        live_item("p2", "pending", Some(NOW - 100), Some(NOW + 100)),
        live_item("p3", "pending", Some(NOW - 2000), Some(NOW - 100)),
        live_item("p4", "pending", Some(NOW - 3700), None),
        l3,
        live_item("l4", "live", Some(NOW - 100), Some(NOW + 1000)),
        live_item("l5", "live", Some(NOW - 2000), Some(NOW - 1800)),
        live_item("l6", "live", Some(NOW - 20000), Some(NOW - 4000)),
        live_item("e1", "ended", Some(NOW - 100_000), Some(NOW - 90_000)),
        live_item("e2", "ended", Some(NOW - 200_000), Some(NOW - 190_000)),
    ];

    ingest_ok(
        &st,
        guid,
        "https://l.example/view-flags-case-table.xml",
        &json!({ "live_items": items }),
    )
    .await;

    let conn = db.lock().expect("lock db");
    // view=all gives every row, with its flags computed at the same `now`
    // the view membership checks use.
    let page = stophammer::query::list_live_items(&conn, &view_query("all"), NOW, &[])
        .expect("list_live_items");
    let flags = flag_rows(&page);

    // (guid, in_now, in_upcoming) — mirrors the case table of
    // tests/adr0064_live_list_tests.rs.
    let cases: &[(&str, bool, bool)] = &[
        ("p1", false, true),
        ("p2", false, true),
        ("p3", false, false),
        ("p4", false, false),
        ("l3", true, false),
        ("l4", true, false),
        ("l5", true, false),
        ("l6", false, false),
        ("e1", false, false),
        ("e2", false, false),
    ];

    let case_guids: HashSet<&str> = cases.iter().map(|(guid, ..)| *guid).collect();
    assert_eq!(
        flags.keys().map(String::as_str).collect::<HashSet<_>>(),
        case_guids,
        "view=all must give exactly the case table rows, got {flags:?}"
    );

    for (guid, in_now, in_upcoming) in cases {
        let (got_now, got_upcoming) = flags
            .get(*guid)
            .unwrap_or_else(|| panic!("row {guid} must be present, got {flags:?}"));
        assert_eq!(
            got_now, in_now,
            "case {guid}: in_now_view must be {in_now}, got {got_now}"
        );
        assert_eq!(
            got_upcoming, in_upcoming,
            "case {guid}: in_upcoming_view must be {in_upcoming}, got {got_upcoming}"
        );
    }
}

// ---------------------------------------------------------------------------
// A `live` row with no relay and an end 2 hours before `now` gives
// `status: "live"`, `in_now_view: false` and `in_upcoming_view: false`.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_live_row_with_no_relay_two_hours_past_its_end_is_in_neither_view() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-view-flags-two-hours-past-end";

    let item = live_item(
        "l-two-hours-past",
        "live",
        Some(NOW - 10_800),
        Some(NOW - 7_200),
    );

    ingest_ok(
        &st,
        guid,
        "https://l.example/view-flags-two-hours-past-end.xml",
        &json!({ "live_items": [item] }),
    )
    .await;

    let conn = db.lock().expect("lock db");
    let page = stophammer::query::list_live_items(&conn, &view_query("all"), NOW, &[])
        .expect("list_live_items");
    let row = serde_json::to_value(&page.data).expect("serialize page data")[0].clone();

    assert_eq!(
        row["status"], "live",
        "the row keeps its stored status, got {row}"
    );
    assert_eq!(
        row["in_now_view"], false,
        "a live row 2 hours past its end, with no relay, is not in the now view, got {row}"
    );
    assert_eq!(
        row["in_upcoming_view"], false,
        "a live row is never in the upcoming view, got {row}"
    );
}

// ---------------------------------------------------------------------------
// Through `GET /v1/feeds/{guid}`, with row times set relative to the real
// clock: a `live` row with an end 2 hours ago gives `in_now_view: false`,
// and a `live` row with an end 1 hour ahead gives `in_now_view: true`.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn feed_read_in_now_view_follows_the_real_clock() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-view-flags-feed-read-real-clock";

    let now = stophammer::db::unix_now();
    let ended_two_hours_ago = live_item(
        "l-ended-two-hours-ago",
        "live",
        Some(now - 10_800),
        Some(now - 7_200),
    );
    let ends_one_hour_ahead = live_item(
        "l-ends-one-hour-ahead",
        "live",
        Some(now - 100),
        Some(now + 3_600),
    );

    ingest_ok(
        &st,
        guid,
        "https://l.example/view-flags-feed-read-real-clock.xml",
        &json!({ "live_items": [ended_two_hours_ago, ends_one_hour_ahead] }),
    )
    .await;

    let body = get_feed(&st, guid).await;
    let live_items = body["data"]["live_items"]
        .as_array()
        .expect("live_items is an array");

    let row_for = |guid: &str| {
        live_items
            .iter()
            .find(|row| row["live_item_guid"] == guid)
            .unwrap_or_else(|| panic!("row {guid} must be present, got {live_items:?}"))
    };

    assert_eq!(
        row_for("l-ended-two-hours-ago")["in_now_view"],
        false,
        "a live row that ended 2 hours ago is not in the now view, got {live_items:?}"
    );
    assert_eq!(
        row_for("l-ends-one-hour-ahead")["in_now_view"],
        true,
        "a live row ending 1 hour ahead is in the now view, got {live_items:?}"
    );
}
