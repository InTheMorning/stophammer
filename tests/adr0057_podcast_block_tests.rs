// ADR 0057 task 002: the node applies podcast:block
//
// A submission from the source URL with a `podcast:block` that applies to this
// index retires the record (update case) or writes nothing (new feed case). The
// answer gives the reason `source_blocked`.
//
// This mirrors the fixture style of tests/adr0051_source_url_tests.rs: a minimal
// verifier chain (content_hash only) so a synthetic feed ingests without needing
// the full default chain.

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
    let signer = Arc::new(common::temp_signer("test-adr0057-block-signer"));
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
        skip_ssrf_validation: true,
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

fn synthetic_feed_data(
    feed_guid: &str,
    title: &str,
    blocks: Option<Vec<serde_json::Value>>,
) -> serde_json::Value {
    let mut data = serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "tracks": [{
            "track_guid": format!("{feed_guid}-track-01"),
            "title": "Track One",
            "explicit": false
        }]
    });
    if let Some(blocks_vec) = blocks {
        data["blocks"] = serde_json::Value::Array(blocks_vec);
    }
    data
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

async fn ingest(app: axum::Router, payload: &serde_json::Value) -> (u16, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri("/ingest/feed")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(payload).unwrap()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let body = body_json(res).await;
    (status, body)
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

// Unit tests for the source_blocks_this_index rule function
#[test]
fn unit_unbounded_yes_blocks_the_feed() {
    let blocks = vec![stophammer::ingest::IngestBlockTag {
        id: None,
        value: "yes".to_string(),
    }];
    assert!(
        stophammer::ingest::source_blocks_this_index(&blocks),
        "ADR 0057 §2 step 3: unbounded 'yes' must block the feed"
    );
}

#[test]
fn unit_musicindex_yes_blocks_the_feed() {
    let blocks = vec![stophammer::ingest::IngestBlockTag {
        id: Some("musicindex".to_string()),
        value: "yes".to_string(),
    }];
    assert!(
        stophammer::ingest::source_blocks_this_index(&blocks),
        "ADR 0057 §2 step 2: id='musicindex' with 'yes' must block the feed"
    );
}

#[test]
fn unit_musicindex_yes_case_insensitive_blocks() {
    let blocks = vec![stophammer::ingest::IngestBlockTag {
        id: Some("MusicIndex".to_string()),
        value: " YES ".to_string(),
    }];
    assert!(
        stophammer::ingest::source_blocks_this_index(&blocks),
        "ADR 0057 §2: comparison must be case-insensitive after trim"
    );
}

#[test]
fn unit_musicindex_no_admits_feed() {
    let blocks = vec![stophammer::ingest::IngestBlockTag {
        id: Some("musicindex".to_string()),
        value: "no".to_string(),
    }];
    assert!(
        !stophammer::ingest::source_blocks_this_index(&blocks),
        "ADR 0057 §2 step 1: id='musicindex' with 'no' must not block"
    );
}

#[test]
fn unit_musicindex_no_with_unbounded_yes_later_admits() {
    let blocks = vec![
        stophammer::ingest::IngestBlockTag {
            id: Some("musicindex".to_string()),
            value: "no".to_string(),
        },
        stophammer::ingest::IngestBlockTag {
            id: None,
            value: "yes".to_string(),
        },
    ];
    assert!(
        !stophammer::ingest::source_blocks_this_index(&blocks),
        "ADR 0057 §2 step 1: id='musicindex' with 'no' stops the check"
    );
}

#[test]
fn unit_different_slug_is_ignored() {
    let blocks = vec![stophammer::ingest::IngestBlockTag {
        id: Some("podcastindex".to_string()),
        value: "yes".to_string(),
    }];
    assert!(
        !stophammer::ingest::source_blocks_this_index(&blocks),
        "ADR 0057 §2 step 4: different slug must be ignored"
    );
}

#[test]
fn unit_unbounded_no_is_ignored() {
    let blocks = vec![stophammer::ingest::IngestBlockTag {
        id: None,
        value: "no".to_string(),
    }];
    assert!(
        !stophammer::ingest::source_blocks_this_index(&blocks),
        "ADR 0057 §2 step 4: unbounded 'no' must be ignored"
    );
}

#[test]
fn unit_invalid_value_is_ignored() {
    let blocks = vec![stophammer::ingest::IngestBlockTag {
        id: None,
        value: "maybe".to_string(),
    }];
    assert!(
        !stophammer::ingest::source_blocks_this_index(&blocks),
        "ADR 0057 §2: value other than 'yes' or 'no' must be ignored"
    );
}

#[test]
fn unit_empty_blocks_does_not_block() {
    let blocks: Vec<stophammer::ingest::IngestBlockTag> = vec![];
    assert!(
        !stophammer::ingest::source_blocks_this_index(&blocks),
        "ADR 0057: empty block list must not block"
    );
}

// Integration tests for the ingest handler

/// An unbounded `yes` from the source URL retires the record. A read of the
/// feed answers `404`. The event log holds one `FeedRetired`, and
/// `feed_blocks` holds no row.
#[tokio::test]
async fn update_unbounded_yes_retires_record() {
    let crawl_token = "adr0057-block-token-update-yes";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0057-feed-update-unbounded-yes";
    let canonical_url = "https://example.com/adr0057-feed-update-unbounded-yes.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0057 Unbounded Yes Feed", None);
    let first_payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-001",
        &feed_data,
    );

    // First ingest without blocks to establish the record
    let (status, body) = ingest(app.clone(), &first_payload).await;
    assert_eq!(status, 200, "first ingest must succeed: {body:?}");
    assert_eq!(
        body["accepted"], true,
        "first ingest must be accepted: {body:?}"
    );

    // Second ingest with unbounded 'yes' block
    let blocks = vec![serde_json::json!({
        "id": serde_json::Value::Null,
        "value": "yes"
    })];
    let feed_data_blocked =
        synthetic_feed_data(feed_guid, "ADR 0057 Unbounded Yes Feed", Some(blocks));
    let blocking_payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-002",
        &feed_data_blocked,
    );

    let (status, body) = ingest(app, &blocking_payload).await;
    assert_eq!(status, 200, "blocking ingest must return 200");
    assert_eq!(
        body["accepted"], false,
        "blocking ingest must be rejected: {body:?}"
    );
    assert_eq!(
        body["reason"], "source_blocked",
        "rejection reason must be 'source_blocked': {body:?}"
    );

    assert_eq!(
        feeds_row_count(&db, feed_guid),
        0,
        "ADR 0057 §3: feed must be retired"
    );
    assert_eq!(
        events_of_type_count(&db, "feed_retired"),
        1,
        "ADR 0057 §3: exactly one feed_retired event must exist"
    );
    assert_eq!(
        feed_blocks_count(&db),
        0,
        "ADR 0057 §3: no block row must be written"
    );
}

