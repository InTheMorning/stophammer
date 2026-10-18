// ADR 0058 task 008: a copy row gives its item facts.
//
// The duration, the enclosure URL and the publication date of each item are
// stored as a JSON array in the order of item GUIDs. Each entry is a value
// or null (when the item has no value). The channel author is a plain
// column, or null when the copy states none. A change of one of the four
// values signs one FeedCopyObserved event. The summary digest stays
// unchanged.
//
// This test file follows the patterns of tests/adr0058_copy_titles_tests.rs.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0058-copy-item-facts-signer"));
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
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
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

/// A mirror that pays a different recipient than the source, so it is a
/// copy and not an alias, with one item carrying the four item-fact values.
fn copy_feed_with_item_facts(
    feed_guid: &str,
    author_name: Option<&str>,
    duration_secs: Option<i64>,
    enclosure_url: Option<&str>,
    pub_date: Option<i64>,
) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Mirror Feed",
        "author_name": author_name,
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [{
            "recipient_name": null,
            "route_type": "keysend",
            "address": "attacker@ln.example",
            "split": 100,
            "fee": false
        }],
        "tracks": [
            {
                "track_guid": "item-guid-1",
                "title": "Item 1",
                "duration_secs": duration_secs,
                "enclosure_url": enclosure_url,
                "pub_date": pub_date,
                "explicit": false
            }
        ]
    })
}

/// Ingests a source feed and one copy at `mirror_url`, and gives the state.
async fn source_and_copy(
    db: &Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
    feed_guid: &str,
    mirror_url: &str,
    copy: &serde_json::Value,
) -> Arc<stophammer::api::AppState> {
    let state = test_app_state(Arc::clone(db), crawl_token);
    let source_feed = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Source Feed",
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [],
        "tracks": [
            { "track_guid": "item-guid-1", "title": "Item 1", "explicit": false }
        ]
    });
    accept_source(
        &state,
        "https://source.example.com/feed.xml",
        crawl_token,
        "source-hash",
        &source_feed,
    )
    .await;
    submit_mirror(&state, mirror_url, crawl_token, "copy-hash-1", copy).await;
    state
}

fn copy_observed_events(db: &Arc<Mutex<rusqlite::Connection>>) -> Vec<stophammer::event::Event> {
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
        .collect()
}

fn copy_observed_count(db: &Arc<Mutex<rusqlite::Connection>>) -> usize {
    copy_observed_events(db).len()
}

fn copy_row(
    db: &Arc<Mutex<rusqlite::Connection>>,
    feed_guid: &str,
    url: &str,
) -> stophammer::db::FeedCopyRow {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_feed_copy(&conn, feed_guid, url)
        .expect("query feed_copies")
        .expect("copy row should exist")
}

async fn copies_response(
    state: &Arc<stophammer::api::AppState>,
    feed_guid: &str,
) -> Vec<serde_json::Value> {
    let resp = stophammer::api::build_router(Arc::clone(state))
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/feeds/{feed_guid}/copies"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert_eq!(resp.status(), 200, "the copies route must answer 200");
    body_json(resp).await["data"]
        .as_array()
        .expect("data array")
        .clone()
}

