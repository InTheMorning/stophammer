// ADR 0106 §1: a GUID change at the source URL changes the record in place.
//
// This mirrors the fixture style of tests/adr0052_move_trigger_tests.rs and
// tests/adr0058_resolve_tests.rs: a minimal verifier chain (content_hash
// only) so a synthetic feed ingests without needing the full default chain.

mod common;

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "test-adr0106-guid-change-admin-token";

/// The five item GUIDs stay the same after a channel GUID change (ADR 0106 §1).
const ITEM_GUIDS: [&str; 5] = ["item-1", "item-2", "item-3", "item-4", "item-5"];

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
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

/// Builds a feed whose item GUIDs stay fixed across a channel GUID change (ADR 0106 §1).
fn feed_data(feed_guid: &str, title: &str) -> serde_json::Value {
    let tracks: Vec<serde_json::Value> = ITEM_GUIDS
        .iter()
        .map(|item_guid| {
            serde_json::json!({
                "track_guid": item_guid,
                "title": format!("Track {item_guid}"),
                "explicit": false,
                "payment_routes": [{
                    "recipient_name": "Track Artist",
                    "route_type": "lnaddress",
                    "address": format!("{item_guid}@example.com"),
                    "split": 100,
                    "fee": false
                }]
            })
        })
        .collect();
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [{
            "recipient_name": "Feed Artist",
            "route_type": "lnaddress",
            "address": "feed@example.com",
            "split": 100,
            "fee": false
        }],
        "tracks": tracks
    })
}

/// Builds a feed body that exceeds the track limit (ADR 0106 §1).
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

/// Submits `data` at `url` as both `canonical_url` and `source_url` (ADR 0106 §1).
/// Returns the response for the caller to inspect.
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

/// Submits `data` at `url` and asserts acceptance (ADR 0106 §1).
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

