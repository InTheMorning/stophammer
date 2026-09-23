// ADR 0049 task 004: the node records which URL gave which podcast:guid,
// replicates each change through a signed FeedUrlObserved event, and keeps
// the stored feed_url of a known GUID stable across a second URL.
//
// The HTTP-level tests here mirror tests/adr0049_rel_tests.rs: a minimal
// verifier chain (crawl_token, content_hash) so a synthetic or fixture feed
// ingests without needing the full default chain.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// HTTP-level helpers (mirrors tests/adr0049_rel_tests.rs)
// ---------------------------------------------------------------------------

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0049-url-obs-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: vec!["crawl_token".to_string(), "content_hash".to_string()],
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

/// A minimal synthetic music feed with one track, for tests that do not need
/// a real-feed fixture.
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

// ---------------------------------------------------------------------------
// DB-level query helpers
// ---------------------------------------------------------------------------

fn stored_feed_url(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> String {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT feed_url FROM feeds WHERE feed_guid = ?1",
        [feed_guid],
        |r| r.get(0),
    )
    .expect("feed row should exist")
}

fn observation_guid(db: &Arc<Mutex<rusqlite::Connection>>, url: &str) -> Option<String> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_feed_url_observation(&conn, url)
        .expect("query observation")
        .map(|observation| observation.feed_guid)
}

fn feed_url_observed_event_count(db: &Arc<Mutex<rusqlite::Connection>>) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT COUNT(*) FROM events WHERE event_type = 'feed_url_observed'",
        [],
        |r| r.get(0),
    )
    .expect("count feed_url_observed events")
}

fn feed_url_observation_rows(db: &Arc<Mutex<rusqlite::Connection>>) -> Vec<(String, String, i64)> {
    let conn = db.lock().expect("lock db");
    let mut stmt = conn
        .prepare("SELECT url, feed_guid, observed_at FROM feed_url_observations ORDER BY url")
        .expect("prepare query");
    stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })
    .expect("query rows")
    .collect::<Result<Vec<_>, _>>()
    .expect("collect rows")
}

// ---------------------------------------------------------------------------
// 1. The three branches of the record rule (db::record_feed_url_observation).
// ---------------------------------------------------------------------------

#[test]
fn record_feed_url_observation_new_url_inserts_and_returns_some() {
    let conn = common::test_db();
    let now = common::now();

    let observation = stophammer::db::record_feed_url_observation(
        &conn,
        "https://example.com/new.xml",
        "guid-new",
        now,
    )
    .expect("record observation")
    .expect("a url the node has never seen must return Some");

    assert_eq!(observation.url, "https://example.com/new.xml");
    assert_eq!(observation.feed_guid, "guid-new");
    assert_eq!(observation.observed_at, now);

    let stored = stophammer::db::get_feed_url_observation(&conn, "https://example.com/new.xml")
        .expect("read observation")
        .expect("row must exist after the insert");
    assert_eq!(stored.feed_guid, "guid-new");
}

#[test]
fn record_feed_url_observation_different_guid_replaces_and_returns_some() {
    let conn = common::test_db();
    let now = common::now();
    stophammer::db::record_feed_url_observation(
        &conn,
        "https://example.com/dual.xml",
        "guid-a",
        now,
    )
    .expect("record first observation");

    let later = now + 100;
    let observation = stophammer::db::record_feed_url_observation(
        &conn,
        "https://example.com/dual.xml",
        "guid-b",
        later,
    )
    .expect("record second observation")
    .expect("a different guid at a known url must return Some");

    assert_eq!(observation.feed_guid, "guid-b");
    assert_eq!(observation.observed_at, later);

    let stored = stophammer::db::get_feed_url_observation(&conn, "https://example.com/dual.xml")
        .expect("read observation")
        .expect("row must exist");
    assert_eq!(
        stored.feed_guid, "guid-b",
        "the row must now name the new guid"
    );
    assert_eq!(
        stored.observed_at, later,
        "the row must record the later observed_at"
    );
}

