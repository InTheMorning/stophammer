// ADR 0070 task 002: the artist list.
//
// docs/tasks/adr-0070-task-002-artist-list.md
//
// Checks:
// - `GET /v1/artist-names` gives one `publisher` row for each publisher feed,
//   even one that no music feed names.
// - A music feed is of the name level (ADR 0070 §4) only when it holds no
//   link to a held publisher feed, or when each such link has
//   `role_agreement` `both` with an agreed role set that does not include
//   `artist`.
// - A `name` row groups the releases of the name level by the sort key of
//   `release_artist`, with `release_count` and a tie-broken raw `name`.
// - The list orders by `(sort_name, kind, key)`, with a working cursor and
//   `from`.
// - The ADR 0044 guards pass for the route.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0070-artist-list-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0070-artist-list-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0070-artist-list-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

/// Builds the same state as [`state`], but with a cache that rebuilds
/// again at most once in `min_rebuild_interval`. ADR 0070 §4a: a test gives
/// a short interval here, so it does not wait 30 seconds.
fn state_with_cache_interval(
    db: Arc<Mutex<rusqlite::Connection>>,
    min_rebuild_interval: std::time::Duration,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0070-artist-list-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0070-artist-list-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::new(min_rebuild_interval),
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

fn feed_url(guid: &str) -> String {
    format!("https://x.example/{guid}.xml")
}

/// Ingests a feed with the given `remote_items` (the album's `<podcast:
/// publisher>` entries, or the publisher's `<podcast:remoteItem
/// medium="music">` entries).
async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    medium: &str,
    author: &str,
    remote_items: Value,
) {
    let payload = json!({
        "canonical_url": feed_url(guid),
        "source_url": feed_url(guid),
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

/// Ingests a feed with an explicit `title`, for a test that reads the
/// title back (a publisher row's `name`, or a tie on `sort_name`).
async fn ingest_titled(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    title: &str,
    medium: &str,
    author: &str,
    remote_items: Value,
) {
    let payload = json!({
        "canonical_url": feed_url(guid),
        "source_url": feed_url(guid),
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": title,
            "raw_medium": medium,
            "explicit": false,
            "author_name": author,
            "remote_items": remote_items,
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

/// An album item of `<podcast:publisher>`: `medium="publisher"`, with
/// `publisher_reference: true` so the node stores `source="podcast_publisher"`
/// (ADR 0069 §1a). `rel` is the raw value the album states, if any.
fn publisher_link_entry(publisher_guid: &str, rel: Option<&str>) -> Value {
    let mut item = json!({
        "position": 0,
        "medium": "publisher",
        "remote_feed_guid": publisher_guid,
        "remote_feed_url": feed_url(publisher_guid),
        "publisher_reference": true,
    });
    if let Some(rel) = rel {
        item["rel"] = json!(rel);
    }
    item
}

/// A publisher item that lists an album: `medium="music"`. `rel` is the raw
/// value the publisher states for the album, if any.
fn music_link_entry(album_guid: &str, rel: Option<&str>) -> Value {
    let mut item = json!({
        "position": 0,
        "medium": "music",
        "remote_feed_guid": album_guid,
        "remote_feed_url": feed_url(album_guid),
    });
    if let Some(rel) = rel {
        item["rel"] = json!(rel);
    }
    item
}

async fn artist_names(st: &Arc<stophammer::api::AppState>, query: &str) -> Value {
    let uri = if query.is_empty() {
        "/v1/artist-names".to_string()
    } else {
        format!("/v1/artist-names?{query}")
    };
    send(st, "GET", &uri, None).await
}

fn rows_of(body: &Value) -> Vec<Value> {
    body["data"]
        .as_array()
        .expect("data must be an array")
        .clone()
}

fn find_row<'a>(rows: &'a [Value], kind: &str, key: &str) -> Option<&'a Value> {
    rows.iter().find(|r| r["kind"] == kind && r["key"] == key)
}

// ── A publisher row for every publisher feed ────────────────────────────────

#[tokio::test]
async fn a_publisher_feed_with_no_music_feed_gives_a_publisher_row() {
    let st = state(common::test_db_arc());
    ingest_titled(
        &st,
        "pub-lonely",
        "Lonely Publisher",
        "publisher",
        "Some Label",
        json!([]),
    )
    .await;

    let body = artist_names(&st, "").await;
    let rows = rows_of(&body);
    let row = find_row(&rows, "publisher", "pub-lonely")
        .unwrap_or_else(|| panic!("no publisher row for pub-lonely: {rows:?}"));

    assert_eq!(row["feed_guid"], json!("pub-lonely"));
    assert_eq!(row["name"], json!("Lonely Publisher"));
    assert_eq!(row["sort_name"], json!("lonely publisher"));
    assert!(
        row["release_count"].is_null(),
        "ADR 0070 §3: release_count is null for a publisher row: {row:?}"
    );
}

// ── The name level (ADR 0070 §4) ────────────────────────────────────────────

#[tokio::test]
async fn an_album_naming_a_held_publisher_with_no_rel_gives_no_name_row() {
    let st = state(common::test_db_arc());
    ingest_titled(
        &st,
        "pub-noRel",
        "No Rel Publisher",
        "publisher",
        "Some Label",
        json!([]),
    )
    .await;
    ingest(
        &st,
        "album-noRel",
        "music",
        "Second Artist",
        json!([publisher_link_entry("pub-noRel", None)]),
    )
    .await;

    let body = artist_names(&st, "").await;
    let rows = rows_of(&body);
    let key = stophammer::model::sort_key("Second Artist");
    assert!(
        find_row(&rows, "name", &key).is_none(),
        "ADR 0070 §4: a held link with no rel does not open the name level: {rows:?}"
    );
}

#[tokio::test]
async fn an_album_with_no_publisher_link_gives_a_name_row_with_release_count_one() {
    let st = state(common::test_db_arc());
    ingest(&st, "album-solo", "music", "Solo Artist Three", json!([])).await;

    let body = artist_names(&st, "").await;
    let rows = rows_of(&body);
    let key = stophammer::model::sort_key("Solo Artist Three");
    let row =
        find_row(&rows, "name", &key).unwrap_or_else(|| panic!("no name row for {key}: {rows:?}"));

    assert_eq!(row["release_count"], json!(1));
    assert_eq!(row["name"], json!("Solo Artist Three"));
    assert_eq!(row["sort_name"], json!(key));
    assert!(
        row["feed_guid"].is_null(),
        "ADR 0070 §3: feed_guid is null for a name row: {row:?}"
    );
}

#[tokio::test]
async fn an_album_whose_only_link_agrees_on_label_gives_a_name_row() {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        "pub-label",
        "publisher",
        "Some Label",
        json!([music_link_entry("album-label", Some("label"))]),
    )
    .await;
    ingest(
        &st,
        "album-label",
        "music",
        "Label Artist Four",
        json!([publisher_link_entry("pub-label", Some("label"))]),
    )
    .await;

    let body = artist_names(&st, "").await;
    let rows = rows_of(&body);
    let key = stophammer::model::sort_key("Label Artist Four");
    let row =
        find_row(&rows, "name", &key).unwrap_or_else(|| panic!("no name row for {key}: {rows:?}"));
    assert_eq!(
        row["release_count"],
        json!(1),
        "ADR 0070 §4: role_agreement both with the role label opens the name level"
    );
}

#[tokio::test]
async fn an_album_with_an_agreed_artist_and_label_link_gives_no_name_row() {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        "pub-artistLabel",
        "publisher",
        "Some Label",
        json!([music_link_entry("album-artistLabel", Some("artist, label"))]),
    )
    .await;
    ingest(
        &st,
        "album-artistLabel",
        "music",
        "No Name Five",
        json!([publisher_link_entry(
            "pub-artistLabel",
            Some("artist, label")
        )]),
    )
    .await;

    let body = artist_names(&st, "").await;
    let rows = rows_of(&body);
    let key = stophammer::model::sort_key("No Name Five");
    assert!(
        find_row(&rows, "name", &key).is_none(),
        "ADR 0070 §4: the agreed role set includes artist, so the release stays at the publisher level: {rows:?}"
    );
}

// ── Grouping by sort key, and a tie on the raw text ─────────────────────────

#[tokio::test]
async fn two_spellings_of_the_same_sort_key_give_one_name_row_with_release_count_two() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(&st, "album-haleen-a", "music", "Haleen", json!([])).await;
    // A second album with a release_artist that differs only in white
    // space. The ingest path trims `author_name` (src/api.rs
    // `non_empty_trimmed`), so the raw text is written directly to exercise
    // `sort_key`'s own trim-and-collapse rule, not the ingest trim.
    ingest(&st, "album-haleen-b", "music", "placeholder", json!([])).await;
    db.lock()
        .expect("lock")
        .execute(
            "UPDATE feeds SET release_artist = ?1 WHERE feed_guid = 'album-haleen-b'",
            rusqlite::params![" haleen "],
        )
        .expect("set release_artist directly");

    let body = artist_names(&st, "").await;
    let rows = rows_of(&body);
    let key = stophammer::model::sort_key("Haleen");
    let matching: Vec<&Value> = rows
        .iter()
        .filter(|r| r["kind"] == "name" && r["key"] == key)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "ADR 0070 §3: the two spellings merge into one name row: {rows:?}"
    );
    assert_eq!(matching[0]["release_count"], json!(2));
}

