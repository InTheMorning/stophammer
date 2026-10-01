// ADR 0068: a publisher row gives its link facts.
//
// docs/tasks/adr-0068-task-001-link-facts.md
//
// With `include=link_facts`, each publisher row of `GET /v1/feeds/recent`
// gives `two_way_link_count`, `stated_rels` and `confirmed_release_artists`.
// Each value is the value that a full read of the same feed gives.

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
const FACT_FIELDS: [&str; 3] = [
    "two_way_link_count",
    "stated_rels",
    "confirmed_release_artists",
];

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

async fn list_row(st: &Arc<stophammer::api::AppState>, query: &str, guid: &str) -> Value {
    let body = send(st, "GET", &format!("/v1/feeds/recent?{query}"), None).await;
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|row| row["feed_guid"] == guid)
        .unwrap_or_else(|| panic!("no row for {guid} in {body}"))
        .clone()
}

#[tokio::test]
async fn a_publisher_row_gives_the_two_way_count_and_the_stated_rels() {
    let st = stored_publisher().await;
    let row = list_row(&st, "medium=publisher&include=link_facts", PUBLISHER_GUID).await;
    assert_eq!(
        row["two_way_link_count"], 2,
        "ADR 0068 §1: two of the three links are two-way"
    );
    assert_eq!(
        row["stated_rels"],
        json!(["label"]),
        "ADR 0068 §1: only the rel of a two-way link counts"
    );

    let full = send(
        &st,
        "GET",
        &format!("/v1/feeds/{PUBLISHER_GUID}?include=publisher"),
        None,
    )
    .await;
    let two_way: Vec<&Value> = full["data"]["publisher"]
        .as_array()
        .expect("publisher view")
        .iter()
        .filter(|r| r["direction"] == "publisher_to_music" && r["two_way_validated"] == true)
        .collect();
    assert_eq!(
        row["two_way_link_count"],
        json!(two_way.len()),
        "ADR 0068 invariant: the list row gives the count of the full read"
    );
}

#[tokio::test]
async fn a_publisher_row_gives_the_confirmed_artists_of_the_full_read() {
    let st = stored_publisher().await;
    let row = list_row(&st, "medium=publisher&include=link_facts", PUBLISHER_GUID).await;
    let full = send(
        &st,
        "GET",
        &format!("/v1/feeds/{PUBLISHER_GUID}?include=publisher"),
        None,
    )
    .await;
    assert_eq!(
        row["confirmed_release_artists"], full["data"]["confirmed_release_artists"],
        "ADR 0068 invariant: the list row gives the confirmed artists of the full read"
    );
    assert_eq!(
        row["confirmed_release_artists"],
        json!(["Artist A", "Artist B"]),
        "ADR 0061 §1: the two albums that name the publisher"
    );
}

#[tokio::test]
async fn a_music_row_gives_no_link_facts() {
    let st = stored_publisher().await;
    let row = list_row(&st, "medium=music&include=link_facts", "album-a").await;
    for key in FACT_FIELDS {
        assert!(
            row.get(key).is_none(),
            "ADR 0068 §1: a music row gives no {key}: {row}"
        );
    }
}

#[tokio::test]
async fn a_row_without_the_include_gives_no_link_facts() {
    let st = stored_publisher().await;
    let row = list_row(&st, "medium=publisher", PUBLISHER_GUID).await;
    for key in FACT_FIELDS {
        assert!(
            row.get(key).is_none(),
            "ADR 0068 §1: a row without include=link_facts gives no {key}: {row}"
        );
    }
}

#[tokio::test]
async fn capabilities_list_the_link_facts_include() {
    let st = state(common::test_db_arc());
    let body = send(&st, "GET", "/v1/node/capabilities", None).await;
    let names = body["include_params"]["feed_list"]
        .as_array()
        .unwrap_or_else(|| panic!("no feed_list include names in {body}"));
    assert!(
        names.iter().any(|name| name == "link_facts"),
        "ADR 0068 §1: capabilities list link_facts for the list route: {body}"
    );
}
