// ADR 0071 §5: search finds a publisher feed on request.
//
// `GET /v1/search?type=publisher` gives the feeds with the medium
// `publisher` whose text matches `q`, in the row shape of a feed result,
// with `entity_type` `publisher`. `type=feed`, `type=track` and a search
// with no `type` keep giving only music feeds and their tracks.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0071-publisher-search-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0071-publisher-search-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0071-publisher-search-admin-token".into(),
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

/// Ingests a feed with `guid`, `medium` and `title`, with `content_hash`
/// distinguishing one ingest of the same feed from another so a changed
/// medium is a real re-ingest, not a no-change skip.
async fn ingest_titled(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    medium: &str,
    title: &str,
    content_hash: &str,
) {
    let url = format!("https://example.com/{guid}.xml");
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": content_hash,
        "feed_data": {
            "feed_guid": guid,
            "title": title,
            "raw_medium": medium,
            "explicit": false,
            "author_name": "An Author",
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

fn entity_ids_of_type<'a>(data: &'a Value, entity_type: &str) -> Vec<&'a str> {
    data.as_array()
        .expect("data is an array")
        .iter()
        .filter(|row| row["entity_type"] == entity_type)
        .map(|row| row["entity_id"].as_str().expect("entity_id is a string"))
        .collect()
}

#[tokio::test]
async fn type_publisher_finds_a_publisher_feed_by_title_word() {
    let st = state(common::test_db_arc());
    ingest_titled(
        &st,
        "pub-lighthouse",
        "publisher",
        "Lighthouse Records",
        "hash-1",
    )
    .await;
    ingest_titled(&st, "music-unrelated", "music", "Unrelated Album", "hash-2").await;

    let body = send(&st, "GET", "/v1/search?type=publisher&q=Lighthouse", None).await;
    let ids = entity_ids_of_type(&body["data"], "publisher");
    assert_eq!(
        ids,
        vec!["pub-lighthouse"],
        "ADR 0071 §5: type=publisher gives the publisher feed whose title matches q: {body}"
    );
}

#[tokio::test]
async fn type_feed_and_no_type_give_no_publisher_feed_for_the_same_word() {
    let st = state(common::test_db_arc());
    ingest_titled(
        &st,
        "pub-lighthouse-2",
        "publisher",
        "Lighthouse Records",
        "hash-1",
    )
    .await;

    for uri in [
        "/v1/search?type=feed&q=Lighthouse",
        "/v1/search?q=Lighthouse",
    ] {
        let body = send(&st, "GET", uri, None).await;
        let rows = body["data"].as_array().expect("data is an array");
        assert!(
            rows.iter()
                .all(|row| row["entity_id"] != "pub-lighthouse-2"),
            "{uri}: ADR 0071 §5 keeps a publisher feed out of this search: {body}"
        );
    }
}

#[tokio::test]
async fn a_feed_that_changes_medium_from_publisher_to_music_moves_between_result_sets() {
    let st = state(common::test_db_arc());
    ingest_titled(
        &st,
        "switcher-1",
        "publisher",
        "Switcher Feed Harborlight",
        "hash-1",
    )
    .await;

    let before_publisher = send(&st, "GET", "/v1/search?type=publisher&q=Harborlight", None).await;
    assert!(
        entity_ids_of_type(&before_publisher["data"], "publisher").contains(&"switcher-1"),
        "the feed starts in the publisher results: {before_publisher}"
    );
    let before_feed = send(&st, "GET", "/v1/search?type=feed&q=Harborlight", None).await;
    assert!(
        before_feed["data"]
            .as_array()
            .expect("data is an array")
            .iter()
            .all(|row| row["entity_id"] != "switcher-1"),
        "the feed starts out of the feed results: {before_feed}"
    );

    // Re-ingest the same feed GUID with a changed medium and a new content
    // hash, so this is a real re-ingest rather than a no-change skip.
    ingest_titled(
        &st,
        "switcher-1",
        "music",
        "Switcher Feed Harborlight",
        "hash-2",
    )
    .await;

    let after_publisher = send(&st, "GET", "/v1/search?type=publisher&q=Harborlight", None).await;
    assert!(
        !entity_ids_of_type(&after_publisher["data"], "publisher").contains(&"switcher-1"),
        "ADR 0071 §5: the feed leaves the publisher results after the medium change: {after_publisher}"
    );
    let after_feed = send(&st, "GET", "/v1/search?type=feed&q=Harborlight", None).await;
    assert!(
        entity_ids_of_type(&after_feed["data"], "feed").contains(&"switcher-1"),
        "the feed enters the feed results after the medium change: {after_feed}"
    );
}

#[tokio::test]
async fn capabilities_list_the_publisher_entity_type() {
    let st = state(common::test_db_arc());
    let body = send(&st, "GET", "/v1/node/capabilities", None).await;
    let types = body["entity_types"]
        .as_array()
        .expect("entity_types is an array");
    assert!(
        types.iter().any(|t| t == "publisher"),
        "ADR 0071 §5: capabilities list the publisher search type: {body}"
    );
}

// ── The repair: `ensure_search_index_publisher_feeds` via `open_db` ─────────

#[tokio::test]
async fn the_repair_adds_publisher_feeds_with_no_search_row_and_a_second_open_adds_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("adr0071-publisher-search-repair.db");

    let db = Arc::new(Mutex::new(stophammer::db::open_db(&db_path)));
    let st = state(Arc::clone(&db));
    ingest_titled(
        &st,
        "pub-repair-1",
        "publisher",
        "Repair Feed One",
        "hash-1",
    )
    .await;
    ingest_titled(
        &st,
        "pub-repair-2",
        "publisher",
        "Repair Feed Two",
        "hash-1",
    )
    .await;
    ingest_titled(&st, "music-repair-1", "music", "Repair Music", "hash-1").await;

    {
        let conn = db.lock().expect("lock the writer connection");
        let before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'publisher'",
                [],
                |row| row.get(0),
            )
            .expect("count publisher rows");
        assert_eq!(
            before, 2,
            "a normal ingest already adds the publisher row for each publisher feed"
        );

        // Simulate a database that predates ADR 0071 §5: it has no
        // `search_entities` row for either publisher feed.
        for guid in ["pub-repair-1", "pub-repair-2"] {
            stophammer::search::delete_from_search_index(&conn, "publisher", guid, "", "", "", "")
                .expect("remove the publisher row");
        }
        let missing: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'publisher'",
                [],
                |row| row.get(0),
            )
            .expect("count publisher rows");
        assert_eq!(missing, 0, "the simulated pre-ADR-0071 state holds no row");
    }

    // Release every reference to the writer connection before reopening the
    // same file: `open_db` opens its own connection to the path.
    drop(st);
    drop(db);

    let started = std::time::Instant::now();
    let reopened = stophammer::db::open_db(&db_path);
    let elapsed = started.elapsed();
    let added: i64 = reopened
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'publisher'",
            [],
            |row| row.get(0),
        )
        .expect("count publisher rows");
    assert_eq!(
        added, 2,
        "ADR 0071 §5: the repair adds each missing publisher feed on open, in {elapsed:?}"
    );
    drop(reopened);

    let reopened_again = stophammer::db::open_db(&db_path);
    let unchanged: i64 = reopened_again
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'publisher'",
            [],
            |row| row.get(0),
        )
        .expect("count publisher rows");
    assert_eq!(
        unchanged, 2,
        "a database where each publisher feed already has a row skips the repair"
    );
}