// ---------------------------------------------------------------------------
// A copy with two items gives the two durations, the two enclosure URLs,
// the two dates and the channel author, in the sequence of item_guids.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn copy_with_two_items_gives_the_item_facts_in_sequence() {
    let crawl_token = "adr0058-copy-item-facts-001-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "test-guid-copy-item-facts-001";

    let source_feed = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Source Feed",
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [],
        "tracks": [
            { "track_guid": "item-guid-1", "title": "Item 1", "explicit": false },
            { "track_guid": "item-guid-2", "title": "Item 2", "explicit": false }
        ]
    });
    accept_source(
        &state,
        "https://source.example.com/feed.xml",
        crawl_token,
        "abc123",
        &source_feed,
    )
    .await;

    let mirror_feed = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Mirror Feed",
        "author_name": "Mirror Author",
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [],
        "tracks": [
            {
                "track_guid": "item-guid-1",
                "title": "Mirror Item 1",
                "duration_secs": 185,
                "enclosure_url": "https://mirror.example.com/track-one.mp3",
                "pub_date": 1_710_200_000,
                "explicit": false
            },
            {
                "track_guid": "item-guid-2",
                "title": "Mirror Item 2",
                "duration_secs": 240,
                "enclosure_url": "https://mirror.example.com/track-two.mp3",
                "pub_date": 1_710_300_000,
                "explicit": false
            }
        ]
    });

    submit_mirror(
        &state,
        "https://mirror.example.com/feed.xml",
        crawl_token,
        "def456",
        &mirror_feed,
    )
    .await;

    let row = copy_row(&db, feed_guid, "https://mirror.example.com/feed.xml");
    assert_eq!(row.item_guids, vec!["item-guid-1", "item-guid-2"]);
    assert_eq!(row.item_durations, Some(vec![Some(185), Some(240)]));
    assert_eq!(
        row.item_enclosure_urls,
        Some(vec![
            Some("https://mirror.example.com/track-one.mp3".to_string()),
            Some("https://mirror.example.com/track-two.mp3".to_string())
        ])
    );
    assert_eq!(
        row.item_pub_dates,
        Some(vec![Some(1_710_200_000), Some(1_710_300_000)])
    );
    assert_eq!(row.author, Some("Mirror Author".to_string()));

    let rows = copies_response(&state, feed_guid).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["item_durations"], serde_json::json!([185, 240]));
    assert_eq!(
        rows[0]["item_enclosure_urls"],
        serde_json::json!([
            "https://mirror.example.com/track-one.mp3",
            "https://mirror.example.com/track-two.mp3"
        ])
    );
    assert_eq!(
        rows[0]["item_pub_dates"],
        serde_json::json!([1_710_200_000, 1_710_300_000])
    );
    assert_eq!(rows[0]["author"], "Mirror Author");
}

// ---------------------------------------------------------------------------
// An item with no duration gives null in its position.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_item_with_no_duration_gives_null_in_its_position() {
    let crawl_token = "adr0058-copy-item-facts-002-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "test-guid-copy-item-facts-002";

    let source_feed = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Source Feed",
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [],
        "tracks": [
            { "track_guid": "item-guid-1", "title": "Item 1", "explicit": false },
            { "track_guid": "item-guid-2", "title": "Item 2", "explicit": false }
        ]
    });
    accept_source(
        &state,
        "https://source.example.com/feed.xml",
        crawl_token,
        "abc123",
        &source_feed,
    )
    .await;

    let mirror_feed = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Mirror Feed",
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [],
        "tracks": [
            {
                "track_guid": "item-guid-1",
                "title": "Mirror Item 1",
                "duration_secs": 185,
                "explicit": false
            },
            {
                "track_guid": "item-guid-2",
                "title": "Mirror Item 2",
                "explicit": false
            }
        ]
    });

    submit_mirror(
        &state,
        "https://mirror.example.com/feed.xml",
        crawl_token,
        "def456",
        &mirror_feed,
    )
    .await;

    let row = copy_row(&db, feed_guid, "https://mirror.example.com/feed.xml");
    assert_eq!(row.item_durations, Some(vec![Some(185), None]));
}

// ---------------------------------------------------------------------------
// A javascript: enclosure URL gives null in its position, and the other
// positions keep their values.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn javascript_enclosure_url_gives_null_and_keeps_the_other_positions() {
    let crawl_token = "adr0058-copy-item-facts-003-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "test-guid-copy-item-facts-003";

    let source_feed = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Source Feed",
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [],
        "tracks": [
            { "track_guid": "item-guid-1", "title": "Item 1", "explicit": false },
            { "track_guid": "item-guid-2", "title": "Item 2", "explicit": false },
            { "track_guid": "item-guid-3", "title": "Item 3", "explicit": false }
        ]
    });
    accept_source(
        &state,
        "https://source.example.com/feed.xml",
        crawl_token,
        "abc123",
        &source_feed,
    )
    .await;

    let mirror_url = "https://mirror.example.com/feed.xml";
    let mirror_feed = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Mirror Feed",
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [],
        "tracks": [
            {
                "track_guid": "item-guid-1",
                "title": "Mirror Item 1",
                "enclosure_url": "https://mirror.example.com/track-one.mp3",
                "explicit": false
            },
            {
                "track_guid": "item-guid-2",
                "title": "Mirror Item 2",
                "enclosure_url": "javascript:alert(1)",
                "explicit": false
            },
            {
                "track_guid": "item-guid-3",
                "title": "Mirror Item 3",
                "enclosure_url": "https://mirror.example.com/track-three.mp3",
                "explicit": false
            }
        ]
    });

    submit_mirror(&state, mirror_url, crawl_token, "def456", &mirror_feed).await;

    // The database keeps the raw value as-is.
    let row = copy_row(&db, feed_guid, mirror_url);
    assert_eq!(
        row.item_enclosure_urls,
        Some(vec![
            Some("https://mirror.example.com/track-one.mp3".to_string()),
            Some("javascript:alert(1)".to_string()),
            Some("https://mirror.example.com/track-three.mp3".to_string())
        ])
    );

    // The response gives null only at the refused position.
    let rows = copies_response(&state, feed_guid).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        rows[0]["item_enclosure_urls"],
        serde_json::json!([
            "https://mirror.example.com/track-one.mp3",
            null,
            "https://mirror.example.com/track-three.mp3"
        ])
    );
}

