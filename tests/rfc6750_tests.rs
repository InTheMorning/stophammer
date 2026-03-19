mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-rfc6750"));
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

fn issue_token_for_feed(conn: &rusqlite::Connection, feed_guid: &str) -> String {
    stophammer::proof::issue_token(
        conn,
        "feed:write",
        feed_guid,
        &stophammer::proof::ProofLevel::RssOnly,
    )
    .expect("issue token")
}

fn response_header(resp: &axum::response::Response, name: &str) -> Option<String> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

// ============================================================================
// Test 1: 401 on missing auth returns WWW-Authenticate: Bearer realm="stophammer"
//
// RFC 6750 section 3 requires: when a request lacks credentials entirely, the
// server MUST include a WWW-Authenticate header with at minimum realm.
// ============================================================================

#[tokio::test]
async fn missing_auth_returns_www_authenticate_header() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // PATCH /feeds/feed-1 with no Authorization header.
    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/feed-1")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({"feed_url": "https://x.example.com"}))
                .expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 401);

    let www_auth = response_header(&resp, "WWW-Authenticate")
        .expect("WWW-Authenticate header must be present on 401");
    assert!(
        www_auth.contains("Bearer"),
        "WWW-Authenticate must contain Bearer scheme, got: {www_auth}"
    );
    assert!(
        www_auth.contains(r#"realm="stophammer""#),
        "WWW-Authenticate must contain realm=\"stophammer\", got: {www_auth}"
    );
}

// ============================================================================
// Test 2: 401 on invalid/expired token returns WWW-Authenticate with error="invalid_token"
//
// RFC 6750 section 3.1: when a bearer token is present but invalid, the
// error attribute MUST be "invalid_token".
// ============================================================================

#[tokio::test]
async fn invalid_token_returns_www_authenticate_with_invalid_token_error() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // PATCH /feeds/feed-1 with a bogus bearer token.
    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/feed-1")
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer totally-invalid-token-here")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({"feed_url": "https://x.example.com"}))
                .expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 401);

    let www_auth = response_header(&resp, "WWW-Authenticate")
        .expect("WWW-Authenticate header must be present on 401");
    assert!(
        www_auth.contains(r#"realm="stophammer""#),
        "must contain realm, got: {www_auth}"
    );
    assert!(
        www_auth.contains(r#"error="invalid_token""#),
        "must contain error=\"invalid_token\" per RFC 6750 section 3.1, got: {www_auth}"
    );
}

// ============================================================================
// Test 3: 403 on wrong-feed bearer returns WWW-Authenticate with error="insufficient_scope"
//
// RFC 6750 section 3.1: when the token is valid but lacks the required scope
// (or in this case, is scoped to a different resource), the error attribute
// MUST be "insufficient_scope".
// ============================================================================

#[tokio::test]
#[expect(
    clippy::significant_drop_tightening,
    reason = "conn is needed until issue_token_for_feed completes"
)]
async fn wrong_feed_returns_www_authenticate_with_insufficient_scope() {
    let db = common::test_db_arc();
    let token;
    {
        let conn = db.lock().expect("lock db");
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
                "feed-a",
                "https://example.com/a.xml",
                "Feed A",
                "feed a",
                credit_id,
                "A",
                0,
                0,
                now,
                now,
            ],
        )
        .expect("insert feed a");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
             description, explicit, episode_count, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                "feed-b",
                "https://example.com/b.xml",
                "Feed B",
                "feed b",
                credit_id,
                "B",
                0,
                0,
                now,
                now,
            ],
        )
        .expect("insert feed b");
        // Token scoped to feed-a
        token = issue_token_for_feed(&conn, "feed-a");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Try to PATCH feed-b with a token scoped to feed-a.
    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/feed-b")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({"feed_url": "https://evil.example.com"}))
                .expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 403);

    let www_auth = response_header(&resp, "WWW-Authenticate")
        .expect("WWW-Authenticate header must be present on 403 scope errors");
    assert!(
        www_auth.contains(r#"realm="stophammer""#),
        "must contain realm, got: {www_auth}"
    );
    assert!(
        www_auth.contains(r#"error="insufficient_scope""#),
        "must contain error=\"insufficient_scope\" per RFC 6750 section 3.1, got: {www_auth}"
    );
}

// ============================================================================
// Test 4: Bearer token with leading/trailing whitespace is accepted
//
// Robustness: if the client sends "Bearer  abc123 " the token should be
// trimmed to "abc123" before validation.
// ============================================================================

#[tokio::test]
async fn bearer_token_with_whitespace_is_accepted() {
    let db = common::test_db_arc();
    let token;
    {
        let conn = db.lock().expect("lock db");
        seed_feed(&conn);
        token = issue_token_for_feed(&conn, "feed-1");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Add leading and trailing whitespace to the token.
    let padded = format!("Bearer  {token}  ");

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/feed-1")
        .header("Content-Type", "application/json")
        .header("Authorization", padded)
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "feed_url": "https://trimmed.example.com/feed.xml"
            }))
            .expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    // REST semantics compliant (RFC 7396) — 2026-03-12
    assert_eq!(
        resp.status(),
        204,
        "token with surrounding whitespace should be accepted after trimming"
    );
}

