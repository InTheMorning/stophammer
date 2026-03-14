mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-issue6.key")
            .expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db,
        chain: Arc::new(stophammer::verify::VerifierChain::new(vec![])),
        signer,
        node_pubkey_hex:  pubkey,
        admin_token:      "test-admin-token".into(),
        sync_token:      None,
        push_client:      reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

fn seed_artist(conn: &rusqlite::Connection, artist_id: &str, name: &str) {
    let now = common::now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![artist_id, name, name.to_lowercase(), now, now],
    )
    .expect("insert artist");
}

fn insert_credit(conn: &rusqlite::Connection, artist_id: &str, display_name: &str) -> i64 {
    let now = common::now();
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        rusqlite::params![display_name, now],
    )
    .expect("insert artist_credit");
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, ?2, 0, ?3, '')",
        rusqlite::params![credit_id, artist_id, display_name],
    )
    .expect("insert artist_credit_name");
    credit_id
}

fn insert_feed(
    conn: &rusqlite::Connection,
    guid: &str,
    title: &str,
    credit_id: i64,
    newest_item_at: Option<i64>,
) {
    let now = common::now();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         explicit, episode_count, newest_item_at, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, ?7, ?8)",
        rusqlite::params![
            guid,
            format!("https://example.com/{guid}"),
            title,
            title.to_lowercase(),
            credit_id,
            newest_item_at,
            now,
            now,
        ],
    )
    .expect("insert feed");
}

// ---------------------------------------------------------------------------
// Issue #6: load_credits_batch exists in db.rs and returns correct data
// ---------------------------------------------------------------------------

#[test]
fn load_credits_batch_returns_all_requested_credits() {
    let conn = common::test_db();
    seed_artist(&conn, "a1", "Artist One");
    seed_artist(&conn, "a2", "Artist Two");
    let c1 = insert_credit(&conn, "a1", "Artist One");
    let c2 = insert_credit(&conn, "a2", "Artist Two");

    let result = stophammer::db::load_credits_batch(&conn, &[c1, c2])
        .expect("load_credits_batch should succeed");

    assert_eq!(result.len(), 2, "should return 2 credits");
    assert!(result.contains_key(&c1), "should contain credit {c1}");
    assert!(result.contains_key(&c2), "should contain credit {c2}");

    let credit1 = &result[&c1];
    assert_eq!(credit1.display_name, "Artist One");
    assert_eq!(credit1.names.len(), 1);
    assert_eq!(credit1.names[0].artist_id, "a1");

    let credit2 = &result[&c2];
    assert_eq!(credit2.display_name, "Artist Two");
    assert_eq!(credit2.names.len(), 1);
    assert_eq!(credit2.names[0].artist_id, "a2");
}

#[test]
fn load_credits_batch_returns_empty_map_for_empty_input() {
    let conn = common::test_db();
    let result = stophammer::db::load_credits_batch(&conn, &[])
        .expect("load_credits_batch should succeed on empty input");
    assert!(result.is_empty(), "should return empty map for empty input");
}

#[test]
fn load_credits_batch_handles_multi_name_credits() {
    let conn = common::test_db();
    seed_artist(&conn, "a1", "Artist One");
    seed_artist(&conn, "a2", "Artist Two");

    let now = common::now();
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        rusqlite::params!["Artist One & Artist Two", now],
    )
    .expect("insert artist_credit");
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, ?2, 0, ?3, ' & ')",
        rusqlite::params![credit_id, "a1", "Artist One"],
    )
    .expect("insert acn 1");
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, ?2, 1, ?3, '')",
        rusqlite::params![credit_id, "a2", "Artist Two"],
    )
    .expect("insert acn 2");

    let result = stophammer::db::load_credits_batch(&conn, &[credit_id])
        .expect("load_credits_batch should succeed");

    assert_eq!(result.len(), 1);
    let credit = &result[&credit_id];
    assert_eq!(credit.display_name, "Artist One & Artist Two");
    assert_eq!(credit.names.len(), 2);
    assert_eq!(credit.names[0].position, 0);
    assert_eq!(credit.names[1].position, 1);
}

// ---------------------------------------------------------------------------
// Issue #6: GET /v1/artists/{id}/feeds uses batch loading (integration test)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn artist_feeds_returns_correct_credits_for_multiple_feeds() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_artist(&conn, "a1", "Test Artist");
        let c1 = insert_credit(&conn, "a1", "Credit Alpha");
        let c2 = insert_credit(&conn, "a1", "Credit Beta");
        // Both feeds credited to a1 but with different credit rows
        insert_feed(&conn, "feed-1", "Album Alpha", c1, Some(1000));
        insert_feed(&conn, "feed-2", "Album Beta", c2, Some(2000));
        drop(conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/v1/artists/a1/feeds")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 200);

    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("parse JSON");
    let data = body["data"].as_array().expect("data is array");
    assert_eq!(data.len(), 2, "should return 2 feeds");

    // Each feed should have its own distinct credit
    let credit_names: Vec<&str> = data
        .iter()
        .map(|f| f["artist_credit"]["display_name"].as_str().unwrap())
        .collect();
    assert!(credit_names.contains(&"Credit Alpha"));
    assert!(credit_names.contains(&"Credit Beta"));
}

// ---------------------------------------------------------------------------
// Issue #6: GET /v1/recent uses batch loading (integration test)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn recent_feeds_returns_correct_credits() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_artist(&conn, "a1", "Artist One");
        seed_artist(&conn, "a2", "Artist Two");
        let c1 = insert_credit(&conn, "a1", "Artist One");
        let c2 = insert_credit(&conn, "a2", "Artist Two");
        insert_feed(&conn, "feed-r1", "Recent One", c1, Some(3000));
        insert_feed(&conn, "feed-r2", "Recent Two", c2, Some(4000));
        drop(conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/v1/recent")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 200);

    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("parse JSON");
    let data = body["data"].as_array().expect("data is array");
    assert_eq!(data.len(), 2, "should return 2 feeds");

    let credit_names: Vec<&str> = data
        .iter()
        .map(|f| f["artist_credit"]["display_name"].as_str().unwrap())
        .collect();
    assert!(credit_names.contains(&"Artist One"));
    assert!(credit_names.contains(&"Artist Two"));
}
