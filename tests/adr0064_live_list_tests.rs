// ADR 0064 task 003b: the list of live items.
//
// docs/tasks/adr-0064-task-003b-node-list-route.md
//
// `GET /v1/live-items` gives the live-event rows of every public feed, with
// `feed_guid`, in the `view` of ADR 0064 section 6. These tests cover the
// three views, the raw filters of `view=all`, the `400` cases, paging, and a
// deleted feed.
//
// A view test that depends on the current time calls
// `stophammer::query::list_live_items` directly, with a fixed `now`, rather
// than the HTTP route. `view=all` never reads the clock, so its tests go
// through the HTTP route end to end. No test here calls
// `live::set_confirming_relay_hosts`; `tests/adr0064_live_confirming_relay_tests.rs`
// owns that case, in its own binary.

mod common;

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0064-live-list-crawl-token";
const ADMIN: &str = "test-adr0064-live-list-admin-token";

/// A fixed clock for the tests that call `list_live_items` directly. Every
/// timestamp below is relative to this value, so a boundary check does not
/// depend on when the test runs.
const NOW: i64 = 1_800_000_000;

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0064-live-list-signer"));
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

/// The `live_item_guid` of each row of an HTTP `data` array, in order.
fn data_guids(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .map(|row| {
            row["live_item_guid"]
                .as_str()
                .expect("live_item_guid")
                .to_string()
        })
        .collect()
}

/// The `live_item_guid` set of a [`stophammer::query::LiveItemsPage`], read
/// through `serde_json` since `LiveItemListResponse`'s fields are private.
fn guid_set(page: &stophammer::query::LiveItemsPage) -> BTreeSet<String> {
    serde_json::to_value(&page.data)
        .expect("serialize page data")
        .as_array()
        .expect("data is an array")
        .iter()
        .map(|row| {
            row["live_item_guid"]
                .as_str()
                .expect("live_item_guid")
                .to_string()
        })
        .collect()
}

/// A [`stophammer::query::LiveItemsQuery`] with only `view` set.
fn view_query(view: &str) -> stophammer::query::LiveItemsQuery {
    stophammer::query::LiveItemsQuery {
        view: Some(view.to_string()),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// With no `view`, the route gives the `now` rows only.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn default_view_gives_the_now_rows_only() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-live-list-default-view";

    let live_now = live_item("live-now", "live", Some(NOW - 100), Some(NOW + 1000));
    let pending_later = live_item("pending-later", "pending", Some(NOW + 1000), None);

    ingest_ok(
        &st,
        guid,
        "https://l.example/live-list-default-view.xml",
        &json!({ "live_items": [live_now, pending_later] }),
    )
    .await;

    let conn = db.lock().expect("lock db");
    let page = stophammer::query::list_live_items(
        &conn,
        &stophammer::query::LiveItemsQuery::default(),
        NOW,
        &[],
    )
    .expect("list_live_items");
    let guids = guid_set(&page);

    assert!(
        guids.contains("live-now"),
        "with no view, the route must give the live row, got {guids:?}"
    );
    assert!(
        !guids.contains("pending-later"),
        "with no view, the route must not give a pending row, got {guids:?}"
    );
}

// ---------------------------------------------------------------------------
// Each row of the case table is in the view the table gives, and not the
// others (docs/plans/adr-0064-live-item-cases.md).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn each_case_table_row_is_in_the_expected_view_and_not_the_others() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-live-list-case-table";

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
        "https://l.example/live-list-case-table.xml",
        &json!({ "live_items": items }),
    )
    .await;

    let conn = db.lock().expect("lock db");
    let now_guids = guid_set(
        &stophammer::query::list_live_items(&conn, &view_query("now"), NOW, &[]).expect("now view"),
    );
    let upcoming_guids = guid_set(
        &stophammer::query::list_live_items(&conn, &view_query("upcoming"), NOW, &[])
            .expect("upcoming view"),
    );
    let all_guids = guid_set(
        &stophammer::query::list_live_items(&conn, &view_query("all"), NOW, &[]).expect("all view"),
    );

    // (guid, in_now, in_upcoming)
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

    for (guid, in_now, in_upcoming) in cases {
        assert_eq!(
            now_guids.contains(*guid),
            *in_now,
            "case {guid}: now membership must be {in_now}, got now rows {now_guids:?}"
        );
        assert_eq!(
            upcoming_guids.contains(*guid),
            *in_upcoming,
            "case {guid}: upcoming membership must be {in_upcoming}, got upcoming rows {upcoming_guids:?}"
        );
        assert!(
            all_guids.contains(*guid),
            "case {guid}: the all view must give every row, got all rows {all_guids:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// `view=all` with each raw filter selects the expected rows. This never
// reads the clock, so the test goes through the real HTTP route.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn view_all_raw_filters_select_the_expected_rows() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-live-list-raw-filters";

    let mut with_relay = live_item("with-relay", "ended", Some(1_000), Some(2_000));
    with_relay["live_value_uri"] = json!("https://relay.example.com/x");
    let without_relay = live_item("without-relay", "ended", Some(3_000), Some(4_000));
    let pending_row = live_item("pending-row", "pending", Some(5_000), Some(6_000));
    let live_row = live_item("live-row", "live", Some(7_000), Some(8_000));

    ingest_ok(
        &st,
        guid,
        "https://l.example/live-list-raw-filters.xml",
        &json!({ "live_items": [with_relay, without_relay, pending_row, live_row] }),
    )
    .await;

    let (status, body) = get_live_items(&st, "view=all&status=live").await;
    assert!(status.is_success(), "status filter request failed: {body}");
    assert_eq!(
        data_guids(&body),
        vec!["live-row"],
        "status=live must select only the live row, got {body}"
    );

    let (status, body) = get_live_items(&st, "view=all&live_value=set").await;
    assert!(status.is_success(), "live_value=set request failed: {body}");
    assert_eq!(
        data_guids(&body),
        vec!["with-relay"],
        "live_value=set must select only the row with a relay link, got {body}"
    );

    let (status, body) = get_live_items(&st, "view=all&live_value=none").await;
    assert!(
        status.is_success(),
        "live_value=none request failed: {body}"
    );
    assert_eq!(
        data_guids(&body),
        vec!["live-row", "pending-row", "without-relay"],
        "live_value=none must select every row with no relay link, got {body}"
    );

    let (status, body) = get_live_items(&st, "view=all&ends_after=7000").await;
    assert!(status.is_success(), "ends_after request failed: {body}");
    assert_eq!(
        data_guids(&body),
        vec!["live-row"],
        "ends_after=7000 must select only the row whose scheduled_end is after 7000, got {body}"
    );

    let (status, body) = get_live_items(&st, "view=all&starts_after=6000").await;
    assert!(status.is_success(), "starts_after request failed: {body}");
    assert_eq!(
        data_guids(&body),
        vec!["live-row"],
        "starts_after=6000 must select only the row whose scheduled_start is after 6000, got {body}"
    );
}

// ---------------------------------------------------------------------------
// A raw filter with a `view` other than `all` gives `400`. An unknown `view`
// or filter value gives `400`.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_raw_filter_with_another_view_and_an_unknown_value_give_400() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));

    let (status, body) = get_live_items(&st, "view=now&status=live").await;
    assert_eq!(
        status,
        http::StatusCode::BAD_REQUEST,
        "a raw filter with view=now must give 400, got {status}: {body}"
    );

    let (status, body) = get_live_items(&st, "status=live").await;
    assert_eq!(
        status,
        http::StatusCode::BAD_REQUEST,
        "a raw filter with the default view must give 400, got {status}: {body}"
    );

    let (status, body) = get_live_items(&st, "view=upcoming&live_value=set").await;
    assert_eq!(
        status,
        http::StatusCode::BAD_REQUEST,
        "a raw filter with view=upcoming must give 400, got {status}: {body}"
    );

    let (status, body) = get_live_items(&st, "view=nonexistent").await;
    assert_eq!(
        status,
        http::StatusCode::BAD_REQUEST,
        "an unknown view must give 400, got {status}: {body}"
    );

    let (status, body) = get_live_items(&st, "view=all&status=paused").await;
    assert_eq!(
        status,
        http::StatusCode::BAD_REQUEST,
        "an unknown status filter value must give 400, got {status}: {body}"
    );

    let (status, body) = get_live_items(&st, "view=all&live_value=maybe").await;
    assert_eq!(
        status,
        http::StatusCode::BAD_REQUEST,
        "an unknown live_value filter value must give 400, got {status}: {body}"
    );
}

