// ADR 0049 task 014: the link rules of podcast-namespace PR #793.
//
// docs/tasks/adr-0049-task-014-pr793-link-rules.md
//
// ADR 0049 §6a: each `publisher` row gives `role_agreement`, and a link whose
// two sides state different role sets is not a confirmed link. ADR 0068 §5:
// the list facts are role tokens, with `agreed_roles`. ADR 0069 §1a: only an
// album item inside `<podcast:publisher>` is a publisher link.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "pr793-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-pr793-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-pr793-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
    })
}

async fn status_of(st: &Arc<stophammer::api::AppState>, uri: &str) -> http::StatusCode {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("build request");
    stophammer::api::build_router(Arc::clone(st))
        .oneshot(req)
        .await
        .expect("send request")
        .status()
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

/// A `remoteItem` that an album gives for a publisher feed. `publisher` is
/// the parser mark of ADR 0069 §3: true for the item inside
/// `<podcast:publisher>`.
fn names(position: i64, publisher_guid: &str, rel: Option<&str>, publisher: bool) -> Value {
    let mut item = json!({
        "position": position,
        "medium": "publisher",
        "remote_feed_guid": publisher_guid,
        "remote_feed_url": format!("https://p.example/{publisher_guid}.xml"),
        "publisher_reference": publisher,
    });
    if let Some(rel) = rel {
        item["rel"] = json!(rel);
    }
    item
}

/// A `remoteItem` that a publisher feed gives for an album.
fn lists(position: i64, album_guid: &str, rel: Option<&str>) -> Value {
    let mut item = json!({
        "position": position,
        "medium": "music",
        "remote_feed_guid": album_guid,
        "remote_feed_url": format!("https://a.example/{album_guid}.xml"),
    });
    if let Some(rel) = rel {
        item["rel"] = json!(rel);
    }
    item
}

async fn album(st: &Arc<stophammer::api::AppState>, guid: &str, artist: &str, items: Value) {
    ingest(
        st,
        guid,
        &format!("https://a.example/{guid}.xml"),
        "music",
        artist,
        items,
    )
    .await;
}

async fn publisher(st: &Arc<stophammer::api::AppState>, guid: &str, name: &str, items: Value) {
    ingest(
        st,
        guid,
        &format!("https://p.example/{guid}.xml"),
        "publisher",
        name,
        items,
    )
    .await;
}

async fn read(st: &Arc<stophammer::api::AppState>, guid: &str) -> Value {
    send(
        st,
        "GET",
        &format!("/v1/feeds/{guid}?include=publisher"),
        None,
    )
    .await["data"]
        .clone()
}

fn row<'a>(data: &'a Value, other_guid: &str) -> &'a Value {
    data["publisher"]
        .as_array()
        .expect("publisher is an array")
        .iter()
        .find(|row| {
            row["publisher_feed_guid"] == other_guid || row["music_feed_guid"] == other_guid
        })
        .unwrap_or_else(|| panic!("no publisher row for {other_guid} in {data}"))
}

async fn list_row(st: &Arc<stophammer::api::AppState>, query: &str, guid: &str) -> Option<Value> {
    let body = send(
        st,
        "GET",
        &format!("/v1/feeds/recent?medium=publisher&include=link_facts{query}"),
        None,
    )
    .await;
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .find(|row| row["feed_guid"] == guid)
        .cloned()
}

async fn facts(st: &Arc<stophammer::api::AppState>, guid: &str) -> Value {
    list_row(st, "", guid)
        .await
        .unwrap_or_else(|| panic!("no list row for {guid}"))
}

/// One album that names `pub-1` with `album_rel`, and `pub-1` lists the album
/// back with `publisher_rel`.
async fn one_link(
    album_rel: Option<&str>,
    publisher_rel: Option<&str>,
) -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    album(
        &st,
        "album-1",
        "Artist One",
        json!([names(0, "pub-1", album_rel, true)]),
    )
    .await;
    publisher(
        &st,
        "pub-1",
        "Publisher One",
        json!([lists(0, "album-1", publisher_rel)]),
    )
    .await;
    st
}

#[tokio::test]
async fn equal_role_sets_in_any_order_agree() {
    let st = one_link(Some("producer artist"), Some("artist producer")).await;
    let link = row(&read(&st, "album-1").await, "pub-1").clone();
    assert_eq!(link["role_agreement"], "both", "ADR 0049 §6a: {link}");
    assert_eq!(
        facts(&st, "pub-1").await["two_way_link_count"],
        1,
        "ADR 0049 §6a: an agreed link counts"
    );
}

#[tokio::test]
async fn a_role_on_one_side_counts_but_is_not_agreed() {
    let st = one_link(None, Some("artist")).await;
    let link = row(&read(&st, "album-1").await, "pub-1").clone();
    assert_eq!(link["role_agreement"], "one_side", "ADR 0049 §6a: {link}");
    assert_eq!(
        link["role"], "artist",
        "ADR 0049 §6 does not change: {link}"
    );
    let row = facts(&st, "pub-1").await;
    assert_eq!(row["two_way_link_count"], 1, "ADR 0049 §6a: {row}");
    assert_eq!(row["stated_rels"], json!(["artist"]), "ADR 0068 §5: {row}");
    assert_eq!(
        row["agreed_roles"],
        json!([]),
        "ADR 0068 §5: a role on one side is not agreed: {row}"
    );
}

