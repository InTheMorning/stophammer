// v4vmm request 5: a search gives each entity one time.
//
// A database from before the feed-scoped track identity keeps a search row
// and a quality row for each track under its bare track GUID. Search then
// gives the track two times. `db::try_open_db` rebuilds the search index
// when such a row exists.

mod common;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const TOKEN: &str = "one-hit-token";
const FEED_GUID: &str = "one-hit-feed";
const TRACK_GUID: &str = "one-hit-track";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-one-hit-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
    })
}

async fn send(app: axum::Router, request: Request<Body>) -> serde_json::Value {
    let resp = app.oneshot(request).await.expect("send request");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    assert!(status.is_success(), "request failed with status {status}");
    serde_json::from_slice(&bytes).expect("parse json")
}

async fn ingest_album(st: &Arc<stophammer::api::AppState>) {
    let payload = serde_json::json!({
        "canonical_url": "https://example.com/one-hit.xml",
        "source_url": "https://example.com/one-hit.xml",
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": "hash-one-hit",
        "feed_data": {
            "feed_guid": FEED_GUID,
            "title": "One Hit Album",
            "raw_medium": "music",
            "explicit": false,
            "author_name": "One Hit Artist",
            "tracks": [{
                "track_guid": TRACK_GUID,
                "title": "The Arbiter",
                "pub_date": 1_700_000_000,
                "explicit": false,
                "payment_routes": [], "value_time_splits": []
            }]
        }
    });
    let request = Request::builder()
        .method("POST")
        .uri("/ingest/feed")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
        .expect("build request");
    send(stophammer::api::build_router(Arc::clone(st)), request).await;
}

async fn track_hits(path: &Path) -> Vec<serde_json::Value> {
    let st = state(Arc::new(Mutex::new(stophammer::db::open_db(path))));
    let request = Request::builder()
        .uri("/v1/search?q=arbiter&type=track")
        .body(Body::empty())
        .expect("build request");
    let body = send(stophammer::api::build_router(st), request).await;
    body["data"].as_array().expect("search data").clone()
}

fn bare_track_rows(path: &Path, table: &str) -> i64 {
    let conn = rusqlite::Connection::open(path).expect("open db");
    conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM {table} \
             WHERE entity_type = 'track' AND entity_id NOT LIKE '[\"%'"
        ),
        [],
        |row| row.get(0),
    )
    .expect("count bare track rows")
}

#[tokio::test]
async fn opening_a_database_with_bare_guid_track_rows_gives_each_track_one_time() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("one-hit.db");

    {
        let db = Arc::new(Mutex::new(stophammer::db::open_db(&path)));
        let st = state(Arc::clone(&db));
        ingest_album(&st).await;
        // The rows that a track got before the feed-scoped identity.
        let conn = db.lock().expect("lock db");
        stophammer::search::populate_search_index(
            &conn,
            "track",
            TRACK_GUID,
            "One Hit Artist",
            "The Arbiter",
            "",
            "",
        )
        .expect("write a bare-GUID search row");
        stophammer::quality::store_quality(&conn, "track", TRACK_GUID, 10)
            .expect("write a bare-GUID quality row");
    }

    // A raw connection does not run the repair, so this shows the defect.
    {
        let conn = rusqlite::Connection::open(&path).expect("open db");
        let hits = stophammer::search::search(&conn, "arbiter", Some("track"), 10, None, None)
            .expect("search before the repair");
        assert_eq!(
            hits.len(),
            2,
            "before the repair, the bare-GUID row must give the track a second time"
        );
    }

    let hits = track_hits(&path).await;
    assert_eq!(
        hits.len(),
        1,
        "after the repair, the search must give the track one time: {hits:?}"
    );
    assert_eq!(
        hits[0]["entity_id"], TRACK_GUID,
        "the hit must be the track"
    );
    assert_eq!(
        hits[0]["href"],
        format!("/v1/feeds/{FEED_GUID}/tracks/{TRACK_GUID}"),
        "the hit must link to the track in its feed"
    );
    assert_eq!(
        bare_track_rows(&path, "search_entities"),
        0,
        "the repair must remove each bare-GUID search row"
    );
    assert_eq!(
        bare_track_rows(&path, "entity_quality"),
        0,
        "the repair must remove each bare-GUID quality row"
    );

    // A second open finds no bare-GUID row and keeps the index as it is.
    assert_eq!(
        track_hits(&path).await.len(),
        1,
        "a second open must give the track one time"
    );
}