fn table_rows(
    db: &Arc<Mutex<rusqlite::Connection>>,
    sql: &str,
    feed_guid: &str,
) -> Vec<Vec<rusqlite::types::Value>> {
    let conn = db.lock().expect("lock database");
    let mut statement = conn.prepare(sql).expect("prepare row query");
    statement
        .query_map([feed_guid], |row| {
            (0..row.as_ref().column_count())
                .map(|index| row.get(index))
                .collect()
        })
        .expect("query rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("read rows")
}

fn all_rows(db: &Arc<Mutex<rusqlite::Connection>>, sql: &str) -> Vec<Vec<rusqlite::types::Value>> {
    let conn = db.lock().expect("lock database");
    let mut statement = conn.prepare(sql).expect("prepare row query");
    statement
        .query_map([], |row| {
            (0..row.as_ref().column_count())
                .map(|index| row.get(index))
                .collect()
        })
        .expect("query rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("read rows")
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

async fn get_blocks(app: axum::Router) -> serde_json::Value {
    let (status, body) = send(app, "GET", "/v1/blocks", Some(ADMIN_TOKEN), None).await;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "GET /v1/blocks must succeed: {body:?}"
    );
    body
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario checks each row, route and answer after a GUID change"
)]
async fn a_free_guid_changes_in_place_at_the_first_submission() {
    let crawl_token = "adr0052-guid-change-auto-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url = "https://doerfelverse.example/feed/auto";
    let g1 = "11111111-1111-1111-1111-111111111114";
    let g2 = uuid::Uuid::new_v4().to_string();

    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-auto-hash-g1",
        &feed_data(g1, "Elijah Lied"),
    )
    .await;

    let feed_count_before: i64 = {
        let conn = db.lock().expect("lock database");
        conn.query_row("SELECT COUNT(*) FROM feeds", [], |row| row.get(0))
            .expect("count feeds")
    };
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
        "a free GUID must change on the first submission: {resp:?}"
    );
    let warnings = resp["warnings"].as_array().expect("warnings array");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().is_some_and(|w| w.contains("ADR 0106"))),
        "the response must name ADR 0106: {resp:?}"
    );

    // ADR 0106 §1: The record keeps its row under the new GUID.
    assert!(stored_feed(&db, g1).is_none(), "the old key must be free");
    assert_eq!(
        stored_feed(&db, &g2)
            .expect("the record must remain")
            .feed_url,
        url
    );
    let feed_count_after: i64 = {
        let conn = db.lock().expect("lock database");
        conn.query_row("SELECT COUNT(*) FROM feeds", [], |row| row.get(0))
            .expect("count feeds")
    };
    assert_eq!(feed_count_after, feed_count_before);
    let blocks = get_blocks(stophammer::api::build_router(Arc::clone(&state))).await;
    assert!(
        blocks["blocks"]
            .as_array()
            .expect("blocks array")
            .is_empty(),
        "the transition must write no block: {blocks:?}"
    );

    // ADR 0106 §3: The old GUID answers 404 with superseded_by.
    let (status, body) = get_feed(stophammer::api::build_router(Arc::clone(&state)), g1).await;
    assert_eq!(status, http::StatusCode::NOT_FOUND);
    assert_eq!(body["superseded_by"].as_str(), Some(g2.as_str()));
    let (track_status, track_body) = send(
        stophammer::api::build_router(Arc::clone(&state)),
        "GET",
        &format!("/v1/feeds/{g1}/tracks/item-1"),
        None,
        None,
    )
    .await;
    assert_eq!(track_status, http::StatusCode::NOT_FOUND);
    assert_eq!(track_body["superseded_by"].as_str(), Some(g2.as_str()));

    let supersession = guid_supersession(&db, g1).expect("a supersession row must exist");
    assert_eq!(supersession.new_guid, g2);
    assert_eq!(supersession.source_url, url);
    let events = {
        let conn = db.lock().expect("lock database");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events")
    };
    let changed_event_index = events
        .iter()
        .position(|event| event.event_type == stophammer::event::EventType::FeedGuidChanged)
        .expect("FeedGuidChanged event");
    assert_eq!(
        events[changed_event_index + 1].event_type,
        stophammer::event::EventType::FeedUpserted
    );
    assert_eq!(
        resp["events_emitted"][0],
        events[changed_event_index].event_id
    );

    // ADR 0106 §1: The same record keeps its five tracks.
    let feed = stored_feed(&db, &g2).expect("the new record must exist");
    assert_eq!(feed.feed_url, url);
    assert_eq!(stored_track_guids(&db, &g2).len(), 5);
    assert_eq!(
        stored_track_guids(&db, &g2),
        ITEM_GUIDS
            .iter()
            .map(|guid| (*guid).to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        table_rows(&db, "SELECT feed_guid, track_guid, title FROM tracks WHERE feed_guid = ?1 ORDER BY track_guid", &g2).len(),
        5
    );
    assert_eq!(
        table_rows(&db, "SELECT feed_guid, track_guid, address FROM payment_routes WHERE feed_guid = ?1 ORDER BY track_guid", &g2).len(),
        5
    );
    assert_eq!(
        table_rows(
            &db,
            "SELECT feed_guid, address FROM feed_payment_routes WHERE feed_guid = ?1",
            &g2
        )
        .len(),
        1
    );
    {
        let conn = db.lock().expect("lock database");
        for (table, column) in stophammer::db::GUID_CHANGE_COLUMNS {
            let count: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
                    [g1],
                    |row| row.get(0),
                )
                .expect("count old GUID keys");
            assert_eq!(count, 0, "{table}.{column} must not keep the old GUID");
        }
    }
    let (feed_status, feed_body) =
        get_feed(stophammer::api::build_router(Arc::clone(&state)), &g2).await;
    assert_eq!(feed_status, http::StatusCode::OK);
    assert!(feed_body["data"]["pending_guid_change"].is_null());
    let (list_status, list_body) = send(
        stophammer::api::build_router(Arc::clone(&state)),
        "GET",
        "/v1/guid-changes?limit=10",
        None,
        None,
    )
    .await;
    assert_eq!(list_status, http::StatusCode::OK);
    assert_eq!(list_body["data"], serde_json::json!([]));
    assert_eq!(list_body["pagination"]["has_more"], false);
    let (route_status, _) = send(
        stophammer::api::build_router(Arc::clone(&state)),
        "POST",
        &format!("/v1/feeds/{g2}/guid-change"),
        Some(ADMIN_TOKEN),
        Some(&serde_json::json!({"decision":"approve","reason":"legacy"})),
    )
    .await;
    assert!(
        route_status == http::StatusCode::NOT_FOUND
            || route_status == http::StatusCode::METHOD_NOT_ALLOWED
    );

    // ADR 0106 §1: A later submission with the new GUID updates the record.
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
// A body over the track limit must leave the stored record unchanged (ADR 0106 §1).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_guid_change_over_the_track_limit_keeps_the_old_record() {
    let crawl_token = "adr0052-guid-change-track-limit-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url = "https://doerfelverse.example/feed/track-limit";
    let g1 = "11111111-1111-1111-1111-111111111120";
    let g2 = uuid::Uuid::new_v4().to_string();

    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0052-track-limit-hash-g1",
        &feed_data(g1, "Elijah Lied"),
    )
    .await;

    // ADR 0106 §1: A body with 501 tracks exceeds the current ingest limit.
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

    // ADR 0106 §1: The old record must still exist with its old GUID.
    let feed = stored_feed(&db, g1).expect("the old record must not be retired");
    assert_eq!(feed.feed_url, url);
    assert_eq!(stored_track_guids(&db, g1).len(), 5);
    assert_eq!(
        table_rows(
            &db,
            "SELECT feed_guid, track_guid, address FROM payment_routes WHERE feed_guid = ?1",
            g1
        )
        .len(),
        5
    );

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
    assert!(
        !events
            .iter()
            .any(|ev| ev.event_type == stophammer::event::EventType::FeedGuidChanged),
        "a rejected body must not sign FeedGuidChanged: {events:?}"
    );
}

