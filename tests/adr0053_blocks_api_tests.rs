// ADR 0053 task 002: the ingest check and the three admin routes.
//
// A block on a GUID or a URL rejects a matching submission at ingest, after
// the crawl-token check and before the verifier chain. `POST`, `GET` and
// `DELETE /v1/blocks` let an operator create, list and remove a block, each
// one needing `X-Admin-Token`.
//
// This mirrors the fixture style of tests/adr0051_source_url_tests.rs: a
// minimal verifier chain (content_hash only) so a synthetic feed ingests
// without needing the full default chain.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "adr0053-test-admin-token";

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0053-blocks-signer"));
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

async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    admin_token: Option<&str>,
    body: Option<&serde_json::Value>,
) -> (http::StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = admin_token {
        builder = builder.header("X-Admin-Token", token);
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
    send(app, "POST", "/ingest/feed", None, Some(payload)).await
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

async fn create_block(
    db: &Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
    kind: &str,
    value: &str,
    reason: &str,
) -> (axum::Router, String) {
    let state = test_app_state(Arc::clone(db), crawl_token);
    let app = stophammer::api::build_router(state);
    let payload = serde_json::json!({ "kind": kind, "value": value, "reason": reason });
    let (status, body) = send(
        app.clone(),
        "POST",
        "/v1/blocks",
        Some(ADMIN_TOKEN),
        Some(&payload),
    )
    .await;
    assert_eq!(status, 201, "block creation must succeed: {body:?}");
    let block_id = body["block_id"]
        .as_str()
        .expect("block_id must be a string")
        .to_string();
    (app, block_id)
}

// ---------------------------------------------------------------------------
// The ingest check (ADR 0053 section 1, task file "Constraints")
// ---------------------------------------------------------------------------

/// A block on a GUID stored in upper case rejects a later ingest of that
/// same GUID in lower case, submitted at a brand new URL. No row is written
/// to `feeds`.
#[tokio::test]
async fn a_block_on_an_uppercase_guid_rejects_a_lowercase_ingest_at_a_new_url() {
    let crawl_token = "adr0053-ingest-token";
    let db = common::test_db_arc();

    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let block_payload = serde_json::json!({
        "kind": "guid",
        "value": "ADR0053-GUID-ONE",
        "reason": "test: blocked guid"
    });
    let (status, _) = send(
        app.clone(),
        "POST",
        "/v1/blocks",
        Some(ADMIN_TOKEN),
        Some(&block_payload),
    )
    .await;
    assert_eq!(status, 201, "the block must be created");

    let feed_guid = "adr0053-guid-one";
    let canonical_url = "https://example.com/adr0053-guid-one-new-url.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0053 Blocked Feed");
    let payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "adr0053-hash-1",
        &feed_data,
    );

    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], false,
        "a blocked guid must not be accepted: {body:?}"
    );
    assert_eq!(
        body["reason"], "blocked",
        "the rejection reason must be 'blocked': {body:?}"
    );
    assert_eq!(
        feeds_row_count(&db, feed_guid),
        0,
        "a blocked ingest must write no row to feeds"
    );
}

/// A block on an exact URL rejects a submission whose `source_url` (not its
/// `canonical_url`) is that URL.
#[tokio::test]
async fn a_block_on_a_url_rejects_an_ingest_whose_source_url_matches() {
    let crawl_token = "adr0053-ingest-token-2";
    let db = common::test_db_arc();

    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let blocked_url = "https://example.com/adr0053-blocked-source.xml";
    let block_payload = serde_json::json!({
        "kind": "url",
        "value": blocked_url,
        "reason": "test: blocked url"
    });
    let (status, _) = send(
        app.clone(),
        "POST",
        "/v1/blocks",
        Some(ADMIN_TOKEN),
        Some(&block_payload),
    )
    .await;
    assert_eq!(status, 201, "the block must be created");

    let feed_guid = "adr0053-url-block-feed";
    let canonical_url = "https://example.com/adr0053-canonical.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0053 URL Blocked Feed");
    // Only source_url matches the blocked value; canonical_url does not.
    let payload = ingest_payload(
        canonical_url,
        blocked_url,
        crawl_token,
        "adr0053-hash-2",
        &feed_data,
    );

    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], false,
        "a submission whose source_url is blocked must be rejected: {body:?}"
    );
    assert_eq!(
        body["reason"], "blocked",
        "the reason must be 'blocked': {body:?}"
    );
}

