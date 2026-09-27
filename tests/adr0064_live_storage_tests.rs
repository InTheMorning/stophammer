// ADR 0064: a live item is an RSS fact.
//
// docs/tasks/adr-0064-task-002-node-storage.md
//
// The node stores the relay link of a live item: `live_value_uri` and
// `live_value_protocol`. It replicates them in `LiveEventsReplaced`, and it
// emits that event only when the deduplicated, sorted set of live rows
// changes.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use rusqlite::params;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0064-crawl-token";
const ADMIN: &str = "test-adr0064-admin-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0064-signer"));
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
/// differ from the first, so the node does not skip it as a duplicate.
async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    url: &str,
    extra: &Value,
    version: u32,
) {
    let mut feed_data = json!({
        "feed_guid": guid,
        "title": format!("Feed {guid}"),
        "raw_medium": "music",
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

/// One `podcast:liveItem` for an ingest payload. `uri` and `protocol` are the
/// `podcast:liveValue` relay link (ADR 0064 section 3).
fn live_item(guid: &str, status: &str, uri: Option<&str>, protocol: Option<&str>) -> Value {
    json!({
        "live_item_guid": guid,
        "title": format!("Live {guid}"),
        "status": status,
        "start_at": 1_700_000_000,
        "end_at": 1_700_003_600,
        "explicit": false,
        "live_value_uri": uri,
        "live_value_protocol": protocol,
    })
}

fn count_live_events_replaced(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> i64 {
    let conn = db.lock().expect("lock");
    conn.query_row(
        "SELECT COUNT(*) FROM events WHERE event_type = 'live_events_replaced' \
         AND subject_guid = ?1",
        params![feed_guid],
        |row| row.get(0),
    )
    .expect("count live_events_replaced events")
}

fn live_rows(
    db: &Arc<Mutex<rusqlite::Connection>>,
    feed_guid: &str,
) -> Vec<stophammer::model::LiveEvent> {
    let conn = db.lock().expect("lock");
    stophammer::db::get_live_events_for_feed(&conn, feed_guid).expect("read live events")
}

// ---------------------------------------------------------------------------
// An ingest with a relay link stores both values, and a second read gives
// them.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ingest_with_relay_link_stores_and_reads_it() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        "live-relay-1",
        "https://l.example/relay1.xml",
        &json!({
            "live_items": [live_item(
                "item-a",
                "live",
                Some("https://relay.example/event?event_id=abc"),
                Some("socket.io"),
            )]
        }),
        1,
    )
    .await;

    let rows = live_rows(&db, "live-relay-1");
    assert_eq!(rows.len(), 1, "the ingest must store one live row");
    assert_eq!(
        rows[0].live_value_uri.as_deref(),
        Some("https://relay.example/event?event_id=abc"),
        "a second read must give the stored relay uri"
    );
    assert_eq!(
        rows[0].live_value_protocol.as_deref(),
        Some("socket.io"),
        "a second read must give the stored relay protocol"
    );
}

// ---------------------------------------------------------------------------
// A second ingest of the same live items in a different RSS order emits no
// new LiveEventsReplaced.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_live_items_in_different_order_emit_no_new_event() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "live-order-1";
    let url = "https://l.example/order1.xml";

    ingest(
        &st,
        guid,
        url,
        &json!({
            "live_items": [
                live_item("item-a", "live", None, None),
                live_item("item-b", "pending", None, None),
            ]
        }),
        1,
    )
    .await;
    let first_count = count_live_events_replaced(&db, guid);
    assert_eq!(
        first_count, 1,
        "the first ingest must emit one LiveEventsReplaced event"
    );

    // Same two rows, reversed RSS order.
    ingest(
        &st,
        guid,
        url,
        &json!({
            "live_items": [
                live_item("item-b", "pending", None, None),
                live_item("item-a", "live", None, None),
            ]
        }),
        2,
    )
    .await;
    let second_count = count_live_events_replaced(&db, guid);
    assert_eq!(
        second_count, first_count,
        "a re-ingest with the same rows in a different RSS order must emit no new event"
    );
}

// ---------------------------------------------------------------------------
// An ingest with one live_item_guid two times emits one event, and a second
// ingest of the same feed emits none.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn duplicate_live_item_guid_emits_one_event_and_then_none() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "live-dup-1";
    let url = "https://l.example/dup1.xml";
    let items = json!({
        "live_items": [
            live_item("item-dup", "live", None, None),
            live_item("item-dup", "live", None, None),
        ]
    });

    ingest(&st, guid, url, &items, 1).await;
    let first_count = count_live_events_replaced(&db, guid);
    assert_eq!(
        first_count, 1,
        "an ingest with one live_item_guid two times must emit exactly one event"
    );

    let rows = live_rows(&db, guid);
    assert_eq!(
        rows.len(),
        1,
        "the stored rows must keep only the first of a duplicated live_item_guid"
    );

    ingest(&st, guid, url, &items, 2).await;
    let second_count = count_live_events_replaced(&db, guid);
    assert_eq!(
        second_count, first_count,
        "a second ingest of the same feed must emit no new event"
    );
}

