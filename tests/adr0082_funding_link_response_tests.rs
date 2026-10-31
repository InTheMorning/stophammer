// ADR 0082 section 4, task 003: a `source_links` row gives `label`.
//
// A `funding` row follows ADR 0054 section 4 only. It never gets the
// scheme rule of ADR 0054 section 4a.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0082-funding-response-crawl-token";
const FEED_GUID: &str = "feed-adr0082-response";
const TRACK_GUID: &str = "track-adr0082-response";
const URL: &str = "https://example.com/adr0082-response/feed.xml";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0082-response-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "adr0082-response-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn send(
    st: &Arc<stophammer::api::AppState>,
    method: &str,
    uri: &str,
    body: Option<&Value>,
) -> Value {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(value) => {
            req = req.header("Content-Type", "application/json");
            Body::from(serde_json::to_vec(value).expect("serialize"))
        }
        None => Body::empty(),
    };
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(req.body(body).expect("build request"))
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert!(
        status.is_success(),
        "{method} {uri} failed with {status}: {json}"
    );
    json
}

fn link(position: i64, link_type: &str, url: &str, label: Option<&str>, path: &str) -> Value {
    json!({
        "position": position,
        "link_type": link_type,
        "url": url,
        "label": label,
        "extraction_path": path,
    })
}

/// One ingest posts four feed links and one track link.
async fn ingest_all(st: &Arc<stophammer::api::AppState>) {
    let payload = json!({
        "canonical_url": URL,
        "source_url": URL,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": "hash-adr0082-response",
        "feed_data": {
            "feed_guid": FEED_GUID,
            "title": "Funding Response Feed",
            "raw_medium": "music",
            "explicit": false,
            "author_name": "Some Artist",
            "links": [
                link(0, "website", "https://example.com", None, "feed.link"),
                link(1, "funding", "https://example.com/donate", Some("Donate"), "feed.podcast:funding"),
                link(2, "funding", "javascript:alert(1)", Some("Bad"), "feed.podcast:funding"),
                link(3, "funding", "www.example.com/donate", Some("No scheme"), "feed.podcast:funding"),
            ],
            "tracks": [{
                "track_guid": TRACK_GUID,
                "title": "A Track",
                "pub_date": 1_700_000_000,
                "duration_secs": 120,
                "explicit": false,
                "links": [
                    link(0, "funding", "https://example.com/track-donate", Some("Tip the track"), "entity.podcast:funding"),
                ],
                "payment_routes": [],
                "value_time_splits": [],
            }],
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

/// The feed route gives `label` on each row, and a `funding` row keeps the
/// plain section 4 rule.
#[tokio::test]
async fn feed_route_gives_label_and_plain_url_rule() {
    let st = state(common::test_db_arc());
    ingest_all(&st).await;

    let feed = send(
        &st,
        "GET",
        &format!("/v1/feeds/{FEED_GUID}?include=source_links"),
        None,
    )
    .await;
    let rows = feed["data"]["source_links"]
        .as_array()
        .expect("source_links array");
    assert_eq!(rows.len(), 4, "four feed rows expected: {rows:?}");

    let by_position = |p: i64| -> &Value {
        rows.iter()
            .find(|r| r["position"] == p)
            .unwrap_or_else(|| panic!("no row at position {p}: {rows:?}"))
    };

    let website = by_position(0);
    assert_eq!(website["label"], Value::Null, "website row: {website}");

    let funding = by_position(1);
    assert_eq!(funding["label"], json!("Donate"), "funding row: {funding}");
    assert_eq!(
        funding["url"],
        json!("https://example.com/donate"),
        "funding row: {funding}"
    );
    assert_eq!(
        funding["source"],
        json!("podcast_funding"),
        "funding row: {funding}"
    );

    let script = by_position(2);
    assert_eq!(script["url"], Value::Null, "javascript row: {script}");
    assert_eq!(script["label"], json!("Bad"), "javascript row: {script}");

    let bare = by_position(3);
    assert_eq!(bare["url"], Value::Null, "no-scheme row: {bare}");
    assert_eq!(
        bare["url_scheme_added"],
        json!(false),
        "no-scheme row: {bare}"
    );
}

/// The track route gives the track `funding` row with its label.
#[tokio::test]
async fn track_route_gives_funding_label() {
    let st = state(common::test_db_arc());
    ingest_all(&st).await;

    let track = send(
        &st,
        "GET",
        &format!("/v1/feeds/{FEED_GUID}/tracks/{TRACK_GUID}?include=source_links"),
        None,
    )
    .await;
    let rows = track["data"]["source_links"]
        .as_array()
        .expect("source_links array");
    let funding = rows
        .iter()
        .find(|r| r["link_type"] == "funding")
        .unwrap_or_else(|| panic!("no funding row in {rows:?}"));
    assert_eq!(
        funding["label"],
        json!("Tip the track"),
        "track funding row: {funding}"
    );
    assert_eq!(
        funding["url"],
        json!("https://example.com/track-donate"),
        "track funding row: {funding}"
    );
}
