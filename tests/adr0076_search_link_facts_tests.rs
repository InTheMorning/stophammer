// ADR 0076 §3: the link facts of a publisher search row.
//
// With `include=link_facts`, each `publisher` row of `GET /v1/search` gives
// `two_way_link_count`, `stated_rels`, `agreed_roles`,
// `confirmed_release_artists` and `first_named_count`. The node calculates
// each value with the functions of a list row
// (`GET /v1/feeds/by-name?medium=publisher&include=link_facts`). A `feed`
// row and a `track` row give none of them. Without the include, a
// `publisher` row gives none of them.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0076-search-link-facts-crawl-token";
const FACT_FIELDS: [&str; 5] = [
    "two_way_link_count",
    "stated_rels",
    "agreed_roles",
    "confirmed_release_artists",
    "first_named_count",
];

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0076-search-link-facts-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0076-search-link-facts-admin-token".into(),
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

async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    medium: &str,
    title: &str,
    items: Value,
) {
    let url = format!("https://example.com/{guid}.xml");
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": title,
            "raw_medium": medium,
            "explicit": false,
            "author_name": format!("Artist {guid}"),
            "remote_items": items,
            "tracks": []
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

async fn ingest_with_track(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    title: &str,
    track_title: &str,
) {
    let url = format!("https://example.com/{guid}.xml");
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": title,
            "raw_medium": "music",
            "explicit": false,
            "author_name": format!("Artist {guid}"),
            "tracks": [{
                "track_guid": format!("{guid}-track-01"),
                "title": track_title,
                "pub_date": 1_700_000_000,
                "explicit": false,
                "payment_routes": [],
                "value_time_splits": []
            }]
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

/// An item of an album that names `publisher` inside `<podcast:publisher>`.
fn names(position: i64, publisher: &str, rel: Option<&str>) -> Value {
    let mut item = json!({
        "position": position,
        "medium": "publisher",
        "remote_feed_guid": publisher,
        "remote_feed_url": format!("https://example.com/{publisher}.xml"),
        "publisher_reference": true,
    });
    if let Some(rel) = rel {
        item["rel"] = json!(rel);
    }
    item
}

/// An item of a publisher feed that lists `album`.
fn lists(position: i64, album: &str, rel: Option<&str>) -> Value {
    let mut item = json!({
        "position": position,
        "medium": "music",
        "remote_feed_guid": album,
        "remote_feed_url": format!("https://example.com/{album}.xml"),
    });
    if let Some(rel) = rel {
        item["rel"] = json!(rel);
    }
    item
}

/// Publisher `pub-glowline` lists `album-glowline`, and the album names
/// `pub-glowline` first with the same stated role, `label`. The link is
/// confirmed (ADR 0049 §6a), so the list row of ADR 0068 gives one two-way
/// link, the agreed role `label`, and a `first_named_count` of 1.
async fn stored() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        "album-glowline",
        "music",
        "Glowline Album One",
        json!([names(0, "pub-glowline", Some("label"))]),
    )
    .await;
    ingest(
        &st,
        "pub-glowline",
        "publisher",
        "Glowline Records Label",
        json!([lists(0, "album-glowline", Some("label"))]),
    )
    .await;
    ingest_with_track(
        &st,
        "feed-thornfield",
        "Thornfield Sessions",
        "Thornfield Interlude",
    )
    .await;
    st
}

async fn search_row(st: &Arc<stophammer::api::AppState>, uri: &str, entity_id: &str) -> Value {
    let body = send(st, "GET", uri, None).await;
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|row| row["entity_id"] == entity_id)
        .unwrap_or_else(|| panic!("no row for {entity_id} in {body} ({uri})"))
        .clone()
}

async fn by_name_row(st: &Arc<stophammer::api::AppState>, guid: &str) -> Value {
    let body = send(
        st,
        "GET",
        "/v1/feeds/by-name?medium=publisher&include=link_facts&limit=200",
        None,
    )
    .await;
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|row| row["feed_guid"] == guid)
        .unwrap_or_else(|| panic!("no by-name row for {guid} in {body}"))
        .clone()
}

#[tokio::test]
async fn a_publisher_search_row_gives_the_values_of_the_list_row() {
    let st = stored().await;
    let search = search_row(
        &st,
        "/v1/search?type=publisher&q=Glowline&include=link_facts",
        "pub-glowline",
    )
    .await;
    let list = by_name_row(&st, "pub-glowline").await;
    for field in FACT_FIELDS {
        assert!(
            search.get(field).is_some(),
            "ADR 0076 §3: a publisher search row gives `{field}` with the include: {search}"
        );
        assert_eq!(
            search[field], list[field],
            "ADR 0076 §3: `{field}` of the search row equals the value of the by-name list row"
        );
    }
    assert_eq!(
        search["two_way_link_count"], 1,
        "album-glowline and pub-glowline confirm one two-way link: {search}"
    );
    assert_eq!(
        search["agreed_roles"],
        json!(["label"]),
        "both sides of the link state label: {search}"
    );
    assert_eq!(
        search["first_named_count"], 1,
        "album-glowline names pub-glowline first with a confirmed link: {search}"
    );
}

#[tokio::test]
async fn a_feed_row_gives_no_link_facts_with_the_include() {
    let st = stored().await;
    let row = search_row(
        &st,
        "/v1/search?type=feed&q=Thornfield&include=link_facts",
        "feed-thornfield",
    )
    .await;
    for field in FACT_FIELDS {
        assert!(
            row.get(field).is_none(),
            "ADR 0076 §3: a feed row gives no `{field}` even with the include: {row}"
        );
    }
}

#[tokio::test]
async fn a_track_row_gives_no_link_facts_with_the_include() {
    let st = stored().await;
    let row = search_row(
        &st,
        "/v1/search?type=track&q=Thornfield&include=link_facts",
        "feed-thornfield-track-01",
    )
    .await;
    assert_eq!(
        row["entity_type"], "track",
        "the row found is a track: {row}"
    );
    for field in FACT_FIELDS {
        assert!(
            row.get(field).is_none(),
            "ADR 0076 §3: a track row gives no `{field}` even with the include: {row}"
        );
    }
}

#[tokio::test]
async fn a_publisher_row_without_the_include_gives_no_link_facts() {
    let st = stored().await;
    let row = search_row(&st, "/v1/search?type=publisher&q=Glowline", "pub-glowline").await;
    for field in FACT_FIELDS {
        assert!(
            row.get(field).is_none(),
            "ADR 0076 §3: a publisher row without the include gives no `{field}`: {row}"
        );
    }
}

#[tokio::test]
async fn capabilities_list_the_search_include() {
    let st = state(common::test_db_arc());
    let body = send(&st, "GET", "/v1/node/capabilities", None).await;
    assert_eq!(
        body["include_params"]["search"],
        json!(["link_facts"]),
        "ADR 0076 §3: capabilities list the include of /v1/search: {body}"
    );
}