/// `id="musicindex"` with `no` beside an unbounded `yes` admits the feed.
#[tokio::test]
async fn update_musicindex_no_overrides_unbounded_yes() {
    let crawl_token = "adr0057-block-token-musicindex-no";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0057-feed-musicindex-no";
    let canonical_url = "https://example.com/adr0057-feed-musicindex-no.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0057 MusicIndex No Feed", None);
    let first_payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-001",
        &feed_data,
    );

    let (status, body) = ingest(app.clone(), &first_payload).await;
    assert_eq!(status, 200, "first ingest must succeed: {body:?}");
    assert_eq!(body["accepted"], true, "first ingest must be accepted");

    // Second ingest with musicindex='no' and unbounded 'yes'
    let blocks = vec![
        serde_json::json!({"id": "musicindex", "value": "no"}),
        serde_json::json!({"id": serde_json::Value::Null, "value": "yes"}),
    ];
    let feed_data_with_blocks =
        synthetic_feed_data(feed_guid, "ADR 0057 MusicIndex No Feed", Some(blocks));
    let payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-002",
        &feed_data_with_blocks,
    );

    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "ADR 0057 §2 step 1: id='musicindex' with 'no' must admit the feed: {body:?}"
    );

    assert_eq!(
        feeds_row_count(&db, feed_guid),
        1,
        "feed must still exist after musicindex='no' block"
    );
}

/// `id="podcastindex"` with `yes` admits the feed.
#[tokio::test]
async fn update_podcastindex_yes_does_not_block() {
    let crawl_token = "adr0057-block-token-podcastindex-yes";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0057-feed-podcastindex-yes";
    let canonical_url = "https://example.com/adr0057-feed-podcastindex-yes.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0057 PodcastIndex Feed", None);
    let first_payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-001",
        &feed_data,
    );

    let (status, body) = ingest(app.clone(), &first_payload).await;
    assert_eq!(status, 200);
    assert_eq!(body["accepted"], true);

    // Second ingest with podcastindex='yes' (should not block this index)
    let blocks = vec![serde_json::json!({"id": "podcastindex", "value": "yes"})];
    let feed_data_with_blocks =
        synthetic_feed_data(feed_guid, "ADR 0057 PodcastIndex Feed", Some(blocks));
    let payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-002",
        &feed_data_with_blocks,
    );

    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "ADR 0057 §1: this index does not answer to 'podcastindex': {body:?}"
    );

    assert_eq!(
        feeds_row_count(&db, feed_guid),
        1,
        "feed must not be retired by podcastindex block"
    );
}