#[tokio::test]
async fn a_publisher_row_and_a_name_row_with_the_same_text_both_show_with_the_name_row_first() {
    let st = state(common::test_db_arc());
    ingest_titled(
        &st,
        "pub-haleen",
        "Haleen",
        "publisher",
        "Some Label",
        json!([]),
    )
    .await;
    ingest(&st, "album-haleen-solo", "music", "Haleen", json!([])).await;

    let body = artist_names(&st, "").await;
    let rows = rows_of(&body);
    let key = stophammer::model::sort_key("Haleen");

    let publisher_row = find_row(&rows, "publisher", "pub-haleen")
        .unwrap_or_else(|| panic!("no publisher row: {rows:?}"));
    let name_row = find_row(&rows, "name", &key).unwrap_or_else(|| panic!("no name row: {rows:?}"));

    assert_eq!(publisher_row["sort_name"], json!(key));
    assert_eq!(name_row["sort_name"], json!(key));

    let publisher_index = rows
        .iter()
        .position(|r| r == publisher_row)
        .expect("publisher row index");
    let name_index = rows
        .iter()
        .position(|r| r == name_row)
        .expect("name row index");
    assert!(
        name_index < publisher_index,
        "ADR 0070 §3: on a tied sort_name, kind \"name\" comes before \"publisher\": {rows:?}"
    );
}

