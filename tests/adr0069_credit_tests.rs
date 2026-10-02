// ADR 0069: an album confirms each publisher that it credits.
//
// docs/tasks/adr-0069-task-001-link-provenance.md and
// docs/tasks/adr-0069-task-002-co-credited-feeds.md
//
// An album names one publisher (inside `<podcast:publisher>`) and credits each
// other party with a bare channel `remoteItem` with `medium="publisher"`. Each
// `publisher` row gives `album_names_as`. A publisher read gives
// `co_credited_feeds`: the other publisher feeds of its confirmed albums.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0068-crawl-token";

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

/// A label release: the album names the label as publisher and credits the
/// artist. The label and the artist each list the album back.
async fn label_release() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    album(
        &st,
        "album-1",
        "Artist One",
        json!([
            names(0, "label", Some("label"), true),
            names(1, "artist", Some("artist"), false)
        ]),
    )
    .await;
    publisher(
        &st,
        "label",
        "Some Label",
        json!([lists(0, "album-1", Some("label"))]),
    )
    .await;
    publisher(
        &st,
        "artist",
        "Artist One",
        json!([lists(0, "album-1", Some("artist"))]),
    )
    .await;
    st
}

// ── Task 001 ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_album_read_names_the_publisher_and_the_credit() {
    let st = label_release().await;
    let data = read(&st, "album-1").await;
    let rows = data["publisher"].as_array().expect("publisher is an array");
    assert_eq!(
        rows.len(),
        2,
        "the album names two publisher feeds: {rows:?}"
    );

    let label = row(&data, "label");
    assert_eq!(label["two_way_validated"], true, "{label}");
    assert_eq!(label["album_names_as"], "publisher", "{label}");
    let artist = row(&data, "artist");
    assert_eq!(artist["two_way_validated"], true, "{artist}");
    assert_eq!(artist["album_names_as"], "credit", "{artist}");
}

#[tokio::test]
async fn the_publisher_reads_give_the_same_values() {
    let st = label_release().await;
    let label = read(&st, "label").await;
    assert_eq!(
        row(&label, "album-1")["album_names_as"],
        "publisher",
        "{label}"
    );
    let artist = read(&st, "artist").await;
    assert_eq!(
        row(&artist, "album-1")["album_names_as"],
        "credit",
        "{artist}"
    );
}

#[tokio::test]
async fn a_listing_that_the_album_does_not_confirm_gives_null() {
    let st = label_release().await;
    publisher(
        &st,
        "claimer",
        "Claimer",
        json!([lists(0, "album-1", Some("label"))]),
    )
    .await;
    let data = read(&st, "claimer").await;
    let listed = row(&data, "album-1");
    assert_eq!(listed["music_names_publisher"], false, "{listed}");
    assert_eq!(
        listed["album_names_as"],
        Value::Null,
        "a one-way listing is not named by the album: {listed}"
    );
}

#[tokio::test]
async fn an_ingest_with_no_publisher_reference_gives_credit() {
    let st = state(common::test_db_arc());
    // An older crawler sends no `publisher_reference`.
    let item = json!({
        "position": 0,
        "medium": "publisher",
        "remote_feed_guid": "label",
        "remote_feed_url": "https://p.example/label.xml",
    });
    album(&st, "album-1", "Artist One", json!([item])).await;
    publisher(
        &st,
        "label",
        "Some Label",
        json!([lists(0, "album-1", None)]),
    )
    .await;
    let data = read(&st, "album-1").await;
    assert_eq!(row(&data, "label")["album_names_as"], "credit", "{data}");
}

// ── Task 002 ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_label_and_its_artist_give_each_other() {
    let st = label_release().await;
    let label = read(&st, "label").await;
    assert_eq!(
        label["co_credited_feeds"],
        json!([{"feed_guid": "artist", "title": "Feed artist", "roles": ["artist"], "album_count": 1}]),
        "{label}"
    );
    let artist = read(&st, "artist").await;
    assert_eq!(
        artist["co_credited_feeds"],
        json!([{"feed_guid": "label", "title": "Feed label", "roles": ["label"], "album_count": 1}]),
        "{artist}"
    );
}

#[tokio::test]
async fn a_feed_that_only_lists_the_album_is_not_co_credited() {
    let st = label_release().await;
    publisher(
        &st,
        "claimer",
        "Claimer",
        json!([lists(0, "album-1", Some("artist"))]),
    )
    .await;
    let label = read(&st, "label").await;
    let guids: Vec<&str> = label["co_credited_feeds"]
        .as_array()
        .expect("co_credited_feeds is an array")
        .iter()
        .map(|entry| entry["feed_guid"].as_str().expect("feed_guid"))
        .collect();
    assert_eq!(
        guids,
        vec!["artist"],
        "the album does not credit the claimer: {label}"
    );
}

#[tokio::test]
async fn a_credit_that_the_artist_does_not_confirm_is_not_co_credited() {
    let st = state(common::test_db_arc());
    album(
        &st,
        "album-1",
        "Artist One",
        json!([
            names(0, "label", Some("label"), true),
            names(1, "artist", Some("artist"), false)
        ]),
    )
    .await;
    publisher(
        &st,
        "label",
        "Some Label",
        json!([lists(0, "album-1", Some("label"))]),
    )
    .await;
    // The artist feed exists, but does not list the album.
    publisher(&st, "artist", "Artist One", json!([])).await;
    let label = read(&st, "label").await;
    assert_eq!(label["co_credited_feeds"], json!([]), "{label}");
}

#[tokio::test]
async fn two_shared_albums_give_album_count_two() {
    let st = state(common::test_db_arc());
    for guid in ["album-1", "album-2"] {
        album(
            &st,
            guid,
            "Artist One",
            json!([
                names(0, "label", Some("label"), true),
                names(1, "artist", Some("artist"), false)
            ]),
        )
        .await;
    }
    publisher(
        &st,
        "label",
        "Some Label",
        json!([
            lists(0, "album-1", Some("label")),
            lists(1, "album-2", Some("label"))
        ]),
    )
    .await;
    publisher(
        &st,
        "artist",
        "Artist One",
        json!([
            lists(0, "album-1", Some("artist")),
            lists(1, "album-2", Some("artist"))
        ]),
    )
    .await;
    let label = read(&st, "label").await;
    assert_eq!(label["co_credited_feeds"][0]["album_count"], 2, "{label}");
}

#[tokio::test]
async fn a_music_feed_read_gives_no_co_credited_feeds() {
    let st = label_release().await;
    let data = read(&st, "album-1").await;
    assert!(
        data.get("co_credited_feeds").is_none(),
        "a music feed gives no co_credited_feeds: {data}"
    );
}