#[tokio::test]
async fn a_guid_change_reverts_at_the_next_submission() {
    let crawl_token = "adr0106-guid-revert-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url = "https://adr0106.example/feed/revert";
    let old_guid = "11111111-1111-1111-1111-111111111161";
    let new_guid = uuid::Uuid::new_v4().to_string();
    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0106-revert-old",
        &feed_data(old_guid, "Feed"),
    )
    .await;
    for (guid, hash) in [
        (new_guid.as_str(), "adr0106-revert-new"),
        (old_guid, "adr0106-revert-back"),
    ] {
        let response = submit(
            stophammer::api::build_router(Arc::clone(&state)),
            url,
            crawl_token,
            hash,
            &feed_data(guid, "Feed"),
        )
        .await;
        assert_eq!(
            response["accepted"], true,
            "GUID update must apply: {response:?}"
        );
    }
    assert!(stored_feed(&db, old_guid).is_some());
    let conn = db.lock().expect("lock database");
    let old_link_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feed_guid_supersessions WHERE old_guid = ?1",
            [old_guid],
            |row| row.get(0),
        )
        .expect("count old GUID links");
    assert_eq!(old_link_count, 0);
    let reverted_link: String = conn
        .query_row(
            "SELECT new_guid FROM feed_guid_supersessions WHERE old_guid = ?1",
            [&new_guid],
            |row| row.get(0),
        )
        .expect("read revert link");
    assert_eq!(reverted_link, old_guid);
}

#[tokio::test]
async fn a_change_error_rolls_back_rows_and_event() {
    let crawl_token = "adr0106-guid-error-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url = "https://adr0106.example/feed/error";
    let old_guid = "11111111-1111-1111-1111-111111111162";
    let new_guid = uuid::Uuid::new_v4().to_string();
    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url,
        crawl_token,
        "adr0106-error-old",
        &feed_data(old_guid, "Feed"),
    )
    .await;
    {
        let conn = db.lock().expect("lock database");
        conn.execute_batch(
            "CREATE TRIGGER fail_guid_route_change BEFORE UPDATE OF feed_guid ON feed_payment_routes \
             BEGIN SELECT RAISE(ABORT, 'forced ADR 0106 failure'); END;",
        )
        .expect("create failure trigger");
    }
    let (status, _) = send(
        stophammer::api::build_router(Arc::clone(&state)),
        "POST",
        "/ingest/feed",
        None,
        Some(&ingest_payload(
            url,
            url,
            crawl_token,
            "adr0106-error-new",
            &feed_data(&new_guid, "Feed"),
        )),
    )
    .await;
    assert_eq!(status, http::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(stored_feed(&db, old_guid).is_some());
    assert!(stored_feed(&db, &new_guid).is_none());
    assert_eq!(stored_track_guids(&db, old_guid).len(), 5);
    assert_eq!(
        table_rows(
            &db,
            "SELECT feed_guid, track_guid, address FROM payment_routes WHERE feed_guid = ?1",
            old_guid
        )
        .len(),
        5
    );
    let events = {
        let conn = db.lock().expect("lock database");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events")
    };
    assert!(
        !events
            .iter()
            .any(|event| { event.event_type == stophammer::event::EventType::FeedGuidChanged })
    );
}

