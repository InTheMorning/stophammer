// ADR 0059 section 6: a copies row and a live row give the summary of the
// indexed feed that `feed_guid` names. Each test goes through the HTTP router.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0059-list-row-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0059-list-row-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0059-list-row-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

/// Ingests one music feed with one track and one live item.
async fn ingest(st: &Arc<stophammer::api::AppState>, guid: &str, image_url: Option<&str>) {
    let url = format!("https://example.com/{guid}.xml");
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": format!("Feed title {guid}"),
            "raw_medium": "music",
            "explicit": false,
            "image_url": image_url,
            "author_name": "Some Artist",
            "tracks": [{
                "track_guid": format!("track-{guid}"),
                "title": "A Track",
                "pub_date": 1_700_000_000,
                "explicit": false,
                "track_number": 1,
                "payment_routes": [],
                "value_time_splits": [],
            }],
            "live_items": [{
                "live_item_guid": format!("live-{guid}"),
                "title": format!("Live item {guid}"),
                "status": "pending",
                "start_at": 1_900_000_000,
                "end_at": 1_900_003_600,
                "explicit": false,
            }],
        }
    });
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    assert!(
        status.is_success(),
        "ingest of {guid} failed with {status}: {}",
        String::from_utf8_lossy(&bytes)
    );
}

/// Adds a copy row of `guid` that has no track, so it differs from the feed.
fn add_copy(db: &Arc<Mutex<rusqlite::Connection>>, guid: &str, copy_image: &str) {
    let conn = db.lock().expect("lock db");
    conn.execute(
        "INSERT INTO feed_copies \
         (feed_guid, url, first_seen, title, item_guids, feed_recipients, \
          track_recipients, summary_digest, image_url) \
         VALUES (?1, 'https://mirror.example.com/copy.xml', 1700000000, 'Copy', \
                 '[]', '[]', '{}', 'digest', ?2)",
        rusqlite::params![guid, copy_image],
    )
    .expect("insert copy");
}

async fn get(st: &Arc<stophammer::api::AppState>, uri: &str) -> Value {
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    assert!(status.is_success(), "GET {uri} failed with {status}");
    serde_json::from_slice(&bytes).expect("parse json")
}

fn row<'a>(body: &'a Value, guid: &str) -> &'a Value {
    body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .find(|r| r["feed_guid"] == guid)
        .unwrap_or_else(|| panic!("no row for {guid}: {body}"))
}

#[tokio::test]
async fn a_copies_row_gives_the_https_image_of_the_indexed_feed_not_of_the_copy() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(&st, "feed-https", Some("https://img.example.com/feed.jpg")).await;
    add_copy(&db, "feed-https", "https://img.example.com/copy.jpg");
    let body = get(&st, "/v1/copies").await;
    assert_eq!(
        row(&body, "feed-https")["feed_image_url"],
        "https://img.example.com/feed.jpg",
        "the copies row must give the image of the indexed feed: {body}"
    );
}

#[tokio::test]
async fn a_copies_row_gives_null_for_a_non_web_image_and_for_no_image() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(&st, "feed-js", Some("javascript:alert(1)")).await;
    ingest(&st, "feed-none", None).await;
    add_copy(&db, "feed-js", "https://img.example.com/copy.jpg");
    add_copy(&db, "feed-none", "https://img.example.com/copy.jpg");
    let body = get(&st, "/v1/copies").await;
    for guid in ["feed-js", "feed-none"] {
        let r = row(&body, guid);
        assert!(
            r.get("feed_image_url").is_some_and(Value::is_null),
            "{guid}: feed_image_url must be present and null: {body}"
        );
    }
}

#[tokio::test]
async fn a_live_row_gives_the_title_and_image_of_its_feed_and_keeps_its_own_title() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(&st, "feed-live", Some("https://img.example.com/live.jpg")).await;
    let body = get(&st, "/v1/live-items?view=all").await;
    let r = row(&body, "feed-live");
    assert_eq!(
        r["feed_title"], "Feed title feed-live",
        "feed_title: {body}"
    );
    assert_eq!(
        r["feed_image_url"], "https://img.example.com/live.jpg",
        "feed_image_url: {body}"
    );
    assert_eq!(
        r["title"], "Live item feed-live",
        "title must stay the title of the live item: {body}"
    );
}

#[tokio::test]
async fn a_live_row_of_a_feed_with_a_non_web_image_gives_null() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(&st, "feed-live-js", Some("javascript:alert(1)")).await;
    let body = get(&st, "/v1/live-items?view=all").await;
    let r = row(&body, "feed-live-js");
    assert!(
        r.get("feed_image_url").is_some_and(Value::is_null),
        "feed_image_url must be present and null: {body}"
    );
}
