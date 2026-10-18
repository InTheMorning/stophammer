// ADR 0054 section 4a, task 005: a link with no scheme reads as `https`.
//
// docs/tasks/adr-0054-task-005-host-name-link.md
//
// A `website` or `web_page` link row whose stored value names a host name
// with no scheme reads as `https://` and the value, and the row gives
// `url_scheme_added`. Each other link row keeps the plain ADR 0054 section 4
// rule and gives `url_scheme_added` false.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0054-host-name-link-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0054-host-name-link-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "adr0054-host-name-link-admin-token".into(),
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

/// Ingests one feed with the given feed-level `links` array, and no track.
async fn ingest_feed_with_links(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    url: &str,
    links: Value,
) {
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": format!("Feed {guid}"),
            "raw_medium": "music",
            "explicit": false,
            "author_name": "Some Artist",
            "links": links,
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

/// Ingests one feed with one track, and the given track-level `links` array.
async fn ingest_track_with_links(
    st: &Arc<stophammer::api::AppState>,
    feed_guid: &str,
    feed_url: &str,
    track_guid: &str,
    links: Value,
) {
    let payload = json!({
        "canonical_url": feed_url,
        "source_url": feed_url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{feed_guid}"),
        "feed_data": {
            "feed_guid": feed_guid,
            "title": format!("Feed {feed_guid}"),
            "raw_medium": "music",
            "explicit": false,
            "author_name": "Some Artist",
            "tracks": [{
                "track_guid": track_guid,
                "title": "A Track",
                "pub_date": 1_700_000_000,
                "duration_secs": 120,
                "explicit": false,
                "links": links,
                "payment_routes": [],
                "value_time_splits": [],
            }],
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

fn link_row<'a>(source_links: &'a [Value], link_type: &str) -> &'a Value {
    source_links
        .iter()
        .find(|row| row["link_type"] == link_type)
        .unwrap_or_else(|| panic!("no {link_type} row in {source_links:?}"))
}

/// Acceptance criteria: a feed read with `include=source_links` of a feed
/// with a `website` link of `www.pixelpalace.party` gives the `website` row
/// with `url` `https://www.pixelpalace.party` and `url_scheme_added` true.
/// The `self_feed` row of the same stored value stays out of scope: it
/// gives `url` null and `url_scheme_added` false.
#[tokio::test]
async fn website_link_with_no_scheme_reads_as_https() {
    let st = state(common::test_db_arc());
    let guid = "feed-adr0054-host-name-link";
    ingest_feed_with_links(
        &st,
        guid,
        "https://a.example/adr0054-host-name-link.xml",
        json!([
            {
                "position": 0,
                "link_type": "self_feed",
                "url": "www.pixelpalace.party",
                "extraction_path": "feed.atom:link[@rel='self']"
            },
            {
                "position": 1,
                "link_type": "website",
                "url": "www.pixelpalace.party",
                "extraction_path": "feed.link"
            }
        ]),
    )
    .await;

    let feed = send(
        &st,
        "GET",
        &format!("/v1/feeds/{guid}?include=source_links"),
        None,
    )
    .await;
    let source_links = feed["data"]["source_links"]
        .as_array()
        .expect("source_links array");

    let website = link_row(source_links, "website");
    assert_eq!(
        website["url"],
        json!("https://www.pixelpalace.party"),
        "website row: {website}"
    );
    assert_eq!(
        website["url_scheme_added"],
        json!(true),
        "website row: {website}"
    );

    let self_feed = link_row(source_links, "self_feed");
    assert_eq!(self_feed["url"], Value::Null, "self_feed row: {self_feed}");
    assert_eq!(
        self_feed["url_scheme_added"],
        json!(false),
        "self_feed row: {self_feed}"
    );
}

/// Acceptance criteria: `strangeloveofmusic.wixsite.com/strangelove` gives
/// the `https://` form and `true`. This runs the rule through a track-level
/// `web_page` link.
#[tokio::test]
async fn web_page_link_with_no_scheme_and_a_path_reads_as_https() {
    let st = state(common::test_db_arc());
    let feed_guid = "feed-adr0054-host-name-link-track";
    let track_guid = "track-adr0054-host-name-link";
    ingest_track_with_links(
        &st,
        feed_guid,
        "https://a.example/adr0054-host-name-link-track.xml",
        track_guid,
        json!([{
            "position": 0,
            "link_type": "web_page",
            "url": "strangeloveofmusic.wixsite.com/strangelove",
            "extraction_path": "entity.link"
        }]),
    )
    .await;

    let track = send(
        &st,
        "GET",
        &format!("/v1/feeds/{feed_guid}/tracks/{track_guid}?include=source_links"),
        None,
    )
    .await;
    let source_links = track["data"]["source_links"]
        .as_array()
        .expect("source_links array");
    let web_page = link_row(source_links, "web_page");
    assert_eq!(
        web_page["url"],
        json!("https://strangeloveofmusic.wixsite.com/strangelove"),
        "web_page row: {web_page}"
    );
    assert_eq!(
        web_page["url_scheme_added"],
        json!(true),
        "web_page row: {web_page}"
    );
}

/// Acceptance criteria: `https://example.com/` gives
/// `(Some("https://example.com/"), false)`. A `website` link that already
/// has a scheme passes through unchanged, and `url_scheme_added` is false.
#[tokio::test]
async fn website_link_with_an_existing_scheme_is_unchanged() {
    let st = state(common::test_db_arc());
    let guid = "feed-adr0054-host-name-link-scheme";
    ingest_feed_with_links(
        &st,
        guid,
        "https://a.example/adr0054-host-name-link-scheme.xml",
        json!([{
            "position": 0,
            "link_type": "website",
            "url": "https://example.com/",
            "extraction_path": "feed.link"
        }]),
    )
    .await;

    let feed = send(
        &st,
        "GET",
        &format!("/v1/feeds/{guid}?include=source_links"),
        None,
    )
    .await;
    let source_links = feed["data"]["source_links"]
        .as_array()
        .expect("source_links array");
    let website = link_row(source_links, "website");
    assert_eq!(
        website["url"],
        json!("https://example.com/"),
        "website row: {website}"
    );
    assert_eq!(
        website["url_scheme_added"],
        json!(false),
        "website row: {website}"
    );
}

/// Acceptance criteria: `The Fraglebay` gives `(None, false)`. A `website`
/// value with no scheme that does not name a host name gives a null `url`.
#[tokio::test]
async fn website_link_that_is_not_a_host_name_gives_a_null_url() {
    let st = state(common::test_db_arc());
    let guid = "feed-adr0054-host-name-link-not-a-host";
    ingest_feed_with_links(
        &st,
        guid,
        "https://a.example/adr0054-host-name-link-not-a-host.xml",
        json!([{
            "position": 0,
            "link_type": "website",
            "url": "The Fraglebay",
            "extraction_path": "feed.link"
        }]),
    )
    .await;

    let feed = send(
        &st,
        "GET",
        &format!("/v1/feeds/{guid}?include=source_links"),
        None,
    )
    .await;
    let source_links = feed["data"]["source_links"]
        .as_array()
        .expect("source_links array");
    let website = link_row(source_links, "website");
    assert_eq!(website["url"], Value::Null, "website row: {website}");
    assert_eq!(
        website["url_scheme_added"],
        json!(false),
        "website row: {website}"
    );
}
