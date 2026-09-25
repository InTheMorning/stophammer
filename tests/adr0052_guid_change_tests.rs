// ADR 0052 sections 4 and 5, task 007: a GUID change at a source URL is
// pending and public. It applies at once when the new GUID is the UUIDv5 of
// the source URL, or when the operator approves it. The transition retires
// the old record with no block.
//
// This mirrors the fixture style of tests/adr0052_move_trigger_tests.rs and
// tests/adr0058_resolve_tests.rs: a minimal verifier chain (content_hash
// only) so a synthetic feed ingests without needing the full default chain.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "test-adr0052-guid-change-admin-token";

/// The five item GUIDs of the Doerfelverse shape (ADR 0052 section 4): a
/// channel GUID change that keeps its item GUIDs.
const ITEM_GUIDS: [&str; 5] = ["item-1", "item-2", "item-3", "item-4", "item-5"];

/// The ADR 0058 Section 1b namespace `guid_origin_matches` derives a GUID
/// from a URL with. Duplicated here (not exported by `model`) so a test can
/// construct a body whose GUID is the `UUIDv5` of its own source URL.
const GUID_ORIGIN_NAMESPACE: uuid::Uuid = uuid::uuid!("ead4c236-bf58-58c6-a2c6-a6b28d128cb6");

/// The `UUIDv5` `guid_origin_matches` derives from `url`.
fn uuidv5_of_url(url: &str) -> String {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let name = without_scheme.strip_suffix('/').unwrap_or(without_scheme);
    uuid::Uuid::new_v5(&GUID_ORIGIN_NAMESPACE, name.as_bytes()).to_string()
}

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0052-guid-change-signer"));
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
        admin_token: ADMIN_TOKEN.into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

/// A synthetic feed with five tracks, the Doerfelverse shape. `feed_guid` is
/// the channel GUID; the five item GUIDs stay fixed across a GUID change.
fn feed_data(feed_guid: &str, title: &str) -> serde_json::Value {
    let tracks: Vec<serde_json::Value> = ITEM_GUIDS
        .iter()
        .map(|item_guid| {
            serde_json::json!({
                "track_guid": item_guid,
                "title": format!("Track {item_guid}"),
                "explicit": false
            })
        })
        .collect();
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "tracks": tracks
    })
}

/// A synthetic feed with `count` tracks, each with its own GUID. Used only
/// to build a body over `MAX_TRACKS_PER_INGEST`: the fix for the
/// partial-failure window must reject this before any transition runs.
fn feed_data_with_track_count(feed_guid: &str, title: &str, count: usize) -> serde_json::Value {
    let tracks: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            serde_json::json!({
                "track_guid": format!("track-{i}"),
                "title": format!("Track {i}"),
                "explicit": false
            })
        })
        .collect();
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "tracks": tracks
    })
}

fn ingest_payload(
    canonical_url: &str,
    source_url: &str,
    crawl_token: &str,
    content_hash: &str,
    data: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": canonical_url,
        "source_url": source_url,
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": content_hash,
        "feed_data": data,
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

/// Submits `data` at `url`, used as both `canonical_url` and `source_url`.
/// Returns the raw ingest response for the caller to inspect.
async fn submit(
    app: axum::Router,
    url: &str,
    crawl_token: &str,
    content_hash: &str,
    data: &serde_json::Value,
) -> serde_json::Value {
    let payload = ingest_payload(url, url, crawl_token, content_hash, data);
    let (status, body) = send(app, "POST", "/ingest/feed", None, Some(&payload)).await;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "ingest must answer 200: {body:?}"
    );
    body
}

/// Submits `data` at `url` and asserts it is accepted.
async fn accept(
    app: axum::Router,
    url: &str,
    crawl_token: &str,
    content_hash: &str,
    data: &serde_json::Value,
) -> serde_json::Value {
    let body = submit(app, url, crawl_token, content_hash, data).await;
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
    body
}

fn stored_feed(
    db: &Arc<Mutex<rusqlite::Connection>>,
    feed_guid: &str,
) -> Option<stophammer::model::Feed> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_feed(&conn, feed_guid).expect("query feed")
}

