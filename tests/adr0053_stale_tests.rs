// ADR 0053 task 005: an older copy does not replace a newer copy.
//
// This mirrors the fixture style of tests/adr0051_source_url_tests.rs and
// tests/adr0052_self_link_tests.rs: a minimal verifier chain (content_hash
// only) so a synthetic feed ingests without needing the full default chain.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

/// `UA` is the record's original source URL. `UM` is the URL its body
/// declares as its own `atom:link rel="self"` (ADR 0052 section 2). The
/// self-link move test uses both; the other tests use `UA` alone.
const UA: &str = "https://wavlake.example/feed/adr0053/a";
const UM: &str = "https://wavlake.example/feed/adr0053/music/a";

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0053-stale-signer"));
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
    force_reingest: bool,
    feed_data: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": canonical_url,
        "source_url": source_url,
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": content_hash,
        "force_reingest": force_reingest,
        "feed_data": feed_data,
    })
}

/// A synthetic music feed with one track. `last_build_date` becomes the
/// body's channel `lastBuildDate` when given, and is left out of the JSON
/// entirely when `None`, matching a crawler that read no such element.
/// `self_link_url` becomes the body's `self_feed` link when given.
fn feed_data_with_last_build_date(
    feed_guid: &str,
    title: &str,
    last_build_date: Option<i64>,
    self_link_url: Option<&str>,
) -> serde_json::Value {
    let links = self_link_url.map_or_else(
        || serde_json::json!([]),
        |url| {
            serde_json::json!([{
                "position": 0,
                "link_type": "self_feed",
                "url": url,
                "extraction_path": "feed.atom:link[@rel='self']"
            }])
        },
    );
    let mut data = serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "links": links,
        "tracks": [{
            "track_guid": format!("{feed_guid}-track-01"),
            "title": "Track One",
            "explicit": false
        }]
    });
    if let Some(last_build_date) = last_build_date {
        data["last_build_date"] = serde_json::json!(last_build_date);
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

async fn ingest(app: axum::Router, payload: &serde_json::Value) {
    let body = ingest_response(app, payload).await;
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
}

fn stored_feed(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> stophammer::model::Feed {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_feed(&conn, feed_guid)
        .expect("query feed")
        .expect("feed row must exist")
}

/// **Older.** A feed accepted with `last_build_date` `T`, then a submission
/// from the source URL with `T - 60`, a new title and `force_reingest:
/// true`: `stale_submission`, and the title does not change.
#[tokio::test]
async fn an_older_last_build_date_is_rejected_as_stale() {
    let crawl_token = "adr0053-stale-older-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0053-stale-older-guid";
    let stored_lbd: i64 = 1_700_000_000;

    let feed_data_first =
        feed_data_with_last_build_date(feed_guid, "Original Title", Some(stored_lbd), None);
    let payload_first = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0053-stale-older-hash-1",
        false,
        &feed_data_first,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_first,
    )
    .await;

    let feed_data_second =
        feed_data_with_last_build_date(feed_guid, "New Title", Some(stored_lbd - 60), None);
    let payload_second = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0053-stale-older-hash-2",
        true,
        &feed_data_second,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_second,
    )
    .await;

    assert_eq!(
        resp["accepted"], false,
        "an older last_build_date must be rejected: {resp:?}"
    );
    assert_eq!(resp["no_change"], false, "{resp:?}");
    assert_eq!(resp["reason"], "stale_submission", "{resp:?}");
    assert_eq!(
        resp["source_url"],
        serde_json::Value::Null,
        "a stale rejection carries no source_url: {resp:?}"
    );
    assert_eq!(
        resp["events_emitted"].as_array().map(Vec::len),
        Some(0),
        "a stale rejection emits no events: {resp:?}"
    );

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.title, "Original Title",
        "a stale submission must not change the title"
    );
    assert_eq!(
        feed.last_build_date,
        Some(stored_lbd),
        "a stale submission must not change the stored last_build_date"
    );
}

/// **Equal.** The same submission as above, but with `last_build_date` equal
/// to the stored value: accepted, and `force_reingest` still does not skip
/// the rule for an equal date because an equal date already passes it.
#[tokio::test]
async fn an_equal_last_build_date_is_accepted() {
    let crawl_token = "adr0053-stale-equal-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0053-stale-equal-guid";
    let stored_lbd: i64 = 1_700_000_000;

    let feed_data_first =
        feed_data_with_last_build_date(feed_guid, "Original Title", Some(stored_lbd), None);
    let payload_first = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0053-stale-equal-hash-1",
        false,
        &feed_data_first,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_first,
    )
    .await;

    let feed_data_second =
        feed_data_with_last_build_date(feed_guid, "New Title", Some(stored_lbd), None);
    let payload_second = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0053-stale-equal-hash-2",
        true,
        &feed_data_second,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_second,
    )
    .await;

    assert_eq!(
        resp["accepted"], true,
        "an equal last_build_date must pass: {resp:?}"
    );
    assert_eq!(resp["reason"], serde_json::Value::Null, "{resp:?}");

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.title, "New Title",
        "an equal last_build_date must still apply the new body"
    );
}

