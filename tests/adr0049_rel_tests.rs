// ADR 0049 task 003: the node stores the raw `rel` attribute of a
// `podcast:remoteItem`, replicates it in the remote-item events, and reports
// it in `include=remote_items`.
//
// The task 002 fixtures (real feed captures) are not merged into this
// worktree yet, so these tests build inline JSON ingest payloads the way
// `tests/adr0043_release_date_tests.rs` does.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use rusqlite::params;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// HTTP-level helpers (mirrors tests/adr0043_release_date_tests.rs)
// ---------------------------------------------------------------------------

fn test_app_state_with_crawl_token(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0049-rel-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: vec!["crawl_token".to_string()],
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

async fn ingest(app: axum::Router, payload: &serde_json::Value) {
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
    assert!(
        resp.status().is_success(),
        "ingest must succeed, got {}",
        resp.status()
    );
}

async fn get_feed_remote_items(app: axum::Router, feed_guid: &str) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/feeds/{feed_guid}?include=remote_items"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    body_json(resp).await
}

// ---------------------------------------------------------------------------
// Ingest payload builders
// ---------------------------------------------------------------------------

/// The Sir Libre Records label case (ADR 0049 §9): a publisher feed lists an
/// album with `medium="music"` and a non-standard `rel="label"`.
fn sirlibre_label_ingest_payload(feed_guid: &str, crawl_token: &str) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": format!("https://example.com/{feed_guid}.xml"),
        "source_url": format!("https://example.com/{feed_guid}.xml"),
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": format!("hash-{feed_guid}"),
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "Sir Libre Records",
            "raw_medium": "publisher",
            "explicit": false,
            "remote_items": [{
                "position": 0,
                "medium": "music",
                "remote_feed_guid": "sirlibre-album-1",
                "remote_feed_url": "https://example.com/sirlibre-album-1.xml",
                "rel": "label"
            }],
            "tracks": [{
                "track_guid": format!("{feed_guid}-track-01"),
                "title": "Label Roundup",
                "explicit": false
            }]
        }
    })
}

/// A feed-level remote item that carries no `rel` attribute at all.
fn no_rel_ingest_payload(feed_guid: &str, crawl_token: &str) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": format!("https://example.com/{feed_guid}.xml"),
        "source_url": format!("https://example.com/{feed_guid}.xml"),
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": format!("hash-{feed_guid}"),
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "No Rel Publisher",
            "raw_medium": "publisher",
            "explicit": false,
            "remote_items": [{
                "position": 0,
                "medium": "music",
                "remote_feed_guid": "no-rel-album-1",
                "remote_feed_url": "https://example.com/no-rel-album-1.xml"
            }],
            "tracks": [{
                "track_guid": format!("{feed_guid}-track-01"),
                "title": "Untitled Roundup",
                "explicit": false
            }]
        }
    })
}

// ---------------------------------------------------------------------------
// 1. The Sir Libre label case: rel is reported through include=remote_items.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sirlibre_label_rel_is_reported_through_remote_items_include() {
    let crawl_token = "adr0049-rel-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);
    let feed_guid = "sirlibre-label";

    let payload = sirlibre_label_ingest_payload(feed_guid, crawl_token);
    ingest(stophammer::api::build_router(Arc::clone(&state)), &payload).await;

    let body = get_feed_remote_items(stophammer::api::build_router(state), feed_guid).await;
    let remote_items = body["data"]["remote_items"]
        .as_array()
        .expect("remote_items must be an array");
    assert_eq!(remote_items.len(), 1, "expected exactly one remote item");
    assert_eq!(
        remote_items[0]["rel"].as_str(),
        Some("label"),
        "the Sir Libre label remote item must report rel == \"label\""
    );
}

