// ADR 0058 task 002: each mirror submission on the primary records a summary
// row of feed_copies, with a limit of 20 rows for each GUID. A new or
// changed summary signs one FeedCopyObserved event.
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

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0058-ingest-copy-signer"));
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

/// A synthetic feed with one track and one feed-level payment route. The
/// route carries `custom_key`/`custom_value` so a test can change one
/// recipient's identity without changing the track set.
fn feed_data_with_recipient(
    feed_guid: &str,
    title: &str,
    track_guid: &str,
    route_address: &str,
    custom_key: Option<&str>,
    custom_value: Option<&str>,
    last_build_date: Option<i64>,
) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "last_build_date": last_build_date,
        "feed_payment_routes": [{
            "recipient_name": null,
            "route_type": "keysend",
            "address": route_address,
            "custom_key": custom_key,
            "custom_value": custom_value,
            "split": 100,
            "fee": false
        }],
        "tracks": [{
            "track_guid": track_guid,
            "title": "Track One",
            "explicit": false
        }]
    })
}

/// A synthetic feed with a self link, matching the fixture of
/// `tests/adr0052_self_link_tests.rs`. `self_link_url` becomes the body's
/// `self_feed` link when given.
fn feed_data_with_self_link(
    feed_guid: &str,
    title: &str,
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
    serde_json::json!({
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

/// Ingests `feed_data` at `url`, used as both `canonical_url` and
/// `source_url`, and asserts it is accepted. Seeds the source record of a
/// GUID.
async fn accept_source(
    state: &Arc<stophammer::api::AppState>,
    url: &str,
    crawl_token: &str,
    content_hash: &str,
    feed_data: &serde_json::Value,
) {
    let payload = ingest_payload(url, url, crawl_token, content_hash, feed_data);
    let body = ingest_response(stophammer::api::build_router(Arc::clone(state)), &payload).await;
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
}

/// Submits `feed_data` at `url`, used as both `canonical_url` and
/// `source_url`, and returns the raw response. Used for a mirror
/// submission, which the node never applies as an update.
async fn submit_mirror(
    state: &Arc<stophammer::api::AppState>,
    url: &str,
    crawl_token: &str,
    content_hash: &str,
    feed_data: &serde_json::Value,
) -> serde_json::Value {
    let payload = ingest_payload(url, url, crawl_token, content_hash, feed_data);
    ingest_response(stophammer::api::build_router(Arc::clone(state)), &payload).await
}

fn event_ids_from(resp: &serde_json::Value) -> Vec<String> {
    resp["events_emitted"]
        .as_array()
        .expect("events_emitted must be an array")
        .iter()
        .map(|v| v.as_str().expect("event id must be a string").to_string())
        .collect()
}

fn event_type_of(db: &Arc<Mutex<rusqlite::Connection>>, event_id: &str) -> String {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT event_type FROM events WHERE event_id = ?1",
        rusqlite::params![event_id],
        |row| row.get(0),
    )
    .expect("query event_type")
}

/// Counts how many of `event_ids` are a `feed_copy_observed` event.
fn feed_copy_observed_count(db: &Arc<Mutex<rusqlite::Connection>>, event_ids: &[String]) -> usize {
    event_ids
        .iter()
        .filter(|id| event_type_of(db, id) == "feed_copy_observed")
        .count()
}

fn feed_copy_row(
    db: &Arc<Mutex<rusqlite::Connection>>,
    feed_guid: &str,
    url: &str,
) -> Option<stophammer::db::FeedCopyRow> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_feed_copy(&conn, feed_guid, url).expect("query feed_copies")
}

fn copy_overflow(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> i64 {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_copy_overflow(&conn, feed_guid).expect("query feed_copy_overflow")
}

fn count_feed_copies(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> i64 {
    let conn = db.lock().expect("lock db");
    stophammer::db::count_feed_copies(&conn, feed_guid).expect("count feed_copies")
}

// ---------------------------------------------------------------------------
// 1. A mirror body from a different URL records a copy and answers
//    source_conflict, with one FeedCopyObserved event.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mirror_body_from_a_different_url_records_a_copy_and_answers_source_conflict() {
    let crawl_token = "adr0058-mirror-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0058-mirror-guid";
    let url_a = "https://example.com/adr0058-mirror/a.xml";
    let url_b = "https://example.com/adr0058-mirror/b.xml";

    let feed_data_a = feed_data_with_recipient(
        feed_guid,
        "Victim Feed",
        "track-a",
        "victim@ln.example",
        None,
        None,
        None,
    );
    accept_source(
        &state,
        url_a,
        crawl_token,
        "adr0058-mirror-hash-a",
        &feed_data_a,
    )
    .await;

    let feed_data_b = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "attacker@ln.example",
        None,
        None,
        None,
    );
    let resp = submit_mirror(
        &state,
        url_b,
        crawl_token,
        "adr0058-mirror-hash-b",
        &feed_data_b,
    )
    .await;

    assert_eq!(
        resp["accepted"], false,
        "a mirror must not be accepted: {resp:?}"
    );
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");

    let event_ids = event_ids_from(&resp);
    assert_eq!(
        feed_copy_observed_count(&db, &event_ids),
        1,
        "a new copy summary must sign exactly one FeedCopyObserved event: {resp:?}"
    );

    let row = feed_copy_row(&db, feed_guid, url_b).expect("the row of url_b must exist");
    assert_eq!(row.feed_recipients.len(), 1, "one feed-level recipient");
    assert_eq!(row.feed_recipients[0].address, "attacker@ln.example");
    assert_eq!(row.item_guids, vec!["track-a".to_string()]);
}

// ---------------------------------------------------------------------------
// 2. The same summary submitted again, with a different content_hash and a
//    different last_build_date, signs no new event and touches last_seen.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_repeated_mirror_summary_signs_no_new_event_and_touches_last_seen() {
    let crawl_token = "adr0058-repeat-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0058-repeat-guid";
    let url_a = "https://example.com/adr0058-repeat/a.xml";
    let url_b = "https://example.com/adr0058-repeat/b.xml";

    let feed_data_a = feed_data_with_recipient(
        feed_guid,
        "Victim Feed",
        "track-a",
        "victim@ln.example",
        None,
        None,
        None,
    );
    accept_source(
        &state,
        url_a,
        crawl_token,
        "adr0058-repeat-hash-a",
        &feed_data_a,
    )
    .await;

    let feed_data_b1 = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "attacker@ln.example",
        None,
        None,
        Some(1_700_000_000),
    );
    let resp_b1 = submit_mirror(
        &state,
        url_b,
        crawl_token,
        "adr0058-repeat-hash-b1",
        &feed_data_b1,
    )
    .await;
    assert_eq!(
        feed_copy_observed_count(&db, &event_ids_from(&resp_b1)),
        1,
        "the first observation of url_b must sign one event: {resp_b1:?}"
    );

    let row_after_first =
        feed_copy_row(&db, feed_guid, url_b).expect("row must exist after first mirror");
    let digest_after_first = row_after_first.summary_digest.clone();
    let first_seen = row_after_first.first_seen;
    let last_seen_after_first = row_after_first.last_seen;
    assert!(
        last_seen_after_first.is_some(),
        "a new row must get last_seen at insert time"
    );

    // Let the clock advance past unix_now()'s one-second resolution so a
    // later touch of last_seen gives a different value.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let feed_data_b2 = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "attacker@ln.example",
        None,
        None,
        Some(1_700_000_500),
    );
    let resp_b2 = submit_mirror(
        &state,
        url_b,
        crawl_token,
        "adr0058-repeat-hash-b2",
        &feed_data_b2,
    )
    .await;

    assert_eq!(
        feed_copy_observed_count(&db, &event_ids_from(&resp_b2)),
        0,
        "a repeated summary with a different hash and last_build_date must sign no new event: {resp_b2:?}"
    );

    let row_after_second = feed_copy_row(&db, feed_guid, url_b).expect("row must still exist");
    assert_eq!(
        row_after_second.summary_digest, digest_after_first,
        "the digest must not change when the summary is unchanged"
    );
    assert_eq!(
        row_after_second.first_seen, first_seen,
        "first_seen must not move on a repeated summary"
    );
    assert!(
        row_after_second.last_seen.expect("last_seen must be set")
            > last_seen_after_first.expect("last_seen must be set"),
        "last_seen must advance on a repeated submission"
    );
}

