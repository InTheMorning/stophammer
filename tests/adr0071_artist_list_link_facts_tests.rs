// ADR 0071 §2: the link facts of an artist list row.
//
// docs/tasks/adr-0071-task-002-artist-list-link-facts.md
//
// With `include=link_facts`, each `publisher` row of `GET /v1/artist-names`
// gives the four fields of ADR 0068 and `first_named_count`. The values are
// the values of the list route of ADR 0068 for the same feed.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0071-artist-list-crawl-token";
const FACT_FIELDS: [&str; 5] = [
    "two_way_link_count",
    "stated_rels",
    "agreed_roles",
    "confirmed_release_artists",
    "first_named_count",
];

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0071-artist-list-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0071-artist-list-admin-token".into(),
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

async fn ingest(st: &Arc<stophammer::api::AppState>, guid: &str, medium: &str, items: Value) {
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
            "author_name": format!("Artist {guid}"),
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

/// Publisher `pub-p` lists four albums:
///
/// - `album-first` names it first. The link is confirmed.
/// - `album-second` names `pub-q` first and `pub-p` second. Both links are
///   confirmed.
/// - `album-conflict` names it first, but the two sides state different
///   roles, so the link is not confirmed.
/// - `album-one-side` is listed, but names no publisher.
///
/// `pub-p` also states `rel="label"` on its link to `album-first`, and the
/// album states the same role, so `label` is an agreed role.
async fn stored() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        "album-first",
        "music",
        json!([names(0, "pub-p", Some("label"))]),
    )
    .await;
    ingest(
        &st,
        "album-second",
        "music",
        json!([names(0, "pub-q", None), names(1, "pub-p", None)]),
    )
    .await;
    ingest(
        &st,
        "album-conflict",
        "music",
        json!([names(0, "pub-p", Some("artist"))]),
    )
    .await;
    ingest(&st, "album-one-side", "music", json!([])).await;
    ingest(
        &st,
        "pub-p",
        "publisher",
        json!([
            lists(0, "album-first", Some("label")),
            lists(1, "album-second", None),
            lists(2, "album-conflict", Some("label")),
            lists(3, "album-one-side", None),
        ]),
    )
    .await;
    ingest(
        &st,
        "pub-q",
        "publisher",
        json!([lists(0, "album-second", None)]),
    )
    .await;
    st
}

async fn artist_row(st: &Arc<stophammer::api::AppState>, query: &str, key: &str) -> Value {
    let body = send(
        st,
        "GET",
        &format!("/v1/artist-names?limit=200{query}"),
        None,
    )
    .await;
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|row| row["key"] == key)
        .unwrap_or_else(|| panic!("no artist row for {key} in {body}"))
        .clone()
}

async fn recent_row(st: &Arc<stophammer::api::AppState>, guid: &str) -> Value {
    let body = send(
        st,
        "GET",
        "/v1/feeds/recent?medium=publisher&include=link_facts&limit=200",
        None,
    )
    .await;
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|row| row["feed_guid"] == guid)
        .unwrap_or_else(|| panic!("no recent row for {guid} in {body}"))
        .clone()
}

#[tokio::test]
async fn a_publisher_row_gives_the_values_of_the_list_route() {
    let st = stored().await;
    let artist = artist_row(&st, "&include=link_facts", "pub-p").await;
    let recent = recent_row(&st, "pub-p").await;
    for field in FACT_FIELDS {
        assert!(
            artist.get(field).is_some(),
            "ADR 0071 §2: a publisher row gives `{field}` with the include: {artist}"
        );
        assert_eq!(
            artist[field], recent[field],
            "ADR 0071 §2: `{field}` of the artist row equals the value of /v1/feeds/recent"
        );
    }
    assert_eq!(
        artist["two_way_link_count"], 2,
        "the first and the second album are confirmed links, the conflict is not: {artist}"
    );
    assert_eq!(
        artist["agreed_roles"],
        json!(["label"]),
        "both sides of the first album state label: {artist}"
    );
}

#[tokio::test]
async fn first_named_count_counts_only_a_confirmed_link_with_rank_one() {
    let st = stored().await;
    let p = artist_row(&st, "&include=link_facts", "pub-p").await;
    assert_eq!(
        p["first_named_count"], 1,
        "ADR 0071 §2: only album-first names pub-p first with a confirmed link. \
         album-second names it second, and album-conflict is not confirmed: {p}"
    );
    let q = artist_row(&st, "&include=link_facts", "pub-q").await;
    assert_eq!(
        q["first_named_count"], 1,
        "album-second names pub-q first with a confirmed link: {q}"
    );
}

#[tokio::test]
async fn a_name_row_and_a_row_without_the_include_give_no_link_facts() {
    let st = stored().await;
    let plain = artist_row(&st, "", "pub-p").await;
    let body = send(
        &st,
        "GET",
        "/v1/artist-names?limit=200&include=link_facts",
        None,
    )
    .await;
    let name_row = body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|row| row["kind"] == "name")
        .unwrap_or_else(|| panic!("no name row in {body}"))
        .clone();
    for field in FACT_FIELDS {
        assert!(
            plain.get(field).is_none(),
            "ADR 0071 §2: a row without the include gives no `{field}`: {plain}"
        );
        assert!(
            name_row.get(field).is_none(),
            "ADR 0071 §2: a name row gives no `{field}`: {name_row}"
        );
    }
}

#[tokio::test]
async fn capabilities_list_the_artist_list_include() {
    let st = state(common::test_db_arc());
    let body = send(&st, "GET", "/v1/node/capabilities", None).await;
    assert_eq!(
        body["include_params"]["artist_list"],
        json!(["link_facts"]),
        "ADR 0071 §2: capabilities list the include of /v1/artist-names: {body}"
    );
}
