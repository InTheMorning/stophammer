// ADR 0051 task 001: the node checks the crawl token before any other work.
//
// This mirrors the fixture style of tests/adr0049_url_observation_tests.rs: a
// minimal verifier chain (content_hash only) so a synthetic feed ingests
// without needing the full default chain.

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
    let signer = Arc::new(common::temp_signer("test-adr0051-auth-signer"));
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

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

fn feed_url_observation_count(db: &Arc<Mutex<rusqlite::Connection>>) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row("SELECT COUNT(*) FROM feed_url_observations", [], |row| {
        row.get(0)
    })
    .expect("count feed_url_observations rows")
}

/// ADR 0051 section 4: the node checks the crawl token before any database
/// read, before the verifier chain, and before the content-hash shortcut.
///
/// A first submission with the right token is accepted. A second submission
/// of the same URL with the same `content_hash` — the shape that would
/// normally read as `NO_CHANGE` — carries the wrong token. The token check
/// runs first, so the answer is a rejection naming the crawl-token check, not
/// a `NO_CHANGE`, and the rejected submission leaves the
/// `feed_url_observations` table untouched.
#[tokio::test]
async fn wrong_token_on_a_repeat_submission_rejects_before_the_content_hash_shortcut() {
    let right_token = "adr0051-right-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), right_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "adr0051-auth-feed";
    let canonical_url = "https://example.com/adr0051-auth-feed.xml";
    let content_hash = "adr0051-auth-hash";
    let feed_data = synthetic_feed_data(feed_guid, "ADR 0051 Auth Feed");

    // 1. First submission, with the right token, is accepted.
    let first_payload = ingest_payload(
        canonical_url,
        canonical_url,
        right_token,
        content_hash,
        &feed_data,
    );
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&first_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let first = body_json(resp).await;
    assert_eq!(
        first["accepted"], true,
        "the first submission with the right token must be accepted: {first:?}"
    );

    let observations_before = feed_url_observation_count(&db);

    // 2. Same URL and the same content_hash, but the wrong token. The token
    //    check must reject this before the content_hash verifier ever runs.
    let repeat_payload = ingest_payload(
        canonical_url,
        canonical_url,
        "adr0051-wrong-token",
        content_hash,
        &feed_data,
    );
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&repeat_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let second = body_json(resp).await;
    assert_eq!(
        second["accepted"], false,
        "a wrong token must be rejected, even on a repeat body: {second:?}"
    );
    assert_eq!(
        second["no_change"], false,
        "a wrong token must not read as NO_CHANGE: {second:?}"
    );
    assert_eq!(
        second["reason"], "[crawl_token] invalid crawl token",
        "the rejection reason must name the crawl-token check: {second:?}"
    );

    let observations_after = feed_url_observation_count(&db);
    assert_eq!(
        observations_before, observations_after,
        "a rejected submission must not change feed_url_observations"
    );
}

// ---------------------------------------------------------------------------
// Task 003: the handler classifies each submission (ADR 0051 section 2)
// before it writes, in the write phase and in the no-change path.
// ---------------------------------------------------------------------------

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

async fn ingest_status_and_body(
    app: axum::Router,
    payload: &serde_json::Value,
) -> (http::StatusCode, serde_json::Value) {
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
    let status = resp.status();
    (status, body_json(resp).await)
}

async fn ingest(app: axum::Router, payload: &serde_json::Value) {
    let body = ingest_response(app, payload).await;
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
}

/// A synthetic feed with one track and one feed-level payment route, for a
/// test that checks a route address survives, or does not survive, a second
/// submission.
fn feed_data_with_route(
    feed_guid: &str,
    title: &str,
    track_guid: &str,
    track_title: &str,
    route_address: &str,
) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [{
            "recipient_name": null,
            "route_type": "lnaddress",
            "address": route_address,
            "custom_key": null,
            "custom_value": null,
            "split": 100,
            "fee": false
        }],
        "tracks": [{
            "track_guid": track_guid,
            "title": track_title,
            "explicit": false
        }]
    })
}

