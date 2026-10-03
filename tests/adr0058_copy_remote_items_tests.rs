// ADR 0058 task 007: a copy row gives its feed list (section 1d).
//
// docs/tasks/adr-0058-task-007-copy-remote-items.md
//
// A copy keeps its channel-level `remoteItem` entries. The node compares them
// with the entries of the record by the feed that each one resolves to, so a
// host that writes its own ID as `feedGuid` does not make a difference
// (musicindex.org request 12, the Longy case).

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0058-copy-remote-items-token";
const RECORD_URL: &str = "https://record.example/publisher.xml";
const COPY_URL: &str = "https://copy.example/artist.xml";
const PUBLISHER_GUID: &str = "publisher-guid";

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0058-copy-remote-items-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: vec!["content_hash".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());

    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
    })
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

async fn ingest_response(app: axum::Router, payload: &serde_json::Value) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(payload).expect("serialize")))
                .expect("build request"),
        )
        .await
        .expect("send request");
    body_json(resp).await
}

async fn accept_source(
    state: &Arc<stophammer::api::AppState>,
    url: &str,
    crawl_token: &str,
    content_hash: &str,
    feed_data: &serde_json::Value,
) {
    let payload = ingest_payload(url, url, crawl_token, content_hash, feed_data);
    let body = ingest_response(stophammer::api::build_router(Arc::clone(state)), &payload).await;
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
}

async fn submit_mirror(
    state: &Arc<stophammer::api::AppState>,
    url: &str,
    crawl_token: &str,
    content_hash: &str,
    feed_data: &serde_json::Value,
) -> serde_json::Value {
    let payload = ingest_payload(url, url, crawl_token, content_hash, feed_data);
    ingest_response(stophammer::api::build_router(Arc::clone(state)), &payload).await
}

fn ingest_payload(
    canonical_url: &str,
    source_url: &str,
    crawl_token: &str,
    content_hash: &str,
    feed_data: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": canonical_url,
        "source_url": source_url,
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": content_hash,
        "feed_data": feed_data,
    })
}

fn copy_observed_count(db: &Arc<Mutex<rusqlite::Connection>>) -> usize {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_events_since(&conn, 0, 10_000)
        .expect("read events")
        .into_iter()
        .filter(|ev| {
            matches!(
                ev.event_type,
                stophammer::event::EventType::FeedCopyObserved
            )
        })
        .count()
}

async fn send(
    state: &Arc<stophammer::api::AppState>,
    method: &str,
    uri: &str,
    body: Option<&Value>,
) -> (http::StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("X-Admin-Token", "test-admin-token");
    let body = match body {
        Some(value) => {
            builder = builder.header("Content-Type", "application/json");
            Body::from(serde_json::to_vec(value).expect("serialize"))
        }
        None => Body::empty(),
    };
    let resp = stophammer::api::build_router(Arc::clone(state))
        .oneshot(builder.body(body).expect("build request"))
        .await
        .expect("send request");
    let status = resp.status();
    (status, body_json(resp).await)
}

async fn copy_row(state: &Arc<stophammer::api::AppState>, feed_guid: &str) -> Value {
    let (status, body) = send(state, "GET", &format!("/v1/feeds/{feed_guid}/copies"), None).await;
    assert_eq!(status, 200, "the copies route must answer 200: {body}");
    let rows = body["data"].as_array().expect("data array");
    assert_eq!(rows.len(), 1, "expected one copy row: {body}");
    rows[0].clone()
}

async fn open_copy_records(state: &Arc<stophammer::api::AppState>) -> usize {
    let (_, body) = send(state, "GET", "/v1/copies", None).await;
    body["data"].as_array().expect("data array").len()
}

/// A channel-level `remoteItem`.
fn entry(medium: &str, feed_guid: &str, feed_url: &str, item_guid: Option<&str>) -> Value {
    let mut item = json!({
        "position": 0,
        "medium": medium,
        "remote_feed_guid": feed_guid,
        "remote_feed_url": feed_url,
    });
    if let Some(item_guid) = item_guid {
        item["item_guid"] = json!(item_guid);
    }
    item
}