// ---------------------------------------------------------------------------
// A change of only live_value_uri emits a new event.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn change_of_only_live_value_uri_emits_new_event() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "live-uri-change-1";
    let url = "https://l.example/urichange1.xml";

    ingest(
        &st,
        guid,
        url,
        &json!({
            "live_items": [live_item(
                "item-a",
                "live",
                Some("https://relay.example/event?event_id=abc"),
                Some("socket.io"),
            )]
        }),
        1,
    )
    .await;
    let first_count = count_live_events_replaced(&db, guid);
    assert_eq!(first_count, 1, "the first ingest must emit one event");

    ingest(
        &st,
        guid,
        url,
        &json!({
            "live_items": [live_item(
                "item-a",
                "live",
                Some("https://relay.example/event?event_id=xyz"),
                Some("socket.io"),
            )]
        }),
        2,
    )
    .await;
    let second_count = count_live_events_replaced(&db, guid);
    assert_eq!(
        second_count,
        first_count + 1,
        "a change of only live_value_uri must emit a new event"
    );

    let rows = live_rows(&db, guid);
    assert_eq!(
        rows[0].live_value_uri.as_deref(),
        Some("https://relay.example/event?event_id=xyz"),
        "the stored row must carry the new relay uri"
    );
}

// ---------------------------------------------------------------------------
// apply of a LiveEventsReplaced payload with no relay fields stores None for
// both. apply of a payload with them stores them.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn apply_stores_relay_fields_or_none() {
    use stophammer::event::{Event, EventPayload, EventType, LiveEventsReplacedPayload};
    use stophammer::model::LiveEvent;

    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let guid = "live-apply-1";
    // Create the feed row first. live_events.feed_guid references feeds.
    ingest(&st, guid, "https://l.example/apply1.xml", &json!({}), 1).await;

    let pool = stophammer::db_pool::DbPool::from_writer_only(Arc::clone(&db));
    let now = common::now();

    // An old payload, built with no live_value_uri or live_value_protocol
    // key at all. It must still deserialize (no deny_unknown_fields, and
    // #[serde(default)] on the new fields).
    let old_shape_value = json!({
        "feed_guid": guid,
        "live_events": [{
            "live_item_guid": "live-old-1",
            "feed_guid": guid,
            "title": "Old Shape Live",
            "content_link": null,
            "status": "live",
            "scheduled_start": now,
            "scheduled_end": now + 3600,
            "created_at": now,
            "updated_at": now
        }]
    });
    let old_shape: LiveEventsReplacedPayload = serde_json::from_value(old_shape_value.clone())
        .expect("a payload with no relay fields must still deserialize");
    let ev = Event {
        event_id: "evt-live-apply-old".into(),
        event_type: EventType::LiveEventsReplaced,
        payload: EventPayload::LiveEventsReplaced(old_shape),
        subject_guid: guid.into(),
        signed_by: "deadbeef".into(),
        signature: "cafebabe".into(),
        seq: 1,
        created_at: now,
        warnings: vec![],
        // The raw string an old node would have signed: no relay keys at all.
        payload_json: old_shape_value.to_string(),
    };
    stophammer::apply::apply_single_event(&pool, &ev).expect("apply of the old-shape event");

    let rows = live_rows(&db, guid);
    let old_row = rows
        .iter()
        .find(|row| row.live_item_guid == "live-old-1")
        .expect("the old-shape row must exist");
    assert_eq!(
        old_row.live_value_uri, None,
        "a payload with no relay fields must store None for the uri"
    );
    assert_eq!(
        old_row.live_value_protocol, None,
        "a payload with no relay fields must store None for the protocol"
    );

    // A payload that names the relay link stores it.
    let with_relay = LiveEventsReplacedPayload {
        feed_guid: guid.into(),
        live_events: vec![LiveEvent {
            live_item_guid: "live-new-1".into(),
            feed_guid: guid.into(),
            title: "New Shape Live".into(),
            content_link: None,
            status: "live".into(),
            scheduled_start: Some(now),
            scheduled_end: Some(now + 3600),
            created_at: now,
            updated_at: now,
            live_value_uri: Some("https://relay.example/event?event_id=new".into()),
            live_value_protocol: Some("socket.io".into()),
        }],
    };
    let payload_json = serde_json::to_string(&with_relay).expect("serialize payload");
    let ev2 = Event {
        event_id: "evt-live-apply-new".into(),
        event_type: EventType::LiveEventsReplaced,
        payload: EventPayload::LiveEventsReplaced(with_relay),
        subject_guid: guid.into(),
        signed_by: "deadbeef".into(),
        signature: "cafebabe".into(),
        seq: 2,
        created_at: now,
        warnings: vec![],
        payload_json,
    };
    stophammer::apply::apply_single_event(&pool, &ev2).expect("apply of the new-shape event");

    let rows = live_rows(&db, guid);
    let new_row = rows
        .iter()
        .find(|row| row.live_item_guid == "live-new-1")
        .expect("the new-shape row must exist");
    assert_eq!(
        new_row.live_value_uri.as_deref(),
        Some("https://relay.example/event?event_id=new"),
        "a payload with relay fields must store the uri"
    );
    assert_eq!(
        new_row.live_value_protocol.as_deref(),
        Some("socket.io"),
        "a payload with relay fields must store the protocol"
    );
}
