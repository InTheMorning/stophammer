// ADR 0053 task 003: retirement blocks.
//
// `DELETE /v1/feeds/{guid}` blocks the feed GUID and its stored URL in the
// same transaction as the retirement, with one signed `FeedBlocked` event
// for each new block. `?block=false` retires with no block, and only the
// admin token may ask for that.
//
// This mirrors the fixture style of tests/adr0053_blocks_api_tests.rs (an
// ingest-based harness with a minimal `content_hash`-only verifier chain).
// ADR 0056 removed the bearer-token path, so this file only exercises the
// admin token.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "adr0053-retire-test-admin-token";

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0053-retirement-signer"));
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
        admin_token: ADMIN_TOKEN.to_string(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

/// A minimal synthetic music feed with one track, for a test that does not
/// need a real-feed fixture.
fn synthetic_feed_data(feed_guid: &str, title: &str) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "tracks": [{
            "track_guid": format!("{feed_guid}-track-01"),
            "title": "Track One",
            "explicit": false
        }]
    })
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

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    if bytes.is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_slice(&bytes).expect("parse json")
}

/// Sends a request carrying zero or more raw headers.
async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: Option<&serde_json::Value>,
) -> (http::StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = if let Some(body) = body {
        builder
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(body).expect("serialize")))
            .expect("build request")
    } else {
        builder.body(Body::empty()).expect("build request")
    };
    let resp = app.oneshot(request).await.expect("send request");
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

async fn ingest(
    app: axum::Router,
    payload: &serde_json::Value,
) -> (http::StatusCode, serde_json::Value) {
    send(app, "POST", "/ingest/feed", &[], Some(payload)).await
}

fn feeds_row_count(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT COUNT(*) FROM feeds WHERE feed_guid = ?1",
        [feed_guid],
        |row| row.get(0),
    )
    .expect("count feeds rows")
}

fn events_of_type_count(db: &Arc<Mutex<rusqlite::Connection>>, event_type: &str) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT COUNT(*) FROM events WHERE event_type = ?1",
        [event_type],
        |row| row.get(0),
    )
    .expect("count events rows")
}

fn feed_blocks_count(db: &Arc<Mutex<rusqlite::Connection>>) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row("SELECT COUNT(*) FROM feed_blocks", [], |row| row.get(0))
        .expect("count feed_blocks rows")
}

fn feed_block_reasons(db: &Arc<Mutex<rusqlite::Connection>>) -> Vec<String> {
    let conn = db.lock().expect("lock db");
    let mut stmt = conn
        .prepare("SELECT reason FROM feed_blocks ORDER BY kind")
        .expect("prepare");
    stmt.query_map([], |row| row.get::<_, String>(0))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect")
}

// ---------------------------------------------------------------------------
// Acceptance criteria (task file, "Acceptance Criteria")
// ---------------------------------------------------------------------------

/// An admin retire with no query blocks the GUID and the URL in the same
/// transaction, and a later ingest of the same feed is then blocked.
#[tokio::test]
async fn admin_retire_with_no_query_blocks_guid_and_url() {
    let crawl_token = "adr0053-retire-token-1";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0053-retire-feed-1";
    let canonical_url = "https://example.com/adr0053-retire-feed-1.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0053 Retire Feed One");
    let payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "adr0053-retire-hash-1",
        &feed_data,
    );

    let (status, body) = ingest(app.clone(), &payload).await;
    assert_eq!(status, 200, "the feed must ingest first: {body:?}");
    assert_eq!(body["accepted"], true, "the first ingest must be accepted");
    assert_eq!(feeds_row_count(&db, feed_guid), 1, "the feed must exist");

    let (status, _) = send(
        app.clone(),
        "DELETE",
        &format!("/v1/feeds/{feed_guid}"),
        &[("X-Admin-Token", ADMIN_TOKEN)],
        None,
    )
    .await;
    assert_eq!(status, 204, "an admin retire must succeed");

    assert_eq!(
        feeds_row_count(&db, feed_guid),
        0,
        "the feed must be gone after retirement"
    );
    assert_eq!(
        feed_blocks_count(&db),
        2,
        "a guid block and a url block must both exist"
    );
    assert_eq!(
        feed_block_reasons(&db),
        vec!["retired".to_string(), "retired".to_string()],
        "each block written by a retirement carries the reason 'retired'"
    );
    assert_eq!(
        events_of_type_count(&db, "feed_retired"),
        1,
        "exactly one feed_retired event must exist"
    );
    assert_eq!(
        events_of_type_count(&db, "feed_blocked"),
        2,
        "exactly one feed_blocked event per new block must exist"
    );

    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], false,
        "the same feed must be blocked after retirement: {body:?}"
    );
    assert_eq!(
        body["reason"], "blocked",
        "the rejection reason must be 'blocked': {body:?}"
    );
}

