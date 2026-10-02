// ADR 0068 section 4: the list keeps only the rows with a link fact.
//
// musicindex.org request 10. With `medium=publisher`, `GET /v1/feeds/recent`
// takes `stated_rel=<value>` and `two_way_links=none`. A filter gives the
// link facts on each row. One request examines at most
// `LINK_FACT_FILTER_SCAN_MAX` rows, and then gives a cursor.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0068-crawl-token";
const PUBLISHER_GUID: &str = "p-link-facts";
const PUBLISHER_URL: &str = "https://p.example/link-facts.xml";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0068-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0068-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
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

async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    url: &str,
    medium: &str,
    author: &str,
    remote_items: Value,
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
            "raw_medium": medium,
            "explicit": false,
            "author_name": author,
            "remote_items": remote_items,
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

fn album_entry(position: i64, guid: &str, rel: Option<&str>) -> Value {
    let mut item = json!({
        "position": position,
        "medium": "music",
        "remote_feed_guid": guid,
        "remote_feed_url": format!("https://a.example/{guid}.xml"),
    });
    if let Some(rel) = rel {
        item["rel"] = json!(rel);
    }
    item
}

/// Stores a publisher that lists three albums. Two albums name it back, so
/// two links are two-way. Only one two-way link states `rel="label"`. The
/// third album names no publisher, so its link is one-way.
async fn stored_publisher() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    let back = json!([{
        "position": 0,
        "medium": "publisher",
        "remote_feed_guid": PUBLISHER_GUID,
        "remote_feed_url": PUBLISHER_URL,
        "publisher_reference": true,
    }]);
    for (guid, author) in [("album-a", "Artist A"), ("album-b", "Artist B")] {
        let url = format!("https://a.example/{guid}.xml");
        ingest(&st, guid, &url, "music", author, back.clone()).await;
    }
    ingest(
        &st,
        "album-c",
        "https://a.example/album-c.xml",
        "music",
        "Artist C",
        json!([]),
    )
    .await;
    ingest(
        &st,
        PUBLISHER_GUID,
        PUBLISHER_URL,
        "publisher",
        "Some Label",
        json!([
            album_entry(0, "album-a", Some("label")),
            album_entry(1, "album-b", None),
            album_entry(2, "album-c", Some("producer")),
        ]),
    )
    .await;
    st
}

/// A publisher that lists one album, which does not name it back. Its one
/// link is one-way.
async fn store_one_way_publisher(st: &Arc<stophammer::api::AppState>, guid: &str) {
    ingest(
        st,
        guid,
        &format!("https://p.example/{guid}.xml"),
        "publisher",
        "One Way",
        json!([album_entry(0, "album-c", Some("label"))]),
    )
    .await;
}

async fn status_of(st: &Arc<stophammer::api::AppState>, uri: &str) -> http::StatusCode {
    stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request")
        .status()
}

fn guids(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .map(|row| row["feed_guid"].as_str().expect("feed_guid").to_string())
        .collect()
}

#[tokio::test]
async fn stated_rel_keeps_only_the_rows_that_state_it_with_their_facts() {
    let st = stored_publisher().await;
    store_one_way_publisher(&st, "p-one-way").await;

    let body = send(
        &st,
        "GET",
        "/v1/feeds/recent?medium=publisher&stated_rel=label",
        None,
    )
    .await;
    assert_eq!(guids(&body), vec![PUBLISHER_GUID.to_string()], "{body}");
    let row = &body["data"][0];
    assert_eq!(
        row["two_way_link_count"], 2,
        "the filter gives the facts: {row}"
    );
    assert_eq!(row["stated_rels"], json!(["label"]), "{row}");
    assert_eq!(body["pagination"]["has_more"], false, "{body}");

    let none = send(
        &st,
        "GET",
        "/v1/feeds/recent?medium=publisher&stated_rel=producer",
        None,
    )
    .await;
    assert!(
        guids(&none).is_empty(),
        "producer is stated only on a one-way link, so no row keeps it: {none}"
    );
}

#[tokio::test]
async fn two_way_links_none_keeps_only_the_rows_with_no_two_way_link() {
    let st = stored_publisher().await;
    store_one_way_publisher(&st, "p-one-way").await;

    let body = send(
        &st,
        "GET",
        "/v1/feeds/recent?medium=publisher&two_way_links=none",
        None,
    )
    .await;
    assert_eq!(guids(&body), vec!["p-one-way".to_string()], "{body}");
    assert_eq!(body["data"][0]["two_way_link_count"], 0, "{body}");

    let both = send(
        &st,
        "GET",
        "/v1/feeds/recent?medium=publisher&two_way_links=none&stated_rel=label",
        None,
    )
    .await;
    assert!(
        guids(&both).is_empty(),
        "no row agrees with both filters: {both}"
    );
}

#[tokio::test]
async fn a_filter_with_a_wrong_value_or_medium_gives_400() {
    let st = stored_publisher().await;
    for uri in [
        "/v1/feeds/recent?stated_rel=label",
        "/v1/feeds/recent?medium=music&two_way_links=none",
        "/v1/feeds/recent?medium=publisher&two_way_links=some",
        "/v1/feeds/recent?medium=publisher&stated_rel=",
    ] {
        assert_eq!(
            status_of(&st, uri).await,
            http::StatusCode::BAD_REQUEST,
            "{uri} must give 400"
        );
    }
}

#[tokio::test]
async fn a_filtered_request_stops_at_the_scan_limit_and_gives_a_cursor() {
    let st = stored_publisher().await;
    // Rows sort by feed GUID, descending, when no row has a dated item. Each
    // filler sorts before the label publisher `p-link-facts`, and none states
    // a rel.
    let fillers = stophammer::query::LINK_FACT_FILTER_SCAN_MAX;
    for n in 0..fillers {
        let guid = format!("q-filler-{n:05}");
        ingest(
            &st,
            &guid,
            &format!("https://p.example/{guid}.xml"),
            "publisher",
            "Filler",
            json!([]),
        )
        .await;
    }

    let first = send(
        &st,
        "GET",
        "/v1/feeds/recent?medium=publisher&stated_rel=label&limit=10",
        None,
    )
    .await;
    assert!(
        guids(&first).is_empty(),
        "the first request examines only the fillers: {:?}",
        guids(&first)
    );
    assert_eq!(first["pagination"]["has_more"], true);
    let cursor = first["pagination"]["cursor"]
        .as_str()
        .expect("a request at the scan limit gives a cursor");

    let second = send(
        &st,
        "GET",
        &format!("/v1/feeds/recent?medium=publisher&stated_rel=label&limit=10&cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(guids(&second), vec![PUBLISHER_GUID.to_string()], "{second}");
    assert_eq!(second["pagination"]["has_more"], false, "{second}");
}