#[test]
fn record_feed_url_observation_same_guid_writes_nothing_and_returns_none() {
    let conn = common::test_db();
    let now = common::now();
    stophammer::db::record_feed_url_observation(
        &conn,
        "https://example.com/same.xml",
        "guid-same",
        now,
    )
    .expect("record first observation");

    let later = now + 100;
    let result = stophammer::db::record_feed_url_observation(
        &conn,
        "https://example.com/same.xml",
        "guid-same",
        later,
    )
    .expect("record repeat observation");

    assert!(
        result.is_none(),
        "the same guid at a known url must write nothing and return None"
    );

    let stored = stophammer::db::get_feed_url_observation(&conn, "https://example.com/same.xml")
        .expect("read observation")
        .expect("row must exist");
    assert_eq!(
        stored.observed_at, now,
        "observed_at must not change when the guid is unchanged"
    );
}

// ---------------------------------------------------------------------------
// 2. Second URL: the stored feed_url stays the first URL, and both URLs get
// an observation of the DETOX album guid.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn second_url_keeps_stored_feed_url_and_observes_both_urls() {
    let crawl_token = "adr0049-url-obs-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9";
    let feed_data = common::adr0049_feed_data("detox-album");

    let url_a = "https://cdn.example.com/detox-album-a.xml";
    let payload_a = ingest_payload(url_a, url_a, crawl_token, "hash-detox-a", &feed_data);
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let url_b = "https://cdn.example.com/detox-album-b.xml";
    let payload_b = ingest_payload(url_b, url_b, crawl_token, "hash-detox-b", &feed_data);
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_b,
    )
    .await;

    assert_eq!(
        stored_feed_url(&db, feed_guid),
        url_a,
        "the stored feed_url must stay the first url the guid arrived through"
    );

    assert_eq!(
        observation_guid(&db, url_a).as_deref(),
        Some(feed_guid),
        "url A must have an observation of the DETOX album guid"
    );
    assert_eq!(
        observation_guid(&db, url_b).as_deref(),
        Some(feed_guid),
        "url B must have an observation of the DETOX album guid"
    );
}

// ---------------------------------------------------------------------------
// 3. Redirect: source_url differs from canonical_url; both get an
// observation of the same guid.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn redirect_records_an_observation_for_both_urls() {
    let crawl_token = "adr0049-url-obs-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "redirect-feed";
    let feed_data = synthetic_feed_data(feed_guid, "Redirect Feed");

    let canonical_url = "https://example.com/redirect/canonical.xml";
    let source_url = "https://example.com/redirect/source.xml";
    let payload = ingest_payload(
        canonical_url,
        source_url,
        crawl_token,
        "hash-redirect",
        &feed_data,
    );
    ingest(stophammer::api::build_router(state), &payload).await;

    assert_eq!(
        observation_guid(&db, canonical_url).as_deref(),
        Some(feed_guid),
        "canonical_url must have an observation"
    );
    assert_eq!(
        observation_guid(&db, source_url).as_deref(),
        Some(feed_guid),
        "source_url must have an observation"
    );
}

// ---------------------------------------------------------------------------
// 4. No change: a repeat submission with the same content_hash and a new
// source_url still records that source_url.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_change_submission_with_a_new_source_url_still_records_it() {
    let crawl_token = "adr0049-url-obs-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "no-change-feed";
    let feed_data = synthetic_feed_data(feed_guid, "No Change Feed");
    let canonical_url = "https://example.com/no-change.xml";
    let content_hash = "hash-no-change";

    let payload1 = ingest_payload(
        canonical_url,
        canonical_url,
        crawl_token,
        content_hash,
        &feed_data,
    );
    let resp1 = ingest_response(stophammer::api::build_router(Arc::clone(&state)), &payload1).await;
    assert_eq!(
        resp1["accepted"].as_bool(),
        Some(true),
        "the first ingest must be accepted: {resp1:?}"
    );
    assert_eq!(resp1["no_change"].as_bool(), Some(false));

    let source_url_2 = "https://example.com/no-change-redirect.xml";
    let payload2 = ingest_payload(
        canonical_url,
        source_url_2,
        crawl_token,
        content_hash,
        &feed_data,
    );
    let resp2 = ingest_response(stophammer::api::build_router(state), &payload2).await;

    assert_eq!(
        resp2["accepted"].as_bool(),
        Some(true),
        "a no-change response must still report accepted: true: {resp2:?}"
    );
    assert_eq!(
        resp2["no_change"].as_bool(),
        Some(true),
        "the same content_hash must short-circuit as no_change: {resp2:?}"
    );

    assert_eq!(
        observation_guid(&db, source_url_2).as_deref(),
        Some(feed_guid),
        "the new source_url must get an observation even on the no-change path"
    );
}