/// A body of `feed_guid` with these channel-level entries, no item and no
/// payment route, so only the list can differ.
fn list_feed(feed_guid: &str, medium: &str, entries: &[Value]) -> Value {
    let entries: Vec<Value> = entries
        .iter()
        .enumerate()
        .map(|(position, item)| {
            let mut item = item.clone();
            item["position"] = json!(position);
            item
        })
        .collect();
    json!({
        "feed_guid": feed_guid,
        "title": format!("Feed {feed_guid}"),
        "raw_medium": medium,
        "explicit": false,
        "feed_payment_routes": [],
        "tracks": [],
        "remote_items": entries,
    })
}

/// Stores the albums A and B, and a record publisher feed at `RECORD_URL`
/// that lists album A by its `podcast:guid`.
async fn albums_and_record(
    db: &Arc<Mutex<rusqlite::Connection>>,
) -> Arc<stophammer::api::AppState> {
    let state = test_app_state(Arc::clone(db), TOKEN);
    for (guid, url) in [
        ("album-a", "https://a.example/a.xml"),
        ("album-b", "https://b.example/b.xml"),
    ] {
        accept_source(
            &state,
            url,
            TOKEN,
            &format!("hash-{guid}"),
            &list_feed(guid, "music", &[]),
        )
        .await;
    }
    accept_source(
        &state,
        RECORD_URL,
        TOKEN,
        "hash-record",
        &list_feed(
            PUBLISHER_GUID,
            "publisher",
            &[entry("music", "album-a", "https://a.example/a.xml", None)],
        ),
    )
    .await;
    state
}

/// The copy lists album A with the host's own ID as `feedGuid`, and the URL
/// of album A.
fn longy_copy() -> Value {
    list_feed(
        PUBLISHER_GUID,
        "publisher",
        &[entry("music", "host-id-a", "https://a.example/a.xml", None)],
    )
}

#[tokio::test]
async fn the_same_album_under_another_feed_guid_is_not_a_difference() {
    let db = common::test_db_arc();
    let state = albums_and_record(&db).await;
    submit_mirror(&state, COPY_URL, TOKEN, "copy-1", &longy_copy()).await;

    let row = copy_row(&state, PUBLISHER_GUID).await;
    assert_eq!(row["differs_remote_items"], false, "ADR 0058 §1d: {row}");
    let entry = &row["remote_items"][0];
    assert_eq!(
        entry["feed_guid"], "host-id-a",
        "the raw value stays: {row}"
    );
    assert_eq!(entry["resolved_feed_guid"], "album-a", "{row}");
    assert_eq!(
        entry["remote_feed_title"], "Feed album-a",
        "ADR 0059: {row}"
    );
}

#[tokio::test]
async fn an_album_that_the_record_does_not_list_is_a_difference() {
    let db = common::test_db_arc();
    let state = albums_and_record(&db).await;
    let copy = list_feed(
        PUBLISHER_GUID,
        "publisher",
        &[
            entry("music", "host-id-a", "https://a.example/a.xml", None),
            entry("music", "album-b", "https://b.example/b.xml", None),
        ],
    );
    submit_mirror(&state, COPY_URL, TOKEN, "copy-1", &copy).await;

    let row = copy_row(&state, PUBLISHER_GUID).await;
    assert_eq!(row["differs_remote_items"], true, "ADR 0058 §2: {row}");
    assert_eq!(row["open"], true, "{row}");
    assert_eq!(
        open_copy_records(&state).await,
        1,
        "GET /v1/copies lists the record"
    );
}

#[tokio::test]
async fn an_album_copy_that_names_another_publisher_is_a_difference() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), TOKEN);
    let named = |publisher: &str| {
        let mut item = entry(
            "publisher",
            publisher,
            &format!("https://p.example/{publisher}.xml"),
            None,
        );
        item["publisher_reference"] = json!(true);
        list_feed("album-c", "music", &[item])
    };
    accept_source(
        &state,
        "https://c.example/c.xml",
        TOKEN,
        "hash-c",
        &named("label-one"),
    )
    .await;
    submit_mirror(&state, COPY_URL, TOKEN, "copy-1", &named("label-two")).await;

    let row = copy_row(&state, "album-c").await;
    assert_eq!(row["differs_remote_items"], true, "ADR 0058 §1d: {row}");
}