fn observation_guid(db: &Arc<Mutex<rusqlite::Connection>>, url: &str) -> Option<String> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_feed_url_observation(&conn, url)
        .expect("query observation")
        .map(|observation| observation.feed_guid)
}

fn artist_credit_count(db: &Arc<Mutex<rusqlite::Connection>>) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row("SELECT COUNT(*) FROM artist_credit", [], |row| row.get(0))
        .expect("count artist_credit rows")
}

/// ADR 0051 section 2, Mirror case: a body from a URL that is not the source
/// URL of the held GUID must not change the record. It gets an observation.
#[tokio::test]
async fn mirror_body_at_a_different_url_is_rejected_as_source_conflict() {
    let crawl_token = "adr0051-mirror-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0051-mirror-guid";
    let url_a = "https://example.com/adr0051-mirror/a.xml";
    let url_x = "https://example.com/adr0051-mirror/x.xml";

    let feed_data_a = feed_data_with_route(
        feed_guid,
        "Victim Feed",
        "adr0051-mirror-track-a",
        "Victim Track",
        "victim@ln.example",
    );
    let payload_a = ingest_payload(
        url_a,
        url_a,
        crawl_token,
        "adr0051-mirror-hash-a",
        &feed_data_a,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let feed_data_x = feed_data_with_route(
        feed_guid,
        "Attacker Feed",
        "adr0051-mirror-track-x",
        "Attacker Track",
        "attacker@ln.example",
    );
    let payload_x = ingest_payload(
        url_x,
        url_x,
        crawl_token,
        "adr0051-mirror-hash-x",
        &feed_data_x,
    );
    let resp_x = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_x,
    )
    .await;

    assert_eq!(
        resp_x["accepted"], false,
        "ADR 0051 section 2: a mirror body must not be accepted: {resp_x:?}"
    );
    assert_eq!(
        resp_x["reason"], "source_conflict",
        "ADR 0051 section 2: a body from a URL that is not the source URL \
         of the held GUID must answer source_conflict: {resp_x:?}"
    );
    assert_eq!(resp_x["source_url"], url_a, "{resp_x:?}");

    {
        let conn = db.lock().expect("lock db");
        let feed = stophammer::db::get_feed(&conn, feed_guid)
            .expect("query feed")
            .expect("feed row must exist");
        assert_eq!(
            feed.title, "Victim Feed",
            "ADR 0051 section 2: a mirror body must not change the feed title"
        );
        assert_eq!(feed.feed_url, url_a);

        let tracks = stophammer::db::get_tracks_for_feed(&conn, feed_guid).expect("query tracks");
        assert_eq!(
            tracks.len(),
            1,
            "ADR 0051 section 2: the track list must not change"
        );
        assert_eq!(tracks[0].track_guid, "adr0051-mirror-track-a");

        let routes = stophammer::db::get_feed_payment_routes_for_feed(&conn, feed_guid)
            .expect("query routes");
        let addresses: Vec<&str> = routes.iter().map(|r| r.address.as_str()).collect();
        assert_eq!(
            addresses,
            vec!["victim@ln.example"],
            "ADR 0051 section 2: a route address must not change"
        );
    }

    assert_eq!(
        observation_guid(&db, url_x).as_deref(),
        Some(feed_guid),
        "the mirror url must still get a feed_url_observations row for the guid"
    );
}