fn stored_track_guids(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> Vec<String> {
    let conn = db.lock().expect("lock db");
    let mut guids: Vec<String> = stophammer::db::get_tracks_for_feed(&conn, feed_guid)
        .expect("query tracks")
        .into_iter()
        .map(|track| track.track_guid)
        .collect();
    guids.sort();
    guids
}

fn guid_change_row(
    db: &Arc<Mutex<rusqlite::Connection>>,
    source_url: &str,
) -> Option<stophammer::db::GuidChangeRow> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_guid_change(&conn, source_url).expect("query feed_guid_changes")
}

fn guid_supersession(
    db: &Arc<Mutex<rusqlite::Connection>>,
    old_guid: &str,
) -> Option<stophammer::db::GuidSupersessionRow> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_guid_supersession(&conn, old_guid).expect("query feed_guid_supersessions")
}

async fn get_feed(app: axum::Router, feed_guid: &str) -> (http::StatusCode, serde_json::Value) {
    send(app, "GET", &format!("/v1/feeds/{feed_guid}"), None, None).await
}

async fn get_guid_changes(app: axum::Router) -> serde_json::Value {
    let (status, body) = send(app, "GET", "/v1/guid-changes", None, None).await;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "GET /v1/guid-changes must succeed: {body:?}"
    );
    body
}

async fn get_blocks(app: axum::Router) -> serde_json::Value {
    let (status, body) = send(app, "GET", "/v1/blocks", Some(ADMIN_TOKEN), None).await;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "GET /v1/blocks must succeed: {body:?}"
    );
    body
}

async fn decide_guid_change(
    app: axum::Router,
    old_guid: &str,
    decision: &str,
    reason: &str,
    admin_token: Option<&str>,
) -> (http::StatusCode, serde_json::Value) {
    send(
        app,
        "POST",
        &format!("/v1/feeds/{old_guid}/guid-change"),
        admin_token,
        Some(&serde_json::json!({ "decision": decision, "reason": reason })),
    )
    .await
}

// ---------------------------------------------------------------------------
// 1. The Doerfelverse shape: a pending change is public, and the record
//    keeps its GUID and its tracks.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_guid_change_that_is_not_the_uuidv5_is_pending_and_public() {
    let crawl_token = "adr0052-guid-change-pending-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url = "https://doerfelverse.example/feed/pending";
    let g1 = "11111111-1111-1111-1111-111111111111";
    let g2 = "22222222-2222-2222-2222-222222222222";

    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-pending-hash-g1",
        &feed_data(g1, "Elijah Lied"),
    )
    .await;

    let resp = submit(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-pending-hash-g2",
        &feed_data(g2, "Elijah Lied"),
    )
    .await;
    assert_eq!(
        resp["accepted"].as_bool(),
        Some(false),
        "a GUID change that is not the UUIDv5 of the URL must not apply: {resp:?}"
    );
    assert_eq!(resp["reason"].as_str(), Some("guid_change_pending"));
    assert!(
        !resp["events_emitted"]
            .as_array()
            .expect("events_emitted array")
            .is_empty(),
        "a new pending row must sign one FeedGuidChangeObserved event: {resp:?}"
    );

    // The record keeps G1 and its five tracks.
    let feed = stored_feed(&db, g1).expect("the record must keep its GUID");
    assert_eq!(feed.feed_url, url);
    assert_eq!(feed.title, "Elijah Lied");
    assert_eq!(stored_track_guids(&db, g1).len(), 5);
    assert!(
        stored_feed(&db, g2).is_none(),
        "no record exists yet for the pending new GUID"
    );

    let row = guid_change_row(&db, url).expect("a pending row must exist");
    assert_eq!(row.old_guid, g1);
    assert_eq!(row.new_guid, g2);
    assert_eq!(row.decision, None);

    // GET /v1/guid-changes lists the row.
    let list = get_guid_changes(stophammer::api::build_router(Arc::clone(&state))).await;
    let data = list["data"].as_array().expect("data array");
    assert!(
        data.iter()
            .any(|entry| entry["source_url"] == url && entry["new_guid"] == g2),
        "GET /v1/guid-changes must list the pending row: {list:?}"
    );

    // GET /v1/feeds/{g1} reports the pending change.
    let (status, body) = get_feed(stophammer::api::build_router(Arc::clone(&state)), g1).await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(body["data"]["pending_guid_change"]["new_guid"], g2);
}