// ---------------------------------------------------------------------------
// 2. A remote item with no rel reports null, not an omitted key.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn remote_item_without_rel_reports_null() {
    let crawl_token = "adr0049-rel-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);
    let feed_guid = "no-rel-publisher";

    let payload = no_rel_ingest_payload(feed_guid, crawl_token);
    ingest(stophammer::api::build_router(Arc::clone(&state)), &payload).await;

    let body = get_feed_remote_items(stophammer::api::build_router(state), feed_guid).await;
    let remote_items = body["data"]["remote_items"]
        .as_array()
        .expect("remote_items must be an array");
    assert_eq!(remote_items.len(), 1, "expected exactly one remote item");
    let item = remote_items[0]
        .as_object()
        .expect("remote item must be an object");
    assert!(
        item.contains_key("rel"),
        "rel must be present in the response, not skipped"
    );
    assert!(
        item["rel"].is_null(),
        "a remote item with no rel must report rel as null, got {:?}",
        item["rel"]
    );
}

// ---------------------------------------------------------------------------
// Helper: insert prerequisite artist + artist_credit + feed rows for tests
// that apply events directly (mirrors tests/apply_tests.rs).
// ---------------------------------------------------------------------------

fn insert_artist(conn: &rusqlite::Connection, artist_id: &str, name: &str, now: i64) {
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![artist_id, name, name.to_lowercase(), now, now],
    )
    .expect("insert artist");
}

fn insert_artist_credit(
    conn: &rusqlite::Connection,
    artist_id: &str,
    display_name: &str,
    now: i64,
) -> i64 {
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        params![display_name, now],
    )
    .expect("insert artist_credit");
    let credit_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![credit_id, artist_id, 0, display_name, ""],
    )
    .expect("insert artist_credit_name");

    credit_id
}

fn insert_feed(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    feed_url: &str,
    title: &str,
    credit_id: i64,
    now: i64,
) {
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            feed_guid,
            feed_url,
            title,
            title.to_lowercase(),
            credit_id,
            0,
            0,
            now,
            now
        ],
    )
    .expect("insert feed");
}

fn stored_feed_remote_item_rel(
    db: &Arc<Mutex<rusqlite::Connection>>,
    feed_guid: &str,
    position: i64,
) -> Option<String> {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT rel FROM feed_remote_items_raw WHERE feed_guid = ?1 AND position = ?2",
        params![feed_guid, position],
        |r| r.get(0),
    )
    .expect("remote item row should exist")
}

// ---------------------------------------------------------------------------
// 3. A FeedRemoteItemsReplaced payload with no rel key decodes and applies.
// ---------------------------------------------------------------------------

#[test]
fn feed_remote_items_replaced_payload_without_rel_key_decodes_and_applies() {
    use stophammer::event::{Event, EventPayload, EventType, FeedRemoteItemsReplacedPayload};

    let db = common::test_db_arc();
    let pool = common::wrap_pool(Arc::clone(&db));
    let now = common::now();

    {
        let conn = db.lock().expect("lock");
        insert_artist(&conn, "artist-no-rel-key", "Artist", now);
        let credit_id = insert_artist_credit(&conn, "artist-no-rel-key", "Artist", now);
        insert_feed(
            &conn,
            "feed-no-rel-key",
            "https://example.com/no-rel-key.xml",
            "No Rel Key Feed",
            credit_id,
            now,
        );
    }

    // A payload from a node that predates this field: the remote item has no
    // "rel" key at all, not even a null one.
    let payload_json = r#"{
        "feed_guid": "feed-no-rel-key",
        "remote_items": [{
            "id": null,
            "feed_guid": "feed-no-rel-key",
            "position": 0,
            "medium": "music",
            "remote_feed_guid": "legacy-remote-guid",
            "remote_feed_url": null,
            "source": "podcast_remote_item"
        }]
    }"#;
    let payload_inner: FeedRemoteItemsReplacedPayload =
        serde_json::from_str(payload_json).expect("payload without a rel key must decode");
    assert_eq!(
        payload_inner.remote_items[0].rel, None,
        "a remote item with no rel key must decode to None"
    );

    let ev = Event {
        event_id: "evt-no-rel-key".into(),
        event_type: EventType::FeedRemoteItemsReplaced,
        payload: EventPayload::FeedRemoteItemsReplaced(payload_inner),
        subject_guid: "feed-no-rel-key".into(),
        signed_by: "deadbeef".into(),
        signature: "cafebabe".into(),
        seq: 1,
        created_at: now,
        warnings: vec![],
        payload_json: payload_json.to_string(),
    };

    let result = stophammer::apply::apply_single_event(&pool, &ev);
    assert!(
        result.is_ok(),
        "apply_single_event should succeed: {result:?}"
    );

    assert_eq!(
        stored_feed_remote_item_rel(&db, "feed-no-rel-key", 0),
        None,
        "a remote item with no rel key must store rel as null"
    );
}

