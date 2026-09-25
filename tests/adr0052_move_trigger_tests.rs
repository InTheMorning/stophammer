// ADR 0052 sections 1 and 2, task 006: a permanent redirect or a declared
// `itunes:new-feed-url` can move a record, beside the self-link move of task
// 003. One move function backs all three triggers.
//
// This mirrors the fixture style of tests/adr0052_self_link_tests.rs: a
// minimal verifier chain (content_hash only) so a synthetic feed ingests
// without needing the full default chain.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

/// `RA` is the record's original source URL. `RB` is a permanent-redirect
/// target. `RX` is an unrelated intermediate hop.
const RA: &str = "https://redirects.example/feed/a";
const RB: &str = "https://redirects.example/feed/b";
const RX: &str = "https://redirects.example/feed/x-intermediate";

/// `NA` is a record's original source URL for the `new-feed-url` tests. `NN`
/// is the URL its body declares as `itunes:new-feed-url`.
const NA: &str = "https://newfeedurl.example/feed/a";
const NN: &str = "https://newfeedurl.example/feed/n";

const ADMIN_TOKEN: &str = "test-admin-token";

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0052-move-trigger-signer"));
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

fn redirect_hop(url: &str, status: u16) -> serde_json::Value {
    serde_json::json!({ "url": url, "status": status })
}

fn ingest_payload(
    canonical_url: &str,
    source_url: &str,
    crawl_token: &str,
    content_hash: &str,
    feed_data: &serde_json::Value,
    redirects: &[serde_json::Value],
) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": canonical_url,
        "source_url": source_url,
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": content_hash,
        "redirects": redirects,
        "feed_data": feed_data,
    })
}