/// **Newer.** The same submission as above, but with `last_build_date` `T +
/// 60`: accepted, and the stored value becomes `T + 60`.
#[tokio::test]
async fn a_newer_last_build_date_is_accepted_and_updates_the_stored_value() {
    let crawl_token = "adr0053-stale-newer-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0053-stale-newer-guid";
    let stored_lbd: i64 = 1_700_000_000;

    let feed_data_first =
        feed_data_with_last_build_date(feed_guid, "Original Title", Some(stored_lbd), None);
    let payload_first = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0053-stale-newer-hash-1",
        false,
        &feed_data_first,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_first,
    )
    .await;

    let feed_data_second =
        feed_data_with_last_build_date(feed_guid, "New Title", Some(stored_lbd + 60), None);
    let payload_second = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0053-stale-newer-hash-2",
        true,
        &feed_data_second,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_second,
    )
    .await;

    assert_eq!(
        resp["accepted"], true,
        "a newer last_build_date must pass: {resp:?}"
    );

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.title, "New Title",
        "a newer last_build_date must apply the new body"
    );
    assert_eq!(
        feed.last_build_date,
        Some(stored_lbd + 60),
        "the stored last_build_date must become the submitted value"
    );
}

/// **No submitted date.** A record with a stored `last_build_date`, then a
/// submission with none: accepted. The rule gives no protection when the
/// submission carries no `last_build_date`.
#[tokio::test]
async fn a_submission_with_no_last_build_date_is_accepted() {
    let crawl_token = "adr0053-stale-no-submitted-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0053-stale-no-submitted-guid";
    let stored_lbd: i64 = 1_700_000_000;

    let feed_data_first =
        feed_data_with_last_build_date(feed_guid, "Original Title", Some(stored_lbd), None);
    let payload_first = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0053-stale-no-submitted-hash-1",
        false,
        &feed_data_first,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_first,
    )
    .await;

    let feed_data_second = feed_data_with_last_build_date(feed_guid, "New Title", None, None);
    let payload_second = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0053-stale-no-submitted-hash-2",
        true,
        &feed_data_second,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_second,
    )
    .await;

    assert_eq!(
        resp["accepted"], true,
        "a submission with no last_build_date must pass: {resp:?}"
    );

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.title, "New Title",
        "a submission with no last_build_date must apply the new body"
    );
    assert_eq!(
        feed.last_build_date, None,
        "a missing submitted last_build_date clears the stored value, same as any other field"
    );
}

/// **No stored date.** A record with no stored `last_build_date`, then a
/// submission with a value: accepted. The rule gives no protection when the
/// record carries no stored `last_build_date`.
#[tokio::test]
async fn a_submission_with_no_stored_last_build_date_is_accepted() {
    let crawl_token = "adr0053-stale-no-stored-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0053-stale-no-stored-guid";

    let feed_data_first = feed_data_with_last_build_date(feed_guid, "Original Title", None, None);
    let payload_first = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0053-stale-no-stored-hash-1",
        false,
        &feed_data_first,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_first,
    )
    .await;

    let feed_data_second =
        feed_data_with_last_build_date(feed_guid, "New Title", Some(1_700_000_000), None);
    let payload_second = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0053-stale-no-stored-hash-2",
        true,
        &feed_data_second,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_second,
    )
    .await;

    assert_eq!(
        resp["accepted"], true,
        "a submission against a record with no stored last_build_date must pass: {resp:?}"
    );

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.title, "New Title",
        "the submission must apply the new body"
    );
    assert_eq!(
        feed.last_build_date,
        Some(1_700_000_000),
        "the stored last_build_date must become the submitted value"
    );
}

/// **Self-link move.** ADR 0053 section 3 binds the Update case only. A
/// self-link move (ADR 0052 section 2) is classified as Mirror, not Update,
/// so it still moves even when its `last_build_date` is older than the
/// stored value.
#[tokio::test]
async fn a_self_link_move_with_an_older_last_build_date_still_moves() {
    let crawl_token = "adr0053-stale-move-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0053-stale-move-guid";
    let stored_lbd: i64 = 1_700_000_000;

    let feed_data_a =
        feed_data_with_last_build_date(feed_guid, "Original Title", Some(stored_lbd), Some(UM));
    let payload_a = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0053-stale-move-hash-a",
        false,
        &feed_data_a,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let feed_data_m =
        feed_data_with_last_build_date(feed_guid, "New Title", Some(stored_lbd - 60), None);
    let payload_m = ingest_payload(
        UM,
        UM,
        crawl_token,
        "adr0053-stale-move-hash-m",
        false,
        &feed_data_m,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_m,
    )
    .await;

    assert_eq!(
        resp["accepted"], true,
        "a self-link move must not be rejected as stale: {resp:?}"
    );
    let warnings = resp["warnings"]
        .as_array()
        .expect("warnings must be an array");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str() == Some(&format!("moved from {UA} to {UM} (ADR 0052 self link)"))),
        "the warning must name both URLs: {resp:?}"
    );

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.feed_url, UM,
        "the stored feed_url must become the self URL"
    );
    assert_eq!(
        feed.title, "New Title",
        "the move must apply the new body even with an older last_build_date"
    );
    assert_eq!(
        feed.last_build_date,
        Some(stored_lbd - 60),
        "the move applies the submitted last_build_date; ADR 0053 section 3 does not bind Mirror"
    );
}