// ── Pagination and from ─────────────────────────────────────────────────────

#[tokio::test]
async fn pages_of_two_give_each_row_one_time_and_from_starts_at_the_first_key_at_or_after_it() {
    let st = state(common::test_db_arc());
    ingest_titled(
        &st,
        "page-apple-pub",
        "Apple Publisher",
        "publisher",
        "Label",
        json!([]),
    )
    .await;
    ingest_titled(
        &st,
        "page-harbor-pub",
        "Harbor Publisher",
        "publisher",
        "Label",
        json!([]),
    )
    .await;
    ingest(&st, "page-honey-album", "music", "Honey Artist", json!([])).await;
    ingest(&st, "page-mango-album", "music", "Mango Artist", json!([])).await;
    ingest(
        &st,
        "page-zephyr-album",
        "music",
        "Zephyr Artist",
        json!([]),
    )
    .await;

    let mut seen: Vec<(String, String)> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        pages += 1;
        assert!(pages <= 10, "pagination must terminate within 10 pages");
        let query = match &cursor {
            Some(c) => format!("limit=2&cursor={c}"),
            None => "limit=2".to_string(),
        };
        let body = artist_names(&st, &query).await;
        let rows = rows_of(&body);
        for row in &rows {
            seen.push((
                row["kind"].as_str().expect("kind").to_string(),
                row["key"].as_str().expect("key").to_string(),
            ));
        }
        let has_more = body["pagination"]["has_more"]
            .as_bool()
            .expect("has_more must be a bool");
        if !has_more {
            assert!(
                body["pagination"]["cursor"].is_null(),
                "has_more false must pair with no cursor"
            );
            break;
        }
        cursor = Some(
            body["pagination"]["cursor"]
                .as_str()
                .expect("a page with has_more true must give a cursor")
                .to_string(),
        );
    }

    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        seen.len(),
        5,
        "each of the 5 rows must appear exactly one time: {seen:?}"
    );
    assert_eq!(unique.len(), 5, "no row repeats across pages: {seen:?}");

    let from_body = artist_names(&st, "from=h").await;
    let from_rows = rows_of(&from_body);
    let from_keys: Vec<String> = from_rows
        .iter()
        .map(|r| r["key"].as_str().expect("key").to_string())
        .collect();
    assert_eq!(
        from_keys,
        vec![
            "page-harbor-pub",
            stophammer::model::sort_key("Honey Artist").as_str(),
            stophammer::model::sort_key("Mango Artist").as_str(),
            stophammer::model::sort_key("Zephyr Artist").as_str(),
        ],
        "ADR 0070 §3: from=h starts at the first key at or after the sort key of \"h\": {from_rows:?}"
    );
}

// ── ADR 0044: the document points at the route's own schema ────────────────

