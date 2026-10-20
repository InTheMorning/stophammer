// ADR 0064 task 006: each live row gives its link.
//
// `GET /v1/feeds/{guid}` gives `live_items`, and `GET /v1/live-items` gives
// the rows of every public feed. Each row now also gives `link` and
// `link_scheme_added`, from the stored RSS `<link>` of the live item (ADR
// 0064 section 6, ADR 0054 section 4a). These tests ingest a live item with
// a stored `source_entity_links` row of `entity_type` `live_item`,
// `link_type` `web_page` and `extraction_path` `entity.link`, then read it
// back through both routes.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0064-live-item-link-crawl-token";
const ADMIN: &str = "test-adr0064-live-item-link-admin-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0064-live-item-link-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: ADMIN.into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

/// Posts one ingest and returns its status and JSON body.
async fn post_ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    url: &str,
    feed_extra: &Value,
    version: u32,
) -> (http::StatusCode, Value) {
    let mut feed_data = json!({
        "feed_guid": guid,
        "title": format!("Feed {guid}"),
        "raw_medium": "music",
        "explicit": false,
        "author_name": "Some Artist",
        "tracks": [],
        "live_items": [],
    });
    for (key, value) in feed_extra.as_object().expect("feed_extra is an object") {
        feed_data[key] = value.clone();
    }
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}-{version}"),
        "feed_data": feed_data,
    });
    let req = Request::builder()
        .method("POST")
        .uri("/ingest/feed")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
        .expect("build request");
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(req)
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// Ingests one feed and asserts it was accepted.
async fn ingest_ok(st: &Arc<stophammer::api::AppState>, guid: &str, url: &str, feed_extra: &Value) {
    let (status, body) = post_ingest(st, guid, url, feed_extra, 1).await;
    assert!(
        status.is_success() && body["accepted"] == true,
        "ingest of {guid} must be accepted, got {status}: {body}"
    );
}

/// One `<podcast:liveItem>` for an ingest payload, with its RSS `links`.
fn live_item_with_links(guid: &str, links: &Value) -> Value {
    json!({
        "live_item_guid": guid,
        "title": format!("Live {guid}"),
        "status": "pending",
        "start_at": 1_800_000_000,
        "end_at": null,
        "explicit": false,
        "links": links,
    })
}

/// A `source_entity_links` row for a plain RSS `<link>` of a live item:
/// `link_type` `web_page`, `extraction_path` `entity.link`.
fn web_page_link(url: &str) -> Value {
    json!({
        "position": 0,
        "link_type": "web_page",
        "url": url,
        "extraction_path": "entity.link"
    })
}

/// A `source_entity_links` row for an Atom alternate link of a live item.
/// Its `link_type` is also `web_page`, but its `extraction_path` differs
/// from a plain RSS `<link>`, so it must not give `link`.
fn atom_alternate_link(url: &str) -> Value {
    json!({
        "position": 0,
        "link_type": "web_page",
        "url": url,
        "extraction_path": "entity.atom:link[@rel='alternate']"
    })
}

/// Gets a feed by GUID and returns its JSON body.
async fn get_feed(st: &Arc<stophammer::api::AppState>, guid: &str) -> Value {
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/feeds/{guid}"))
        .body(Body::empty())
        .expect("build request");
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(req)
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("the response is JSON");
    assert!(status.is_success(), "GET /v1/feeds/{guid} failed: {body}");
    body
}

/// One `live_items` row of a feed read, by `live_item_guid`.
fn live_item_row<'a>(feed_body: &'a Value, live_item_guid: &str) -> &'a Value {
    feed_body["data"]["live_items"]
        .as_array()
        .expect("live_items is an array")
        .iter()
        .find(|row| row["live_item_guid"] == live_item_guid)
        .unwrap_or_else(|| panic!("no live_items row {live_item_guid} in {feed_body}"))
}

/// Calls `GET /v1/live-items` with the given raw query string (no leading
/// `?`) and returns its status and JSON body.
async fn get_live_items(
    st: &Arc<stophammer::api::AppState>,
    query: &str,
) -> (http::StatusCode, Value) {
    let uri = if query.is_empty() {
        "/v1/live-items".to_string()
    } else {
        format!("/v1/live-items?{query}")
    };
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("build request");
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(req)
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// One row of `GET /v1/live-items?view=all`, by `feed_guid` and
/// `live_item_guid`.
fn list_row<'a>(body: &'a Value, feed_guid: &str, live_item_guid: &str) -> &'a Value {
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|row| row["feed_guid"] == feed_guid && row["live_item_guid"] == live_item_guid)
        .unwrap_or_else(|| panic!("no row ({feed_guid}, {live_item_guid}) in {body}"))
}