/// A block on an exact URL rejects a submission whose `canonical_url` (not
/// its `source_url`) is that URL.
#[tokio::test]
async fn a_block_on_a_url_rejects_an_ingest_whose_canonical_url_matches() {
    let crawl_token = "adr0053-ingest-token-3";
    let db = common::test_db_arc();

    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let blocked_url = "https://example.com/adr0053-blocked-canonical.xml";
    let block_payload = serde_json::json!({
        "kind": "url",
        "value": blocked_url,
        "reason": "test: blocked url"
    });
    let (status, _) = send(
        app.clone(),
        "POST",
        "/v1/blocks",
        Some(ADMIN_TOKEN),
        Some(&block_payload),
    )
    .await;
    assert_eq!(status, 201, "the block must be created");

    let feed_guid = "adr0053-canonical-block-feed";
    let source_url = "https://example.com/adr0053-canonical-block-source.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0053 Canonical Blocked Feed");
    // Only canonical_url matches the blocked value; source_url does not.
    let payload = ingest_payload(
        blocked_url,
        source_url,
        crawl_token,
        "adr0053-hash-5",
        &feed_data,
    );

    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], false,
        "a submission whose canonical_url is blocked must be rejected: {body:?}"
    );
    assert_eq!(
        body["reason"], "blocked",
        "the reason must be 'blocked': {body:?}"
    );
}

/// The crawl-token check runs before the block check. A wrong token on a
/// submission that would otherwise be blocked answers the token error, not
/// `blocked`.
#[tokio::test]
async fn a_wrong_crawl_token_answers_the_token_error_before_the_block_check() {
    let right_token = "adr0053-right-token";
    let db = common::test_db_arc();

    let state = test_app_state(Arc::clone(&db), right_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0053-token-order-feed";
    let block_payload = serde_json::json!({
        "kind": "guid",
        "value": feed_guid,
        "reason": "test: token order"
    });
    let (status, _) = send(
        app.clone(),
        "POST",
        "/v1/blocks",
        Some(ADMIN_TOKEN),
        Some(&block_payload),
    )
    .await;
    assert_eq!(status, 201, "the block must be created");

    let canonical_url = "https://example.com/adr0053-token-order.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0053 Token Order Feed");
    let payload = ingest_payload(
        canonical_url,
        canonical_url,
        "adr0053-wrong-token",
        "adr0053-hash-3",
        &feed_data,
    );

    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], false,
        "a wrong token must be rejected: {body:?}"
    );
    assert_ne!(
        body["reason"], "blocked",
        "the wrong-token rejection must not read as 'blocked': {body:?}"
    );
    assert_eq!(
        body["reason"], "[crawl_token] invalid crawl token",
        "the token check must run first and name itself: {body:?}"
    );
}

/// After the block is removed, the same ingest that was rejected is now
/// accepted.
#[tokio::test]
async fn removing_a_block_lets_the_same_ingest_through() {
    let crawl_token = "adr0053-after-delete-token";
    let db = common::test_db_arc();
    let (app, block_id) = create_block(
        &db,
        crawl_token,
        "guid",
        "adr0053-after-delete-feed",
        "test",
    )
    .await;

    let feed_guid = "adr0053-after-delete-feed";
    let canonical_url = "https://example.com/adr0053-after-delete.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0053 After Delete Feed");
    let payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "adr0053-hash-4",
        &feed_data,
    );

    let (_, body) = ingest(app.clone(), &payload).await;
    assert_eq!(
        body["reason"], "blocked",
        "the ingest must be blocked before the delete: {body:?}"
    );

    let (status, _) = send(
        app.clone(),
        "DELETE",
        &format!("/v1/blocks/{block_id}"),
        Some(ADMIN_TOKEN),
        None,
    )
    .await;
    assert_eq!(status, 204, "the delete must succeed");

    let (_, body) = ingest(app, &payload).await;
    assert_eq!(
        body["accepted"], true,
        "the same ingest must be accepted once the block is gone: {body:?}"
    );
}

// ---------------------------------------------------------------------------
// POST /v1/blocks
// ---------------------------------------------------------------------------