#[tokio::test]
async fn adr0044_guards_pass_for_artist_names() {
    let st = state(common::test_db_arc());
    ingest_titled(
        &st,
        "pub-adr0044",
        "ADR 0044 Publisher",
        "publisher",
        "Some Label",
        json!([]),
    )
    .await;

    let doc = serde_json::to_value(stophammer::openapi::primary_document()).expect("document");
    let schema = &doc["paths"]["/v1/artist-names"]["get"]["responses"]["200"]["content"]["application/json"]
        ["schema"];
    let data_items = &schema["properties"]["data"]["items"];
    let r = data_items
        .get("$ref")
        .and_then(Value::as_str)
        .expect("the data items schema must be a $ref");
    let name = r.trim_start_matches("#/components/schemas/");
    let schemas = doc["components"]["schemas"]
        .as_object()
        .expect("components.schemas");
    assert!(
        schemas.contains_key(name),
        "ADR 0044: {r} names no schema in components.schemas"
    );
    let props: std::collections::BTreeSet<String> = schemas[name]["properties"]
        .as_object()
        .expect("schema properties")
        .keys()
        .cloned()
        .collect();
    assert!(
        !props.is_empty(),
        "the schema of /v1/artist-names names no field"
    );

    let body = artist_names(&st, "").await;
    let rows = rows_of(&body);
    assert!(!rows.is_empty(), "the route must give at least one row");
    for row in &rows {
        for key in row.as_object().expect("a row is an object").keys() {
            assert!(
                props.contains(key),
                "ADR 0044: /v1/artist-names sends `{key}`, and its schema does not name it"
            );
        }
    }
}

// ── ADR 0070 §4a: the in-memory cache ───────────────────────────────────────

#[tokio::test]
async fn two_requests_with_no_change_between_them_build_the_list_one_time() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest_titled(
        &st,
        "cache-pub-1",
        "Cache Publisher One",
        "publisher",
        "Some Label",
        json!([]),
    )
    .await;

    let first = artist_names(&st, "").await;
    assert!(
        find_row(&rows_of(&first), "publisher", "cache-pub-1").is_some(),
        "the first request must see the publisher feed: {first:?}"
    );

    // Change the row directly, bypassing the event log, so `events.seq`
    // does not move. If the second request reuses the first build, it
    // still shows the feed under its old title.
    db.lock()
        .expect("lock")
        .execute(
            "UPDATE feeds SET title = 'Renamed With No Event' WHERE feed_guid = 'cache-pub-1'",
            [],
        )
        .expect("rename directly, with no event");

    let second = artist_names(&st, "").await;
    let second_rows = rows_of(&second);
    let row = find_row(&second_rows, "publisher", "cache-pub-1")
        .unwrap_or_else(|| panic!("no publisher row: {second:?}"));
    assert_eq!(
        row["name"],
        json!("Cache Publisher One"),
        "ADR 0070 §4a: with no change to events.seq, the second request reuses the first build: {second:?}"
    );
}

#[tokio::test]
async fn a_request_before_the_interval_ends_gives_the_old_copy() {
    let db = common::test_db_arc();
    let st = state_with_cache_interval(Arc::clone(&db), std::time::Duration::from_secs(3));
    ingest_titled(
        &st,
        "cache-pub-before",
        "Cache Publisher Before",
        "publisher",
        "Some Label",
        json!([]),
    )
    .await;
    let first = artist_names(&st, "").await;
    assert!(
        find_row(&rows_of(&first), "publisher", "cache-pub-before").is_some(),
        "the first request must build the list: {first:?}"
    );

    ingest_titled(
        &st,
        "cache-pub-after",
        "Cache Publisher After",
        "publisher",
        "Some Label",
        json!([]),
    )
    .await;

    let second = artist_names(&st, "").await;
    assert!(
        find_row(&rows_of(&second), "publisher", "cache-pub-after").is_none(),
        "ADR 0070 §4a: events.seq changed, but the last build is well inside the interval, \
         so the new feed must not show yet: {second:?}"
    );
}

#[tokio::test]
async fn a_request_after_the_interval_gives_the_change() {
    let db = common::test_db_arc();
    let st = state_with_cache_interval(Arc::clone(&db), std::time::Duration::from_millis(30));
    ingest_titled(
        &st,
        "cache-pub-old",
        "Cache Publisher Old",
        "publisher",
        "Some Label",
        json!([]),
    )
    .await;
    let first = artist_names(&st, "").await;
    assert!(
        find_row(&rows_of(&first), "publisher", "cache-pub-old").is_some(),
        "the first request must build the list: {first:?}"
    );

    ingest_titled(
        &st,
        "cache-pub-new",
        "Cache Publisher New",
        "publisher",
        "Some Label",
        json!([]),
    )
    .await;

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let second = artist_names(&st, "").await;
    assert!(
        find_row(&rows_of(&second), "publisher", "cache-pub-new").is_some(),
        "ADR 0070 §4a: events.seq changed, and the last build is older than the interval, \
         so a request now must rebuild: {second:?}"
    );
}