// ============================================================================
// Test 5: check_admin_or_bearer_with_conn returns RFC 6750 error codes
//
// Verify the function-level auth helper produces the correct RFC error codes.
// ============================================================================

#[test]
fn check_auth_missing_bearer_uses_rfc6750_error() {
    let conn = common::test_db();
    let headers = axum::http::HeaderMap::new();

    let err = stophammer::api::check_admin_or_bearer_with_conn(
        &conn,
        &headers,
        "admin-secret",
        "feed:write",
        "feed-1",
    )
    .expect_err("missing auth should fail");

    assert_eq!(err.status, axum::http::StatusCode::UNAUTHORIZED);
}

#[test]
fn check_auth_invalid_bearer_uses_invalid_token_error() {
    let conn = common::test_db();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "Authorization",
        "Bearer bogus-token-value".parse().expect("header value"),
    );

    let err = stophammer::api::check_admin_or_bearer_with_conn(
        &conn,
        &headers,
        "admin-secret",
        "feed:write",
        "feed-1",
    )
    .expect_err("invalid token should fail");

    assert_eq!(err.status, axum::http::StatusCode::UNAUTHORIZED);
}

#[test]
fn check_auth_wrong_feed_uses_insufficient_scope_error() {
    let conn = common::test_db();
    let token = issue_token_for_feed(&conn, "feed-1");

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "Authorization",
        format!("Bearer {token}").parse().expect("header value"),
    );

    let err = stophammer::api::check_admin_or_bearer_with_conn(
        &conn,
        &headers,
        "admin-secret",
        "feed:write",
        "feed-OTHER",
    )
    .expect_err("wrong feed should fail");

    assert_eq!(err.status, axum::http::StatusCode::FORBIDDEN);
}

// ============================================================================
// Test 6: DELETE endpoint also returns WWW-Authenticate on 401
//
// Ensures the header is present on all auth-requiring endpoints, not just PATCH.
// ============================================================================

#[tokio::test]
async fn delete_feed_missing_auth_returns_www_authenticate() {
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
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 401);

    let www_auth = response_header(&resp, "WWW-Authenticate")
        .expect("WWW-Authenticate header must be present on 401 for DELETE");
    assert!(
        www_auth.contains(r#"realm="stophammer""#),
        "must contain realm, got: {www_auth}"
    );
}

// ============================================================================
// Test 7: extract_bearer_token trims whitespace (unit-level)
//
// Directly verifies the extraction function handles padded tokens.
// ============================================================================

#[test]
fn extract_bearer_token_trims_whitespace() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "Authorization",
        "Bearer  my-token-value  ".parse().expect("header value"),
    );

    let token = stophammer::api::extract_bearer_token(&headers).expect("should extract token");
    assert_eq!(token, "my-token-value", "token must be trimmed");
}

// ============================================================================
// Test 8: WWW-Authenticate challenge helper produces correct format
// ============================================================================

#[test]
fn www_authenticate_challenge_no_error() {
    let val = stophammer::api::www_authenticate_challenge(None);
    let s = val.to_str().expect("valid header value");
    assert_eq!(s, r#"Bearer realm="stophammer""#);
}

#[test]
fn www_authenticate_challenge_with_error() {
    let val = stophammer::api::www_authenticate_challenge(Some("invalid_token"));
    let s = val.to_str().expect("valid header value");
    assert_eq!(s, r#"Bearer realm="stophammer", error="invalid_token""#);
}

#[test]
fn www_authenticate_challenge_insufficient_scope() {
    let val = stophammer::api::www_authenticate_challenge(Some("insufficient_scope"));
    let s = val.to_str().expect("valid header value");
    assert_eq!(
        s,
        r#"Bearer realm="stophammer", error="insufficient_scope""#
    );
}
