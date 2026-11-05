// ADR 0083 section 4, task 002: the node stores the URLs that a record left
// by a move. An operator relocation and a feed delete remove the rows of the
// GUID. No ingest writes the table yet.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "test-admin-token";

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0083-left-url-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: vec!["content_hash".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());

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

fn feed_data(feed_guid: &str) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Left URL Feed",
        "raw_medium": "episodic",
        "explicit": false,
        "tracks": [{
            "track_guid": format!("{feed_guid}-track-01"),
            "title": "Track One",
            "explicit": false
        }]
    })
}

async fn ingest(state: &Arc<stophammer::api::AppState>, crawl_token: &str, guid: &str, url: &str) {
    let payload = serde_json::json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": format!("{guid}-hash"),
        "redirects": [],
        "feed_data": feed_data(guid),
    });
    let resp = stophammer::api::build_router(Arc::clone(state))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert_eq!(resp.status(), 200, "ingest must succeed: {resp:?}");
}

fn left_urls(db: &Arc<Mutex<rusqlite::Connection>>, guid: &str) -> Vec<String> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_left_urls(&conn, guid).expect("read left urls")
}

fn add(db: &Arc<Mutex<rusqlite::Connection>>, guid: &str, url: &str) {
    let conn = db.lock().expect("lock db");
    stophammer::db::add_left_url(&conn, guid, url).expect("add left url");
}

#[test]
fn a_url_added_two_times_gives_one_row() {
    let db = common::test_db_arc();
    add(&db, "g1", "https://left.example/a");
    add(&db, "g1", "https://left.example/a");
    assert_eq!(
        left_urls(&db, "g1"),
        vec!["https://left.example/a".to_string()],
        "a second add of the same URL must give no second row"
    );
}

#[test]
fn remove_left_url_removes_one_row_and_keeps_the_others() {
    let db = common::test_db_arc();
    add(&db, "g1", "https://left.example/a");
    add(&db, "g1", "https://left.example/b");
    add(&db, "g2", "https://left.example/a");
    {
        let conn = db.lock().expect("lock db");
        stophammer::db::remove_left_url(&conn, "g1", "https://left.example/a")
            .expect("remove left url");
    }
    assert_eq!(
        left_urls(&db, "g1"),
        vec!["https://left.example/b".to_string()],
        "only the named URL must go"
    );
    assert_eq!(
        left_urls(&db, "g2"),
        vec!["https://left.example/a".to_string()],
        "the same URL of a different GUID must stay"
    );
}

#[tokio::test]
async fn a_relocation_keeps_the_left_urls_and_adds_the_old_url() {
    let crawl_token = "adr0083-relocate-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let guid = "adr0083-relocate-guid";
    ingest(&state, crawl_token, guid, "https://left.example/feed/a").await;
    add(&db, guid, "https://left.example/old-1");
    add(&db, guid, "https://left.example/old-2");
    add(&db, "adr0083-other-guid", "https://left.example/old-1");

    let req = Request::builder()
        .method("PATCH")
        .uri(format!("/v1/feeds/{guid}"))
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", ADMIN_TOKEN)
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "feed_url": "https://left.example/feed/relocated",
                "reason": "operator relocation for test"
            }))
            .expect("serialize"),
        ))
        .expect("build request");
    let resp = stophammer::api::build_router(Arc::clone(&state))
        .oneshot(req)
        .await
        .expect("send request");
    assert_eq!(resp.status(), 204, "the relocation must succeed: {resp:?}");

    assert_eq!(
        left_urls(&db, guid),
        vec![
            "https://left.example/feed/a".to_string(),
            "https://left.example/old-1".to_string(),
            "https://left.example/old-2".to_string(),
        ],
        "a relocation must keep each left URL and add the URL that it leaves"
    );
    assert_eq!(
        left_urls(&db, "adr0083-other-guid").len(),
        1,
        "a relocation must keep the rows of a different GUID"
    );
}

#[tokio::test]
async fn delete_feed_removes_the_left_urls_of_that_guid() {
    let crawl_token = "adr0083-delete-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let guid = "adr0083-delete-guid";
    ingest(&state, crawl_token, guid, "https://left.example/feed/d").await;
    add(&db, guid, "https://left.example/old");
    add(&db, "adr0083-other-guid", "https://left.example/old");
    {
        let mut conn = db.lock().expect("lock db");
        conn.execute(
            "INSERT INTO feed_move_state (feed_guid, arrival_tag, return_count, last_return_at) \
             VALUES (?1, 'https://left.example/tag', 2, 100)",
            rusqlite::params![guid],
        )
        .expect("insert move state");
        stophammer::db::delete_feed(&mut conn, guid).expect("delete feed");
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM feed_move_state WHERE feed_guid = ?1",
                rusqlite::params![guid],
                |row| row.get(0),
            )
            .expect("count move state");
        assert_eq!(rows, 0, "a feed delete must remove the move state");
    }
    assert!(
        left_urls(&db, guid).is_empty(),
        "a feed delete must remove the left URLs of the GUID"
    );
    assert_eq!(
        left_urls(&db, "adr0083-other-guid").len(),
        1,
        "a feed delete must keep the rows of a different GUID"
    );
}

#[test]
fn migration_0061_gives_an_arrival_tag_to_a_record_with_left_urls() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), "token");
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(ingest(&state, "token", "moved", "https://fill.example/b"));
    rt.block_on(ingest(&state, "token", "still", "https://fill.example/c"));
    let conn = db.lock().expect("lock");
    conn.execute(
        "UPDATE feeds SET declared_new_feed_url = 'https://fill.example/a' WHERE feed_guid = 'moved'",
        [],
    )
    .expect("set the tag");
    stophammer::db::add_left_url(&conn, "moved", "https://fill.example/a").expect("add");
    conn.execute_batch(include_str!("../migrations/0061_feed_move_state_fill.sql"))
        .expect("run the migration");
    assert_eq!(
        stophammer::db::get_move_state(&conn, "moved")
            .expect("state")
            .arrival_tag
            .as_deref(),
        Some("https://fill.example/a"),
        "the stored tag must become the arrival tag (ADR 0083 section 4)"
    );
    assert_eq!(
        stophammer::db::get_move_state(&conn, "still")
            .expect("state")
            .arrival_tag,
        None,
        "a record with no left URL must get no arrival tag"
    );
}
