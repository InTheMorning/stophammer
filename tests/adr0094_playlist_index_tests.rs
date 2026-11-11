//! ADR 0094 §1a: playlist rows stay out of each current search scope.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0094-playlist-crawl-token";
const ADMIN_TOKEN: &str = "adr0094-playlist-admin-token";
const PLAYLIST_GUID: &str = "adr0094-playlist-feed";
const PLAYLIST_TITLE: &str = "Gossamer Playlist Index";

type Db = Arc<Mutex<rusqlite::Connection>>;

fn state(db: Db) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0094-playlist-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: ADMIN_TOKEN.into(),
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
) -> (http::StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("X-Admin-Token", ADMIN_TOKEN);
    let body = if let Some(body) = body {
        request = request.header("Content-Type", "application/json");
        Body::from(serde_json::to_vec(body).expect("serialize request"))
    } else {
        Body::empty()
    };
    let response = stophammer::api::build_router(Arc::clone(st))
        .oneshot(request.body(body).expect("build request"))
        .await
        .expect("send request");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("read response")
        .to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

async fn ingest(st: &Arc<stophammer::api::AppState>, medium: &str, hash: &str) {
    let url = format!("https://example.com/{PLAYLIST_GUID}.xml");
    let body = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": hash,
        "feed_data": {
            "feed_guid": PLAYLIST_GUID,
            "title": PLAYLIST_TITLE,
            "author_name": "Gossamer Artist",
            "raw_medium": medium,
            "explicit": false
        }
    });
    let (status, response) = send(st, "POST", "/ingest/feed", Some(&body)).await;
    assert_eq!(status, http::StatusCode::OK, "ingest response: {response}");
}

fn ids(body: &Value) -> Vec<&str> {
    body["data"]
        .as_array()
        .expect("search data is an array")
        .iter()
        .map(|row| row["entity_id"].as_str().expect("entity_id is a string"))
        .collect()
}

#[tokio::test]
async fn playlist_rows_are_not_hits_in_current_search_scopes() {
    let st = state(common::test_db_arc());
    ingest(&st, "musicL", "playlist-hash-1").await;

    for path in [
        "/v1/search?type=feed&q=Gossamer",
        "/v1/search?type=publisher&q=Gossamer",
        "/v1/search?q=Gossamer",
        "/v1/search?type=feed&title=Gossamer",
        "/v1/search?type=publisher&title=Gossamer",
        "/v1/search?title=Gossamer",
        "/v1/search?type=feed&title=Gossamer&sort=name",
        "/v1/search?type=publisher&title=Gossamer&sort=name",
        "/v1/search?title=Gossamer&sort=name",
    ] {
        let (status, body) = send(&st, "GET", path, None).await;
        assert_eq!(status, http::StatusCode::OK, "{path}: {body}");
        assert!(
            !ids(&body).contains(&PLAYLIST_GUID),
            "ADR 0094 §1a: {path} must not return a playlist row: {body}"
        );
    }
}

#[tokio::test]
async fn medium_changes_move_one_feed_row_between_types() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    for (medium, hash, expected) in [
        ("musicL", "playlist-hash-1", "playlist"),
        ("music", "playlist-hash-2", "feed"),
        ("musicL", "playlist-hash-3", "playlist"),
    ] {
        ingest(&st, medium, hash).await;
        let conn = db.lock().expect("lock writer connection");
        let rows: Vec<String> = conn
            .prepare("SELECT entity_type FROM search_entities WHERE entity_id = ?1")
            .expect("prepare entity row query")
            .query_map([PLAYLIST_GUID], |row| row.get(0))
            .expect("query entity rows")
            .collect::<Result<_, _>>()
            .expect("read entity rows");
        assert_eq!(rows, [expected], "the {medium} feed has its expected row");
    }
}

#[tokio::test]
async fn opening_repairs_a_missing_playlist_row_once() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let path = dir.path().join("adr0094-playlist-repair.db");
    let db = Arc::new(Mutex::new(stophammer::db::open_db(&path)));
    let st = state(Arc::clone(&db));
    ingest(&st, "musicL", "playlist-repair-hash").await;
    {
        let conn = db.lock().expect("lock writer connection");
        stophammer::search::delete_from_search_index(&conn, "playlist", PLAYLIST_GUID)
            .expect("remove playlist row");
    }
    drop(st);
    drop(db);

    let started = std::time::Instant::now();
    let reopened = stophammer::db::open_db(&path);
    let elapsed = started.elapsed();
    let count: i64 = reopened
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'playlist' AND entity_id = ?1",
            [PLAYLIST_GUID],
            |row| row.get(0),
        )
        .expect("count repaired playlist rows");
    assert_eq!(
        count, 1,
        "open adds the missing playlist row in {elapsed:?}"
    );
    drop(reopened);

    let reopened = stophammer::db::open_db(&path);
    let count: i64 = reopened
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'playlist' AND entity_id = ?1",
            [PLAYLIST_GUID],
            |row| row.get(0),
        )
        .expect("count playlist rows after second open");
    assert_eq!(count, 1, "a second open adds no duplicate row");
}

#[tokio::test]
async fn retire_deletes_the_playlist_row() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(&st, "musicL", "playlist-retire-hash").await;
    let (status, body) = send(&st, "DELETE", &format!("/v1/feeds/{PLAYLIST_GUID}"), None).await;
    assert_eq!(
        status,
        http::StatusCode::NO_CONTENT,
        "retire response: {body}"
    );
    let conn = db.lock().expect("lock writer connection");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'playlist' AND entity_id = ?1",
            [PLAYLIST_GUID],
            |row| row.get(0),
        )
        .expect("count playlist rows after retire");
    assert_eq!(count, 0, "retire deletes the playlist row");
}