#[tokio::test]
async fn a_guid_held_by_another_record_answers_record_conflict() {
    let crawl_token = "adr0106-guid-conflict-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let url_a = "https://adr0106.example/feed/a";
    let url_b = "https://adr0106.example/feed/b";
    let guid_a = "11111111-1111-1111-1111-111111111163";
    let guid_b = "11111111-1111-1111-1111-111111111164";
    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url_a,
        crawl_token,
        "adr0106-conflict-a",
        &feed_data(guid_a, "Feed A"),
    )
    .await;
    accept(
        stophammer::api::build_router(Arc::clone(&state)),
        url_b,
        crawl_token,
        "adr0106-conflict-b",
        &feed_data(guid_b, "Feed B"),
    )
    .await;
    let (status, response) = send(
        stophammer::api::build_router(Arc::clone(&state)),
        "POST",
        "/ingest/feed",
        None,
        Some(&ingest_payload(
            url_a,
            url_a,
            crawl_token,
            "adr0106-conflict-a2",
            &feed_data(guid_b, "Changed"),
        )),
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(response["reason"], "record_conflict");
    assert_eq!(stored_feed(&db, guid_a).expect("record A").title, "Feed A");
    assert_eq!(stored_feed(&db, guid_b).expect("record B").title, "Feed B");
}

#[test]
fn the_guid_change_column_list_covers_the_migrated_schema() {
    let conn = common::test_db();
    let changed: HashSet<(&str, &str)> = stophammer::db::GUID_CHANGE_COLUMNS
        .iter()
        .copied()
        .collect();
    let stays: HashSet<(&str, &str)> = stophammer::db::GUID_CHANGE_STAYS
        .iter()
        .map(|(table, column, _reason)| (*table, *column))
        .collect();
    let mut statement = conn
        .prepare(
            "SELECT m.name, p.name FROM sqlite_master AS m \
             JOIN pragma_table_info(m.name) AS p \
             WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%'",
        )
        .expect("prepare schema columns");
    let columns = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("read schema columns");
    for column in columns {
        let (table, name) = column.expect("read schema column");
        if name.contains("feed_guid")
            && !changed.contains(&(table.as_str(), name.as_str()))
            && !stays.contains(&(table.as_str(), name.as_str()))
        {
            panic!(
                "ADR 0106 §2: {table}.{name} holds a feed GUID. Add it to the change of change_feed_guid_sql, or to the list of columns that stay with its reason."
            );
        }
    }
}

#[test]
fn migration_0066_drops_existing_pending_guid_rows() {
    let conn = common::test_db();
    conn.execute_batch(
        "CREATE TABLE feed_guid_changes (source_url TEXT PRIMARY KEY, old_guid TEXT NOT NULL, new_guid TEXT NOT NULL, first_seen INTEGER NOT NULL) STRICT; \
         INSERT INTO feed_guid_changes VALUES ('https://example.com/feed.xml', 'old', 'new', 1);",
    )
    .expect("seed historical pending row");
    conn.execute_batch(include_str!(
        "../migrations/0066_drop_feed_guid_changes.sql"
    ))
    .expect("run migration 0066");
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'feed_guid_changes')",
            [],
            |row| row.get(0),
        )
        .expect("check pending table");
    assert!(!exists);
}