// ---------------------------------------------------------------------------
// 3. A changed custom_value signs one new event and gives a new digest.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_changed_custom_value_signs_a_new_event_with_a_new_digest() {
    let crawl_token = "adr0058-changed-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0058-changed-guid";
    let url_a = "https://example.com/adr0058-changed/a.xml";
    let url_b = "https://example.com/adr0058-changed/b.xml";

    let feed_data_a = feed_data_with_recipient(
        feed_guid,
        "Victim Feed",
        "track-a",
        "victim@ln.example",
        None,
        None,
        None,
    );
    accept_source(
        &state,
        url_a,
        crawl_token,
        "adr0058-changed-hash-a",
        &feed_data_a,
    )
    .await;

    let feed_data_b1 = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "03aaaa",
        Some("696969"),
        Some("aaa111"),
        None,
    );
    submit_mirror(
        &state,
        url_b,
        crawl_token,
        "adr0058-changed-hash-b1",
        &feed_data_b1,
    )
    .await;
    let digest_before = feed_copy_row(&db, feed_guid, url_b)
        .expect("row must exist")
        .summary_digest;

    let feed_data_b2 = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "03aaaa",
        Some("696969"),
        Some("bbb222"),
        None,
    );
    let resp_b2 = submit_mirror(
        &state,
        url_b,
        crawl_token,
        "adr0058-changed-hash-b2",
        &feed_data_b2,
    )
    .await;

    assert_eq!(
        feed_copy_observed_count(&db, &event_ids_from(&resp_b2)),
        1,
        "a changed custom_value must sign one new FeedCopyObserved event: {resp_b2:?}"
    );

    let row_after = feed_copy_row(&db, feed_guid, url_b).expect("row must exist");
    assert_ne!(
        row_after.summary_digest, digest_before,
        "a changed custom_value must change the digest"
    );
    assert_eq!(
        row_after.feed_recipients[0].custom_value.as_deref(),
        Some("bbb222")
    );
}