// ---------------------------------------------------------------------------
// 2. A second submission with the same new GUID signs no new event, and
//    last_seen changes.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_repeated_pending_guid_signs_no_new_event_and_touches_last_seen() {
    let crawl_token = "adr0052-guid-change-repeat-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url = "https://doerfelverse.example/feed/repeat";
    let g1 = "11111111-1111-1111-1111-111111111112";
    let g2 = "22222222-2222-2222-2222-222222222223";

    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-repeat-hash-g1",
        &feed_data(g1, "Elijah Lied"),
    )
    .await;
    submit(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-repeat-hash-g2",
        &feed_data(g2, "Elijah Lied"),
    )
    .await;

    let row_after_first = guid_change_row(&db, url).expect("row must exist after first g2");
    let first_seen = row_after_first.first_seen;
    let last_seen_after_first = row_after_first.last_seen.expect("last_seen must be set");

    // Let the clock advance past unix_now()'s one-second resolution so a
    // later touch of last_seen gives a different value.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let resp = submit(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-repeat-hash-g2",
        &feed_data(g2, "Elijah Lied"),
    )
    .await;
    assert_eq!(resp["reason"].as_str(), Some("guid_change_pending"));
    assert!(
        resp["events_emitted"]
            .as_array()
            .expect("events_emitted array")
            .is_empty(),
        "a repeated pending GUID must sign no new event: {resp:?}"
    );

    let row_after_second = guid_change_row(&db, url).expect("row must still exist");
    assert_eq!(
        row_after_second.first_seen, first_seen,
        "first_seen must not move on a repeated submission"
    );
    assert!(
        row_after_second.last_seen.expect("last_seen must be set") > last_seen_after_first,
        "last_seen must advance on a repeated submission"
    );
}

// ---------------------------------------------------------------------------
// 3. A return to the old GUID deletes the pending row, and the record keeps
//    its track identities.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_submission_with_the_old_guid_deletes_the_pending_row() {
    let crawl_token = "adr0052-guid-change-return-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url = "https://doerfelverse.example/feed/return";
    let g1 = "11111111-1111-1111-1111-111111111113";
    let g2 = "22222222-2222-2222-2222-222222222224";

    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-return-hash-g1",
        &feed_data(g1, "Elijah Lied"),
    )
    .await;
    submit(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-return-hash-g2",
        &feed_data(g2, "Elijah Lied"),
    )
    .await;
    assert!(guid_change_row(&db, url).is_some(), "the row must exist");

    let before_guids = stored_track_guids(&db, g1);

    let resp = accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-return-hash-g1-again",
        &feed_data(g1, "Elijah Lied (corrected)"),
    )
    .await;
    assert!(
        !resp["events_emitted"]
            .as_array()
            .expect("events_emitted array")
            .is_empty(),
        "the delete of the pending row must sign a FeedGuidChangeObserved event: {resp:?}"
    );

    assert!(
        guid_change_row(&db, url).is_none(),
        "a return to the old GUID must delete the pending row"
    );
    let feed = stored_feed(&db, g1).expect("the record must still exist under g1");
    assert_eq!(feed.title, "Elijah Lied (corrected)");
    assert_eq!(
        stored_track_guids(&db, g1),
        before_guids,
        "the record must keep its track identities"
    );
}