#[tokio::test]
async fn a_replica_applying_the_events_has_the_same_rows_and_supersession() {
    let crawl_token = "adr0052-guid-change-replica-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);

    // ADR 0106 §5: The primary applies one GUID change event.
    let auto_url = "https://doerfelverse.example/feed/replica-auto";
    let auto_g1 = "11111111-1111-1111-1111-111111111119";
    let auto_g2 = uuid::Uuid::new_v4().to_string();
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
            .any(|ev| ev.event_type == stophammer::event::EventType::FeedGuidChanged),
        "the primary must have signed a FeedGuidChanged event"
    );
    assert!(
        !events
            .iter()
            .any(|ev| ev.event_type == stophammer::event::EventType::FeedGuidSuperseded),
        "the primary must not sign a FeedGuidSuperseded event"
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

    // ADR 0106 §5: The GUID change and its link replicate.
    assert!(
        stored_feed(&replica_db, auto_g1).is_none(),
        "the replica must have changed the old GUID"
    );
    let replica_new_feed =
        stored_feed(&replica_db, &auto_g2).expect("the replica must have admitted the new record");
    assert_eq!(replica_new_feed.feed_url, auto_url);
    assert_eq!(stored_track_guids(&replica_db, &auto_g2).len(), 5);
    for sql in [
        "SELECT * FROM feeds ORDER BY feed_guid",
        "SELECT * FROM tracks ORDER BY feed_guid, track_guid",
        "SELECT * FROM payment_routes ORDER BY feed_guid, track_guid, id",
        "SELECT * FROM feed_guid_supersessions ORDER BY old_guid",
    ] {
        assert_eq!(
            all_rows(&replica_db, sql),
            all_rows(&db, sql),
            "primary and replica rows must match for {sql}"
        );
    }

    let primary_supersession = guid_supersession(&db, auto_g1).expect("primary supersession");
    let replica_supersession =
        guid_supersession(&replica_db, auto_g1).expect("replica supersession");
    assert_eq!(replica_supersession.new_guid, primary_supersession.new_guid);
    assert_eq!(
        replica_supersession.source_url,
        primary_supersession.source_url
    );
}

// ── ADR 0106 §6, §7 and §8: task 002 ───────────────────────────────────────

/// Submits `data` with the source URL, the canonical URL and the redirect
/// hops that the crawler reports (ADR 0106 §6).
async fn submit_with_redirects(
    app: axum::Router,
    source_url: &str,
    canonical_url: &str,
    crawl_token: &str,
    content_hash: &str,
    data: &serde_json::Value,
    redirects: &[serde_json::Value],
) -> serde_json::Value {
    let mut payload = ingest_payload(canonical_url, source_url, crawl_token, content_hash, data);
    payload["redirects"] = serde_json::Value::Array(redirects.to_vec());
    let (status, body) = send(app, "POST", "/ingest/feed", None, Some(&payload)).await;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "ingest must answer 200: {body:?}"
    );
    body
}

fn redirect_hop(url: &str, status: u16) -> serde_json::Value {
    serde_json::json!({ "url": url, "status": status })
}

fn feed_count(db: &Arc<Mutex<rusqlite::Connection>>) -> i64 {
    let conn = db.lock().expect("lock database");
    conn.query_row("SELECT COUNT(*) FROM feeds", [], |row| row.get(0))
        .expect("count feeds")
}

/// A test bench with record `R` at `S` with `G1` (ADR 0106 §6).
struct Bench {
    db: Arc<Mutex<rusqlite::Connection>>,
    state: Arc<stophammer::api::AppState>,
}

const TOKEN_002: &str = "adr0106-task-002-token";
const S: &str = "https://doerfelverse.example/artists/r/s.xml";
const F: &str = "https://cdn.example/artists/r/f.xml";
const G1: &str = "11111111-1111-1111-1111-111111111121";
const G2: &str = "22222222-2222-2222-2222-222222222122";
const G3: &str = "33333333-3333-3333-3333-333333333123";

impl Bench {
    async fn new() -> Self {
        let db = common::test_db_arc();
        let state = test_app_state(Arc::clone(&db), TOKEN_002);
        let bench = Self { db, state };
        accept(bench.app(), S, TOKEN_002, "r-at-s", &feed_data(G1, "R")).await;
        bench
    }

    fn app(&self) -> axum::Router {
        stophammer::api::build_router(Arc::clone(&self.state))
    }

    async fn redirect(&self, status: u16, data: &serde_json::Value) -> serde_json::Value {
        submit_with_redirects(
            self.app(),
            S,
            F,
            TOKEN_002,
            &format!("redirect-{status}-{}", data["feed_guid"]),
            data,
            &[redirect_hop(F, status)],
        )
        .await
    }
}