// ---------------------------------------------------------------------------
// Acceptance criterion 1: a stored `<link>` with a scheme reads unchanged,
// with `link_scheme_added` false, in the feed read and in the list route.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_link_with_a_scheme_reads_unchanged_in_the_feed_read_and_the_list_route() {
    let st = state(common::test_db_arc());
    let guid = "feed-live-item-link-with-scheme";
    let live_item_guid = "live-with-scheme";

    ingest_ok(
        &st,
        guid,
        "https://l.example/live-item-link-with-scheme.xml",
        &json!({
            "live_items": [live_item_with_links(
                live_item_guid,
                &json!([web_page_link("https://www.pixelpalace.party/")])
            )]
        }),
    )
    .await;

    let feed = get_feed(&st, guid).await;
    let row = live_item_row(&feed, live_item_guid);
    assert_eq!(
        row["link"],
        json!("https://www.pixelpalace.party/"),
        "the feed read must give the stored link unchanged, got {row}"
    );
    assert_eq!(
        row["link_scheme_added"],
        json!(false),
        "a link that already has a scheme must give link_scheme_added false, got {row}"
    );

    let (status, body) = get_live_items(&st, "view=all").await;
    assert!(status.is_success(), "GET /v1/live-items failed: {body}");
    let list_row = list_row(&body, guid, live_item_guid);
    assert_eq!(
        list_row["link"],
        json!("https://www.pixelpalace.party/"),
        "the list route must give the stored link unchanged, got {list_row}"
    );
    assert_eq!(
        list_row["link_scheme_added"],
        json!(false),
        "the list route must give link_scheme_added false, got {list_row}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance criterion 2: a stored `<link>` with no scheme, that names a
// host name, reads as `https://` and the value, with link_scheme_added true
// (ADR 0054 section 4a).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_link_with_no_scheme_reads_as_https_and_marks_the_scheme_added() {
    let st = state(common::test_db_arc());
    let guid = "feed-live-item-link-bare-host";
    let live_item_guid = "live-bare-host";

    ingest_ok(
        &st,
        guid,
        "https://l.example/live-item-link-bare-host.xml",
        &json!({
            "live_items": [live_item_with_links(
                live_item_guid,
                &json!([web_page_link("www.pixelpalace.party")])
            )]
        }),
    )
    .await;

    let feed = get_feed(&st, guid).await;
    let row = live_item_row(&feed, live_item_guid);
    assert_eq!(
        row["link"],
        json!("https://www.pixelpalace.party"),
        "a bare host name link must read with an added https scheme, got {row}"
    );
    assert_eq!(
        row["link_scheme_added"],
        json!(true),
        "a bare host name link must give link_scheme_added true, got {row}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance criterion 3: a live item with no stored `<link>` gives `link`
// null and `link_scheme_added` false.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_live_item_with_no_link_gives_null_link() {
    let st = state(common::test_db_arc());
    let guid = "feed-live-item-no-link";
    let live_item_guid = "live-no-link";

    ingest_ok(
        &st,
        guid,
        "https://l.example/live-item-no-link.xml",
        &json!({ "live_items": [live_item_with_links(live_item_guid, &json!([]))] }),
    )
    .await;

    let feed = get_feed(&st, guid).await;
    let row = live_item_row(&feed, live_item_guid);
    assert_eq!(
        row["link"],
        Value::Null,
        "a live item with no stored link must give link null, got {row}"
    );
    assert_eq!(
        row["link_scheme_added"],
        json!(false),
        "a live item with no stored link must give link_scheme_added false, got {row}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance criterion 4: a live item with only an Atom link gives `link`
// null. The Atom link has the same link_type as a plain RSS link, but a
// different extraction_path, so it must not supply the value.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_live_item_with_only_an_atom_link_gives_null_link() {
    let st = state(common::test_db_arc());
    let guid = "feed-live-item-atom-only-link";
    let live_item_guid = "live-atom-only";

    ingest_ok(
        &st,
        guid,
        "https://l.example/live-item-atom-only-link.xml",
        &json!({
            "live_items": [live_item_with_links(
                live_item_guid,
                &json!([atom_alternate_link("https://www.pixelpalace.party/atom")])
            )]
        }),
    )
    .await;

    let feed = get_feed(&st, guid).await;
    let row = live_item_row(&feed, live_item_guid);
    assert_eq!(
        row["link"],
        Value::Null,
        "an Atom-only link must not supply link, got {row}"
    );
    assert_eq!(
        row["link_scheme_added"],
        json!(false),
        "an Atom-only link must give link_scheme_added false, got {row}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance criterion 5: two feeds with the same live_item_guid each give
// their own link, in the feed read and in the list route.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_feeds_with_the_same_live_item_guid_each_give_their_own_link() {
    let st = state(common::test_db_arc());
    let shared_guid = "live-shared-guid";
    let feed_a = "feed-live-item-link-shared-a";
    let feed_b = "feed-live-item-link-shared-b";

    ingest_ok(
        &st,
        feed_a,
        "https://l.example/live-item-link-shared-a.xml",
        &json!({
            "live_items": [live_item_with_links(
                shared_guid,
                &json!([web_page_link("https://a.example/stream")])
            )]
        }),
    )
    .await;
    ingest_ok(
        &st,
        feed_b,
        "https://l.example/live-item-link-shared-b.xml",
        &json!({
            "live_items": [live_item_with_links(
                shared_guid,
                &json!([web_page_link("https://b.example/stream")])
            )]
        }),
    )
    .await;

    let body_a = get_feed(&st, feed_a).await;
    let row_a = live_item_row(&body_a, shared_guid);
    assert_eq!(
        row_a["link"],
        json!("https://a.example/stream"),
        "feed a's feed read must give feed a's own link, got {row_a}"
    );

    let body_b = get_feed(&st, feed_b).await;
    let row_b = live_item_row(&body_b, shared_guid);
    assert_eq!(
        row_b["link"],
        json!("https://b.example/stream"),
        "feed b's feed read must give feed b's own link, got {row_b}"
    );

    let (status, body) = get_live_items(&st, "view=all").await;
    assert!(status.is_success(), "GET /v1/live-items failed: {body}");
    let list_row_a = list_row(&body, feed_a, shared_guid);
    assert_eq!(
        list_row_a["link"],
        json!("https://a.example/stream"),
        "the list route must give feed a's own link on feed a's row, got {list_row_a}"
    );
    let list_row_b = list_row(&body, feed_b, shared_guid);
    assert_eq!(
        list_row_b["link"],
        json!("https://b.example/stream"),
        "the list route must give feed b's own link on feed b's row, got {list_row_b}"
    );
}