/// ADR 0051 section 1: a fetch through the stored source URL applies its
/// content, even when the request carries a new `canonical_url`. The stored
/// `feed_url` stays the source URL.
#[tokio::test]
async fn redirect_from_the_source_url_applies_content_and_keeps_the_stored_feed_url() {
    let crawl_token = "adr0051-redirect-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0051-redirect-guid";
    let url_a = "https://example.com/adr0051-redirect/a.xml";
    let url_y = "https://example.com/adr0051-redirect/y-cdn.xml";

    let feed_data_1 = synthetic_feed_data(feed_guid, "Original Title");
    let payload_1 = ingest_payload(
        url_a,
        url_a,
        crawl_token,
        "adr0051-redirect-hash-1",
        &feed_data_1,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_1,
    )
    .await;

    let feed_data_2 = synthetic_feed_data(feed_guid, "Updated Title");
    let payload_2 = ingest_payload(
        url_y,
        url_a,
        crawl_token,
        "adr0051-redirect-hash-2",
        &feed_data_2,
    );
    let resp_2 = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_2,
    )
    .await;

    assert_eq!(
        resp_2["accepted"], true,
        "ADR 0051 section 1: content fetched through the stored source URL \
         must apply, even with a new canonical_url: {resp_2:?}"
    );

    let conn = db.lock().expect("lock db");
    let feed = stophammer::db::get_feed(&conn, feed_guid)
        .expect("query feed")
        .expect("feed row must exist");
    assert_eq!(feed.title, "Updated Title");
    assert_eq!(
        feed.feed_url, url_a,
        "the stored feed_url must stay the source URL"
    );
}

/// ADR 0051 section 2, GUID change case: a held URL that starts to declare a
/// new GUID answers `guid_change_pending`, not an HTTP 500, and writes
/// nothing (the Doerfelverse defect this ADR closes).
#[tokio::test]
async fn a_new_guid_at_a_held_url_answers_guid_change_pending() {
    let crawl_token = "adr0051-guidchange-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let held_guid = "adr0051-guidchange-held";
    let new_guid = "adr0051-guidchange-new";
    let url_a = "https://example.com/adr0051-guidchange/a.xml";

    let feed_data_1 = synthetic_feed_data(held_guid, "Held Feed");
    let payload_1 = ingest_payload(
        url_a,
        url_a,
        crawl_token,
        "adr0051-guidchange-hash-1",
        &feed_data_1,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_1,
    )
    .await;

    let feed_data_2 = synthetic_feed_data(new_guid, "Changed Feed");
    let payload_2 = ingest_payload(
        url_a,
        url_a,
        crawl_token,
        "adr0051-guidchange-hash-2",
        &feed_data_2,
    );
    let (status_2, resp_2) = ingest_status_and_body(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_2,
    )
    .await;

    assert_eq!(
        status_2, 200,
        "a guid change must not answer HTTP 500: {resp_2:?}"
    );
    assert_eq!(resp_2["accepted"], false, "{resp_2:?}");
    assert_eq!(resp_2["reason"], "guid_change_pending", "{resp_2:?}");

    let conn = db.lock().expect("lock db");
    assert!(
        stophammer::db::get_feed(&conn, new_guid)
            .expect("query feed")
            .is_none(),
        "a guid_change_pending submission must not create a feeds row for the new guid"
    );
    let held_feed = stophammer::db::get_feed(&conn, held_guid)
        .expect("query feed")
        .expect("the held feed must still exist");
    assert_eq!(
        held_feed.title, "Held Feed",
        "the held record must not change"
    );
    assert!(
        stophammer::db::get_tracks_for_feed(&conn, new_guid)
            .expect("query tracks")
            .is_empty()
    );
    assert!(
        stophammer::db::get_feed_payment_routes_for_feed(&conn, new_guid)
            .expect("query routes")
            .is_empty()
    );
}