/// An admin retire with `?block=false` writes no block, and a later ingest
/// of the same feed is then accepted.
#[tokio::test]
async fn admin_retire_with_block_false_writes_no_block() {
    let crawl_token = "adr0053-retire-token-2";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0053-retire-feed-2";
    let canonical_url = "https://example.com/adr0053-retire-feed-2.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0053 Retire Feed Two");
    let payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "adr0053-retire-hash-2",
        &feed_data,
    );

    let (status, body) = ingest(app.clone(), &payload).await;
    assert_eq!(status, 200, "the feed must ingest first: {body:?}");
    assert_eq!(body["accepted"], true, "the first ingest must be accepted");

    let (status, _) = send(
        app.clone(),
        "DELETE",
        &format!("/v1/feeds/{feed_guid}?block=false"),
        &[("X-Admin-Token", ADMIN_TOKEN)],
        None,
    )
    .await;
    assert_eq!(status, 204, "an admin retire with block=false must succeed");

    assert_eq!(
        feeds_row_count(&db, feed_guid),
        0,
        "the feed must be gone after retirement"
    );
    assert_eq!(
        feed_blocks_count(&db),
        0,
        "block=false must write no feed_blocks row"
    );
    assert_eq!(
        events_of_type_count(&db, "feed_blocked"),
        0,
        "block=false must sign no feed_blocked event"
    );

    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "the same feed must be accepted again after an unblocked retirement: {body:?}"
    );
}

/// Retiring a feed whose GUID is already blocked still succeeds, and only
/// the URL block is new.
#[tokio::test]
async fn retiring_an_already_guid_blocked_feed_only_adds_the_url_block() {
    let crawl_token = "adr0053-retire-token-4";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0053-retire-feed-4";
    let canonical_url = "https://example.com/adr0053-retire-feed-4.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0053 Retire Feed Four");
    let payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "adr0053-retire-hash-4",
        &feed_data,
    );

    let (status, body) = ingest(app.clone(), &payload).await;
    assert_eq!(status, 200, "the feed must ingest first: {body:?}");
    assert_eq!(body["accepted"], true, "the first ingest must be accepted");

    // An operator blocks the GUID on its own, ahead of any retirement. A
    // block removes nothing by itself (ADR 0053 Invariants), so the feed
    // stays.
    let block_payload = serde_json::json!({
        "kind": "guid",
        "value": feed_guid,
        "reason": "pre-existing"
    });
    let (status, body) = send(
        app.clone(),
        "POST",
        "/v1/blocks",
        &[("X-Admin-Token", ADMIN_TOKEN)],
        Some(&block_payload),
    )
    .await;
    assert_eq!(
        status, 201,
        "the pre-existing block must be created: {body:?}"
    );
    assert_eq!(
        feed_blocks_count(&db),
        1,
        "only the guid block exists so far"
    );
    assert_eq!(events_of_type_count(&db, "feed_blocked"), 1);

    let (status, _) = send(
        app.clone(),
        "DELETE",
        &format!("/v1/feeds/{feed_guid}"),
        &[("X-Admin-Token", ADMIN_TOKEN)],
        None,
    )
    .await;
    assert_eq!(
        status, 204,
        "retiring a feed whose guid is already blocked must still succeed"
    );

    assert_eq!(feeds_row_count(&db, feed_guid), 0, "the feed must be gone");
    assert_eq!(
        feed_blocks_count(&db),
        2,
        "the guid block stays and the url block is added"
    );
    assert_eq!(
        events_of_type_count(&db, "feed_blocked"),
        2,
        "only the url block signs a new feed_blocked event; the existing \
         guid pair writes nothing"
    );
    assert_eq!(events_of_type_count(&db, "feed_retired"), 1);
}
