// ADR 0067: a gone source retires its feed.
//
// The node counts a gone answer (`http_status` 404 or 410, no `feed_data`)
// from the stored source URL of a record. A second counted answer 24 hours
// or more after the first retires the record with the reason `source_gone`.
//
// This mirrors the fixture style of tests/adr0057_podcast_block_tests.rs: a
// minimal verifier chain (content_hash only) so a synthetic feed ingests
// without needing the full default chain.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

/// One day, in seconds: the gap ADR 0067 section 3 names.
const ONE_DAY_SECS: i64 = 24 * 60 * 60;

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
    source_gone_hosts: &[&str],
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0067-source-gone-signer"));
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
        source_gone_hosts: source_gone_hosts.iter().map(|h| (*h).to_string()).collect(),
        skip_ssrf_validation: true,
    })
}

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

fn body_ingest_payload(
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

/// A crawler report of a gone source URL: no `feed_data`, per ADR 0067
/// section 1.
fn gone_report_payload(
    canonical_url: &str,
    source_url: &str,
    crawl_token: &str,
    http_status: u16,
    redirects: &[serde_json::Value],
) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": canonical_url,
        "source_url": source_url,
        "crawl_token": crawl_token,
        "http_status": http_status,
        "content_hash": "",
        "redirects": redirects,
        "feed_data": null,
    })
}

fn redirect_hop(url: &str, status: u16) -> serde_json::Value {
    serde_json::json!({ "url": url, "status": status })
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

/// Reads the `payload_json` of the newest `feed_retired` event.
fn last_feed_retired_payload(db: &Arc<Mutex<rusqlite::Connection>>) -> String {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT payload_json FROM events WHERE event_type = 'feed_retired' \
         ORDER BY seq DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .expect("read last feed_retired payload")
}

fn source_gone_answers_row_count(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT COUNT(*) FROM source_gone_answers WHERE feed_guid = ?1",
        [feed_guid],
        |row| row.get(0),
    )
    .expect("count source_gone_answers rows")
}

/// Moves the `first_gone_at` of `feed_guid`'s row back by `seconds_ago`, so a
/// test can simulate the gap ADR 0067 section 3 checks without mocking the
/// clock the handler reads (`db::unix_now`).
fn backdate_first_gone_at(
    db: &Arc<Mutex<rusqlite::Connection>>,
    feed_guid: &str,
    seconds_ago: i64,
) {
    let conn = db.lock().expect("lock db");
    let now = stophammer::db::unix_now();
    conn.execute(
        "UPDATE source_gone_answers SET first_gone_at = ?1 WHERE feed_guid = ?2",
        rusqlite::params![now - seconds_ago, feed_guid],
    )
    .expect("backdate first_gone_at");
}

/// Ingests a fresh synthetic feed whose source URL and canonical URL are both
/// `url`, so its stored `feed_url` equals `url` (ADR 0067 section 2 needs a
/// stored source URL to count against).
async fn seed_feed(app: axum::Router, url: &str, crawl_token: &str, feed_guid: &str) {
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0067 Test Feed");
    let payload = body_ingest_payload(url, url, crawl_token, "hash-seed", &feed_data);
    let (status, body) = ingest(app, &payload).await;
    assert_eq!(status, 200, "seed ingest must succeed: {body:?}");
    assert_eq!(
        body["accepted"], true,
        "seed ingest must be accepted: {body:?}"
    );
}

