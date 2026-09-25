// ADR 0052 task 001: a self link a source ingest declares for its own feed
// can move a later mirror submission at that exact URL.
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

/// `UA` is the record's original source URL. `UM` is the URL its body
/// declares as its own `atom:link rel="self"`.
const UA: &str = "https://wavlake.example/feed/a";
const UM: &str = "https://wavlake.example/feed/music/a";

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0052-self-link-signer"));
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

/// A synthetic music feed with one track. `self_link_url` becomes the body's
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

async fn ingest(app: axum::Router, payload: &serde_json::Value) {
    let body = ingest_response(app, payload).await;
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
}

fn declared_self_url(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> Option<String> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_declared_self_url(&conn, feed_guid).expect("query declared_self_url")
}

fn stored_feed(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> stophammer::model::Feed {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_feed(&conn, feed_guid)
        .expect("query feed")
        .expect("feed row must exist")
}

fn feed_upserted_count_for(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT COUNT(*) FROM events WHERE event_type = 'feed_upserted' AND subject_guid = ?1",
        rusqlite::params![feed_guid],
        |row| row.get(0),
    )
    .expect("count feed_upserted events")
}

fn latest_feed_upserted_feed_url(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> String {
    let conn = db.lock().expect("lock db");
    let payload_json: String = conn
        .query_row(
            "SELECT payload_json FROM events \
             WHERE event_type = 'feed_upserted' AND subject_guid = ?1 \
             ORDER BY seq DESC LIMIT 1",
            rusqlite::params![feed_guid],
            |row| row.get(0),
        )
        .expect("query latest feed_upserted event");
    let value: serde_json::Value =
        serde_json::from_str(&payload_json).expect("parse feed_upserted payload");
    value["feed"]["feed_url"]
        .as_str()
        .expect("feed_url present on feed_upserted payload")
        .to_string()
}

/// Inserts a `source_entity_links` row directly, bypassing ingest. This
/// stands in for a self link a mirror body wrote before ADR 0052 existed —
/// evidence the move must not read (ADR 0052 Guards).
fn insert_legacy_self_link_row(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str, url: &str) {
    let conn = db.lock().expect("lock db");
    conn.execute(
        "INSERT INTO source_entity_links \
         (feed_guid, entity_type, entity_id, position, link_type, url, source, \
          extraction_path, observed_at) \
         VALUES (?1, 'feed', ?1, 0, 'self_feed', ?2, 'rss_link', \
          'feed.atom:link[@rel=''self'']', ?3)",
        rusqlite::params![feed_guid, url, stophammer::db::unix_now()],
    )
    .expect("insert legacy source_entity_links row");
}

/// **Record.** A feed admitted at `UA` with a self link `UM` records `UM` as
/// `feeds.declared_self_url`.
#[tokio::test]
async fn a_source_ingest_records_its_declared_self_url() {
    let crawl_token = "adr0052-record-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-record-guid";

    let feed_data = feed_data_with_self_link(feed_guid, "Original Title", Some(UM));
    let payload = ingest_payload(UA, UA, crawl_token, "adr0052-record-hash", &feed_data);
    ingest(stophammer::api::build_router(state), &payload).await;

    assert_eq!(
        declared_self_url(&db, feed_guid).as_deref(),
        Some(UM),
        "a source ingest must record its body's self link"
    );
}

/// **Move.** A submission at the declared self URL, with the same GUID and a
/// new title, moves the record: `accepted: true`, the stored `feed_url`
/// becomes the self URL, the title changes, the warning names both URLs, and
/// one `feed_upserted` event carries the new `feed_url`.
#[tokio::test]
async fn a_submission_at_the_declared_self_url_moves_the_record() {
    let crawl_token = "adr0052-move-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-move-guid";

    let feed_data_a = feed_data_with_self_link(feed_guid, "Original Title", Some(UM));
    let payload_a = ingest_payload(UA, UA, crawl_token, "adr0052-move-hash-a", &feed_data_a);
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let events_before = feed_upserted_count_for(&db, feed_guid);

    let feed_data_m = feed_data_with_self_link(feed_guid, "New Title", None);
    let payload_m = ingest_payload(UM, UM, crawl_token, "adr0052-move-hash-m", &feed_data_m);
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_m,
    )
    .await;

    assert_eq!(
        resp["accepted"], true,
        "a submission at the declared self URL must be accepted: {resp:?}"
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
    assert_eq!(feed.title, "New Title", "the move must apply the new body");

    let events_after = feed_upserted_count_for(&db, feed_guid);
    assert_eq!(
        events_after - events_before,
        1,
        "the move must emit exactly one feed_upserted event"
    );
    assert_eq!(
        latest_feed_upserted_feed_url(&db, feed_guid),
        UM,
        "the feed_upserted event must carry the new feed_url"
    );
}

/// **After the move.** Once the record has moved to `UM`, a submission at
/// the old URL `UA` is a mirror: `source_conflict` with `source_url` `UM`.
#[tokio::test]
async fn a_submission_at_the_old_url_after_a_move_is_a_mirror() {
    let crawl_token = "adr0052-after-move-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-after-move-guid";

    let feed_data_a = feed_data_with_self_link(feed_guid, "Original Title", Some(UM));
    let payload_a = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0052-after-move-hash-a",
        &feed_data_a,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let feed_data_m = feed_data_with_self_link(feed_guid, "New Title", None);
    let payload_m = ingest_payload(
        UM,
        UM,
        crawl_token,
        "adr0052-after-move-hash-m",
        &feed_data_m,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_m,
    )
    .await;

    let feed_data_again = feed_data_with_self_link(feed_guid, "Attacker Title", Some(UM));
    let payload_again = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0052-after-move-hash-again",
        &feed_data_again,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_again,
    )
    .await;

    assert_eq!(
        resp["accepted"], false,
        "a submission at the pre-move URL must be rejected: {resp:?}"
    );
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");
    assert_eq!(resp["source_url"], UM, "{resp:?}");
}

