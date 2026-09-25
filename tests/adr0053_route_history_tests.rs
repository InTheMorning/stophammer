// ADR 0053 task 006: the route-history read and its replay guarantee.
//
// `GET /v1/feeds/{guid}/route-history` reads the signed event log and reports
// each change of the payment recipients of a feed and its tracks. This
// mirrors the fixture style of tests/adr0053_blocks_api_tests.rs: a minimal
// verifier chain (content_hash only) so a synthetic feed ingests without
// needing the full default chain.

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
    let signer = Arc::new(common::temp_signer("test-adr0053-route-history-signer"));
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
        admin_token: "adr0053-route-history-admin-token".to_string(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

/// A minimal synthetic music feed with one feed-level route and one track
/// that carries its own route.
fn feed_data_with_feed_route(
    feed_guid: &str,
    title: &str,
    track_guid: &str,
    feed_route_name: &str,
    feed_route_address: &str,
    feed_route_split: i64,
    track_route_address: &str,
) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [{
            "recipient_name": feed_route_name,
            "route_type": "lnaddress",
            "address": feed_route_address,
            "custom_key": null,
            "custom_value": null,
            "split": feed_route_split,
            "fee": false
        }],
        "tracks": [{
            "track_guid": track_guid,
            "title": "Track One",
            "explicit": false,
            "payment_routes": [{
                "recipient_name": "Track Artist",
                "route_type": "lnaddress",
                "address": track_route_address,
                "custom_key": null,
                "custom_value": null,
                "split": 100,
                "fee": false
            }]
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
    body: Option<&serde_json::Value>,
) -> (http::StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let request = if let Some(body) = body {
        builder = builder.header("Content-Type", "application/json");
        builder
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
    send(app, "POST", "/ingest/feed", Some(payload)).await
}

// ---------------------------------------------------------------------------
// The read (ADR 0053 section 4, task file "Acceptance Criteria")
// ---------------------------------------------------------------------------

/// A feed with a feed route and one track with its own route, then an
/// update of the feed route to a new address. The history has three
/// entries: the first feed set, the first track set, and the feed change.
#[tokio::test]
async fn the_history_has_the_first_sets_then_a_feed_route_change() {
    let crawl_token = "adr0053-route-history-token-1";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0053-route-history-feed-1";
    let track_guid = format!("{feed_guid}-track-01");
    let canonical_url = "https://example.com/adr0053-route-history-1.xml";

    let v1 = feed_data_with_feed_route(
        feed_guid,
        "Route History Feed",
        &track_guid,
        "Artist",
        "a@ln.example",
        100,
        "t@ln.example",
    );
    let (status, body) = ingest(
        app.clone(),
        &ingest_payload(canonical_url, canonical_url, crawl_token, "rh-hash-1", &v1),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "the first ingest must succeed: {body:?}"
    );

    let v2 = feed_data_with_feed_route(
        feed_guid,
        "Route History Feed",
        &track_guid,
        "Artist",
        "b@ln.example",
        100,
        "t@ln.example",
    );
    let (status, body) = ingest(
        app.clone(),
        &ingest_payload(canonical_url, canonical_url, crawl_token, "rh-hash-2", &v2),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "the second ingest must succeed: {body:?}"
    );

    let (status, body) = send(
        app,
        "GET",
        &format!("/v1/feeds/{feed_guid}/route-history"),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let entries = body["data"].as_array().expect("data array");
    assert_eq!(
        entries.len(),
        3,
        "the history must have three entries: {entries:?}"
    );

    assert_eq!(entries[0]["subject"], "feed");
    assert_eq!(entries[0]["track_guid"], serde_json::Value::Null);
    assert_eq!(entries[0]["old_recipients"], serde_json::Value::Null);
    assert_eq!(
        entries[0]["new_recipients"],
        serde_json::json!([{ "address": "a@ln.example", "split": 100 }])
    );

    assert_eq!(entries[1]["subject"], "track");
    assert_eq!(entries[1]["track_guid"], track_guid);
    assert_eq!(entries[1]["old_recipients"], serde_json::Value::Null);
    assert_eq!(
        entries[1]["new_recipients"],
        serde_json::json!([{ "address": "t@ln.example", "split": 100 }])
    );

    assert_eq!(entries[2]["subject"], "feed");
    assert_eq!(
        entries[2]["old_recipients"],
        serde_json::json!([{ "address": "a@ln.example", "split": 100 }])
    );
    assert_eq!(
        entries[2]["new_recipients"],
        serde_json::json!([{ "address": "b@ln.example", "split": 100 }])
    );

    let seqs: Vec<i64> = entries
        .iter()
        .map(|e| e["seq"].as_i64().expect("seq is an integer"))
        .collect();
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "seq must strictly increase: {seqs:?}"
    );
}

/// An update that changes only the feed route's recipient name adds no new
/// entry: the recipient set (address, split) did not change.
#[tokio::test]
async fn a_route_name_only_change_adds_no_new_entry() {
    let crawl_token = "adr0053-route-history-token-2";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0053-route-history-feed-2";
    let track_guid = format!("{feed_guid}-track-01");
    let canonical_url = "https://example.com/adr0053-route-history-2.xml";

    let v1 = feed_data_with_feed_route(
        feed_guid,
        "Name Change Feed",
        &track_guid,
        "Artist",
        "c@ln.example",
        100,
        "t2@ln.example",
    );
    let (status, body) = ingest(
        app.clone(),
        &ingest_payload(canonical_url, canonical_url, crawl_token, "rh-hash-3", &v1),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "the first ingest must succeed: {body:?}"
    );

    // Same address and split; only the recipient name changes.
    let v2 = feed_data_with_feed_route(
        feed_guid,
        "Name Change Feed",
        &track_guid,
        "New Artist Name",
        "c@ln.example",
        100,
        "t2@ln.example",
    );
    let (status, body) = ingest(
        app.clone(),
        &ingest_payload(canonical_url, canonical_url, crawl_token, "rh-hash-4", &v2),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "the second ingest must succeed: {body:?}"
    );

    let (status, body) = send(
        app,
        "GET",
        &format!("/v1/feeds/{feed_guid}/route-history"),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let entries = body["data"].as_array().expect("data array");
    assert_eq!(
        entries.len(),
        2,
        "a route-name-only change must add no entry: {entries:?}"
    );
}

/// An update that changes only the feed route's split adds a new entry.
#[tokio::test]
async fn a_route_split_only_change_adds_a_new_entry() {
    let crawl_token = "adr0053-route-history-token-3";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0053-route-history-feed-3";
    let track_guid = format!("{feed_guid}-track-01");
    let canonical_url = "https://example.com/adr0053-route-history-3.xml";

    let v1 = feed_data_with_feed_route(
        feed_guid,
        "Split Change Feed",
        &track_guid,
        "Artist",
        "d@ln.example",
        100,
        "t3@ln.example",
    );
    let (status, body) = ingest(
        app.clone(),
        &ingest_payload(canonical_url, canonical_url, crawl_token, "rh-hash-5", &v1),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "the first ingest must succeed: {body:?}"
    );

    // Same address and name; only the split changes.
    let v2 = feed_data_with_feed_route(
        feed_guid,
        "Split Change Feed",
        &track_guid,
        "Artist",
        "d@ln.example",
        90,
        "t3@ln.example",
    );
    let (status, body) = ingest(
        app.clone(),
        &ingest_payload(canonical_url, canonical_url, crawl_token, "rh-hash-6", &v2),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "the second ingest must succeed: {body:?}"
    );

    let (status, body) = send(
        app,
        "GET",
        &format!("/v1/feeds/{feed_guid}/route-history"),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let entries = body["data"].as_array().expect("data array");
    assert_eq!(
        entries.len(),
        3,
        "a split-only change must add a new entry: {entries:?}"
    );
    assert_eq!(
        entries[2]["old_recipients"],
        serde_json::json!([{ "address": "d@ln.example", "split": 100 }])
    );
    assert_eq!(
        entries[2]["new_recipients"],
        serde_json::json!([{ "address": "d@ln.example", "split": 90 }])
    );
}

/// A GUID with no route-naming event answers 404.
#[tokio::test]
async fn a_guid_with_no_events_answers_404() {
    let db = common::test_db_arc();
    let state = test_app_state(db, "adr0053-route-history-token-4");
    let app = stophammer::api::build_router(state);

    let (status, body) = send(
        app,
        "GET",
        "/v1/feeds/adr0053-route-history-missing-feed/route-history",
        None,
    )
    .await;
    assert_eq!(
        status, 404,
        "a guid with no route-history events must answer 404: {body:?}"
    );
}

/// The same history read from a second database that applied the first
/// database's events matches the first database's answer.
#[tokio::test]
async fn the_same_history_from_a_replica_database_matches() {
    let crawl_token = "adr0053-route-history-token-5";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0053-route-history-feed-5";
    let track_guid = format!("{feed_guid}-track-01");
    let canonical_url = "https://example.com/adr0053-route-history-5.xml";

    let v1 = feed_data_with_feed_route(
        feed_guid,
        "Replica Feed",
        &track_guid,
        "Artist",
        "e@ln.example",
        100,
        "t5@ln.example",
    );
    let (status, body) = ingest(
        app.clone(),
        &ingest_payload(canonical_url, canonical_url, crawl_token, "rh-hash-7", &v1),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "the first ingest must succeed: {body:?}"
    );

    let v2 = feed_data_with_feed_route(
        feed_guid,
        "Replica Feed",
        &track_guid,
        "Artist",
        "f@ln.example",
        100,
        "t5@ln.example",
    );
    let (status, body) = ingest(
        app.clone(),
        &ingest_payload(canonical_url, canonical_url, crawl_token, "rh-hash-8", &v2),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["accepted"], true,
        "the second ingest must succeed: {body:?}"
    );

    let (status, primary_history) = send(
        app,
        "GET",
        &format!("/v1/feeds/{feed_guid}/route-history"),
        None,
    )
    .await;
    assert_eq!(status, 200);

    // Replay every event of the primary database onto a second database, in
    // seq order, the way a community node applies a push (ADR 0053 §4 plan
    // decision, "the signed events are never removed, so a route history
    // from the events is complete").
    let events = {
        let conn = db.lock().expect("lock primary db");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read primary events")
    };
    assert!(!events.is_empty(), "the primary must have recorded events");

    let replica_db = common::test_db_arc();
    let replica_pool = stophammer::db_pool::DbPool::from_writer_only(Arc::clone(&replica_db));
    for ev in &events {
        stophammer::apply::apply_single_event(&replica_pool, ev).expect("apply event to replica");
    }

    let replica_state = test_app_state(replica_db, crawl_token);
    let replica_app = stophammer::api::build_router(replica_state);
    let (status, replica_history) = send(
        replica_app,
        "GET",
        &format!("/v1/feeds/{feed_guid}/route-history"),
        None,
    )
    .await;
    assert_eq!(status, 200);

    assert_eq!(
        primary_history["data"], replica_history["data"],
        "the route history must be equal from the replica database"
    );
}
