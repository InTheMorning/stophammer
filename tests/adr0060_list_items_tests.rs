// ADR 0060: a list feed keeps its items.
//
// docs/tasks/adr-0060-task-002-node-list-items.md
//
// A `musicL` feed keeps the `itemGuid` and the `title` of each channel
// `podcast:remoteItem`, and each entry gives the indexed track that it
// names. The value block of the list is source data that no route gives.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0060-crawl-token";
const ADMIN: &str = "test-adr0060-admin-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0060-signer"));
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

async fn send(
    st: &Arc<stophammer::api::AppState>,
    method: &str,
    uri: &str,
    body: Option<&Value>,
) -> (http::StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("X-Admin-Token", ADMIN);
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
    (status, json)
}

/// Ingests one feed. `version` makes the content hash of a second ingest
/// differ from the first.
async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    url: &str,
    medium: &str,
    extra: &Value,
    version: u32,
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
        "content_hash": format!("hash-{guid}-{version}"),
        "feed_data": feed_data,
    });
    let (status, body) = send(st, "POST", "/ingest/feed", Some(&payload)).await;
    assert!(
        status.is_success(),
        "ingest of {guid} failed with {status}: {body}"
    );
}

fn track(guid: &str) -> Value {
    json!({
        "track_guid": guid,
        "title": format!("Track {guid}"),
        "pub_date": 1_700_000_000,
        "explicit": false,
        "payment_routes": [],
        "value_time_splits": [],
    })
}

fn entry(position: i64, feed_guid: &str, feed_url: &str, item_guid: Option<&str>) -> Value {
    json!({
        "position": position,
        "medium": "music",
        "remote_feed_guid": feed_guid,
        "remote_feed_url": feed_url,
        "item_guid": item_guid,
        "item_title": item_guid.map(|g| format!("Title of {g}")),
    })
}

fn route(address: &str, split: i64) -> Value {
    json!({
        "recipient_name": "Curator",
        "route_type": "node",
        "address": address,
        "custom_key": null,
        "custom_value": null,
        "split": split,
        "fee": false,
    })
}

async fn remote_items(st: &Arc<stophammer::api::AppState>, guid: &str) -> Vec<Value> {
    let (status, body) = send(
        st,
        "GET",
        &format!("/v1/feeds/{guid}?include=remote_items"),
        None,
    )
    .await;
    assert!(status.is_success(), "read of {guid} failed with {status}");
    body["data"]["remote_items"]
        .as_array()
        .expect("remote_items")
        .clone()
}

fn by_guid<'a>(items: &'a [Value], feed_guid: &str) -> &'a Value {
    items
        .iter()
        .find(|e| e["remote_feed_guid"] == feed_guid)
        .unwrap_or_else(|| panic!("no entry for {feed_guid}"))
}

fn list_value_addresses(db: &Arc<Mutex<rusqlite::Connection>>, guid: &str) -> Vec<String> {
    let conn = db.lock().expect("lock");
    let mut stmt = conn
        .prepare("SELECT address FROM feed_list_value_raw WHERE feed_guid = ?1 ORDER BY position")
        .expect("prepare");
    stmt.query_map([guid], |row| row.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows")
}

#[tokio::test]
async fn a_track_entry_gives_its_item_guid_title_and_indexed_track() {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        "album-1",
        "https://a.example/1.xml",
        "music",
        &json!({ "tracks": [track("t1")] }),
        1,
    )
    .await;
    ingest(
        &st,
        "list-1",
        "https://l.example/1.xml",
        "musicL",
        &json!({ "remote_items": [
            entry(0, "album-1", "https://a.example/1.xml", Some("t1")),
            entry(1, "album-1", "https://a.example/1.xml", Some("t-missing")),
        ]}),
        1,
    )
    .await;

    let items = remote_items(&st, "list-1").await;
    let indexed = &items[0];
    assert_eq!(
        indexed["remote_item_guid"], "t1",
        "ADR 0060 guard 2: the entry gives its stored itemGuid"
    );
    assert_eq!(
        indexed["remote_item_title"], "Title of t1",
        "ADR 0060 guard 2: the entry gives its stored title"
    );
    assert_eq!(
        indexed["remote_track_guid"], "t1",
        "ADR 0060 guard 2: the entry gives the track_guid of the indexed track"
    );

    let missing = &items[1];
    assert_eq!(
        missing["remote_item_guid"], "t-missing",
        "the entry keeps its itemGuid when the track is not indexed"
    );
    assert!(
        missing["remote_track_guid"].is_null(),
        "ADR 0060 guard 2: an entry for a track that is not indexed gives null in remote_track_guid"
    );
}

#[tokio::test]
async fn an_album_entry_gives_null_in_each_track_field() {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        "album-2",
        "https://a.example/2.xml",
        "music",
        &json!({ "tracks": [track("t2")] }),
        1,
    )
    .await;
    ingest(
        &st,
        "list-2",
        "https://l.example/2.xml",
        "musicL",
        &json!({ "remote_items": [entry(0, "album-2", "https://a.example/2.xml", None)] }),
        1,
    )
    .await;

    let items = remote_items(&st, "list-2").await;
    let album = by_guid(&items, "album-2");
    for key in ["remote_item_guid", "remote_item_title", "remote_track_guid"] {
        assert!(
            album.get(key).is_some_and(Value::is_null),
            "ADR 0060 §3: an entry with no itemGuid gives the key {key} with null"
        );
    }
}

