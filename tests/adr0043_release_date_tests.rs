// ADR 0043: a feed publication date records its source element.
//
// `lastBuildDate` is the time a feed file was generated. It must never supply
// `release_date`, and a claim must name the element the parser actually read.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state_with_crawl_token(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0043-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec { names: vec![] };
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

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

const OLDEST_ITEM_AT: i64 = 1_672_732_800; // 2023-01-03
const BUILD_TIME: i64 = 1_790_071_200; // 2026-09-22

fn ingest_payload(
    feed_guid: &str,
    crawl_token: &str,
    pub_date: Option<i64>,
    last_build_date: Option<i64>,
) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": format!("https://example.com/{feed_guid}.xml"),
        "source_url": format!("https://example.com/{feed_guid}.xml"),
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": format!("hash-{feed_guid}"),
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "ADR 0043 Album",
            "raw_medium": "music",
            "explicit": false,
            "pub_date": pub_date,
            "last_build_date": last_build_date,
            "tracks": [{
                "track_guid": format!("{feed_guid}-track-01"),
                "title": "Track One",
                "pub_date": OLDEST_ITEM_AT,
                "explicit": false,
                "payment_routes": [],
                "value_time_splits": []
            }]
        }
    })
}

fn claim_path(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str, claim_type: &str) -> String {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT extraction_path FROM source_release_claims \
         WHERE feed_guid = ?1 AND entity_type = 'feed' AND claim_type = ?2",
        rusqlite::params![feed_guid, claim_type],
        |row| row.get::<_, String>(0),
    )
    .unwrap_or_else(|err| panic!("no {claim_type} claim for {feed_guid}: {err}"))
}

async fn get_feed(app: axum::Router, feed_guid: &str) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/feeds/{feed_guid}"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    body_json(resp).await
}

async fn ingest(app: axum::Router, payload: &serde_json::Value) {
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
    assert!(
        resp.status().is_success(),
        "ingest must succeed, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn build_time_never_supplies_the_release_date() {
    let crawl_token = "adr0043-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0043-build-only";

    let payload = ingest_payload(feed_guid, crawl_token, None, Some(BUILD_TIME));
    ingest(stophammer::api::build_router(Arc::clone(&state)), &payload).await;

    assert_eq!(
        claim_path(&db, feed_guid, "release_date"),
        "oldest_item.pub_date",
        "with no pubDate the release_date claim must name the oldest item"
    );
    assert_eq!(
        claim_path(&db, feed_guid, "last_build_date"),
        "feed.last_build_date",
        "lastBuildDate must be kept as its own claim"
    );

    let body = get_feed(stophammer::api::build_router(state), feed_guid).await;
    let feed = &body["data"];
    assert_eq!(
        feed["release_date"].as_i64(),
        Some(OLDEST_ITEM_AT),
        "release_date must come from the oldest item, not the build time"
    );
    assert_eq!(
        feed["last_build_date"].as_i64(),
        Some(BUILD_TIME),
        "the feed must return its build time separately"
    );
}

#[tokio::test]
async fn pub_date_supplies_the_release_date_when_both_are_present() {
    let crawl_token = "adr0043-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);
    let feed_guid = "adr0043-both-dates";
    let channel_pub_date = 1_600_000_000;

    let payload = ingest_payload(
        feed_guid,
        crawl_token,
        Some(channel_pub_date),
        Some(BUILD_TIME),
    );
    ingest(stophammer::api::build_router(Arc::clone(&state)), &payload).await;

    assert_eq!(
        claim_path(&db, feed_guid, "release_date"),
        "feed.pub_date",
        "with a pubDate the claim must name the feed element"
    );

    let body = get_feed(stophammer::api::build_router(state), feed_guid).await;
    assert_eq!(
        body["data"]["release_date"].as_i64(),
        Some(channel_pub_date),
        "release_date must come from pubDate, not from the build time"
    );
}
