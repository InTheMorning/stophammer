// Sprint 4A: PATCH endpoints return 204 No Content (RFC 7396 compliance)
// REST semantics compliant (RFC 7396) — 2026-03-12

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
    let temp_dir = tempfile::tempdir().expect("create temp signer dir");
    let key_path = temp_dir.path().join("test-sprint4a.key");
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create(&key_path).expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(vec![])),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

fn seed_feed(conn: &rusqlite::Connection) -> (i64, i64) {
    let now = common::now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params!["artist-1", "Test Artist", "test artist", now, now],
    )
    .expect("insert artist");
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        rusqlite::params!["Test Artist", now],
    )
    .expect("insert artist_credit");
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![credit_id, "artist-1", 0, "Test Artist", ""],
    )
    .expect("insert artist_credit_name");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         description, explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            "feed-1",
            "https://example.com/feed.xml",
            "Test Album",
            "test album",
            credit_id,
            "A test feed",
            0,
            0,
            now,
            now,
        ],
    )
    .expect("insert feed");
    (credit_id, now)
}

fn insert_track(
    conn: &rusqlite::Connection,
    track_guid: &str,
    feed_guid: &str,
    credit_id: i64,
    title: &str,
    now: i64,
) {
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         description, explicit, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            track_guid,
            feed_guid,
            credit_id,
            title,
            title.to_lowercase(),
            "A test track",
            0,
            now,
            now,
        ],
    )
    .expect("insert track");
}

fn issue_token_for_feed(conn: &rusqlite::Connection, feed_guid: &str) -> String {
    stophammer::proof::issue_token(
        conn,
        "feed:write",
        feed_guid,
        &stophammer::proof::ProofLevel::RssOnly,
    )
    .expect("issue token")
}

// ---------------------------------------------------------------------------
// Test: PATCH /v1/feeds/{guid} with valid token returns 204 No Content, empty body
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_feed_returns_204_no_content() {
    let db = common::test_db_arc();
    let token;
    {
        let conn = db.lock().expect("lock db");
        seed_feed(&conn);
        token = issue_token_for_feed(&conn, "feed-1");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/feed-1")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "feed_url": "https://new-url.example.com/feed.xml"
            }))
            .expect("serialize JSON"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        204,
        "PATCH /v1/feeds/feed-1 should return 204 No Content"
    );

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert!(
        bytes.is_empty(),
        "204 No Content must have an empty body, got {} bytes",
        bytes.len()
    );

    // Verify the URL was actually updated in the database.
    let url: String = {
        let conn = db.lock().expect("lock db");
        conn.query_row(
            "SELECT feed_url FROM feeds WHERE feed_guid = 'feed-1'",
            [],
            |r| r.get(0),
        )
        .expect("get feed_url")
    };
    assert_eq!(url, "https://new-url.example.com/feed.xml");
}

// ---------------------------------------------------------------------------
// Test: PATCH /v1/tracks/{guid} with valid token returns 204 No Content, empty body
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_track_returns_204_no_content() {
    let db = common::test_db_arc();
    let token;
    {
        let conn = db.lock().expect("lock db");
        let (credit_id, now) = seed_feed(&conn);
        insert_track(&conn, "track-1", "feed-1", credit_id, "Song One", now);
        token = issue_token_for_feed(&conn, "feed-1");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/tracks/track-1")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "enclosure_url": "https://cdn.example.com/new-song.mp3"
            }))
            .expect("serialize JSON"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        204,
        "PATCH /v1/tracks/track-1 should return 204 No Content"
    );

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert!(
        bytes.is_empty(),
        "204 No Content must have an empty body, got {} bytes",
        bytes.len()
    );

    // Verify the URL was actually updated in the database.
    let url: String = {
        let conn = db.lock().expect("lock db");
        conn.query_row(
            "SELECT enclosure_url FROM tracks WHERE track_guid = 'track-1'",
            [],
            |r| r.get(0),
        )
        .expect("get enclosure_url")
    };
    assert_eq!(url, "https://cdn.example.com/new-song.mp3");
}

// ---------------------------------------------------------------------------
// Test: PATCH /v1/feeds/{guid} with empty body (no changes) returns 204 No Content
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_feed_empty_body_returns_204_no_content() {
    let db = common::test_db_arc();
    let token;
    {
        let conn = db.lock().expect("lock db");
        seed_feed(&conn);
        token = issue_token_for_feed(&conn, "feed-1");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/feed-1")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({})).expect("serialize JSON"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        204,
        "PATCH with empty body should return 204 No Content"
    );

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert!(
        bytes.is_empty(),
        "204 No Content must have an empty body, got {} bytes",
        bytes.len()
    );
}

// ---------------------------------------------------------------------------
// Test: PATCH /v1/feeds/{guid} with no auth returns 401 with WWW-Authenticate
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_feed_no_auth_returns_401() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/feed-1")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "feed_url": "https://evil.example.com/feed.xml"
            }))
            .expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 401, "PATCH without auth should return 401");

    let www_auth = resp
        .headers()
        .get("WWW-Authenticate")
        .expect("WWW-Authenticate header must be present on 401")
        .to_str()
        .expect("header to str");
    assert!(
        www_auth.contains("Bearer"),
        "WWW-Authenticate must contain Bearer scheme, got: {www_auth}"
    );
}

// ---------------------------------------------------------------------------
// Test: PATCH /v1/tracks/{guid} with no auth returns 401 with WWW-Authenticate
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_track_no_auth_returns_401() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        let (credit_id, now) = seed_feed(&conn);
        insert_track(&conn, "track-1", "feed-1", credit_id, "Song One", now);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/tracks/track-1")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "enclosure_url": "https://evil.example.com/song.mp3"
            }))
            .expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 401, "PATCH without auth should return 401");

    let www_auth = resp
        .headers()
        .get("WWW-Authenticate")
        .expect("WWW-Authenticate header must be present on 401")
        .to_str()
        .expect("header to str");
    assert!(
        www_auth.contains("Bearer"),
        "WWW-Authenticate must contain Bearer scheme, got: {www_auth}"
    );
}
