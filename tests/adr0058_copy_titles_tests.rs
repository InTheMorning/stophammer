// ADR 0058 task 006: a copy row gives its item titles and image URL.
//
// Item titles are stored as a JSON array in the order of item GUIDs.
// Each entry is a string or null (when the item has no title).
// The image URL is the channel image URL of the copy, or null.
// A change of the channel title, an item title or the image signs one
// FeedCopyObserved event. The summary digest stays unchanged.
//
// This test file follows the patterns of tests/adr0058_ingest_copy_tests.rs.

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
    let signer = Arc::new(common::temp_signer("test-adr0058-copy-titles-signer"));
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

/// A mirror feed with three items: two with titles and one without.
/// Item 2 has no title (empty string).
fn feed_data_with_three_items(
    feed_guid: &str,
    title: &str,
    image_url: Option<&str>,
    item_1_title: &str,
    item_2_title: &str,
) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "image_url": image_url,
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [],
        "tracks": [
            {
                "track_guid": "item-guid-1",
                "title": item_1_title,
                "explicit": false
            },
            {
                "track_guid": "item-guid-2",
                "title": item_2_title,
                "explicit": false
            },
            {
                "track_guid": "item-guid-3",
                "title": "",
                "explicit": false
            }
        ]
    })
}

#[tokio::test]
async fn mirror_body_with_three_items_gives_row_with_three_item_guids_and_channel_image() {
    let crawl_token = "adr0058-copy-titles-001-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "test-guid-copy-titles-001";

    // Set up: ingest the source feed.
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

    // Act: ingest a mirror at a different URL.
    let mirror_feed = feed_data_with_three_items(
        feed_guid,
        "Mirror Feed",
        Some("https://mirror.example.com/image.jpg"),
        "Mirror Item 1",
        "Mirror Item 2",
    );

    submit_mirror(
        &state,
        "https://mirror.example.com/feed.xml",
        crawl_token,
        "def456",
        &mirror_feed,
    )
    .await;

    // Assert: the copy row has the correct item_guids, item_titles and image_url.
    let conn = db.lock().unwrap();
    let copy_row =
        stophammer::db::get_feed_copy(&conn, feed_guid, "https://mirror.example.com/feed.xml")
            .unwrap()
            .expect("copy row should exist");

    assert_eq!(
        copy_row.item_guids,
        vec!["item-guid-1", "item-guid-2", "item-guid-3"]
    );
    assert_eq!(
        copy_row.item_titles,
        Some(vec![
            Some("Mirror Item 1".to_string()),
            Some("Mirror Item 2".to_string()),
            None
        ])
    );
    assert_eq!(
        copy_row.image_url,
        Some("https://mirror.example.com/image.jpg".to_string())
    );
}

#[tokio::test]
async fn item_title_change_signs_one_feed_copy_observed_event() {
    let crawl_token = "adr0058-copy-titles-002-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "test-guid-copy-titles-002";
    let mirror_url = "https://mirror.example.com/feed.xml";

    // Set up: ingest source and mirror.
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

    // Ingest a mirror with different recipients (so it's a copy, not an alias).
    let mirror_feed = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Mirror Feed",
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
            { "track_guid": "item-guid-1", "title": "Original Title 1", "explicit": false },
            { "track_guid": "item-guid-2", "title": "Original Title 2", "explicit": false }
        ]
    });

    submit_mirror(&state, mirror_url, crawl_token, "def456", &mirror_feed).await;
    let events_before = copy_observed_count(&db);
    let digest_before = copy_row(&db, feed_guid, mirror_url).summary_digest;

    // Act: ingest the mirror again with changed item titles.
    let mirror_feed_updated = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Mirror Feed",
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
            { "track_guid": "item-guid-1", "title": "Updated Title 1", "explicit": false },
            { "track_guid": "item-guid-2", "title": "Updated Title 2", "explicit": false }
        ]
    });

    submit_mirror(
        &state,
        mirror_url,
        crawl_token,
        "def457",
        &mirror_feed_updated,
    )
    .await;

    assert_eq!(
        copy_observed_count(&db),
        events_before + 1,
        "a changed item title must sign exactly one FeedCopyObserved event"
    );
    assert_eq!(
        copy_row(&db, feed_guid, mirror_url).summary_digest,
        digest_before,
        "a changed item title must keep the summary digest"
    );

    // Assert: the row now has the updated titles.
    let conn = db.lock().unwrap();
    let copy_row = stophammer::db::get_feed_copy(&conn, feed_guid, mirror_url)
        .unwrap()
        .expect("copy row should exist");

    assert_eq!(
        copy_row.item_titles,
        Some(vec![
            Some("Updated Title 1".to_string()),
            Some("Updated Title 2".to_string())
        ])
    );
}