/// A `404` for a stored source URL at a listed host, then a second one 24
/// hours later, retires the record. The second response has the reason
/// `source_gone` and one event. The event log has a `FeedRetired` event with
/// `source_gone`.
#[tokio::test]
async fn listed_host_404_then_24h_later_retires() {
    let crawl_token = "adr0067-token-listed-404-retires";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token, &["gone-listed.example.com"]);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0067-feed-listed-404-retires";
    let url = "https://gone-listed.example.com/feed.xml";
    seed_feed(app.clone(), url, crawl_token, feed_guid).await;

    let first = gone_report_payload(url, url, crawl_token, 404, &[]);
    let (status, body) = ingest(app.clone(), &first).await;
    assert_eq!(status, 200);
    assert_eq!(body["accepted"], false);
    assert_eq!(body["reason"], "source_gone_observed", "{body:?}");

    backdate_first_gone_at(&db, feed_guid, ONE_DAY_SECS);

    let second = gone_report_payload(url, url, crawl_token, 404, &[]);
    let (status, body) = ingest(app, &second).await;
    assert_eq!(status, 200);
    assert_eq!(body["accepted"], false);
    assert_eq!(body["reason"], "source_gone", "{body:?}");
    assert_eq!(
        body["events_emitted"].as_array().map(Vec::len),
        Some(1),
        "{body:?}"
    );

    assert_eq!(
        feeds_row_count(&db, feed_guid),
        0,
        "ADR 0067 section 3: the second answer must retire the record"
    );
    assert_eq!(events_of_type_count(&db, "feed_retired"), 1);
    let payload = last_feed_retired_payload(&db);
    assert!(
        payload.contains("source_gone"),
        "the feed_retired event must carry the reason source_gone: {payload}"
    );
}

/// A second `404` 23 hours after the first does not retire the record.
#[tokio::test]
async fn second_404_23h_after_first_does_not_retire() {
    let crawl_token = "adr0067-token-23h-no-retire";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token, &["gone-23h.example.com"]);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0067-feed-23h-no-retire";
    let url = "https://gone-23h.example.com/feed.xml";
    seed_feed(app.clone(), url, crawl_token, feed_guid).await;

    let first = gone_report_payload(url, url, crawl_token, 404, &[]);
    let (_, body) = ingest(app.clone(), &first).await;
    assert_eq!(body["reason"], "source_gone_observed", "{body:?}");

    backdate_first_gone_at(&db, feed_guid, 23 * 60 * 60);

    let second = gone_report_payload(url, url, crawl_token, 404, &[]);
    let (status, body) = ingest(app, &second).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["reason"], "source_gone_observed",
        "23 hours must not retire the record: {body:?}"
    );
    assert_eq!(
        feeds_row_count(&db, feed_guid),
        1,
        "the record must still exist after 23 hours"
    );
}

/// A `404`, then an ingest of a body for the record, then a `404` 24 hours
/// after the first: no retirement. The body deleted the row.
#[tokio::test]
async fn body_ingest_between_two_gone_answers_resets_the_count() {
    let crawl_token = "adr0067-token-body-resets";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token, &["gone-reset.example.com"]);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0067-feed-body-resets";
    let url = "https://gone-reset.example.com/feed.xml";
    seed_feed(app.clone(), url, crawl_token, feed_guid).await;

    let first = gone_report_payload(url, url, crawl_token, 404, &[]);
    let (_, body) = ingest(app.clone(), &first).await;
    assert_eq!(body["reason"], "source_gone_observed", "{body:?}");
    assert_eq!(source_gone_answers_row_count(&db, feed_guid), 1);

    // Back-date first_gone_at so, if the row survived, the next report
    // would retire the record.
    backdate_first_gone_at(&db, feed_guid, ONE_DAY_SECS);

    // A body ingest for the same record deletes the row (ADR 0067 section 2).
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0067 Test Feed");
    let body_payload = body_ingest_payload(url, url, crawl_token, "hash-body-reset", &feed_data);
    let (status, body) = ingest(app.clone(), &body_payload).await;
    assert_eq!(status, 200, "{body:?}");
    assert_eq!(body["accepted"], true, "{body:?}");
    assert_eq!(
        source_gone_answers_row_count(&db, feed_guid),
        0,
        "a body ingest must clear the gone-answer count"
    );

    let second = gone_report_payload(url, url, crawl_token, 404, &[]);
    let (status, body) = ingest(app, &second).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["reason"], "source_gone_observed",
        "the count must start over after a body ingest: {body:?}"
    );
    assert_eq!(
        feeds_row_count(&db, feed_guid),
        1,
        "the record must survive when the count was reset"
    );
}