// ---------------------------------------------------------------------------
// A second submission with only a new enclosure URL signs one
// FeedCopyObserved event, and does not change summary_digest.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_enclosure_url_change_signs_one_event_and_keeps_the_digest() {
    let crawl_token = "adr0058-copy-item-facts-004-token";
    let db = common::test_db_arc();
    let feed_guid = "test-guid-copy-item-facts-004";
    let mirror_url = "https://mirror.example.com/feed.xml";
    let copy = copy_feed_with_item_facts(
        feed_guid,
        Some("Mirror Author"),
        Some(185),
        Some("https://mirror.example.com/track-a.mp3"),
        Some(1_710_200_000),
    );
    let state = source_and_copy(&db, crawl_token, feed_guid, mirror_url, &copy).await;

    let events_before = copy_observed_count(&db);
    let digest_before = copy_row(&db, feed_guid, mirror_url).summary_digest;

    let changed = copy_feed_with_item_facts(
        feed_guid,
        Some("Mirror Author"),
        Some(185),
        Some("https://mirror.example.com/track-a-renamed.mp3"),
        Some(1_710_200_000),
    );
    submit_mirror(&state, mirror_url, crawl_token, "copy-hash-2", &changed).await;

    assert_eq!(
        copy_observed_count(&db),
        events_before + 1,
        "a changed enclosure URL must sign exactly one FeedCopyObserved event"
    );
    assert_eq!(
        copy_row(&db, feed_guid, mirror_url).summary_digest,
        digest_before,
        "a changed enclosure URL must keep the summary digest"
    );
    assert_eq!(
        copy_row(&db, feed_guid, mirror_url).item_enclosure_urls,
        Some(vec![Some(
            "https://mirror.example.com/track-a-renamed.mp3".to_string()
        )])
    );
}

// ---------------------------------------------------------------------------
// A resolved copy that changes only an enclosure URL stays resolved.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_keep_source_resolution_holds_after_an_enclosure_url_change() {
    let crawl_token = "adr0058-copy-item-facts-005-token";
    let db = common::test_db_arc();
    let feed_guid = "test-guid-copy-item-facts-005";
    let mirror_url = "https://mirror.example.com/feed.xml";
    let copy = copy_feed_with_item_facts(
        feed_guid,
        Some("Mirror Author"),
        Some(185),
        Some("https://mirror.example.com/track-a.mp3"),
        Some(1_710_200_000),
    );
    let state = source_and_copy(&db, crawl_token, feed_guid, mirror_url, &copy).await;

    let digest = copy_row(&db, feed_guid, mirror_url).summary_digest;
    {
        let conn = db.lock().expect("lock db");
        stophammer::db::set_feed_copy_resolution(
            &conn,
            feed_guid,
            mirror_url,
            "keep_source",
            "operator kept the source",
            stophammer::db::unix_now(),
            &digest,
            None,
        )
        .expect("set resolution");
    }

    let changed = copy_feed_with_item_facts(
        feed_guid,
        Some("Mirror Author"),
        Some(185),
        Some("https://mirror.example.com/track-a-renamed.mp3"),
        Some(1_710_200_000),
    );
    submit_mirror(&state, mirror_url, crawl_token, "copy-hash-2", &changed).await;

    let rows = copies_response(&state, feed_guid).await;
    assert_eq!(rows.len(), 1, "expected one copy row: {rows:?}");
    assert_eq!(rows[0]["open"], false, "the resolution must hold: {rows:?}");
    assert_eq!(rows[0]["resolution"]["current"], true, "{rows:?}");
    assert_eq!(
        rows[0]["item_enclosure_urls"],
        serde_json::json!(["https://mirror.example.com/track-a-renamed.mp3"])
    );
}