/// `id="MusicIndex"` with ` YES ` (case and whitespace insensitive) retires the record.
#[tokio::test]
async fn update_musicindex_case_insensitive_blocks() {
    let crawl_token = "adr0057-block-token-case-insensitive";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0057-feed-case-insensitive";
    let canonical_url = "https://example.com/adr0057-feed-case-insensitive.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0057 Case Insensitive Feed", None);
    let first_payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-001",
        &feed_data,
    );

    let (status, body) = ingest(app.clone(), &first_payload).await;
    assert_eq!(status, 200);
    assert_eq!(body["accepted"], true);

    // Second ingest with case variation
    let blocks = vec![serde_json::json!({"id": "MusicIndex", "value": " YES "})];
    let feed_data_with_blocks =
        synthetic_feed_data(feed_guid, "ADR 0057 Case Insensitive Feed", Some(blocks));
    let payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-002",
        &feed_data_with_blocks,
    );

    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], false,
        "ADR 0057 §2: comparison must be case-insensitive: {body:?}"
    );
    assert_eq!(body["reason"], "source_blocked");
}

/// A `yes` tag in a mirror body changes nothing, and the answer is the ADR
/// 0051 answer for a mirror.
#[tokio::test]
async fn mirror_block_does_not_retire() {
    let crawl_token = "adr0057-block-token-mirror";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0057-feed-mirror";
    let source_url = "https://example.com/source/adr0057-feed-mirror.xml";
    let mirror_url = "https://mirror.example.com/adr0057-feed-mirror.xml";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0057 Mirror Feed", None);

    // First ingest from source URL to establish the record
    let first_payload = ingest_payload(source_url, source_url, crawl_token, "hash-001", &feed_data);
    let (status, body) = ingest(app.clone(), &first_payload).await;
    assert_eq!(status, 200, "first ingest must succeed: {body:?}");
    assert_eq!(body["accepted"], true, "first ingest must be accepted");

    // Second ingest from mirror URL with block tags
    // This should be classified as Mirror (different URL, same GUID)
    // The block rule should not apply to mirrors
    let blocks = vec![serde_json::json!({"id": serde_json::Value::Null, "value": "yes"})];
    let feed_data_with_blocks =
        synthetic_feed_data(feed_guid, "ADR 0057 Mirror Feed", Some(blocks));
    let mirror_payload = ingest_payload(
        mirror_url,
        mirror_url,
        crawl_token,
        "hash-002",
        &feed_data_with_blocks,
    );

    let (status, body) = ingest(app, &mirror_payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["reason"], "source_conflict",
        "ADR 0057 §3: block rule does not apply to mirrors; answer is ADR 0051: {body:?}"
    );

    assert_eq!(
        feeds_row_count(&db, feed_guid),
        1,
        "mirror submission must not retire the feed"
    );
}

/// A new feed with an unbounded `yes` writes no feed, and the answer is
/// `source_blocked` with no event.
#[tokio::test]
async fn new_feed_unbounded_yes_writes_nothing() {
    let crawl_token = "adr0057-block-token-new-feed";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0057-feed-new-blocked";
    let canonical_url = "https://example.com/adr0057-feed-new-blocked.xml";

    // New feed with unbounded 'yes' block (no prior submission)
    let blocks = vec![serde_json::json!({"id": serde_json::Value::Null, "value": "yes"})];
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0057 New Blocked Feed", Some(blocks));
    let payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-new",
        &feed_data,
    );

    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], false,
        "ADR 0057 §3: new feed with block must be rejected: {body:?}"
    );
    assert_eq!(
        body["reason"], "source_blocked",
        "ADR 0057 §3: rejection reason must be 'source_blocked'"
    );
    assert_eq!(
        body["events_emitted"],
        serde_json::json!([]),
        "ADR 0057 §3: no event must be emitted for blocked new feed"
    );

    assert_eq!(
        feeds_row_count(&db, feed_guid),
        0,
        "ADR 0057 §3: no feed must be written"
    );
    assert_eq!(
        events_of_type_count(&db, "feed_retired"),
        0,
        "no FeedRetired event must be signed"
    );
    assert_eq!(feed_blocks_count(&db), 0, "no block row must be written");
}