/// A second `POST /v1/blocks` for the same kind/value pair answers `409`
/// with the `block_id` of the first row, and only one `feed_blocked` event
/// row exists.
#[tokio::test]
async fn a_repeat_post_of_the_same_pair_answers_409_with_the_existing_block_id() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), "adr0053-post-token");
    let app = stophammer::api::build_router(state);
    let payload = serde_json::json!({
        "kind": "url",
        "value": "https://example.com/adr0053-conflict.xml",
        "reason": "first reason"
    });

    let (status, first) = send(
        app.clone(),
        "POST",
        "/v1/blocks",
        Some(ADMIN_TOKEN),
        Some(&payload),
    )
    .await;
    assert_eq!(
        status, 201,
        "the first post must create the block: {first:?}"
    );
    let block_id = first["block_id"].as_str().expect("block_id").to_string();

    let repeat_payload = serde_json::json!({
        "kind": "url",
        "value": "https://example.com/adr0053-conflict.xml",
        "reason": "a different reason"
    });
    let (status, second) = send(
        app,
        "POST",
        "/v1/blocks",
        Some(ADMIN_TOKEN),
        Some(&repeat_payload),
    )
    .await;
    assert_eq!(status, 409, "a repeat pair must answer 409: {second:?}");
    assert_eq!(
        second["block_id"], block_id,
        "the 409 body must carry the existing block_id: {second:?}"
    );
    assert_eq!(
        events_of_type_count(&db, "feed_blocked"),
        1,
        "only one feed_blocked event row must exist"
    );
}

/// `POST /v1/blocks` without the admin token answers `403`. With the token
/// but an empty (after trim) `reason`, it answers `400`.
#[tokio::test]
async fn post_blocks_requires_the_admin_token_and_a_nonempty_reason() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), "adr0053-post-token-2");
    let app = stophammer::api::build_router(state);

    let payload = serde_json::json!({
        "kind": "guid",
        "value": "adr0053-no-token-feed",
        "reason": "a reason"
    });
    let (status, _) = send(app.clone(), "POST", "/v1/blocks", None, Some(&payload)).await;
    assert_eq!(status, 403, "a missing admin token must answer 403");

    let empty_reason_payload = serde_json::json!({
        "kind": "guid",
        "value": "adr0053-empty-reason-feed",
        "reason": "   "
    });
    let (status, _) = send(
        app,
        "POST",
        "/v1/blocks",
        Some(ADMIN_TOKEN),
        Some(&empty_reason_payload),
    )
    .await;
    assert_eq!(status, 400, "an empty reason after trim must answer 400");
}

// ---------------------------------------------------------------------------
// GET /v1/blocks and DELETE /v1/blocks/{block_id}
// ---------------------------------------------------------------------------

/// `GET /v1/blocks` lists a created row. `DELETE` removes it and answers
/// `204`; a second `DELETE` of the same id answers `404`. Exactly one
/// `feed_unblocked` event row exists after the delete.
#[tokio::test]
async fn get_lists_the_row_and_delete_then_a_second_delete_answers_404() {
    let db = common::test_db_arc();
    let (app, block_id) = create_block(
        &db,
        "adr0053-get-delete-token",
        "guid",
        "adr0053-get-delete-feed",
        "test: get and delete",
    )
    .await;

    let (status, body) = send(app.clone(), "GET", "/v1/blocks", Some(ADMIN_TOKEN), None).await;
    assert_eq!(status, 200);
    let blocks = body["blocks"].as_array().expect("blocks array");
    assert!(
        blocks.iter().any(|b| b["block_id"] == block_id),
        "the listed rows must include the created block: {blocks:?}"
    );

    let (status, _) = send(
        app.clone(),
        "DELETE",
        &format!("/v1/blocks/{block_id}"),
        Some(ADMIN_TOKEN),
        None,
    )
    .await;
    assert_eq!(status, 204, "the first delete must succeed");

    let (status, _) = send(
        app,
        "DELETE",
        &format!("/v1/blocks/{block_id}"),
        Some(ADMIN_TOKEN),
        None,
    )
    .await;
    assert_eq!(
        status, 404,
        "a second delete of the same id must answer 404"
    );

    assert_eq!(
        events_of_type_count(&db, "feed_unblocked"),
        1,
        "exactly one feed_unblocked event row must exist"
    );
}