/// A `404` from a host that is not listed gets `source_gone_ignored`, and no
/// row.
#[tokio::test]
async fn unlisted_host_404_is_ignored() {
    let crawl_token = "adr0067-token-unlisted-404";
    let db = common::test_db_arc();
    // No host is listed.
    let state = test_app_state(Arc::clone(&db), crawl_token, &[]);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0067-feed-unlisted-404";
    let url = "https://not-listed.example.com/feed.xml";
    seed_feed(app.clone(), url, crawl_token, feed_guid).await;

    let report = gone_report_payload(url, url, crawl_token, 404, &[]);
    let (status, body) = ingest(app, &report).await;
    assert_eq!(status, 200);
    assert_eq!(body["reason"], "source_gone_ignored", "{body:?}");
    assert_eq!(source_gone_answers_row_count(&db, feed_guid), 0);
    assert_eq!(feeds_row_count(&db, feed_guid), 1);
}

/// A `410` from a host that is not listed counts, and the second one
/// retires.
#[tokio::test]
async fn unlisted_host_410_counts_and_retires() {
    let crawl_token = "adr0067-token-410-any-host";
    let db = common::test_db_arc();
    // No host is listed: a 410 must count regardless.
    let state = test_app_state(Arc::clone(&db), crawl_token, &[]);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0067-feed-410-any-host";
    let url = "https://any-host.example.com/feed.xml";
    seed_feed(app.clone(), url, crawl_token, feed_guid).await;

    let first = gone_report_payload(url, url, crawl_token, 410, &[]);
    let (_, body) = ingest(app.clone(), &first).await;
    assert_eq!(body["reason"], "source_gone_observed", "{body:?}");

    backdate_first_gone_at(&db, feed_guid, ONE_DAY_SECS);

    let second = gone_report_payload(url, url, crawl_token, 410, &[]);
    let (status, body) = ingest(app, &second).await;
    assert_eq!(status, 200);
    assert_eq!(body["reason"], "source_gone", "{body:?}");
    assert_eq!(feeds_row_count(&db, feed_guid), 0);
}

/// A report for a URL that is no stored source URL gets
/// `source_gone_ignored`.
#[tokio::test]
async fn unknown_url_is_ignored() {
    let crawl_token = "adr0067-token-unknown-url";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token, &["gone-unknown.example.com"]);
    let app = stophammer::api::build_router(state);

    let url = "https://gone-unknown.example.com/never-ingested.xml";
    let report = gone_report_payload(url, url, crawl_token, 404, &[]);
    let (status, body) = ingest(app, &report).await;
    assert_eq!(status, 200);
    assert_eq!(body["reason"], "source_gone_ignored", "{body:?}");
}

/// A report with a redirect gets `source_gone_ignored`.
#[tokio::test]
async fn report_with_a_redirect_is_ignored() {
    let crawl_token = "adr0067-token-redirect-ignored";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token, &["gone-redirect.example.com"]);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0067-feed-redirect-ignored";
    let url = "https://gone-redirect.example.com/feed.xml";
    seed_feed(app.clone(), url, crawl_token, feed_guid).await;

    let hop = redirect_hop("https://gone-redirect.example.com/feed-final.xml", 301);
    let report = gone_report_payload(url, url, crawl_token, 404, &[hop]);
    let (status, body) = ingest(app, &report).await;
    assert_eq!(status, 200);
    assert_eq!(body["reason"], "source_gone_ignored", "{body:?}");
    assert_eq!(source_gone_answers_row_count(&db, feed_guid), 0);
}

/// A request with no `feed_data` and the status `500` keeps its present
/// response: the write phase still requires `feed_data`.
#[tokio::test]
async fn no_feed_data_status_500_keeps_present_response() {
    let crawl_token = "adr0067-token-500-unchanged";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token, &["gone-500.example.com"]);
    let app = stophammer::api::build_router(state);

    let url = "https://gone-500.example.com/feed.xml";
    let report = gone_report_payload(url, url, crawl_token, 500, &[]);
    let (status, body) = ingest(app, &report).await;
    assert_eq!(
        status, 400,
        "a request with no feed_data and a status other than 404/410 keeps its present 400: {body:?}"
    );
    assert_eq!(body["error"], "feed_data is required for successful ingest");
}