// ---------------------------------------------------------------------------
// 5. A second ingest from the same URL with the same GUID emits no new
// FeedUrlObserved event.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn repeat_ingest_from_the_same_url_and_guid_emits_no_new_event() {
    let crawl_token = "adr0049-url-obs-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "repeat-feed";
    let feed_data = synthetic_feed_data(feed_guid, "Repeat Feed");
    let url = "https://example.com/repeat.xml";

    let payload1 = ingest_payload(url, url, crawl_token, "hash-repeat-1", &feed_data);
    ingest(stophammer::api::build_router(Arc::clone(&state)), &payload1).await;
    assert_eq!(feed_url_observed_event_count(&db), 1);

    // A different content_hash forces a real re-ingest, not the no-change
    // short-circuit, so the write phase runs again for the same url and guid.
    let payload2 = ingest_payload(url, url, crawl_token, "hash-repeat-2", &feed_data);
    ingest(stophammer::api::build_router(state), &payload2).await;

    assert_eq!(
        feed_url_observed_event_count(&db),
        1,
        "the same url and guid must not add a second FeedUrlObserved event"
    );
}

// ---------------------------------------------------------------------------
// 6. A rejected ingest records no observation.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rejected_ingest_records_no_observation() {
    let crawl_token = "adr0049-url-obs-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "rejected-feed";
    let feed_data = synthetic_feed_data(feed_guid, "Rejected Feed");
    let url = "https://example.com/rejected.xml";

    let payload = ingest_payload(url, url, "wrong-crawl-token", "hash-rejected", &feed_data);
    let resp = ingest_response(stophammer::api::build_router(state), &payload).await;

    assert_eq!(
        resp["accepted"].as_bool(),
        Some(false),
        "a wrong crawl_token must be rejected: {resp:?}"
    );
    assert_eq!(
        observation_guid(&db, url),
        None,
        "a rejected ingest must record no observation"
    );
}

// ---------------------------------------------------------------------------
// 7. Replication: applying each FeedUrlObserved event from one database on a
// second database gives the same feed_url_observations table.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn feed_url_observations_replicate_identically_from_the_event_log() {
    let crawl_token = "adr0049-url-obs-token";
    let db1 = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db1), crawl_token);
    let feed_guid = "e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9";
    let feed_data = common::adr0049_feed_data("detox-album");

    let url_a = "https://cdn.example.com/replicated-a.xml";
    let payload_a = ingest_payload(url_a, url_a, crawl_token, "hash-replicated-a", &feed_data);
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let url_b = "https://cdn.example.com/replicated-b.xml";
    let payload_b = ingest_payload(url_b, url_b, crawl_token, "hash-replicated-b", &feed_data);
    ingest(stophammer::api::build_router(state), &payload_b).await;

    let url_events: Vec<stophammer::event::Event> = {
        let conn = db1.lock().expect("lock db1");
        stophammer::db::get_events_since(&conn, 0, 1000)
            .expect("read events")
            .into_iter()
            .filter(|ev| matches!(ev.event_type, stophammer::event::EventType::FeedUrlObserved))
            .collect()
    };
    assert_eq!(
        url_events.len(),
        2,
        "the two-url ingest sequence must emit exactly two FeedUrlObserved events, got {url_events:?}"
    );
    for ev in &url_events {
        assert_eq!(ev.subject_guid, feed_guid);
    }

    let db2 = common::test_db_arc();
    let pool2 = common::wrap_pool(Arc::clone(&db2));
    for ev in &url_events {
        let result = stophammer::apply::apply_single_event(&pool2, ev);
        assert!(
            result.is_ok(),
            "apply on the second database should succeed: {result:?}"
        );
    }

    assert_eq!(
        feed_url_observation_rows(&db1),
        feed_url_observation_rows(&db2),
        "the feed_url_observations table must be identical after replaying the event log"
    );
}