/// A feed that removes its tag after a retirement is admitted on the next
/// submission, with the same track IDs as before.
#[tokio::test]
async fn feed_removes_block_is_readmitted_with_same_track_ids() {
    let crawl_token = "adr0057-block-token-remove";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0057-feed-remove-block";
    let canonical_url = "https://example.com/adr0057-feed-remove-block.xml";
    let track_guid = format!("{feed_guid}-track-01");

    // First ingest without blocks
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0057 Remove Block Feed", None);
    let first_payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-001",
        &feed_data,
    );

    let (status, body) = ingest(app.clone(), &first_payload).await;
    assert_eq!(status, 200);
    assert_eq!(body["accepted"], true);

    // Get the track GUID from the first submission
    let get_track_req = Request::builder()
        .method("GET")
        .uri(format!("/v1/feeds/{feed_guid}/tracks/{track_guid}").as_str())
        .body(Body::empty())
        .unwrap();
    let get_track_res = app.clone().oneshot(get_track_req).await.unwrap();
    let track_data_before: serde_json::Value = body_json(get_track_res).await;
    let track_href_before = track_data_before
        .get("href")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    // Second ingest with block
    let blocks = vec![serde_json::json!({"id": serde_json::Value::Null, "value": "yes"})];
    let feed_data_blocked =
        synthetic_feed_data(feed_guid, "ADR 0057 Remove Block Feed", Some(blocks));
    let blocking_payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-002",
        &feed_data_blocked,
    );

    let (status, body) = ingest(app.clone(), &blocking_payload).await;
    assert_eq!(status, 200);
    assert_eq!(body["accepted"], false);

    assert_eq!(
        feeds_row_count(&db, feed_guid),
        0,
        "feed must be retired by block"
    );

    // Third ingest without block (publisher removes the tag)
    let feed_data_unblocked = synthetic_feed_data(feed_guid, "ADR 0057 Remove Block Feed", None);
    let unblocking_payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-003",
        &feed_data_unblocked,
    );

    let (status, body) = ingest(app.clone(), &unblocking_payload).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "ADR 0057 §4: feed must be admitted again after block removal: {body:?}"
    );

    assert_eq!(
        feeds_row_count(&db, feed_guid),
        1,
        "feed must be recreated as a new feed"
    );

    // Check that the track has the same GUID
    let get_track_req_2 = Request::builder()
        .method("GET")
        .uri(format!("/v1/feeds/{feed_guid}/tracks/{track_guid}").as_str())
        .body(Body::empty())
        .unwrap();
    let get_track_res_2 = app.oneshot(get_track_req_2).await.unwrap();
    let track_data_after: serde_json::Value = body_json(get_track_res_2).await;
    let track_href_after = track_data_after
        .get("href")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    assert_eq!(
        track_href_before, track_href_after,
        "ADR 0057 §4: track scoped identities must be the same"
    );
}

// Note: The operator block precedence is already tested by existing ADR 0053 tests.
// The ADR 0057 block rule is applied AFTER the operator block check, so if an
// operator block exists, the answer is always "blocked" before ADR 0057 is even checked.
// This is correct per ADR 0057 §3: "The node applies the rule after the check of
// an operator block".

/// The sequence of ADR 0057 §2 reads every tag. `id="musicindex"` with `no`
/// wins also when an unbounded `yes` comes first in the source.
#[test]
fn unit_musicindex_no_after_unbounded_yes_admits() {
    let blocks = vec![
        stophammer::ingest::IngestBlockTag {
            id: None,
            value: "yes".to_string(),
        },
        stophammer::ingest::IngestBlockTag {
            id: Some("musicindex".to_string()),
            value: "no".to_string(),
        },
    ];
    assert!(
        !stophammer::ingest::source_blocks_this_index(&blocks),
        "ADR 0057 §2 step 1: id='musicindex' with 'no' admits the feed in any source order"
    );
}

/// An operator block of ADR 0053 is checked before the rule of ADR 0057, so
/// a blocked GUID answers `blocked`, also when the body has an unbounded `yes`.
#[tokio::test]
async fn operator_block_answers_blocked_before_the_source_block() {
    let crawl_token = "adr0057-operator-block-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0057-feed-operator-block";
    let canonical_url = "https://example.com/adr0057-feed-operator-block.xml";

    let block = serde_json::json!({
        "kind": "guid",
        "value": feed_guid,
        "reason": "test: operator block"
    });
    let resp = app
        .clone()
        .oneshot(
            http::Request::builder()
                .method("POST")
                .uri("/v1/blocks")
                .header("content-type", "application/json")
                .header("X-Admin-Token", "test-admin-token")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&block).expect("json"),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert!(
        resp.status().is_success(),
        "the operator block must be created: {}",
        resp.status()
    );

    let blocks = vec![serde_json::json!({ "id": serde_json::Value::Null, "value": "yes" })];
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0057 Operator Block", Some(blocks));
    let payload = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        "hash-op",
        &feed_data,
    );
    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200, "the ingest must answer: {body:?}");
    assert_eq!(
        body["reason"], "blocked",
        "ADR 0057 invariant: an operator block of ADR 0053 is checked first: {body:?}"
    );
}