/// A synthetic music feed with one track. `new_feed_url` becomes the body's
/// channel `itunes:new-feed-url` value when given.
fn feed_data(feed_guid: &str, title: &str, new_feed_url: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "new_feed_url": new_feed_url,
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

fn stored_feed(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> stophammer::model::Feed {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_feed(&conn, feed_guid)
        .expect("query feed")
        .expect("feed row must exist")
}

fn declared_new_feed_url(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> Option<String> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_declared_new_feed_url(&conn, feed_guid)
        .expect("query declared_new_feed_url")
}

// ---------------------------------------------------------------------------
// The permanent-redirect trigger
// ---------------------------------------------------------------------------

/// A record at `RA`. A submission with `source_url` `RA`, `canonical_url`
/// `RB`, one `301` hop and the same GUID moves the record to `RB`, and the
/// warning names `permanent redirect`.
#[tokio::test]
async fn a_permanent_redirect_moves_the_record() {
    let crawl_token = "adr0052-redirect-move-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-redirect-move-guid";

    let data_a = feed_data(feed_guid, "Original Title", None);
    let payload_a = ingest_payload(
        RA,
        RA,
        crawl_token,
        "adr0052-redirect-move-hash-a",
        &data_a,
        &[],
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let data_b = feed_data(feed_guid, "New Title", None);
    let payload_b = ingest_payload(
        RB,
        RA,
        crawl_token,
        "adr0052-redirect-move-hash-b",
        &data_b,
        &[redirect_hop(RA, 301)],
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_b,
    )
    .await;

    assert_eq!(
        resp["accepted"], true,
        "a permanent-redirect submission must be accepted: {resp:?}"
    );
    let warnings = resp["warnings"]
        .as_array()
        .expect("warnings must be an array");
    assert!(
        warnings.iter().any(|w| w.as_str()
            == Some(&format!(
                "moved from {RA} to {RB} (ADR 0052 permanent redirect)"
            ))),
        "the warning must name the permanent redirect trigger: {resp:?}"
    );

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.feed_url, RB,
        "the stored feed_url must become the redirect target"
    );
    assert_eq!(feed.title, "New Title", "the move must apply the new body");
}

/// The same submission with a `302` hop instead of `301`: the content
/// applies, and `feed_url` stays `RA`.
#[tokio::test]
async fn a_302_redirect_hop_does_not_move_the_record() {
    let crawl_token = "adr0052-redirect-302-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-redirect-302-guid";

    let data_a = feed_data(feed_guid, "Original Title", None);
    let payload_a = ingest_payload(
        RA,
        RA,
        crawl_token,
        "adr0052-redirect-302-hash-a",
        &data_a,
        &[],
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let data_b = feed_data(feed_guid, "New Title", None);
    let payload_b = ingest_payload(
        RB,
        RA,
        crawl_token,
        "adr0052-redirect-302-hash-b",
        &data_b,
        &[redirect_hop(RA, 302)],
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_b,
    )
    .await;

    assert_eq!(
        resp["accepted"], true,
        "a 302 hop must still apply the content: {resp:?}"
    );
    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.feed_url, RA,
        "a 302 hop must not move the stored feed_url"
    );
    assert_eq!(feed.title, "New Title", "the content must still apply");
}

/// The same submission with a `301` hop then a `302` hop: the chain is not
/// entirely permanent, so no move happens.
#[tokio::test]
async fn a_redirect_chain_with_one_non_permanent_hop_does_not_move() {
    let crawl_token = "adr0052-redirect-mixed-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-redirect-mixed-guid";

    let data_a = feed_data(feed_guid, "Original Title", None);
    let payload_a = ingest_payload(
        RA,
        RA,
        crawl_token,
        "adr0052-redirect-mixed-hash-a",
        &data_a,
        &[],
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let data_b = feed_data(feed_guid, "New Title", None);
    let payload_b = ingest_payload(
        RB,
        RA,
        crawl_token,
        "adr0052-redirect-mixed-hash-b",
        &data_b,
        &[redirect_hop(RA, 301), redirect_hop(RX, 302)],
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_b,
    )
    .await;

    assert_eq!(
        resp["accepted"], true,
        "a mixed redirect chain must still apply the content: {resp:?}"
    );
    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.feed_url, RA,
        "a mixed redirect chain must not move the stored feed_url"
    );
}

/// The same submission with a `308` hop, when the target is the source URL
/// of a different record: `record_conflict`, and nothing changes.
#[tokio::test]
async fn a_permanent_redirect_to_a_held_url_answers_record_conflict() {
    let crawl_token = "adr0052-redirect-conflict-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-redirect-conflict-guid";
    let other_guid = "adr0052-redirect-conflict-other-guid";

    let data_a = feed_data(feed_guid, "Original Title", None);
    let payload_a = ingest_payload(
        RA,
        RA,
        crawl_token,
        "adr0052-redirect-conflict-hash-a",
        &data_a,
        &[],
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let data_other = feed_data(other_guid, "Other Feed", None);
    let payload_other = ingest_payload(
        RB,
        RB,
        crawl_token,
        "adr0052-redirect-conflict-hash-other",
        &data_other,
        &[],
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_other,
    )
    .await;

    let data_move = feed_data(feed_guid, "Attacker Title", None);
    let payload_move = ingest_payload(
        RB,
        RA,
        crawl_token,
        "adr0052-redirect-conflict-hash-move",
        &data_move,
        &[redirect_hop(RA, 308)],
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_move,
    )
    .await;

    assert_eq!(resp["accepted"], false, "{resp:?}");
    assert_eq!(resp["reason"], "record_conflict", "{resp:?}");

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.feed_url, RA,
        "the record must not move onto a URL held by a different record"
    );
    assert_eq!(
        feed.title, "Original Title",
        "a record_conflict submission must write nothing"
    );

    let other = stored_feed(&db, other_guid);
    assert_eq!(other.feed_url, RB, "the other record must be unaffected");
}

// ---------------------------------------------------------------------------
// The new-feed-url trigger
// ---------------------------------------------------------------------------

/// A record at `NA` whose body declared `new_feed_url` `NN`. A submission
/// from `NN` with the same GUID moves the record to `NN`, and the warning
/// names `new-feed-url`.
#[tokio::test]
async fn a_submission_at_the_declared_new_feed_url_moves_the_record() {
    let crawl_token = "adr0052-newfeedurl-move-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-newfeedurl-move-guid";

    let data_a = feed_data(feed_guid, "Original Title", Some(NN));
    let payload_a = ingest_payload(
        NA,
        NA,
        crawl_token,
        "adr0052-newfeedurl-move-hash-a",
        &data_a,
        &[],
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    assert_eq!(
        declared_new_feed_url(&db, feed_guid).as_deref(),
        Some(NN),
        "a source ingest must record its body's new_feed_url"
    );

    let data_n = feed_data(feed_guid, "New Title", None);
    let payload_n = ingest_payload(
        NN,
        NN,
        crawl_token,
        "adr0052-newfeedurl-move-hash-n",
        &data_n,
        &[],
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_n,
    )
    .await;

    assert_eq!(
        resp["accepted"], true,
        "a submission at the declared new_feed_url must be accepted: {resp:?}"
    );
    let warnings = resp["warnings"]
        .as_array()
        .expect("warnings must be an array");
    assert!(
        warnings.iter().any(
            |w| w.as_str() == Some(&format!("moved from {NA} to {NN} (ADR 0052 new-feed-url)"))
        ),
        "the warning must name the new-feed-url trigger: {resp:?}"
    );

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.feed_url, NN,
        "the stored feed_url must become the declared new_feed_url"
    );
    assert_eq!(feed.title, "New Title", "the move must apply the new body");
}

/// A mirror body that declares `new_feed_url` does not write the column, and
/// a later submission from that URL does not move the record.
#[tokio::test]
async fn a_mirror_bodys_new_feed_url_does_not_write_the_column() {
    let crawl_token = "adr0052-newfeedurl-mirror-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-newfeedurl-mirror-guid";
    let mirror_url = "https://newfeedurl.example/feed/mirror";
    let declared_in_mirror = "https://newfeedurl.example/feed/mirror-target";

    let data_a = feed_data(feed_guid, "Original Title", None);
    let payload_a = ingest_payload(
        NA,
        NA,
        crawl_token,
        "adr0052-newfeedurl-mirror-hash-a",
        &data_a,
        &[],
    );
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;

    let data_mirror = feed_data(feed_guid, "Attacker Title", Some(declared_in_mirror));
    let payload_mirror = ingest_payload(
        mirror_url,
        mirror_url,
        crawl_token,
        "adr0052-newfeedurl-mirror-hash-m",
        &data_mirror,
        &[],
    );
    let resp = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_mirror,
    )
    .await;
    assert_eq!(resp["accepted"], false, "{resp:?}");
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");

    assert_eq!(
        declared_new_feed_url(&db, feed_guid),
        None,
        "a mirror body's new_feed_url must not write declared_new_feed_url"
    );

    let data_again = feed_data(feed_guid, "Attacker Title Again", None);
    let payload_again = ingest_payload(
        declared_in_mirror,
        declared_in_mirror,
        crawl_token,
        "adr0052-newfeedurl-mirror-hash-again",
        &data_again,
        &[],
    );
    let resp_again = ingest_response(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_again,
    )
    .await;
    assert_eq!(resp_again["accepted"], false, "{resp_again:?}");
    assert_eq!(resp_again["reason"], "source_conflict", "{resp_again:?}");

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(
        feed.feed_url, NA,
        "the record must not move through an undeclared new_feed_url"
    );
}

// ---------------------------------------------------------------------------
// Relocation and backward compatibility
// ---------------------------------------------------------------------------

/// An operator relocation clears `declared_new_feed_url` in the same
/// transaction as `feed_url`.
#[tokio::test]
async fn a_relocation_clears_declared_new_feed_url() {
    let crawl_token = "adr0052-relocate-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-relocate-guid";

    let data_a = feed_data(feed_guid, "Original Title", Some(NN));
    let payload_a = ingest_payload(NA, NA, crawl_token, "adr0052-relocate-hash-a", &data_a, &[]);
    ingest(
        stophammer::api::build_router(Arc::clone(&state)),
        &payload_a,
    )
    .await;
    assert_eq!(
        declared_new_feed_url(&db, feed_guid).as_deref(),
        Some(NN),
        "the record must start with a declared new_feed_url"
    );

    let relocated_url = "https://newfeedurl.example/feed/relocated";
    let req = Request::builder()
        .method("PATCH")
        .uri(format!("/v1/feeds/{feed_guid}"))
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", ADMIN_TOKEN)
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "feed_url": relocated_url,
                "reason": "operator relocation for test"
            }))
            .expect("serialize"),
        ))
        .expect("build request");
    let resp = stophammer::api::build_router(Arc::clone(&state))
        .oneshot(req)
        .await
        .expect("send request");
    assert_eq!(
        resp.status(),
        204,
        "the relocation request must succeed: {resp:?}"
    );

    assert_eq!(
        declared_new_feed_url(&db, feed_guid),
        None,
        "a relocation must clear declared_new_feed_url"
    );
}

/// A request with no `redirects` key and no `new_feed_url`, `locked` or
/// `locked_owner` fields is accepted as before, matching an older crawler.
#[tokio::test]
async fn a_request_with_no_redirects_key_and_no_new_fields_is_accepted_as_before() {
    let crawl_token = "adr0052-back-compat-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0052-back-compat-guid";

    let payload = serde_json::json!({
        "canonical_url": RA,
        "source_url": RA,
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "adr0052-back-compat-hash",
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "Legacy Crawler Title",
            "raw_medium": "episodic",
            "explicit": false,
            "tracks": [{
                "track_guid": format!("{feed_guid}-track-01"),
                "title": "Track One",
                "explicit": false
            }]
        }
    });

    let resp = ingest_response(stophammer::api::build_router(state), &payload).await;
    assert_eq!(
        resp["accepted"], true,
        "an older crawler payload with no redirects field must still be accepted: {resp:?}"
    );

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(feed.feed_url, RA);
}