#[tokio::test]
async fn an_entry_that_resolves_by_url_gives_the_track_of_that_feed() {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        "album-3",
        "https://a.example/3.xml",
        "music",
        &json!({ "tracks": [track("t3")] }),
        1,
    )
    .await;
    ingest(
        &st,
        "list-3",
        "https://l.example/3.xml",
        "musicL",
        &json!({ "remote_items": [
            entry(0, "guid-not-indexed", "https://a.example/3.xml", Some("t3")),
        ]}),
        1,
    )
    .await;

    let items = remote_items(&st, "list-3").await;
    assert_eq!(
        by_guid(&items, "guid-not-indexed")["remote_track_guid"],
        "t3",
        "ADR 0060 guard 3: an entry that resolves its feed by URL gives the track of the feed at that URL"
    );
}

#[tokio::test]
async fn a_list_value_block_is_stored_and_not_served() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        "list-4",
        "https://l.example/4.xml",
        "musicL",
        &json!({ "feed_payment_routes": [route("node-a", 60), route("node-b", 40)] }),
        1,
    )
    .await;

    assert_eq!(
        list_value_addresses(&db, "list-4"),
        vec!["node-a".to_string(), "node-b".to_string()],
        "ADR 0060 guard 4: the node stores the value block of a list feed in feed_list_value_raw"
    );
    let (_, body) = send(&st, "GET", "/v1/feeds/list-4?include=payment_routes", None).await;
    assert_eq!(
        body["data"]["payment_routes"],
        json!([]),
        "ADR 0060 guard 4: include=payment_routes on a list feed gives an empty list"
    );

    ingest(
        &st,
        "list-4",
        "https://l.example/4.xml",
        "musicL",
        &json!({ "feed_payment_routes": [route("node-c", 100)] }),
        2,
    )
    .await;
    assert_eq!(
        list_value_addresses(&db, "list-4"),
        vec!["node-c".to_string()],
        "ADR 0060 §4: a second ingest replaces the rows of the value block"
    );
}

#[tokio::test]
async fn a_community_node_applies_the_same_value_block_rows() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        "list-5",
        "https://l.example/5.xml",
        "musicL",
        &json!({ "feed_payment_routes": [route("node-a", 100)] }),
        1,
    )
    .await;
    // The block is removed. The primary must sign this change too, or a
    // community node keeps the old row.
    ingest(
        &st,
        "list-5",
        "https://l.example/5.xml",
        "musicL",
        &json!({ "feed_payment_routes": [] }),
        2,
    )
    .await;

    let events = {
        let conn = db.lock().expect("lock");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("events")
    };
    let list_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == stophammer::event::EventType::FeedListValueReplaced)
        .collect();
    assert_eq!(
        list_events.len(),
        2,
        "ADR 0060 §4: the primary signs a FeedListValueReplaced event for the block and for its removal"
    );

    let community_db = common::test_db_arc();
    let community = stophammer::db_pool::DbPool::from_writer_only(Arc::clone(&community_db));
    let mut applied_rows_after_first = None;
    for ev in &events {
        stophammer::apply::apply_single_event(&community, ev).expect("apply on the community node");
        if ev.event_id == list_events[0].event_id {
            applied_rows_after_first = Some(list_value_addresses(&community_db, "list-5"));
        }
    }
    assert_eq!(
        applied_rows_after_first,
        Some(vec!["node-a".to_string()]),
        "ADR 0060 guard 4: a community node that applies the event stores the same rows"
    );
    assert!(
        list_value_addresses(&community_db, "list-5").is_empty(),
        "ADR 0060 §4: the removal of the block also removes the rows on a community node"
    );
}

#[tokio::test]
async fn a_delete_of_the_list_feed_removes_its_value_rows() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        "list-6",
        "https://l.example/6.xml",
        "musicL",
        &json!({ "feed_payment_routes": [route("node-a", 100)] }),
        1,
    )
    .await;
    assert_eq!(
        list_value_addresses(&db, "list-6").len(),
        1,
        "the value block is stored"
    );

    let (status, _) = send(&st, "DELETE", "/v1/feeds/list-6", None).await;
    assert!(status.is_success(), "delete failed with {status}");
    assert!(
        list_value_addresses(&db, "list-6").is_empty(),
        "ADR 0060 §4: a delete of the list feed removes its rows of feed_list_value_raw"
    );
}

#[test]
fn migration_0043_adds_the_columns_and_the_table() {
    let conn = common::test_db();
    let columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('feed_remote_items_raw')")
        .expect("prepare")
        .query_map([], |row| row.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    for column in ["remote_item_guid", "remote_item_title"] {
        assert!(
            columns.iter().any(|c| c == column),
            "ADR 0060 §2: migration 0043 adds {column} to feed_remote_items_raw"
        );
    }
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'feed_list_value_raw'",
            [],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(
        tables, 1,
        "ADR 0060 §4: migration 0043 creates feed_list_value_raw"
    );
}