// ---------------------------------------------------------------------------
// 4 and 5. A GUID that is the UUIDv5 of the source URL transitions at once,
// and a later submission with the new GUID is an update.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_uuidv5_guid_transitions_at_once_and_the_new_guid_then_updates() {
    let crawl_token = "adr0052-guid-change-auto-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url = "https://doerfelverse.example/feed/auto";
    let g1 = "11111111-1111-1111-1111-111111111114";
    let g2 = uuidv5_of_url(url);

    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-auto-hash-g1",
        &feed_data(g1, "Elijah Lied"),
    )
    .await;

    let resp = submit(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-auto-hash-g2",
        &feed_data(&g2, "Elijah Lied"),
    )
    .await;
    assert_eq!(
        resp["accepted"].as_bool(),
        Some(true),
        "the UUIDv5 case must transition at once: {resp:?}"
    );
    let warnings = resp["warnings"].as_array().expect("warnings array");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().is_some_and(|w| w.contains("ADR 0052 UUIDv5"))),
        "the response must name the UUIDv5 trigger: {resp:?}"
    );

    // G1 is retired with no block.
    assert!(
        stored_feed(&db, g1).is_none(),
        "the old record must be retired"
    );
    let blocks = get_blocks(stophammer::api::build_router(Arc::clone(&state))).await;
    assert!(
        blocks["blocks"]
            .as_array()
            .expect("blocks array")
            .is_empty(),
        "the transition must write no block: {blocks:?}"
    );

    // GET /v1/feeds/{g1} answers 404 with superseded_by.
    let (status, body) = get_feed(stophammer::api::build_router(Arc::clone(&state)), g1).await;
    assert_eq!(status, http::StatusCode::NOT_FOUND);
    assert_eq!(body["superseded_by"].as_str(), Some(g2.as_str()));

    let supersession = guid_supersession(&db, g1).expect("a supersession row must exist");
    assert_eq!(supersession.new_guid, g2);
    assert_eq!(supersession.source_url, url);

    // The new record is admitted with the same five tracks.
    let feed = stored_feed(&db, &g2).expect("the new record must exist");
    assert_eq!(feed.feed_url, url);
    assert_eq!(stored_track_guids(&db, &g2).len(), 5);
    assert!(
        guid_change_row(&db, url).is_none(),
        "the transition must delete any pending row"
    );

    // 5. After the transition, a submission with the new GUID is an update.
    let resp2 = accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-auto-hash-g2-update",
        &feed_data(&g2, "Elijah Lied (remaster)"),
    )
    .await;
    assert!(
        resp2["warnings"]
            .as_array()
            .expect("warnings array")
            .is_empty(),
        "a plain update names no move or GUID-change trigger: {resp2:?}"
    );
    let feed_after_update = stored_feed(&db, &g2).expect("the record must still exist");
    assert_eq!(feed_after_update.title, "Elijah Lied (remaster)");
}

// ---------------------------------------------------------------------------
// A body over MAX_TRACKS_PER_INGEST must reject before the transition runs:
// the partial-failure window. The track-count check (step 3b) runs before
// guid_change_transition, which now runs immediately before step 11, so a
// rejection there must not retire the old record.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_uuidv5_guid_change_over_the_track_limit_does_not_retire_the_old_record() {
    let crawl_token = "adr0052-guid-change-track-limit-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url = "https://doerfelverse.example/feed/track-limit";
    let g1 = "11111111-1111-1111-1111-111111111120";
    let g2 = uuidv5_of_url(url);

    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-track-limit-hash-g1",
        &feed_data(g1, "Elijah Lied"),
    )
    .await;

    // MAX_TRACKS_PER_INGEST is 500; 501 tracks must reject before any
    // transition runs, even though the GUID is the UUIDv5 of the URL.
    let oversized = feed_data_with_track_count(&g2, "Elijah Lied", 501);
    let payload = ingest_payload(
        url,
        url,
        crawl_token,
        "adr0052-track-limit-hash-g2",
        &oversized,
    );
    let (status, body) = send(
        stophammer::api::build_router(Arc::clone(&state)),
        "POST",
        "/ingest/feed",
        None,
        Some(&payload),
    )
    .await;
    assert_eq!(
        status,
        http::StatusCode::BAD_REQUEST,
        "a body over the track limit must be rejected before any transition: {body:?}"
    );

    // The old record must still exist, unretired.
    let feed = stored_feed(&db, g1).expect("the old record must not be retired");
    assert_eq!(feed.feed_url, url);

    let (get_status, _) = get_feed(stophammer::api::build_router(Arc::clone(&state)), g1).await;
    assert_eq!(
        get_status,
        http::StatusCode::OK,
        "GET /v1/feeds/{{old_guid}} must still answer 200"
    );

    let events: Vec<stophammer::event::Event> = {
        let conn = db.lock().expect("lock db");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events")
    };
    assert!(
        !events
            .iter()
            .any(|ev| ev.event_type == stophammer::event::EventType::FeedRetired),
        "no FeedRetired event must exist: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|ev| ev.event_type == stophammer::event::EventType::FeedGuidSuperseded),
        "no FeedGuidSuperseded event must exist: {events:?}"
    );
}