// ---------------------------------------------------------------------------
// Two pages with `limit=1` give each row one time, in the order `feed_guid`,
// `live_item_guid`.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_pages_with_limit_1_give_each_row_one_time_in_feed_then_item_order() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));

    ingest_ok(
        &st,
        "aa-feed-live-list-paging",
        "https://l.example/aa-live-list-paging.xml",
        &json!({ "live_items": [live_item("z-item", "ended", Some(1), Some(2))] }),
    )
    .await;
    ingest_ok(
        &st,
        "bb-feed-live-list-paging",
        "https://l.example/bb-live-list-paging.xml",
        &json!({ "live_items": [live_item("a-item", "ended", Some(1), Some(2))] }),
    )
    .await;

    let (status, body) = get_live_items(&st, "view=all&limit=1").await;
    assert!(status.is_success(), "page 1 failed: {body}");
    assert_eq!(
        data_guids(&body),
        vec!["z-item"],
        "page 1 must hold the row of the lower feed_guid, got {body}"
    );
    assert_eq!(
        body["pagination"]["has_more"], true,
        "page 1 must report more rows, got {body}"
    );
    let cursor = body["pagination"]["cursor"]
        .as_str()
        .expect("page 1 must give a cursor")
        .to_string();

    let (status, body) = get_live_items(&st, &format!("view=all&limit=1&cursor={cursor}")).await;
    assert!(status.is_success(), "page 2 failed: {body}");
    assert_eq!(
        data_guids(&body),
        vec!["a-item"],
        "page 2 must hold the row of the higher feed_guid, got {body}"
    );
    assert_eq!(
        body["pagination"]["has_more"], false,
        "page 2 must report no more rows, got {body}"
    );
    assert_eq!(
        body["pagination"]["cursor"],
        Value::Null,
        "page 2 must give no cursor, got {body}"
    );
}

// ---------------------------------------------------------------------------
// A deleted feed gives no row.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_deleted_feed_gives_no_row() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "feed-live-list-deleted";

    ingest_ok(
        &st,
        guid,
        "https://l.example/live-list-deleted.xml",
        &json!({ "live_items": [live_item("deleted-item", "ended", Some(1), Some(2))] }),
    )
    .await;

    let (status, body) = get_live_items(&st, "view=all").await;
    assert!(status.is_success(), "listing before delete failed: {body}");
    assert!(
        data_guids(&body).contains(&"deleted-item".to_string()),
        "the row must be listed before the feed is deleted, got {body}"
    );

    {
        let mut conn = db.lock().expect("lock db");
        stophammer::db::delete_feed(&mut conn, guid).expect("delete feed");
    }

    let (status, body) = get_live_items(&st, "view=all").await;
    assert!(status.is_success(), "listing after delete failed: {body}");
    assert!(
        !data_guids(&body).contains(&"deleted-item".to_string()),
        "a deleted feed must give no row, got {body}"
    );
}
