mod common;

use rusqlite::params;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn insert_artist(conn: &rusqlite::Connection, artist_id: &str, name: &str, now: i64) {
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![artist_id, name, name.to_lowercase(), now, now],
    )
    .expect("insert artist");
}

fn insert_artist_credit(
    conn: &rusqlite::Connection,
    artist_id: &str,
    display_name: &str,
    now: i64,
) -> i64 {
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        params![display_name, now],
    )
    .expect("insert artist_credit");
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![credit_id, artist_id, 0, display_name, ""],
    )
    .expect("insert artist_credit_name");
    credit_id
}

fn insert_feed(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    feed_url: &str,
    title: &str,
    credit_id: i64,
    now: i64,
) {
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         description, explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            feed_guid, feed_url, title, title.to_lowercase(),
            credit_id, "A test feed", 0, 0, now, now,
        ],
    )
    .expect("insert feed");
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
        params![
            track_guid, feed_guid, credit_id, title,
            title.to_lowercase(), "A test track", 0, now, now,
        ],
    )
    .expect("insert track");
}

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-auth-atomicity.key")
            .expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
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

fn seed_feed(conn: &rusqlite::Connection) -> (i64, i64) {
    let now = common::now();
    insert_artist(conn, "artist-1", "Test Artist", now);
    let credit_id = insert_artist_credit(conn, "artist-1", "Test Artist", now);
    insert_feed(conn, "feed-1", "https://example.com/feed.xml", "Test Album", credit_id, now);
    (credit_id, now)
}

fn issue_token_for_feed(conn: &rusqlite::Connection, feed_guid: &str) -> String {
    stophammer::proof::issue_token(conn, "feed:write", feed_guid, &stophammer::proof::ProofLevel::RssOnly)
        .expect("issue token")
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

use http::Request;
use tower::ServiceExt;

// ============================================================================
// Test 1: check_admin_or_bearer_with_conn accepts valid admin token
//
// Verifies the new conn-based auth function correctly validates admin tokens
// without needing to acquire a separate lock.
// ============================================================================

#[test]
fn check_admin_or_bearer_with_conn_accepts_admin_token() {
    let conn = common::test_db();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("X-Admin-Token", "my-secret".parse().expect("header value"));

    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn, &headers, "my-secret", "feed:write", "feed-1",
    );
    assert!(result.is_ok(), "admin token should be accepted");
}

// ============================================================================
// Test 2: check_admin_or_bearer_with_conn rejects wrong admin token
// ============================================================================

#[test]
fn check_admin_or_bearer_with_conn_rejects_bad_admin_token() {
    let conn = common::test_db();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("X-Admin-Token", "wrong-token".parse().expect("header value"));

    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn, &headers, "my-secret", "feed:write", "feed-1",
    );
    assert!(result.is_err(), "wrong admin token should be rejected");
}

// ============================================================================
// Test 3: check_admin_or_bearer_with_conn accepts valid bearer token
//
// The conn-based variant must validate bearer tokens using the provided
// connection rather than acquiring a new lock -- proving auth and DB ops
// share the same lock scope.
// ============================================================================

#[test]
fn check_admin_or_bearer_with_conn_accepts_valid_bearer() {
    let conn = common::test_db();
    let token = issue_token_for_feed(&conn, "feed-1");

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "Authorization",
        format!("Bearer {token}").parse().expect("header value"),
    );

    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn, &headers, "admin-secret", "feed:write", "feed-1",
    );
    assert!(result.is_ok(), "valid bearer token should be accepted");
}

// ============================================================================
// Test 4: check_admin_or_bearer_with_conn rejects bearer for wrong feed
// ============================================================================

#[test]
fn check_admin_or_bearer_with_conn_rejects_wrong_feed() {
    let conn = common::test_db();
    let token = issue_token_for_feed(&conn, "feed-1");

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "Authorization",
        format!("Bearer {token}").parse().expect("header value"),
    );

    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn, &headers, "admin-secret", "feed:write", "feed-OTHER",
    );
    assert!(result.is_err(), "bearer for wrong feed should be rejected");
}

// ============================================================================
// Test 5: check_admin_or_bearer_with_conn rejects missing auth
// ============================================================================

#[test]
fn check_admin_or_bearer_with_conn_rejects_missing_auth() {
    let conn = common::test_db();
    let headers = axum::http::HeaderMap::new();

    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn, &headers, "admin-secret", "feed:write", "feed-1",
    );
    assert!(result.is_err(), "missing auth should be rejected");
}