#[tokio::test]
async fn two_list_entries_of_one_feed_are_two_entries() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), TOKEN);
    let track = |item_guid: &str| {
        entry(
            "music",
            "album-a",
            "https://a.example/a.xml",
            Some(item_guid),
        )
    };
    accept_source(
        &state,
        RECORD_URL,
        TOKEN,
        "hash-list",
        &list_feed("list-guid", "musicL", &[track("track-1"), track("track-2")]),
    )
    .await;
    submit_mirror(
        &state,
        COPY_URL,
        TOKEN,
        "copy-1",
        &list_feed("list-guid", "musicL", &[track("track-1")]),
    )
    .await;

    let row = copy_row(&state, "list-guid").await;
    assert_eq!(row["remote_items"][0]["item_guid"], "track-1", "{row}");
    assert_eq!(
        row["differs_remote_items"], true,
        "ADR 0058 §1d: the copy lacks the second track of the same feed: {row}"
    );
}

#[tokio::test]
async fn the_same_entries_in_another_sequence_sign_no_event() {
    let db = common::test_db_arc();
    let state = albums_and_record(&db).await;
    let a = entry("music", "host-id-a", "https://a.example/a.xml", None);
    let b = entry("music", "album-b", "https://b.example/b.xml", None);
    submit_mirror(
        &state,
        COPY_URL,
        TOKEN,
        "copy-1",
        &list_feed(PUBLISHER_GUID, "publisher", &[a.clone(), b.clone()]),
    )
    .await;
    let before = copy_observed_count(&db);
    submit_mirror(
        &state,
        COPY_URL,
        TOKEN,
        "copy-2",
        &list_feed(PUBLISHER_GUID, "publisher", &[b, a]),
    )
    .await;
    assert_eq!(
        copy_observed_count(&db),
        before,
        "ADR 0058 §1d: the sequence is not a change"
    );
}

#[tokio::test]
async fn a_row_with_no_list_gets_it_once_and_an_old_resolution_holds() {
    let db = common::test_db_arc();
    let state = albums_and_record(&db).await;
    let copy = list_feed(
        PUBLISHER_GUID,
        "publisher",
        &[entry("music", "album-b", "https://b.example/b.xml", None)],
    );
    submit_mirror(&state, COPY_URL, TOKEN, "copy-1", &copy).await;

    // The row and its resolution as a node wrote them before section 1d.
    {
        let conn = db.lock().expect("lock db");
        conn.execute(
            "UPDATE feed_copies SET remote_items = NULL, remote_items_digest = NULL, \
             resolution = 'keep_source', resolution_reason = 'old', resolved_at = 1, \
             resolved_digest = summary_digest, resolved_remote_items_digest = NULL \
             WHERE url = ?1",
            [COPY_URL],
        )
        .expect("write the old row");
    }
    let row = copy_row(&state, PUBLISHER_GUID).await;
    assert_eq!(row["remote_items"], Value::Null, "{row}");
    assert_eq!(
        row["differs_remote_items"], false,
        "a row with no list gives false: {row}"
    );

    let before = copy_observed_count(&db);
    submit_mirror(&state, COPY_URL, TOKEN, "copy-2", &copy).await;
    assert_eq!(
        copy_observed_count(&db),
        before + 1,
        "the list arrives with one event"
    );

    let row = copy_row(&state, PUBLISHER_GUID).await;
    assert_eq!(row["differs_remote_items"], true, "{row}");
    assert_eq!(
        row["open"], false,
        "ADR 0058 §1d: the old resolution holds: {row}"
    );
    assert_eq!(row["resolution"]["current"], true, "{row}");
}

#[tokio::test]
async fn a_new_resolution_names_the_list_and_a_list_change_opens_it() {
    let db = common::test_db_arc();
    let state = albums_and_record(&db).await;
    let b = entry("music", "album-b", "https://b.example/b.xml", None);
    submit_mirror(
        &state,
        COPY_URL,
        TOKEN,
        "copy-1",
        &list_feed(PUBLISHER_GUID, "publisher", std::slice::from_ref(&b)),
    )
    .await;

    let (status, body) = send(
        &state,
        "POST",
        &format!("/v1/feeds/{PUBLISHER_GUID}/copies/resolve"),
        Some(&json!({ "url": COPY_URL, "decision": "keep_source", "reason": "checked" })),
    )
    .await;
    assert!(status.is_success(), "resolve failed with {status}: {body}");
    assert_eq!(copy_row(&state, PUBLISHER_GUID).await["open"], false);

    let a = entry("music", "host-id-a", "https://a.example/a.xml", None);
    submit_mirror(
        &state,
        COPY_URL,
        TOKEN,
        "copy-2",
        &list_feed(PUBLISHER_GUID, "publisher", &[b, a]),
    )
    .await;
    let row = copy_row(&state, PUBLISHER_GUID).await;
    assert_eq!(
        row["open"], true,
        "ADR 0058 §1d: a list change opens the copy: {row}"
    );
    assert_eq!(row["resolution"]["current"], false, "{row}");
}

