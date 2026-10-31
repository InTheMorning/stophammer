// ADR 0082 §2 and §3, task 002: the node stores the label of a link.
//
// A `funding` link gets the source `podcast_funding`. A change of only a label
// gives a new `SourceEntityLinksReplaced` event.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use stophammer::model::SourceEntityLink;

const TOKEN: &str = "adr0082-funding-link-crawl-token";
const GUID: &str = "feed-adr0082-funding";
const URL: &str = "https://example.com/adr0082/feed.xml";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0082-funding-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "adr0082-funding-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

/// Posts one feed with the given feed-level `links` array.
async fn ingest(st: &Arc<stophammer::api::AppState>, links: Value) {
    let payload = json!({
        "canonical_url": URL,
        "source_url": URL,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{GUID}-{links}"),
        "feed_data": {
            "feed_guid": GUID,
            "title": "Funding Feed",
            "raw_medium": "music",
            "explicit": false,
            "author_name": "Some Artist",
            "links": links,
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
        "ingest failed with {status}: {}",
        String::from_utf8_lossy(&bytes)
    );
}

fn stored_links(db: &Arc<Mutex<rusqlite::Connection>>) -> Vec<SourceEntityLink> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_source_entity_links_for_feed(&conn, GUID).expect("read links")
}

fn replaced_event_count(db: &Arc<Mutex<rusqlite::Connection>>) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT COUNT(*) FROM events WHERE event_type = 'source_entity_links_replaced'",
        [],
        |row| row.get(0),
    )
    .expect("count events")
}

fn link(link_type: &str, url: &str, label: Option<&str>) -> Value {
    json!({
        "position": 0,
        "link_type": link_type,
        "url": url,
        "label": label,
    })
}

/// A JSON link with no `label` reads as `None`. A `None` label is not
/// serialized. A `Some` label is serialized.
#[test]
fn label_serde_keeps_old_payload_bytes() {
    let old = json!({
        "id": null,
        "feed_guid": "f",
        "entity_type": "feed",
        "entity_id": "f",
        "position": 0,
        "link_type": "website",
        "url": "https://example.com",
        "source": "rss_link",
        "extraction_path": "feed.link",
        "observed_at": 1,
    });
    let mut parsed: SourceEntityLink = serde_json::from_value(old).expect("parse old payload");
    assert_eq!(parsed.label, None, "a payload with no label reads as None");

    let without = serde_json::to_value(&parsed).expect("serialize");
    assert!(
        without.get("label").is_none(),
        "a None label must add no label key: {without}"
    );

    parsed.label = Some("Support".into());
    let with = serde_json::to_value(&parsed).expect("serialize");
    assert_eq!(with["label"], "Support", "a Some label must be serialized");
}

/// A `website` link and a `funding` link with a label give two rows.
#[tokio::test]
async fn funding_link_gets_source_and_label() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        json!([
            link("website", "https://example.com/site", None),
            link("funding", "https://example.com/fund", Some("Support us")),
        ]),
    )
    .await;

    let links = stored_links(&db);
    assert_eq!(links.len(), 2, "expected two stored link rows: {links:?}");
    let funding = links
        .iter()
        .find(|l| l.link_type == "funding")
        .expect("funding row");
    assert_eq!(funding.source, "podcast_funding", "funding source");
    assert_eq!(
        funding.label.as_deref(),
        Some("Support us"),
        "funding label"
    );
    let website = links
        .iter()
        .find(|l| l.link_type == "website")
        .expect("website row");
    assert_eq!(website.source, "rss_link", "website source");
    assert_eq!(website.label, None, "website label");
}

/// A second ingest that changes only the funding label adds an event.
#[tokio::test]
async fn label_change_adds_links_replaced_event() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        json!([link("funding", "https://example.com/fund", Some("Old"))]),
    )
    .await;
    let before = replaced_event_count(&db);
    assert!(before >= 1, "first ingest must emit a links event");

    ingest(
        &st,
        json!([link("funding", "https://example.com/fund", Some("New"))]),
    )
    .await;
    let after = replaced_event_count(&db);
    assert_eq!(
        after,
        before + 1,
        "a label change must add one SourceEntityLinksReplaced event"
    );
    let links = stored_links(&db);
    assert_eq!(links[0].label.as_deref(), Some("New"), "new label stored");
}

/// Two `funding` links with the same URL store one row with the first label.
#[tokio::test]
async fn same_url_funding_links_keep_first_label() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        json!([
            link("funding", "https://example.com/fund", Some("First")),
            link("funding", "https://example.com/fund", Some("Second")),
        ]),
    )
    .await;
    let links = stored_links(&db);
    assert_eq!(links.len(), 1, "expected one row: {links:?}");
    assert_eq!(links[0].label.as_deref(), Some("First"), "first label wins");
}