async fn assert_permanent_redirect_moves_and_changes(status: u16) {
    let b = Bench::new().await;
    let resp = b.redirect(status, &feed_data(G2, "R")).await;
    assert_eq!(resp["accepted"], true, "the change must apply: {resp:?}");
    assert!(stored_feed(&b.db, G1).is_none(), "no record has G1");
    let changed = stored_feed(&b.db, G2).expect("R must have G2");
    assert_eq!(changed.feed_url, F, "R must move to F");
    assert_eq!(feed_count(&b.db), 1, "no new record");
    assert_eq!(stored_track_guids(&b.db, G2), ITEM_GUIDS.to_vec());
    let (code, body) = get_feed(b.app(), G1).await;
    assert_eq!(code, http::StatusCode::NOT_FOUND);
    assert_eq!(body["superseded_by"].as_str(), Some(G2));
    let left: Vec<String> = {
        let conn = b.db.lock().expect("lock database");
        stophammer::db::get_left_urls(&conn, G2).expect("left URLs")
    };
    assert_eq!(left, vec![S.to_string()], "S must be a left URL of G2");
}

/// ADR 0106 criterion 8, with `301`.
#[tokio::test]
async fn a_301_to_a_free_guid_moves_and_changes_the_record() {
    assert_permanent_redirect_moves_and_changes(301).await;
}

/// ADR 0106 criterion 8, with `308`.
#[tokio::test]
async fn a_308_to_a_free_guid_moves_and_changes_the_record() {
    assert_permanent_redirect_moves_and_changes(308).await;
}

/// ADR 0106 criterion 9.
#[tokio::test]
async fn a_301_to_the_url_of_a_different_record_is_record_conflict() {
    let b = Bench::new().await;
    accept(b.app(), F, TOKEN_002, "r2-at-f", &feed_data(G3, "R2")).await;
    let resp = b.redirect(301, &feed_data(G2, "R")).await;
    assert_eq!(resp["accepted"], false, "{resp:?}");
    assert_eq!(resp["reason"], "record_conflict", "{resp:?}");
    assert_eq!(stored_feed(&b.db, G1).expect("R keeps G1").feed_url, S);
    assert_eq!(stored_feed(&b.db, G3).expect("R2 keeps G3").feed_url, F);
    assert!(stored_feed(&b.db, G2).is_none(), "no record has G2");
}

/// ADR 0106 criterion 10.
#[tokio::test]
async fn a_new_feed_url_with_a_free_guid_is_a_new_record() {
    let b = Bench::new().await;
    let mut tagged = feed_data(G1, "R");
    tagged["new_feed_url"] = serde_json::json!(F);
    accept(b.app(), S, TOKEN_002, "r-tags-f", &tagged).await;
    let resp = accept(b.app(), F, TOKEN_002, "g2-at-f", &feed_data(G2, "New")).await;
    assert_eq!(resp["accepted"], true, "{resp:?}");
    assert_eq!(stored_feed(&b.db, G1).expect("R keeps G1").feed_url, S);
    assert_eq!(stored_feed(&b.db, G2).expect("a new record").feed_url, F);
    assert_eq!(feed_count(&b.db), 2);
    assert!(guid_supersession(&b.db, G1).is_none(), "no link from G1");
}

/// Task 002 criterion 5: during an ADR 0083 §6 wait, the GUID changes and the
/// record keeps `S`.
#[tokio::test]
async fn a_redirect_during_a_wait_changes_the_guid_and_keeps_the_source_url() {
    let b = Bench::new().await;
    {
        let conn = b.db.lock().expect("lock database");
        conn.execute(
            "INSERT INTO feed_left_urls (feed_guid, url) VALUES (?1, ?2)",
            rusqlite::params![G1, F],
        )
        .expect("insert left URL");
        conn.execute(
            "INSERT INTO feed_move_state (feed_guid, return_count, last_return_at) \
             VALUES (?1, 1, ?2) ON CONFLICT(feed_guid) DO UPDATE SET \
             return_count = 1, last_return_at = excluded.last_return_at",
            rusqlite::params![G1, stophammer::db::unix_now()],
        )
        .expect("start a wait");
    }
    let resp = b.redirect(301, &feed_data(G2, "R")).await;
    assert_eq!(resp["accepted"], true, "{resp:?}");
    assert!(stored_feed(&b.db, G1).is_none(), "no record has G1");
    assert_eq!(stored_feed(&b.db, G2).expect("R has G2").feed_url, S);
    let warnings = resp["warnings"].as_array().expect("warnings array");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().is_some_and(|w| w.contains("refused"))),
        "the wait must give the warning: {resp:?}"
    );
}