// ---------------------------------------------------------------------------
// A third submission with no change signs no event.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_body_with_the_same_item_facts_signs_no_event() {
    let crawl_token = "adr0058-copy-item-facts-006-token";
    let db = common::test_db_arc();
    let feed_guid = "test-guid-copy-item-facts-006";
    let mirror_url = "https://mirror.example.com/feed.xml";
    let copy = copy_feed_with_item_facts(
        feed_guid,
        Some("Mirror Author"),
        Some(185),
        Some("https://mirror.example.com/track-a.mp3"),
        Some(1_710_200_000),
    );
    let state = source_and_copy(&db, crawl_token, feed_guid, mirror_url, &copy).await;

    let before = copy_observed_count(&db);
    assert!(before >= 1, "the first copy body must sign an event");
    submit_mirror(&state, mirror_url, crawl_token, "copy-hash-2", &copy).await;
    assert_eq!(
        copy_observed_count(&db),
        before,
        "the same item facts must sign no event"
    );
}

// ---------------------------------------------------------------------------
// A row with no item facts gets them at the next submission, with one
// event.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_row_with_no_item_facts_gets_them_at_the_next_submission() {
    let crawl_token = "adr0058-copy-item-facts-007-token";
    let db = common::test_db_arc();
    let feed_guid = "test-guid-copy-item-facts-007";
    let mirror_url = "https://mirror.example.com/feed.xml";
    let copy = copy_feed_with_item_facts(
        feed_guid,
        Some("Mirror Author"),
        Some(185),
        Some("https://mirror.example.com/track-a.mp3"),
        Some(1_710_200_000),
    );
    let state = source_and_copy(&db, crawl_token, feed_guid, mirror_url, &copy).await;

    // The row as a node wrote it before ADR 0058 section 1e.
    {
        let conn = db.lock().expect("lock db");
        conn.execute(
            "UPDATE feed_copies SET item_durations = NULL, item_enclosure_urls = NULL, \
             item_pub_dates = NULL, author = NULL WHERE url = ?1",
            [mirror_url],
        )
        .expect("clear the new columns");
    }
    assert_eq!(copy_row(&db, feed_guid, mirror_url).item_durations, None);

    let before = copy_observed_count(&db);
    submit_mirror(&state, mirror_url, crawl_token, "copy-hash-2", &copy).await;

    assert_eq!(
        copy_observed_count(&db),
        before + 1,
        "the backfill must sign exactly one event"
    );
    let row = copy_row(&db, feed_guid, mirror_url);
    assert_eq!(row.item_durations, Some(vec![Some(185)]));
    assert_eq!(
        row.item_enclosure_urls,
        Some(vec![Some(
            "https://mirror.example.com/track-a.mp3".to_string()
        )])
    );
    assert_eq!(row.item_pub_dates, Some(vec![Some(1_710_200_000)]));
    assert_eq!(row.author, Some("Mirror Author".to_string()));
}

// ---------------------------------------------------------------------------
// An old event with none of the four fields applies, and gives null in
// each.
// ---------------------------------------------------------------------------

#[test]
fn an_old_payload_with_none_of_the_four_fields_deserializes() {
    let payload: stophammer::event::FeedCopyObservedPayload =
        serde_json::from_value(serde_json::json!({
            "feed_guid": "g",
            "url": "https://mirror.example.com/feed.xml",
            "first_seen": 1,
            "title": "Mirror Feed",
            "item_guids": ["item-guid-1"],
            "feed_recipients": [],
            "track_recipients": {},
            "summary_digest": "d"
        }))
        .expect("an old payload must deserialize");
    assert_eq!(payload.item_durations, None);
    assert_eq!(payload.item_enclosure_urls, None);
    assert_eq!(payload.item_pub_dates, None);
    assert_eq!(payload.author, None);
}