/// ADR 0051 section 2, `RecordConflict` case: a submission whose URL is held
/// by one record and whose GUID is held by a different record writes
/// nothing, on either side, and adds no observation.
#[tokio::test]
async fn a_submission_that_touches_two_held_records_answers_record_conflict() {
    let crawl_token = "adr0051-recordconflict-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let guid_g = "adr0051-recordconflict-g";
    let guid_b = "adr0051-recordconflict-b";
    let url_g = "https://example.com/adr0051-recordconflict/g.xml";
    let url_b = "https://example.com/adr0051-recordconflict/b.xml";

    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            url_g,
            url_g,
            crawl_token,
            "adr0051-recordconflict-hash-g",
            &synthetic_feed_data(guid_g, "Feed G"),
        ),
    )
    .await;
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            url_b,
            url_b,
            crawl_token,
            "adr0051-recordconflict-hash-b",
            &synthetic_feed_data(guid_b, "Feed B"),
        ),
    )
    .await;

    let observations_before = feed_url_observation_count(&db);

    let resp_conflict = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            url_b,
            url_b,
            crawl_token,
            "adr0051-recordconflict-hash-conflict",
            &synthetic_feed_data(guid_g, "Conflict Body"),
        ),
    )
    .await;

    assert_eq!(resp_conflict["accepted"], false, "{resp_conflict:?}");
    assert_eq!(
        resp_conflict["reason"], "record_conflict",
        "{resp_conflict:?}"
    );

    {
        let conn = db.lock().expect("lock db");
        let feed_g = stophammer::db::get_feed(&conn, guid_g)
            .expect("query feed")
            .expect("feed G must still exist");
        assert_eq!(feed_g.title, "Feed G", "feed G must not change");
        let feed_b = stophammer::db::get_feed(&conn, guid_b)
            .expect("query feed")
            .expect("feed B must still exist");
        assert_eq!(feed_b.title, "Feed B", "feed B must not change");
    }

    let observations_after = feed_url_observation_count(&db);
    assert_eq!(
        observations_before, observations_after,
        "a record_conflict submission must not change feed_url_observations"
    );
}

/// ADR 0051 section 2, `RecordConflict` in the no-change path: a submission
/// that reads as `NO_CHANGE` by URL and content hash, but declares the GUID
/// of a different held record, must still classify before it records an
/// observation.
#[tokio::test]
async fn a_no_change_submission_that_declares_a_different_held_guid_answers_record_conflict() {
    let crawl_token = "adr0051-nochangeconflict-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let guid_a = "adr0051-nochangeconflict-a";
    let guid_b = "adr0051-nochangeconflict-b";
    let url_a = "https://example.com/adr0051-nochangeconflict/a.xml";
    let url_b = "https://example.com/adr0051-nochangeconflict/b.xml";
    let hash_h = "adr0051-nochangeconflict-hash-h";

    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            url_a,
            url_a,
            crawl_token,
            hash_h,
            &synthetic_feed_data(guid_a, "Feed A"),
        ),
    )
    .await;
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            url_b,
            url_b,
            crawl_token,
            "adr0051-nochangeconflict-hash-b",
            &synthetic_feed_data(guid_b, "Feed B"),
        ),
    )
    .await;

    let observations_before = feed_url_observation_count(&db);

    // Same url and the same content_hash as feed A's own submission — the
    // shape that reads as NO_CHANGE — but this body declares feed B's guid.
    let resp_conflict = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            url_a,
            url_a,
            crawl_token,
            hash_h,
            &synthetic_feed_data(guid_b, "Feed A Body Again"),
        ),
    )
    .await;

    assert_eq!(
        resp_conflict["accepted"], false,
        "ADR 0051 section 2: the no-change path must classify before it \
         records an observation: {resp_conflict:?}"
    );
    assert_eq!(
        resp_conflict["no_change"], false,
        "a classified conflict must not read as no_change: {resp_conflict:?}"
    );
    assert_eq!(
        resp_conflict["reason"], "record_conflict",
        "{resp_conflict:?}"
    );

    let observations_after = feed_url_observation_count(&db);
    assert_eq!(
        observations_before, observations_after,
        "a no-change record_conflict must not add or change an observation"
    );
}