/// Task 002 criterion 6.
#[tokio::test]
async fn a_302_changes_the_guid_and_keeps_the_source_url() {
    let b = Bench::new().await;
    let resp = b.redirect(302, &feed_data(G2, "R")).await;
    assert_eq!(resp["accepted"], true, "{resp:?}");
    assert!(stored_feed(&b.db, G1).is_none(), "no record has G1");
    assert_eq!(stored_feed(&b.db, G2).expect("R has G2").feed_url, S);
}

fn insert_hold(db: &Arc<Mutex<rusqlite::Connection>>, guid: &str, url: &str) {
    let conn = db.lock().expect("lock database");
    conn.execute(
        "INSERT INTO retired_source_holds (feed_guid, source_url, reason, retired_at) \
         VALUES (?1, ?2, 'gone', ?3)",
        rusqlite::params![guid, url, stophammer::db::unix_now()],
    )
    .expect("insert hold");
}

fn hold_count(db: &Arc<Mutex<rusqlite::Connection>>, guid: &str) -> i64 {
    let conn = db.lock().expect("lock database");
    conn.query_row(
        "SELECT COUNT(*) FROM retired_source_holds WHERE feed_guid = ?1",
        [guid],
        |row| row.get(0),
    )
    .expect("count holds")
}

/// ADR 0106 criterion 11.
#[tokio::test]
async fn a_held_guid_for_a_different_url_is_retired_source_conflict() {
    let b = Bench::new().await;
    insert_hold(&b.db, G2, "https://other.example/feed.xml");
    let resp = submit(b.app(), S, TOKEN_002, "g2-held", &feed_data(G2, "R")).await;
    assert_eq!(resp["accepted"], false, "{resp:?}");
    assert_eq!(resp["reason"], "retired_source_conflict", "{resp:?}");
    assert_eq!(stored_feed(&b.db, G1).expect("R keeps G1").feed_url, S);
    assert!(stored_feed(&b.db, G2).is_none(), "no record has G2");
    assert_eq!(hold_count(&b.db, G2), 1, "the hold stays");
}

/// ADR 0106 criterion 12.
#[tokio::test]
async fn a_held_guid_for_the_source_url_changes_and_deletes_the_hold() {
    let b = Bench::new().await;
    insert_hold(&b.db, G2, S);
    let resp = submit(b.app(), S, TOKEN_002, "g2-held-at-s", &feed_data(G2, "R")).await;
    assert_eq!(resp["accepted"], true, "{resp:?}");
    assert!(stored_feed(&b.db, G1).is_none(), "no record has G1");
    assert_eq!(stored_feed(&b.db, G2).expect("R has G2").feed_url, S);
    assert_eq!(hold_count(&b.db, G2), 0, "the admission deletes the hold");
}

/// ADR 0106 criterion 13.
#[tokio::test]
async fn a_guid_change_with_a_source_block_retires_the_record_under_its_stored_guid() {
    let b = Bench::new().await;
    let mut blocked = feed_data(G2, "R");
    blocked["blocks"] = serde_json::json!([{"id": null, "value": "yes"}]);
    let resp = submit(b.app(), S, TOKEN_002, "g2-blocked", &blocked).await;
    assert_eq!(resp["accepted"], false, "{resp:?}");
    assert_eq!(resp["reason"], "source_blocked", "{resp:?}");
    assert!(stored_feed(&b.db, G1).is_none(), "R is retired");
    assert!(stored_feed(&b.db, G2).is_none(), "no record has G2");
    assert_eq!(feed_count(&b.db), 0);
    assert!(guid_supersession(&b.db, G1).is_none(), "no link from G1");
}