// ---------------------------------------------------------------------------
// 4. The NoChange path also records a copy summary.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_no_change_path_records_the_same_shape_of_copy_row() {
    let crawl_token = "adr0058-no-change-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0058-no-change-guid";
    let url_a = "https://example.com/adr0058-no-change/a.xml";
    let url_b = "https://example.com/adr0058-no-change/b.xml";
    let cached_hash = "adr0058-no-change-hash-b";

    let feed_data_a = feed_data_with_recipient(
        feed_guid,
        "Victim Feed",
        "track-a",
        "victim@ln.example",
        None,
        None,
        None,
    );
    accept_source(
        &state,
        url_a,
        crawl_token,
        "adr0058-no-change-hash-a",
        &feed_data_a,
    )
    .await;

    // Seed the crawl cache for url_b directly. This stands in for an accept
    // that happened before ADR 0051 existed — a mirror answer today never
    // writes this cache (ADR 0058 phase plan, Risk Areas).
    {
        let conn = db.lock().expect("lock db");
        stophammer::db::upsert_feed_crawl_cache(
            &conn,
            url_b,
            cached_hash,
            stophammer::db::unix_now(),
        )
        .expect("seed crawl cache");
    }

    let feed_data_b = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "cached-mirror@ln.example",
        None,
        None,
        None,
    );
    let resp_b = submit_mirror(&state, url_b, crawl_token, cached_hash, &feed_data_b).await;

    assert_eq!(
        resp_b["accepted"], true,
        "a cache-matched mirror must still answer accepted: true: {resp_b:?}"
    );
    assert_eq!(
        resp_b["no_change"], true,
        "a cache-matched submission must answer no_change: true: {resp_b:?}"
    );

    let event_ids = event_ids_from(&resp_b);
    assert_eq!(
        feed_copy_observed_count(&db, &event_ids),
        1,
        "the NoChange branch must sign one FeedCopyObserved event too: {resp_b:?}"
    );

    let row = feed_copy_row(&db, feed_guid, url_b).expect("the row of url_b must exist");
    assert_eq!(row.feed_recipients.len(), 1, "one feed-level recipient");
    assert_eq!(row.feed_recipients[0].address, "cached-mirror@ln.example");
    assert_eq!(row.item_guids, vec!["track-a".to_string()]);
}