// ---------------------------------------------------------------------------
// 6. reject holds for the same new GUID; a different new GUID opens a new
//    row.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reject_holds_for_the_same_guid_and_a_different_guid_opens_a_new_row() {
    let crawl_token = "adr0052-guid-change-reject-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url = "https://doerfelverse.example/feed/reject";
    let g1 = "11111111-1111-1111-1111-111111111115";
    let g2 = "22222222-2222-2222-2222-222222222225";
    let g3 = "33333333-3333-3333-3333-333333333335";

    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-reject-hash-g1",
        &feed_data(g1, "Elijah Lied"),
    )
    .await;
    submit(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-reject-hash-g2",
        &feed_data(g2, "Elijah Lied"),
    )
    .await;

    let (status, decide_body) = decide_guid_change(
        stophammer::api::build_router(Arc::clone(&state)),
        g1,
        "reject",
        "confirmed tool error, waiting for a correction",
        Some(ADMIN_TOKEN),
    )
    .await;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "reject must succeed: {decide_body:?}"
    );
    assert!(decide_body["event_id"].as_str().is_some());

    let resp = submit(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-reject-hash-g2",
        &feed_data(g2, "Elijah Lied"),
    )
    .await;
    assert_eq!(
        resp["reason"].as_str(),
        Some("guid_change_rejected"),
        "the same new GUID must answer guid_change_rejected: {resp:?}"
    );

    let resp_g3 = submit(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-reject-hash-g3",
        &feed_data(g3, "Elijah Lied"),
    )
    .await;
    assert_eq!(
        resp_g3["reason"].as_str(),
        Some("guid_change_pending"),
        "a different new GUID must open a new pending row: {resp_g3:?}"
    );
    let row = guid_change_row(&db, url).expect("row must exist for g3");
    assert_eq!(row.new_guid, g3);
    assert_eq!(
        row.decision, None,
        "a different new GUID must reset the decision"
    );
}

// ---------------------------------------------------------------------------
// 7. approve runs the transition at the next submission of the new GUID.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn approve_runs_the_transition_at_the_next_submission() {
    let crawl_token = "adr0052-guid-change-approve-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url = "https://doerfelverse.example/feed/approve";
    let g1 = "11111111-1111-1111-1111-111111111116";
    let g2 = "22222222-2222-2222-2222-222222222226";

    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-approve-hash-g1",
        &feed_data(g1, "Elijah Lied"),
    )
    .await;
    submit(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-approve-hash-g2",
        &feed_data(g2, "Elijah Lied"),
    )
    .await;

    let (status, _) = decide_guid_change(
        stophammer::api::build_router(Arc::clone(&state)),
        g1,
        "approve",
        "confirmed with the publisher",
        Some(ADMIN_TOKEN),
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);

    // The node keeps no body from the earlier submission: the transition
    // needs the next submission of g2 to run.
    assert!(
        stored_feed(&db, g1).is_some(),
        "approval alone must not retire the record yet"
    );

    let resp = submit(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-approve-hash-g2-again",
        &feed_data(g2, "Elijah Lied"),
    )
    .await;
    assert_eq!(
        resp["accepted"].as_bool(),
        Some(true),
        "the next submission of the approved GUID must transition: {resp:?}"
    );
    let warnings = resp["warnings"].as_array().expect("warnings array");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().is_some_and(|w| w.contains("ADR 0052 approved"))),
        "the response must name the approved trigger: {resp:?}"
    );

    assert!(stored_feed(&db, g1).is_none(), "g1 must now be retired");
    assert!(stored_feed(&db, g2).is_some(), "g2 must now be admitted");
    assert_eq!(
        guid_supersession(&db, g1)
            .expect("a supersession row must exist")
            .new_guid,
        g2
    );
}

