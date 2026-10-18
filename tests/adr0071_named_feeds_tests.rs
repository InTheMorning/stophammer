// ADR 0071 §3: the named feeds of a release row.
//
// docs/tasks/adr-0071-task-004-named-feeds.md
//
// With `include=named_feeds`, each music row of `GET /v1/feeds/recent` and
// `GET /v1/feeds/by-name` gives one entry for each feed that the release
// names inside `<podcast:publisher>`, in rank order.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const ROUTES: [&str; 2] = ["/v1/feeds/recent", "/v1/feeds/by-name"];

const TOKEN: &str = "adr0071-named-feeds-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0071-named-feeds-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0071-named-feeds-admin-token".into(),
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

async fn ingest_as(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    medium: &str,
    author: &str,
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
            "title": format!("Feed {guid}"),
            "raw_medium": medium,
            "explicit": false,
            "author_name": author,
            "remote_items": items,
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

/// An item of an album inside `<podcast:publisher>` that names `publisher`.
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

/// Stores these feeds:
///
/// - `album-two` names `label-a` with `rel="label"`, then `artist-b`. Both
///   feeds list it, and `label-a` states `label` too.
/// - `album-ghost` names `ghost`, a feed that the node does not hold, then
///   `label-a`.
/// - `album-none` names no feed.
async fn stored() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    ingest_as(
        &st,
        "album-two",
        "music",
        "Artist B",
        json!([
            names(0, "label-a", Some("label")),
            names(1, "artist-b", None)
        ]),
    )
    .await;
    ingest_as(
        &st,
        "album-ghost",
        "music",
        "Artist C",
        json!([names(0, "ghost", None), names(1, "label-a", None)]),
    )
    .await;
    ingest_as(&st, "album-none", "music", "Artist D", json!([])).await;
    ingest_as(
        &st,
        "label-a",
        "publisher",
        "Label A",
        json!([
            lists(0, "album-two", Some("label")),
            lists(1, "album-ghost", None)
        ]),
    )
    .await;
    ingest_as(
        &st,
        "artist-b",
        "publisher",
        "Artist B",
        json!([lists(0, "album-two", None)]),
    )
    .await;
    st
}

async fn row(st: &Arc<stophammer::api::AppState>, uri: &str, guid: &str) -> Value {
    let body = send(st, "GET", uri, None).await;
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|r| r["feed_guid"] == guid)
        .unwrap_or_else(|| panic!("{uri}: no row for {guid} in {body}"))
        .clone()
}

#[tokio::test]
async fn an_album_gives_each_named_feed_in_rank_order() {
    let st = stored().await;
    let full = send(&st, "GET", "/v1/feeds/album-two?include=publisher", None).await;
    let view: Vec<&Value> = full["data"]["publisher"]
        .as_array()
        .expect("publisher is an array")
        .iter()
        .filter(|r| r["direction"] == "music_to_publisher")
        .collect();
    for route in ROUTES {
        let r = row(
            &st,
            &format!("{route}?limit=200&include=named_feeds"),
            "album-two",
        )
        .await;
        let entries = r["named_feeds"]
            .as_array()
            .expect("named_feeds is an array");
        let guids: Vec<&Value> = entries.iter().map(|e| &e["publisher_feed_guid"]).collect();
        assert_eq!(
            guids,
            vec![&json!("label-a"), &json!("artist-b")],
            "{route}: ADR 0071 §3 gives the entries in rank order: {r}"
        );
        assert_eq!(entries[0]["album_names_rank"], 1, "{route}: {r}");
        assert_eq!(entries[1]["album_names_rank"], 2, "{route}: {r}");
        for entry in entries {
            let same = view
                .iter()
                .find(|v| v["publisher_feed_guid"] == entry["publisher_feed_guid"])
                .unwrap_or_else(|| panic!("no publisher view row for {entry}"));
            for field in [
                "remote_feed_title",
                "album_names_rank",
                "role",
                "role_agreement",
            ] {
                assert_eq!(
                    entry[field], same[field],
                    "{route}: `{field}` equals the value of the publisher view: {entry}"
                );
            }
        }
        assert_eq!(entries[0]["role"], "label", "{route}: {r}");
        assert_eq!(entries[0]["role_agreement"], "both", "{route}: {r}");
    }
}

#[tokio::test]
async fn a_feed_that_the_node_does_not_hold_gives_a_null_title() {
    let st = stored().await;
    for route in ROUTES {
        let r = row(
            &st,
            &format!("{route}?limit=200&include=named_feeds"),
            "album-ghost",
        )
        .await;
        let entries = r["named_feeds"]
            .as_array()
            .expect("named_feeds is an array");
        assert_eq!(entries[0]["publisher_feed_guid"], "ghost", "{route}: {r}");
        assert!(
            entries[0]["remote_feed_title"].is_null(),
            "{route}: ADR 0071 §3 gives a null title for a feed that the node does not hold: {r}"
        );
        assert_eq!(
            entries[1]["album_names_rank"], 2,
            "{route}: the held feed keeps rank 2: {r}"
        );
    }
}

#[tokio::test]
async fn a_release_with_no_named_feed_gives_an_empty_list() {
    let st = stored().await;
    for route in ROUTES {
        let r = row(
            &st,
            &format!("{route}?limit=200&include=named_feeds"),
            "album-none",
        )
        .await;
        assert_eq!(
            r["named_feeds"],
            json!([]),
            "{route}: ADR 0071 §3 gives an empty list: {r}"
        );
    }
}

#[tokio::test]
async fn a_publisher_row_and_a_row_without_the_include_give_no_named_feeds() {
    let st = stored().await;
    for route in ROUTES {
        let p = row(
            &st,
            &format!("{route}?limit=200&medium=publisher&include=named_feeds"),
            "label-a",
        )
        .await;
        assert!(
            p.get("named_feeds").is_none(),
            "{route}: a publisher row gives no named_feeds: {p}"
        );
        let plain = row(&st, &format!("{route}?limit=200"), "album-two").await;
        assert!(
            plain.get("named_feeds").is_none(),
            "{route}: a row without the include gives no named_feeds: {plain}"
        );
    }
}

#[tokio::test]
async fn capabilities_list_the_named_feeds_include() {
    let st = state(common::test_db_arc());
    let body = send(&st, "GET", "/v1/node/capabilities", None).await;
    assert_eq!(
        body["include_params"]["feed_list"],
        json!(["link_facts", "named_feeds"]),
        "ADR 0071 §3: capabilities list the include of the release lists: {body}"
    );
}