#[tokio::test]
async fn javascript_image_gives_null_image_url_in_response() {
    let crawl_token = "adr0058-copy-titles-003-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "test-guid-copy-titles-003";
    let mirror_url = "https://mirror.example.com/feed.xml";

    // Set up: ingest source.
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
        "abc123",
        &source_feed,
    )
    .await;

    // Act: ingest a mirror with javascript: image URL.
    let mirror_feed = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Mirror Feed",
        "image_url": "javascript:alert('xss')",
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [],
        "tracks": [
            { "track_guid": "item-guid-1", "title": "Mirror Item 1", "explicit": false }
        ]
    });

    submit_mirror(&state, mirror_url, crawl_token, "def456", &mirror_feed).await;

    // Assert: the database row has the invalid URL, but the response gives null.
    {
        let conn = db.lock().unwrap();
        let copy_row = stophammer::db::get_feed_copy(&conn, feed_guid, mirror_url)
            .unwrap()
            .expect("copy row should exist");

        // The database stores the invalid URL as-is.
        assert_eq!(
            copy_row.image_url,
            Some("javascript:alert('xss')".to_string())
        );
    }

    // The API response should give null because web_url_or_none filters it out.
    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/v1/feeds/{feed_guid}/copies"))
        .header("content-type", "application/json")
        .body(Body::empty())
        .unwrap();

    let router = stophammer::api::build_router(Arc::clone(&state));
    let res = router.oneshot(get_req).await.expect("send request");
    assert_eq!(res.status(), 200);

    let response = body_json(res).await;
    let data = response["data"].as_array().unwrap();
    assert!(!data.is_empty());
    let copy = &data[0];
    assert_eq!(copy["image_url"], serde_json::Value::Null);
}

// ---------------------------------------------------------------------------
// Helpers for the tests below.
// ---------------------------------------------------------------------------

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

/// A mirror that pays a different recipient than the source, so it is a
/// copy and not an alias.
fn copy_feed(feed_guid: &str, image_url: Option<&str>, item_title: &str) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Mirror Feed",
        "image_url": image_url,
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
            { "track_guid": "item-guid-1", "title": item_title, "explicit": false }
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

// ---------------------------------------------------------------------------
// A keep_source resolution holds after a change of a title or the image.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_keep_source_resolution_holds_after_a_title_or_image_change() {
    let crawl_token = "adr0058-copy-titles-004-token";
    let db = common::test_db_arc();
    let feed_guid = "test-guid-copy-titles-004";
    let mirror_url = "https://mirror.example.com/feed.xml";
    let state = source_and_copy(
        &db,
        crawl_token,
        feed_guid,
        mirror_url,
        &copy_feed(
            feed_guid,
            Some("https://mirror.example.com/a.jpg"),
            "Title A",
        ),
    )
    .await;

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

    let changed = copy_feed(
        feed_guid,
        Some("https://mirror.example.com/b.jpg"),
        "Title B",
    );
    submit_mirror(&state, mirror_url, crawl_token, "copy-hash-2", &changed).await;

    let rows = copies_response(&state, feed_guid).await;
    assert_eq!(rows.len(), 1, "expected one copy row: {rows:?}");
    assert_eq!(rows[0]["open"], false, "the resolution must hold: {rows:?}");
    assert_eq!(rows[0]["resolution"]["current"], true, "{rows:?}");
    assert_eq!(rows[0]["item_titles"], serde_json::json!(["Title B"]));
    assert_eq!(rows[0]["image_url"], "https://mirror.example.com/b.jpg");
}

// ---------------------------------------------------------------------------
// A body with the same summary signs no event.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_body_with_the_same_summary_signs_no_event() {
    let crawl_token = "adr0058-copy-titles-005-token";
    let db = common::test_db_arc();
    let feed_guid = "test-guid-copy-titles-005";
    let mirror_url = "https://mirror.example.com/feed.xml";
    let copy = copy_feed(
        feed_guid,
        Some("https://mirror.example.com/a.jpg"),
        "Title A",
    );
    let state = source_and_copy(&db, crawl_token, feed_guid, mirror_url, &copy).await;

    let before = copy_observed_count(&db);
    assert!(before >= 1, "the first copy body must sign an event");
    submit_mirror(&state, mirror_url, crawl_token, "copy-hash-2", &copy).await;
    assert_eq!(
        copy_observed_count(&db),
        before,
        "the same summary must sign no event"
    );
}