// ---------------------------------------------------------------------------
// 8. The routes answer 403 with no admin token.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_guid_change_route_answers_403_with_no_admin_token() {
    let crawl_token = "adr0052-guid-change-403-token";
    let db = common::test_db_arc();
    let state = test_app_state(db, crawl_token);

    let (status, _) = decide_guid_change(
        stophammer::api::build_router(state),
        "11111111-1111-1111-1111-111111111117",
        "approve",
        "no token given",
        None,
    )
    .await;
    assert_eq!(status, http::StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// 9. A replica that applies the events has the same rows and supersession,
//    with last_seen null.
// ---------------------------------------------------------------------------

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario covers both a pending row and a transition, so the replica assertions can compare both against the primary"
)]
async fn a_replica_applying_the_events_has_the_same_rows_and_supersession() {
    let crawl_token = "adr0052-guid-change-replica-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);

    // A pending row, still open.
    let pending_url = "https://doerfelverse.example/feed/replica-pending";
    let pending_g1 = "11111111-1111-1111-1111-111111111118";
    let pending_g2 = "22222222-2222-2222-2222-222222222227";
    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        pending_url,
        crawl_token,
        "adr0052-replica-hash-g1",
        &feed_data(pending_g1, "Elijah Lied"),
    )
    .await;
    submit(
        stophammer::api::build_router(Arc::clone(&state)),
        pending_url,
        crawl_token,
        "adr0052-replica-hash-g2",
        &feed_data(pending_g2, "Elijah Lied"),
    )
    .await;

    // A transition, applied at once.
    let auto_url = "https://doerfelverse.example/feed/replica-auto";
    let auto_g1 = "11111111-1111-1111-1111-111111111119";
    let auto_g2 = uuidv5_of_url(auto_url);
    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        auto_url,
        crawl_token,
        "adr0052-replica-hash-auto-g1",
        &feed_data(auto_g1, "Elijah Lied"),
    )
    .await;
    submit(
        stophammer::api::build_router(Arc::clone(&state)),
        auto_url,
        crawl_token,
        "adr0052-replica-hash-auto-g2",
        &feed_data(&auto_g2, "Elijah Lied"),
    )
    .await;

    let events: Vec<stophammer::event::Event> = {
        let conn = db.lock().expect("lock db");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events")
    };
    assert!(
        events
            .iter()
            .any(|ev| ev.event_type == stophammer::event::EventType::FeedGuidChangeObserved),
        "the primary must have signed a FeedGuidChangeObserved event"
    );
    assert!(
        events
            .iter()
            .any(|ev| ev.event_type == stophammer::event::EventType::FeedGuidSuperseded),
        "the primary must have signed a FeedGuidSuperseded event"
    );

    let replica_db = common::test_db_arc();
    let replica_pool = common::wrap_pool(Arc::clone(&replica_db));
    for ev in &events {
        let result = stophammer::apply::apply_single_event(&replica_pool, ev);
        assert!(
            result.is_ok(),
            "apply on the replica should succeed: {result:?}"
        );
    }

    // The pending row replicates, with last_seen null.
    let primary_pending = guid_change_row(&db, pending_url).expect("primary row must exist");
    let replica_pending =
        guid_change_row(&replica_db, pending_url).expect("replica row must exist");
    assert_eq!(replica_pending.source_url, primary_pending.source_url);
    assert_eq!(replica_pending.old_guid, primary_pending.old_guid);
    assert_eq!(replica_pending.new_guid, primary_pending.new_guid);
    assert_eq!(replica_pending.first_seen, primary_pending.first_seen);
    assert_eq!(replica_pending.decision, primary_pending.decision);
    assert!(
        primary_pending.last_seen.is_some(),
        "the primary must have a last_seen"
    );
    assert_eq!(
        replica_pending.last_seen, None,
        "an applied FeedGuidChangeObserved must never write last_seen on a replica"
    );

    // The transition and its supersession replicate.
    assert!(
        stored_feed(&replica_db, auto_g1).is_none(),
        "the replica must have retired the old record"
    );
    let replica_new_feed =
        stored_feed(&replica_db, &auto_g2).expect("the replica must have admitted the new record");
    assert_eq!(replica_new_feed.feed_url, auto_url);
    assert_eq!(stored_track_guids(&replica_db, &auto_g2).len(), 5);
    assert!(
        guid_change_row(&replica_db, auto_url).is_none(),
        "the replica must have deleted the pending-row delete marker's target"
    );

    let primary_supersession = guid_supersession(&db, auto_g1).expect("primary supersession");
    let replica_supersession =
        guid_supersession(&replica_db, auto_g1).expect("replica supersession");
    assert_eq!(replica_supersession.new_guid, primary_supersession.new_guid);
    assert_eq!(
        replica_supersession.source_url,
        primary_supersession.source_url
    );
}