/// ADR 0051 Guards: a `guid_change_pending` rejection writes no new
/// `artist_credit` row.
#[tokio::test]
async fn artist_credit_count_is_unchanged_by_guid_change_pending() {
    let crawl_token = "adr0051-artistcredit-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);

    let held_guid = "adr0051-artistcredit-guidchange-held";
    let new_guid = "adr0051-artistcredit-guidchange-new";
    let url_gc = "https://example.com/adr0051-artistcredit/guidchange.xml";
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            url_gc,
            url_gc,
            crawl_token,
            "adr0051-artistcredit-hash-1",
            &synthetic_feed_data(held_guid, "Held"),
        ),
    )
    .await;

    let credits_before = artist_credit_count(&db);
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            url_gc,
            url_gc,
            crawl_token,
            "adr0051-artistcredit-hash-2",
            &synthetic_feed_data(new_guid, "New"),
        ),
    )
    .await;
    assert_eq!(resp["reason"], "guid_change_pending", "{resp:?}");
    assert_eq!(
        artist_credit_count(&db),
        credits_before,
        "a guid_change_pending rejection must not add an artist_credit row"
    );
}

/// ADR 0051 Guards: a `record_conflict` rejection in the write phase writes
/// no new `artist_credit` row.
#[tokio::test]
async fn artist_credit_count_is_unchanged_by_record_conflict() {
    let crawl_token = "adr0051-artistcredit-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);

    let guid_g = "adr0051-artistcredit-recordconflict-g";
    let guid_b = "adr0051-artistcredit-recordconflict-b";
    let url_g = "https://example.com/adr0051-artistcredit/g.xml";
    let url_b = "https://example.com/adr0051-artistcredit/b.xml";
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            url_g,
            url_g,
            crawl_token,
            "adr0051-artistcredit-hash-g",
            &synthetic_feed_data(guid_g, "G"),
        ),
    )
    .await;
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            url_b,
            url_b,
            crawl_token,
            "adr0051-artistcredit-hash-b",
            &synthetic_feed_data(guid_b, "B"),
        ),
    )
    .await;

    let credits_before = artist_credit_count(&db);
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            url_b,
            url_b,
            crawl_token,
            "adr0051-artistcredit-hash-conflict",
            &synthetic_feed_data(guid_g, "Conflict"),
        ),
    )
    .await;
    assert_eq!(resp["reason"], "record_conflict", "{resp:?}");
    assert_eq!(
        artist_credit_count(&db),
        credits_before,
        "a record_conflict rejection must not add an artist_credit row"
    );
}

/// ADR 0051 Guards: a `record_conflict` rejection in the no-change path
/// writes no new `artist_credit` row.
#[tokio::test]
async fn artist_credit_count_is_unchanged_by_a_no_change_record_conflict() {
    let crawl_token = "adr0051-artistcredit-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);

    let held_guid = "adr0051-artistcredit-nochange-held";
    let other_guid = "adr0051-artistcredit-nochange-other";
    let held_url = "https://example.com/adr0051-artistcredit/nochange-held.xml";
    let other_url = "https://example.com/adr0051-artistcredit/nochange-other.xml";
    let held_hash = "adr0051-artistcredit-nochange-hash-held";
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            held_url,
            held_url,
            crawl_token,
            held_hash,
            &synthetic_feed_data(held_guid, "Held"),
        ),
    )
    .await;
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            other_url,
            other_url,
            crawl_token,
            "adr0051-artistcredit-nochange-hash-other",
            &synthetic_feed_data(other_guid, "Other"),
        ),
    )
    .await;

    let credits_before = artist_credit_count(&db);
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &ingest_payload(
            held_url,
            held_url,
            crawl_token,
            held_hash,
            &synthetic_feed_data(other_guid, "Held Body Again"),
        ),
    )
    .await;
    assert_eq!(resp["reason"], "record_conflict", "{resp:?}");
    assert_eq!(
        artist_credit_count(&db),
        credits_before,
        "a no-change record_conflict rejection must not add an artist_credit row"
    );
}