#[tokio::test]
async fn different_role_sets_are_not_a_confirmed_link() {
    let st = state(common::test_db_arc());
    album(
        &st,
        "album-1",
        "Artist One",
        json!([
            names(0, "artist", Some("artist"), true),
            names(1, "label", Some("recordLabel"), true)
        ]),
    )
    .await;
    publisher(
        &st,
        "artist",
        "Artist One",
        json!([lists(0, "album-1", Some("artist"))]),
    )
    .await;
    publisher(
        &st,
        "label",
        "Some Label",
        json!([lists(0, "album-1", Some("label"))]),
    )
    .await;

    let label = read(&st, "label").await;
    let link = row(&label, "album-1");
    assert_eq!(link["role_agreement"], "conflict", "ADR 0049 §6a: {link}");
    assert_eq!(
        link["two_way_validated"], true,
        "ADR 0049 §6a: the row keeps each raw fact: {link}"
    );
    assert_eq!(
        label["confirmed_release_artists"],
        json!([]),
        "ADR 0049 §6a: {label}"
    );
    assert_eq!(
        label["unconfirmed_release_artists"],
        json!(["Artist One"]),
        "ADR 0049 §6a: {label}"
    );
    assert_eq!(
        label["co_credited_feeds"],
        json!([]),
        "ADR 0049 §6a: {label}"
    );

    let row = facts(&st, "label").await;
    assert_eq!(row["two_way_link_count"], 0, "ADR 0049 §6a: {row}");
    assert_eq!(row["stated_rels"], json!([]), "ADR 0049 §6a: {row}");
    assert_eq!(row["agreed_roles"], json!([]), "ADR 0049 §6a: {row}");

    let artist = read(&st, "artist").await;
    assert_eq!(
        artist["co_credited_feeds"],
        json!([]),
        "ADR 0049 §6a: the label is not co-credited: {artist}"
    );
}

#[tokio::test]
async fn no_role_on_either_side_gives_null() {
    let st = one_link(None, None).await;
    let link = row(&read(&st, "album-1").await, "pub-1").clone();
    assert_eq!(link["role_agreement"], Value::Null, "ADR 0049 §6a: {link}");
    assert_eq!(
        facts(&st, "pub-1").await["two_way_link_count"],
        1,
        "ADR 0049 §6a: a link with no role counts"
    );
}

#[tokio::test]
async fn one_party_with_five_roles_gives_five_tokens() {
    let rel = "artist host author label producer";
    let st = one_link(Some(rel), Some(rel)).await;
    let tokens = json!(["artist", "author", "host", "label", "producer"]);
    let row = facts(&st, "pub-1").await;
    assert_eq!(row["stated_rels"], tokens, "ADR 0068 §5: {row}");
    assert_eq!(row["agreed_roles"], tokens, "ADR 0068 §5: {row}");
    for token in ["artist", "label"] {
        assert!(
            list_row(&st, &format!("&stated_rel={token}"), "pub-1")
                .await
                .is_some(),
            "ADR 0068 §5: stated_rel={token} matches one token of the set"
        );
    }
}

#[tokio::test]
async fn stated_rel_is_lowercased() {
    let st = one_link(Some("label"), Some("label")).await;
    assert!(
        list_row(&st, "&stated_rel=Label", "pub-1").await.is_some(),
        "ADR 0068 §5: stated_rel=Label finds a row with label"
    );
}

#[tokio::test]
async fn stated_rel_with_two_tokens_is_refused() {
    let st = one_link(Some("label"), Some("label")).await;
    assert_eq!(
        status_of(
            &st,
            "/v1/feeds/recent?medium=publisher&stated_rel=artist%20label"
        )
        .await,
        http::StatusCode::BAD_REQUEST,
        "ADR 0068 §5: stated_rel is one role token"
    );
}

#[tokio::test]
async fn a_bare_item_next_to_a_publisher_element_is_not_a_link() {
    let st = state(common::test_db_arc());
    album(
        &st,
        "album-1",
        "Artist One",
        json!([
            names(0, "pub-2", Some("label"), false),
            names(1, "pub-1", Some("artist"), true)
        ]),
    )
    .await;
    publisher(
        &st,
        "pub-1",
        "Publisher One",
        json!([lists(0, "album-1", Some("artist"))]),
    )
    .await;
    publisher(
        &st,
        "pub-2",
        "Publisher Two",
        json!([lists(0, "album-1", Some("label"))]),
    )
    .await;

    let data = read(&st, "album-1").await;
    let rows = data["publisher"].as_array().expect("publisher is an array");
    assert_eq!(
        rows.len(),
        1,
        "ADR 0069 §1a: the bare item gives no row: {data}"
    );
    assert_eq!(row(&data, "pub-1")["album_names_as"], "publisher", "{data}");

    assert_eq!(
        data["publisher_feed_title"], "Feed pub-1",
        "ADR 0069 §1a: the bare item at position 0 is not the publisher: {data}"
    );

    let other = read(&st, "pub-2").await;
    assert_eq!(
        other["distinct_release_artists"],
        json!([]),
        "ADR 0069 §1a: the bare item does not name pub-2: {other}"
    );
    let listed = row(&other, "album-1");
    assert_eq!(
        listed["music_names_publisher"], false,
        "ADR 0069 §1a: {listed}"
    );
    assert_eq!(
        listed["album_names_as"],
        Value::Null,
        "ADR 0069 §1a: {listed}"
    );
}

#[tokio::test]
async fn the_first_bare_item_of_an_album_with_no_publisher_element_is_a_link() {
    // The parser marks the first bare `medium="publisher"` item of an album
    // with no `<podcast:publisher>` as `publisher_reference` (ADR 0069 §3).
    let st = one_link(None, None).await;
    let data = read(&st, "album-1").await;
    let rows = data["publisher"].as_array().expect("publisher is an array");
    assert_eq!(rows.len(), 1, "ADR 0069 §1a: {data}");
    assert_eq!(rows[0]["two_way_validated"], true, "{data}");
    assert_eq!(rows[0]["album_names_as"], "publisher", "{data}");
}