/// **No record, no move.** A feed admitted at `UA` with no self link, then a
/// submission at `UM`: `source_conflict`, and `feed_url` stays `UA`.
#[tokio::test]
async fn a_submission_with_no_declared_self_url_never_moves() {
    let crawl_token = "adr0052-no-record-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-no-record-guid";

    let feed_data_a = feed_data_with_self_link(feed_guid, "Original Title", None);
    let payload_a = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0052-no-record-hash-a",
        &feed_data_a,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let feed_data_m = feed_data_with_self_link(feed_guid, "Attacker Title", None);
    let payload_m = ingest_payload(
        UM,
        UM,
        crawl_token,
        "adr0052-no-record-hash-m",
        &feed_data_m,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_m,
    )
    .await;

    assert_eq!(resp["accepted"], false, "{resp:?}");
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(feed.feed_url, UA, "feed_url must stay the source URL");
}

/// **A different URL.** A self link `UM`, then a submission at a third URL:
/// `source_conflict`.
#[tokio::test]
async fn a_submission_at_an_unrelated_url_never_moves() {
    let crawl_token = "adr0052-different-url-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-different-url-guid";
    let other_url = "https://wavlake.example/feed/other";

    let feed_data_a = feed_data_with_self_link(feed_guid, "Original Title", Some(UM));
    let payload_a = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0052-different-url-hash-a",
        &feed_data_a,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let feed_data_other = feed_data_with_self_link(feed_guid, "Attacker Title", None);
    let payload_other = ingest_payload(
        other_url,
        other_url,
        crawl_token,
        "adr0052-different-url-hash-other",
        &feed_data_other,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_other,
    )
    .await;

    assert_eq!(resp["accepted"], false, "{resp:?}");
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");
}

/// **Old links do not count.** A feed with a `self_feed` row in
/// `source_entity_links` and a null `declared_self_url`, then a submission at
/// that URL: `source_conflict`. Only `feeds.declared_self_url` counts (ADR
/// 0052 Guards).
#[tokio::test]
async fn a_self_feed_row_in_source_entity_links_never_moves() {
    let crawl_token = "adr0052-legacy-link-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-legacy-link-guid";

    let feed_data_a = feed_data_with_self_link(feed_guid, "Original Title", None);
    let payload_a = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0052-legacy-link-hash-a",
        &feed_data_a,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    assert_eq!(
        declared_self_url(&db, feed_guid),
        None,
        "declared_self_url must start null"
    );
    insert_legacy_self_link_row(&db, feed_guid, UM);

    let feed_data_m = feed_data_with_self_link(feed_guid, "Attacker Title", None);
    let payload_m = ingest_payload(
        UM,
        UM,
        crawl_token,
        "adr0052-legacy-link-hash-m",
        &feed_data_m,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_m,
    )
    .await;

    assert_eq!(resp["accepted"], false, "{resp:?}");
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.feed_url, UA,
        "a source_entity_links row alone must not move the record"
    );
}