// ============================================================================
// Test 6: DELETE /feeds/{guid} with admin token uses atomic auth+write
//
// End-to-end test: the handler must succeed with admin auth, proving
// the single-lock-scope path works through the full handler.
// ============================================================================

#[tokio::test]
async fn retire_feed_admin_atomic() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/feed-1")
        .header("X-Admin-Token", "test-admin-token")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 204, "retire with admin token should return 204");

    // Verify feed actually deleted.
    let count: i64 = {
        let conn = db.lock().expect("lock db");
        conn.query_row("SELECT COUNT(*) FROM feeds WHERE feed_guid = 'feed-1'", [], |r| r.get(0))
            .expect("count feeds")
    };
    assert_eq!(count, 0, "feed should be deleted after retire");
}

// ============================================================================
// Test 7: DELETE /feeds/{guid} with bearer token uses atomic auth+write
// ============================================================================

#[tokio::test]
async fn retire_feed_bearer_atomic() {
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
        .method("DELETE")
        .uri("/v1/feeds/feed-1")
        .header("Authorization", format!("Bearer {token}"))
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 204, "retire with bearer token should return 204");
}

// ============================================================================
// Test 8: DELETE /feeds/{guid}/tracks/{track_guid} with bearer uses atomic auth+write
// ============================================================================

#[tokio::test]
async fn remove_track_bearer_atomic() {
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
        .method("DELETE")
        .uri("/v1/feeds/feed-1/tracks/track-1")
        .header("Authorization", format!("Bearer {token}"))
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 204, "remove track with bearer token should return 204");

    let count: i64 = {
        let conn = db.lock().expect("lock db");
        conn.query_row("SELECT COUNT(*) FROM tracks WHERE track_guid = 'track-1'", [], |r| r.get(0))
            .expect("count tracks")
    };
    assert_eq!(count, 0, "track should be deleted");
}

// ============================================================================
// Test 9: PATCH /feeds/{guid} with bearer uses atomic auth+write
// REST semantics compliant (RFC 7396) — 2026-03-12
// ============================================================================

#[tokio::test]
async fn patch_feed_bearer_atomic() {
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
    assert_eq!(resp.status(), 204);

    let url: String = {
        let conn = db.lock().expect("lock db");
        conn.query_row("SELECT feed_url FROM feeds WHERE feed_guid = 'feed-1'", [], |r| r.get(0))
            .expect("get feed_url")
    };
    assert_eq!(url, "https://new-url.example.com/feed.xml");
}

// ============================================================================
// Test 10: PATCH /tracks/{guid} with bearer uses atomic auth+write
// REST semantics compliant (RFC 7396) — 2026-03-12
// ============================================================================

#[tokio::test]
async fn patch_track_bearer_atomic() {
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
    assert_eq!(resp.status(), 204);

    let url: String = {
        let conn = db.lock().expect("lock db");
        conn.query_row("SELECT enclosure_url FROM tracks WHERE track_guid = 'track-1'", [], |r| r.get(0))
            .expect("get enclosure_url")
    };
    assert_eq!(url, "https://cdn.example.com/new-song.mp3");
}

// ============================================================================
// Test 11: Bearer token for wrong feed cannot retire a different feed
//
// Verifies that auth validation inside the atomic scope correctly rejects
// cross-feed bearer tokens.
// ============================================================================

#[tokio::test]
#[expect(clippy::significant_drop_tightening, reason = "conn is needed until issue_token_for_feed completes")]
async fn retire_feed_bearer_wrong_feed_returns_403() {
    let db = common::test_db_arc();
    let token;
    {
        let conn = db.lock().expect("lock db");
        let now = common::now();
        insert_artist(&conn, "artist-1", "Test Artist", now);
        let credit_id = insert_artist_credit(&conn, "artist-1", "Test Artist", now);
        insert_feed(&conn, "feed-1", "https://example.com/a.xml", "Feed A", credit_id, now);
        insert_feed(&conn, "feed-2", "https://example.com/b.xml", "Feed B", credit_id, now);
        // Token is scoped to feed-2, but we try to delete feed-1.
        token = issue_token_for_feed(&conn, "feed-2");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/feed-1")
        .header("Authorization", format!("Bearer {token}"))
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 403, "bearer for wrong feed should return 403");

    // Verify feed-1 was NOT deleted.
    let count: i64 = {
        let conn = db.lock().expect("lock db");
        conn.query_row("SELECT COUNT(*) FROM feeds WHERE feed_guid = 'feed-1'", [], |r| r.get(0))
            .expect("count feeds")
    };
    assert_eq!(count, 1, "feed should still exist after rejected auth");
}