// ---------------------------------------------------------------------------
// 4. A FeedRemoteItemsReplaced event with rel applies identically on a
// second database, as it does during community-node replication.
// ---------------------------------------------------------------------------

#[test]
fn feed_remote_items_replaced_event_with_rel_applies_identically_on_second_database() {
    use stophammer::event::{Event, EventPayload, EventType, FeedRemoteItemsReplacedPayload};
    use stophammer::model::FeedRemoteItemRaw;

    let now = common::now();

    let db1 = common::test_db_arc();
    let db2 = common::test_db_arc();
    let pool1 = common::wrap_pool(Arc::clone(&db1));
    let pool2 = common::wrap_pool(Arc::clone(&db2));

    for db in [&db1, &db2] {
        let conn = db.lock().expect("lock");
        insert_artist(&conn, "artist-rel-replicated", "Artist", now);
        let credit_id = insert_artist_credit(&conn, "artist-rel-replicated", "Artist", now);
        insert_feed(
            &conn,
            "feed-rel-replicated",
            "https://example.com/rel-replicated.xml",
            "Replicated Rel Feed",
            credit_id,
            now,
        );
    }

    let payload_inner = FeedRemoteItemsReplacedPayload {
        feed_guid: "feed-rel-replicated".into(),
        remote_items: vec![FeedRemoteItemRaw {
            id: None,
            feed_guid: "feed-rel-replicated".into(),
            position: 0,
            medium: Some("music".into()),
            remote_feed_guid: "sirlibre-album-1".into(),
            remote_feed_url: Some("https://example.com/sirlibre-album-1.xml".into()),
            rel: Some("label".into()),
            source: "podcast_remote_item".into(),
        }],
    };
    let payload_json = serde_json::to_string(&payload_inner).expect("serialize payload");
    let ev = Event {
        event_id: "evt-rel-replicated".into(),
        event_type: EventType::FeedRemoteItemsReplaced,
        payload: EventPayload::FeedRemoteItemsReplaced(payload_inner),
        subject_guid: "feed-rel-replicated".into(),
        signed_by: "deadbeef".into(),
        signature: "cafebabe".into(),
        seq: 1,
        created_at: now,
        warnings: vec![],
        payload_json,
    };

    let result1 = stophammer::apply::apply_single_event(&pool1, &ev);
    assert!(
        result1.is_ok(),
        "apply on the first database should succeed: {result1:?}"
    );
    let result2 = stophammer::apply::apply_single_event(&pool2, &ev);
    assert!(
        result2.is_ok(),
        "apply on the second database should succeed: {result2:?}"
    );

    assert_eq!(
        stored_feed_remote_item_rel(&db1, "feed-rel-replicated", 0).as_deref(),
        Some("label"),
        "the first database must store the declared rel"
    );
    assert_eq!(
        stored_feed_remote_item_rel(&db2, "feed-rel-replicated", 0).as_deref(),
        Some("label"),
        "the second database must store the same rel as the first"
    );
}