#[tokio::test]
async fn an_old_event_applies_on_a_replica_with_null_item_facts() {
    let crawl_token = "adr0058-copy-item-facts-008-token";
    let db_a = common::test_db_arc();
    let feed_guid = "test-guid-copy-item-facts-008";
    let mirror_url = "https://mirror.example.com/feed.xml";
    let copy = copy_feed_with_item_facts(
        feed_guid,
        Some("Mirror Author"),
        Some(185),
        Some("https://mirror.example.com/track-a.mp3"),
        Some(1_710_200_000),
    );
    source_and_copy(&db_a, crawl_token, feed_guid, mirror_url, &copy).await;

    let mut events = {
        let conn = db_a.lock().expect("lock db_a");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events")
    };
    // Make each FeedCopyObserved event look as a node signed it before
    // section 1e: with none of the four fields.
    for ev in &mut events {
        if let stophammer::event::EventPayload::FeedCopyObserved(p) = &mut ev.payload {
            p.item_durations = None;
            p.item_enclosure_urls = None;
            p.item_pub_dates = None;
            p.author = None;
            ev.payload_json = serde_json::to_string(p).expect("serialize payload");
            assert!(
                !ev.payload_json.contains("item_durations")
                    && !ev.payload_json.contains("item_enclosure_urls")
                    && !ev.payload_json.contains("item_pub_dates")
                    && !ev.payload_json.contains("\"author\""),
                "the old payload must have none of the four fields: {}",
                ev.payload_json
            );
        }
    }

    let db_b = common::test_db_arc();
    let pool_b = common::wrap_pool(Arc::clone(&db_b));
    for ev in &events {
        let result = stophammer::apply::apply_single_event(&pool_b, ev);
        assert!(
            result.is_ok(),
            "event {:?} must apply: {result:?}",
            ev.event_type
        );
    }

    let row = copy_row(&db_b, feed_guid, mirror_url);
    assert_eq!(
        row.item_durations, None,
        "an old event must leave item_durations null"
    );
    assert_eq!(
        row.item_enclosure_urls, None,
        "an old event must leave item_enclosure_urls null"
    );
    assert_eq!(
        row.item_pub_dates, None,
        "an old event must leave item_pub_dates null"
    );
    assert_eq!(row.author, None, "an old event must leave author null");
}

// ---------------------------------------------------------------------------
// A replica that applies the events gives the same four fields.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_replica_gives_the_same_four_item_fact_fields() {
    let crawl_token = "adr0058-copy-item-facts-009-token";
    let db_a = common::test_db_arc();
    let feed_guid = "test-guid-copy-item-facts-009";
    let mirror_url = "https://mirror.example.com/feed.xml";
    let copy = copy_feed_with_item_facts(
        feed_guid,
        Some("Mirror Author"),
        Some(185),
        Some("https://mirror.example.com/track-a.mp3"),
        Some(1_710_200_000),
    );
    let state_a = source_and_copy(&db_a, crawl_token, feed_guid, mirror_url, &copy).await;

    let events = {
        let conn = db_a.lock().expect("lock db_a");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events")
    };
    let db_b = common::test_db_arc();
    let pool_b = common::wrap_pool(Arc::clone(&db_b));
    for ev in &events {
        let result = stophammer::apply::apply_single_event(&pool_b, ev);
        assert!(
            result.is_ok(),
            "event {:?} must apply: {result:?}",
            ev.event_type
        );
    }
    let state_b = test_app_state(Arc::clone(&db_b), "unused-b-token");

    let rows_a = copies_response(&state_a, feed_guid).await;
    let rows_b = copies_response(&state_b, feed_guid).await;
    assert_eq!(rows_a.len(), 1, "{rows_a:?}");
    assert_eq!(rows_b.len(), 1, "{rows_b:?}");
    for field in [
        "item_durations",
        "item_enclosure_urls",
        "item_pub_dates",
        "author",
    ] {
        assert_eq!(
            rows_a[0][field], rows_b[0][field],
            "the replica must give the same {field}"
        );
    }
    assert_eq!(rows_b[0]["item_durations"], serde_json::json!([185]));
    assert_eq!(
        rows_b[0]["item_enclosure_urls"],
        serde_json::json!(["https://mirror.example.com/track-a.mp3"])
    );
    assert_eq!(
        rows_b[0]["item_pub_dates"],
        serde_json::json!([1_710_200_000])
    );
    assert_eq!(rows_b[0]["author"], "Mirror Author");
}