#[test]
fn old_payloads_with_no_list_deserialize() {
    let observed: stophammer::event::FeedCopyObservedPayload = serde_json::from_value(json!({
        "feed_guid": "g",
        "url": COPY_URL,
        "first_seen": 1,
        "title": "Copy",
        "item_guids": [],
        "feed_recipients": [],
        "track_recipients": {},
        "summary_digest": "d"
    }))
    .expect("an old FeedCopyObserved payload must deserialize");
    assert_eq!(observed.remote_items, None);
    let resolved: stophammer::event::FeedCopyResolvedPayload = serde_json::from_value(json!({
        "feed_guid": "g",
        "url": COPY_URL,
        "decision": "keep_source",
        "reason": "r",
        "resolved_at": 1,
        "resolved_digest": "d"
    }))
    .expect("an old FeedCopyResolved payload must deserialize");
    assert_eq!(resolved.resolved_remote_items_digest, None);
}

#[tokio::test]
async fn an_old_event_applies_on_a_replica_with_no_list() {
    let db_a = common::test_db_arc();
    let state_a = albums_and_record(&db_a).await;
    submit_mirror(&state_a, COPY_URL, TOKEN, "copy-1", &longy_copy()).await;

    let mut events = {
        let conn = db_a.lock().expect("lock db_a");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events")
    };
    for ev in &mut events {
        if let stophammer::event::EventPayload::FeedCopyObserved(p) = &mut ev.payload {
            p.remote_items = None;
            ev.payload_json = serde_json::to_string(p).expect("serialize payload");
            assert!(
                !ev.payload_json.contains("remote_items"),
                "{}",
                ev.payload_json
            );
        }
    }
    let db_b = common::test_db_arc();
    let pool_b = common::wrap_pool(Arc::clone(&db_b));
    for ev in &events {
        stophammer::apply::apply_single_event(&pool_b, ev)
            .unwrap_or_else(|err| panic!("event {:?} must apply: {err:?}", ev.event_type));
    }
    let state_b = test_app_state(Arc::clone(&db_b), "unused-b-token");
    let row = copy_row(&state_b, PUBLISHER_GUID).await;
    assert_eq!(row["remote_items"], Value::Null, "{row}");
}

#[tokio::test]
async fn a_replica_gives_the_same_list_and_difference() {
    let db_a = common::test_db_arc();
    let state_a = albums_and_record(&db_a).await;
    let copy = list_feed(
        PUBLISHER_GUID,
        "publisher",
        &[
            entry("music", "host-id-a", "https://a.example/a.xml", None),
            entry("music", "album-b", "https://b.example/b.xml", None),
        ],
    );
    submit_mirror(&state_a, COPY_URL, TOKEN, "copy-1", &copy).await;

    let events = {
        let conn = db_a.lock().expect("lock db_a");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events")
    };
    let db_b = common::test_db_arc();
    let pool_b = common::wrap_pool(Arc::clone(&db_b));
    for ev in &events {
        stophammer::apply::apply_single_event(&pool_b, ev)
            .unwrap_or_else(|err| panic!("event {:?} must apply: {err:?}", ev.event_type));
    }
    let state_b = test_app_state(Arc::clone(&db_b), "unused-b-token");
    let row_a = copy_row(&state_a, PUBLISHER_GUID).await;
    let row_b = copy_row(&state_b, PUBLISHER_GUID).await;
    for field in ["remote_items", "differs_remote_items"] {
        assert_eq!(
            row_a[field], row_b[field],
            "the replica must give the same {field}"
        );
    }
    assert_eq!(row_b["differs_remote_items"], true, "{row_b}");
}

#[tokio::test]
async fn a_javascript_feed_url_gives_null() {
    let db = common::test_db_arc();
    let state = albums_and_record(&db).await;
    let copy = list_feed(
        PUBLISHER_GUID,
        "publisher",
        &[entry("music", "album-x", "javascript:alert(1)", None)],
    );
    submit_mirror(&state, COPY_URL, TOKEN, "copy-1", &copy).await;
    let row = copy_row(&state, PUBLISHER_GUID).await;
    assert_eq!(
        row["remote_items"][0]["feed_url"],
        Value::Null,
        "ADR 0054 §4: {row}"
    );
}