// ---------------------------------------------------------------------------
// A row with no titles gets them at the next submission, with one event.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_row_with_no_titles_gets_them_at_the_next_submission() {
    let crawl_token = "adr0058-copy-titles-006-token";
    let db = common::test_db_arc();
    let feed_guid = "test-guid-copy-titles-006";
    let mirror_url = "https://mirror.example.com/feed.xml";
    let copy = copy_feed(
        feed_guid,
        Some("https://mirror.example.com/a.jpg"),
        "Title A",
    );
    let state = source_and_copy(&db, crawl_token, feed_guid, mirror_url, &copy).await;

    // The row as a node wrote it before ADR 0058 section 1c.
    {
        let conn = db.lock().expect("lock db");
        conn.execute(
            "UPDATE feed_copies SET item_titles = NULL, image_url = NULL WHERE url = ?1",
            [mirror_url],
        )
        .expect("clear the new columns");
    }
    assert_eq!(copy_row(&db, feed_guid, mirror_url).item_titles, None);

    let before = copy_observed_count(&db);
    submit_mirror(&state, mirror_url, crawl_token, "copy-hash-2", &copy).await;

    assert_eq!(
        copy_observed_count(&db),
        before + 1,
        "the backfill must sign exactly one event"
    );
    let row = copy_row(&db, feed_guid, mirror_url);
    assert_eq!(row.item_titles, Some(vec![Some("Title A".to_string())]));
    assert_eq!(
        row.image_url.as_deref(),
        Some("https://mirror.example.com/a.jpg")
    );
}

// ---------------------------------------------------------------------------
// An old event with neither new field applies, and gives null titles.
// ---------------------------------------------------------------------------

#[test]
fn an_old_payload_with_neither_new_field_deserializes() {
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
    assert_eq!(payload.item_titles, None);
    assert_eq!(payload.image_url, None);
}

#[tokio::test]
async fn an_old_event_applies_on_a_replica_with_null_titles() {
    let crawl_token = "adr0058-copy-titles-007-token";
    let db_a = common::test_db_arc();
    let feed_guid = "test-guid-copy-titles-007";
    let mirror_url = "https://mirror.example.com/feed.xml";
    let copy = copy_feed(
        feed_guid,
        Some("https://mirror.example.com/a.jpg"),
        "Title A",
    );
    source_and_copy(&db_a, crawl_token, feed_guid, mirror_url, &copy).await;

    let mut events = {
        let conn = db_a.lock().expect("lock db_a");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events")
    };
    // Make each FeedCopyObserved event look as a node signed it before
    // section 1c: with neither new field.
    for ev in &mut events {
        // Apply reads the signed `payload_json`, so the old form goes there.
        if let stophammer::event::EventPayload::FeedCopyObserved(p) = &mut ev.payload {
            p.item_titles = None;
            p.image_url = None;
            ev.payload_json = serde_json::to_string(p).expect("serialize payload");
            assert!(
                !ev.payload_json.contains("item_titles") && !ev.payload_json.contains("image_url"),
                "the old payload must have neither new field: {}",
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
        row.item_titles, None,
        "an old event must leave item_titles null"
    );
    assert_eq!(
        row.image_url, None,
        "an old event must leave image_url null"
    );
}

// ---------------------------------------------------------------------------
// A community node that applies the new event gives the same three fields.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_replica_gives_the_same_item_guids_titles_and_image() {
    let crawl_token = "adr0058-copy-titles-008-token";
    let db_a = common::test_db_arc();
    let feed_guid = "test-guid-copy-titles-008";
    let mirror_url = "https://mirror.example.com/feed.xml";
    let copy = copy_feed(
        feed_guid,
        Some("https://mirror.example.com/a.jpg"),
        "Title A",
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
    for field in ["item_guids", "item_titles", "image_url"] {
        assert_eq!(
            rows_a[0][field], rows_b[0][field],
            "the replica must give the same {field}"
        );
    }
    assert_eq!(rows_b[0]["item_titles"], serde_json::json!(["Title A"]));
    assert_eq!(rows_b[0]["image_url"], "https://mirror.example.com/a.jpg");
}