// ---------------------------------------------------------------------------
// 5. 20 URLs for one GUID make 20 rows; the 21st makes no row and no event,
//    and the overflow count is 1.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_21st_url_for_one_guid_makes_no_row_and_increments_overflow() {
    let crawl_token = "adr0058-overflow-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0058-overflow-guid";
    let url_a = "https://example.com/adr0058-overflow/source.xml";

    let feed_data_a = feed_data_with_recipient(
        feed_guid,
        "Victim Feed",
        "track-a",
        "victim@ln.example",
        None,
        None,
        None,
    );
    accept_source(
        &state,
        url_a,
        crawl_token,
        "adr0058-overflow-hash-a",
        &feed_data_a,
    )
    .await;

    let mirror_data = feed_data_with_recipient(
        feed_guid,
        "Mirror Feed",
        "track-a",
        "mirror@ln.example",
        None,
        None,
        None,
    );

    let max_rows = usize::try_from(stophammer::db::MAX_COPIES_PER_GUID)
        .expect("MAX_COPIES_PER_GUID fits usize");
    for i in 0..max_rows {
        let url = format!("https://example.com/adr0058-overflow/mirror-{i}.xml");
        let resp = submit_mirror(
            &state,
            &url,
            crawl_token,
            &format!("adr0058-overflow-hash-{i}"),
            &mirror_data,
        )
        .await;
        assert_eq!(
            resp["reason"], "source_conflict",
            "mirror {i} must be a source_conflict: {resp:?}"
        );
    }

    assert_eq!(
        count_feed_copies(&db, feed_guid),
        stophammer::db::MAX_COPIES_PER_GUID,
        "20 different mirror urls must make 20 rows"
    );
    assert_eq!(
        copy_overflow(&db, feed_guid),
        0,
        "no overflow before the 21st url"
    );

    let overflow_url = "https://example.com/adr0058-overflow/mirror-overflow.xml";
    let resp_overflow = submit_mirror(
        &state,
        overflow_url,
        crawl_token,
        "adr0058-overflow-hash-overflow",
        &mirror_data,
    )
    .await;
    assert_eq!(
        resp_overflow["reason"], "source_conflict",
        "{resp_overflow:?}"
    );
    assert_eq!(
        feed_copy_observed_count(&db, &event_ids_from(&resp_overflow)),
        0,
        "the 21st url must sign no FeedCopyObserved event: {resp_overflow:?}"
    );
    assert!(
        feed_copy_row(&db, feed_guid, overflow_url).is_none(),
        "the 21st url must get no feed_copies row"
    );
    assert_eq!(
        count_feed_copies(&db, feed_guid),
        stophammer::db::MAX_COPIES_PER_GUID,
        "the row count must stay at the limit"
    );
    assert_eq!(
        copy_overflow(&db, feed_guid),
        1,
        "the 21st url must add one to the overflow counter"
    );
}

// ---------------------------------------------------------------------------
// 6. A self-link move of ADR 0052 makes no row.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_self_link_move_makes_no_copy_row() {
    let crawl_token = "adr0058-move-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0058-move-guid";
    let url_a = "https://wavlake.example/adr0058-move/a.xml";
    let url_m = "https://wavlake.example/adr0058-move/music/a.xml";

    let feed_data_a = feed_data_with_self_link(feed_guid, "Original Title", Some(url_m));
    accept_source(
        &state,
        url_a,
        crawl_token,
        "adr0058-move-hash-a",
        &feed_data_a,
    )
    .await;

    let feed_data_m = feed_data_with_self_link(feed_guid, "New Title", None);
    let resp_m = submit_mirror(
        &state,
        url_m,
        crawl_token,
        "adr0058-move-hash-m",
        &feed_data_m,
    )
    .await;

    assert_eq!(
        resp_m["accepted"], true,
        "a submission at the declared self URL must move the record: {resp_m:?}"
    );
    assert!(
        feed_copy_row(&db, feed_guid, url_m).is_none(),
        "a move is not a copy: url_m must get no feed_copies row"
    );
}