/// **A mirror body does not write the column.** A mirror submission with a
/// self link leaves `declared_self_url` unchanged.
#[tokio::test]
async fn a_mirror_submission_does_not_write_declared_self_url() {
    let crawl_token = "adr0052-mirror-write-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-mirror-write-guid";
    let url_x = "https://wavlake.example/feed/x";
    let self_link_in_mirror_body = "https://wavlake.example/feed/music/x";

    let feed_data_a = feed_data_with_self_link(feed_guid, "Original Title", Some(UM));
    let payload_a = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0052-mirror-write-hash-a",
        &feed_data_a,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;
    assert_eq!(declared_self_url(&db, feed_guid).as_deref(), Some(UM));

    let feed_data_x =
        feed_data_with_self_link(feed_guid, "Attacker Title", Some(self_link_in_mirror_body));
    let payload_x = ingest_payload(
        url_x,
        url_x,
        crawl_token,
        "adr0052-mirror-write-hash-x",
        &feed_data_x,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_x,
    )
    .await;
    assert_eq!(resp["accepted"], false, "{resp:?}");
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");

    assert_eq!(
        declared_self_url(&db, feed_guid).as_deref(),
        Some(UM),
        "a mirror body's own self link must not overwrite declared_self_url"
    );
}

/// **A move must not stop at `no_change`.** Task 003. A submission from the
/// declared self link, with a hash the crawl cache already holds, still
/// moves the record. The content-hash verifier alone would answer
/// `no_change`; the self-link move takes priority.
#[tokio::test]
async fn a_no_change_hash_at_the_self_link_still_moves_the_record() {
    let crawl_token = "adr0052-t003-move-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-t003-move-guid";
    let cached_hash = "adr0052-t003-cached-hash";

    let feed_data_a = feed_data_with_self_link(feed_guid, "Original Title", Some(UM));
    let payload_a = ingest_payload(UA, UA, crawl_token, "adr0052-t003-hash-a", &feed_data_a);
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    // The node crawl cache already holds this hash for the self-link URL,
    // as it would when the node accepted that URL before ADR 0051.
    {
        let conn = db.lock().expect("lock db");
        stophammer::db::upsert_feed_crawl_cache(&conn, UM, cached_hash, stophammer::db::unix_now())
            .expect("seed feed_crawl_cache");
    }

    let feed_data_m = feed_data_with_self_link(feed_guid, "New Title", None);
    let mut payload_m = ingest_payload(UM, UM, crawl_token, cached_hash, &feed_data_m);
    payload_m["force_reingest"] = serde_json::json!(false);
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_m,
    )
    .await;

    assert_eq!(
        resp["accepted"], true,
        "a self-link move must be accepted even at a cached hash: {resp:?}"
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
        "the stored feed_url must become the self URL, not stay at no_change"
    );
    assert_eq!(feed.title, "New Title", "the move must apply the new body");
}

/// **A plain mirror still gets `no_change`.** Task 003. A mirror submission
/// from a URL that is not the self link, with a hash the crawl cache already
/// holds, still answers `no_change`, and the record does not move.
#[tokio::test]
async fn a_no_change_hash_at_an_unrelated_mirror_url_still_answers_no_change() {
    let crawl_token = "adr0052-t003-nomove-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-t003-nomove-guid";
    let mirror_url = "https://wavlake.example/feed/mirror-c";
    let cached_hash = "adr0052-t003-nomove-cached-hash";

    let feed_data_a = feed_data_with_self_link(feed_guid, "Original Title", Some(UM));
    let payload_a = ingest_payload(
        UA,
        UA,
        crawl_token,
        "adr0052-t003-nomove-hash-a",
        &feed_data_a,
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    {
        let conn = db.lock().expect("lock db");
        stophammer::db::upsert_feed_crawl_cache(
            &conn,
            mirror_url,
            cached_hash,
            stophammer::db::unix_now(),
        )
        .expect("seed feed_crawl_cache");
    }

    let feed_data_c = feed_data_with_self_link(feed_guid, "Attacker Title", None);
    let payload_c = ingest_payload(
        mirror_url,
        mirror_url,
        crawl_token,
        cached_hash,
        &feed_data_c,
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_c,
    )
    .await;

    assert_eq!(resp["accepted"], true, "{resp:?}");
    assert_eq!(
        resp["no_change"], true,
        "a plain mirror at a cached hash must still answer no_change: {resp:?}"
    );

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(feed.feed_url, UA, "a plain mirror must not move the record");
}