// ---------------------------------------------------------------------------
// 7. Replication: applying the FeedCopyObserved events of this test's
//    sequence to a second database gives the same rows, with last_seen null.
// ---------------------------------------------------------------------------

/// Ingests a source feed at `url_a`, then a mirror at `url_b` twice, the
/// second time with a changed recipient. Returns the primary database, with
/// two `FeedCopyObserved` events on the log for `url_b`.
async fn seed_replicated_copy_pair(crawl_token: &str) -> Arc<Mutex<rusqlite::Connection>> {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0058-replica-guid";
    let url_a = "https://example.com/adr0058-replica/a.xml";
    let url_b = "https://example.com/adr0058-replica/b.xml";

    let feed_data_a = feed_data_with_recipient(
        feed_guid,
        "Victim Feed",
        "track-a",
        "victim@ln.example",
        None,
        None,
        None,
    );
    accept_source(
        &state,
        url_a,
        crawl_token,
        "adr0058-replica-hash-a",
        &feed_data_a,
    )
    .await;

    let feed_data_b1 = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "attacker@ln.example",
        None,
        None,
        None,
    );
    submit_mirror(
        &state,
        url_b,
        crawl_token,
        "adr0058-replica-hash-b1",
        &feed_data_b1,
    )
    .await;

    // A second, changed body at url_b signs a second FeedCopyObserved event
    // for the same pair.
    let feed_data_b2 = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "attacker-2@ln.example",
        None,
        None,
        None,
    );
    submit_mirror(
        &state,
        url_b,
        crawl_token,
        "adr0058-replica-hash-b2",
        &feed_data_b2,
    )
    .await;

    db
}

#[tokio::test]
async fn feed_copies_replicate_identically_from_the_event_log() {
    let crawl_token = "adr0058-replica-token";
    let feed_guid = "adr0058-replica-guid";
    let url_b = "https://example.com/adr0058-replica/b.xml";
    let db1 = seed_replicated_copy_pair(crawl_token).await;

    let copy_events: Vec<stophammer::event::Event> = {
        let conn = db1.lock().expect("lock db1");
        stophammer::db::get_events_since(&conn, 0, 1000)
            .expect("read events")
            .into_iter()
            .filter(|ev| {
                matches!(
                    ev.event_type,
                    stophammer::event::EventType::FeedCopyObserved
                )
            })
            .collect()
    };
    assert_eq!(
        copy_events.len(),
        2,
        "the two-mirror-body sequence must sign exactly two FeedCopyObserved events, got {copy_events:?}"
    );

    let db2 = common::test_db_arc();
    let pool2 = common::wrap_pool(Arc::clone(&db2));
    for ev in &copy_events {
        let result = stophammer::apply::apply_single_event(&pool2, ev);
        assert!(
            result.is_ok(),
            "apply on the second database should succeed: {result:?}"
        );
    }

    let row1 = feed_copy_row(&db1, feed_guid, url_b).expect("row must exist on the primary");
    let row2 = feed_copy_row(&db2, feed_guid, url_b).expect("row must exist on the replica");

    assert_eq!(row1.feed_guid, row2.feed_guid);
    assert_eq!(row1.url, row2.url);
    assert_eq!(row1.first_seen, row2.first_seen);
    assert_eq!(row1.title, row2.title);
    assert_eq!(row1.item_guids, row2.item_guids);
    assert_eq!(row1.feed_recipients, row2.feed_recipients);
    assert_eq!(row1.track_recipients, row2.track_recipients);
    assert_eq!(row1.summary_digest, row2.summary_digest);

    assert!(
        row1.last_seen.is_some(),
        "the primary must have a last_seen"
    );
    assert_eq!(
        row2.last_seen, None,
        "an applied FeedCopyObserved must never write last_seen on a replica"
    );
}
