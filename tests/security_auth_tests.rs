#![expect(clippy::significant_drop_tightening, reason = "MutexGuard<Connection> must be held for the full scope in test setup")]

mod common;

use rusqlite::params;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

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
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-security-auth.key")
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

fn seed_two_feeds(conn: &rusqlite::Connection) -> (i64, i64) {
    let now = common::now();
    insert_artist(conn, "artist-1", "Test Artist", now);
    let credit_id = insert_artist_credit(conn, "artist-1", "Test Artist", now);
    insert_feed(conn, "feed-A", "https://example.com/a.xml", "Feed A", credit_id, now);
    insert_feed(conn, "feed-B", "https://example.com/b.xml", "Feed B", credit_id, now);
    (credit_id, now)
}

fn issue_token_for_feed(conn: &rusqlite::Connection, feed_guid: &str) -> String {
    stophammer::proof::issue_token(conn, "feed:write", feed_guid, &stophammer::proof::ProofLevel::RssOnly)
        .expect("issue token")
}

fn rss_with_podcast_txt(txt_content: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Test Podcast</title>
    <podcast:txt>{txt_content}</podcast:txt>
  </channel>
</rss>"#
    )
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn json_request(method: &str, uri: &str, body: &serde_json::Value) -> Request<axum::body::Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

// ============================================================================
// ATTACK 1: Token replay after feed deletion
//
// Scenario: An attacker obtains a token for feed-A, feed-A is deleted,
// the attacker then tries to use the token to PATCH feed-B.
//
// FINDING: PROTECTED (SG-07). delete_feed now cascade-deletes proof_tokens
// and proof_challenges for the feed, so the token is gone by the time the
// attacker tries to replay it. The PATCH returns 401 (invalid token).
// ============================================================================

#[tokio::test]
async fn attack1_token_replay_after_feed_deletion() {
    let db = common::test_db_arc();
    let token;
    {
        let conn = db.lock().expect("lock db");
        let (credit_id, now) = seed_two_feeds(&conn);
        insert_track(&conn, "track-B1", "feed-B", credit_id, "Song B1", now);
        // Issue token for feed-A
        token = issue_token_for_feed(&conn, "feed-A");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Delete feed-A (with admin token)
    let delete_req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/feed-A")
        .header("X-Admin-Token", "test-admin-token")
        .body(axum::body::Body::empty())
        .expect("build delete request");

    let resp = app.clone().oneshot(delete_req).await.expect("delete feed");
    assert_eq!(resp.status(), 204, "feed-A should be deleted");

    // Verify feed-A is actually deleted
    {
        let conn = db.lock().expect("lock");
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM feeds WHERE feed_guid = 'feed-A'", [], |r| r.get(0),
        ).expect("count");
        assert_eq!(count, 0, "feed-A should be gone");
    }

    // SG-07: token for feed-A should be cleaned up on feed delete
    {
        let conn = db.lock().expect("lock");
        let result = stophammer::proof::validate_token(&conn, &token, "feed:write").expect("validate");
        assert_eq!(result, None,
            "token should be deleted when feed is deleted (SG-07)");
    }

    // Attack: try to use the feed-A token to PATCH feed-B -- returns 401 (token gone)
    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/feed-B")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({"feed_url": "https://evil.example.com"}))
                .expect("serialize"),
        ))
        .expect("build patch request");

    let resp = app.oneshot(patch_req).await.expect("patch feed-B");
    assert_eq!(resp.status(), 401,
        "PROTECTED: token for deleted feed-A is cleaned up (SG-07), returns 401");
}

// ============================================================================
// ATTACK 1b: Orphaned token -- after SG-07, tokens are cleaned up on feed
// delete, so there are no orphaned tokens. The PATCH returns 401 because the
// token no longer exists in the proof_tokens table.
// ============================================================================

#[tokio::test]
async fn attack1b_orphaned_token_patch_deleted_feed_returns_401() {
    let db = common::test_db_arc();
    let token;
    {
        let conn = db.lock().expect("lock db");
        let now = common::now();
        insert_artist(&conn, "artist-1", "Test Artist", now);
        let credit_id = insert_artist_credit(&conn, "artist-1", "Test Artist", now);
        insert_feed(&conn, "feed-orphan", "https://example.com/orphan.xml", "Orphan Feed", credit_id, now);
        token = issue_token_for_feed(&conn, "feed-orphan");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Delete the feed
    let delete_req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/feed-orphan")
        .header("X-Admin-Token", "test-admin-token")
        .body(axum::body::Body::empty())
        .expect("build delete");
    let resp = app.clone().oneshot(delete_req).await.expect("delete");
    assert_eq!(resp.status(), 204);

    // Try to PATCH the now-deleted feed with its own token -- returns 401
    // because SG-07 cleaned up the token on feed deletion.
    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/feed-orphan")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({"feed_url": "https://evil.com"}))
                .expect("serialize"),
        ))
        .expect("build patch");

    let resp = app.oneshot(patch_req).await.expect("patch");
    assert_eq!(resp.status(), 401,
        "PROTECTED: token cleaned up on feed delete (SG-07), returns 401");
}

// ============================================================================
// ATTACK 2: Challenge race condition (double-spend)
//
// The resolve_challenge function uses:
//   UPDATE proof_challenges SET state = ?1 WHERE challenge_id = ?2 AND state = 'pending'
//
// With SQLite + Mutex, only one thread can hold the connection at a time,
// so a true concurrent double-spend is impossible at the DB layer.
//
// Additionally, the handle_proofs_assert handler acquires the DB mutex
// before calling get_challenge and resolve_challenge, so the entire
// sequence is atomic.
//
// FINDING: PROTECTED (by SQLite's single-writer + Rust Mutex)
// ============================================================================

#[test]
fn attack2_resolve_challenge_is_idempotent() {
    let conn = common::test_db();
    let (challenge_id, _) =
        stophammer::proof::create_challenge(&conn, "feed-abc", "feed:write", "nonce-for-double-spend")
            .unwrap();

    // First resolution: should succeed
    stophammer::proof::resolve_challenge(&conn, &challenge_id, "valid").unwrap();

    let ch = stophammer::proof::get_challenge(&conn, &challenge_id)
        .unwrap()
        .expect("challenge should exist");
    assert_eq!(ch.state, "valid");

    // Second resolution attempt: WHERE state = 'pending' won't match,
    // so UPDATE affects 0 rows. The challenge stays "valid".
    stophammer::proof::resolve_challenge(&conn, &challenge_id, "valid").unwrap();

    let ch2 = stophammer::proof::get_challenge(&conn, &challenge_id)
        .unwrap()
        .expect("challenge should still exist");
    assert_eq!(ch2.state, "valid", "state should still be valid (idempotent)");
}

#[tokio::test]
async fn attack2_double_assert_returns_400_on_second() {
    let mock_server = MockServer::start().await;
    let db = common::test_db_arc();
    {
        let conn = db.lock().unwrap();
        let now = common::now();
        insert_artist(&conn, "artist-dbl", "Double Artist", now);
        let credit_id = insert_artist_credit(&conn, "artist-dbl", "Double Artist", now);
        insert_feed(&conn, "feed-double", &mock_server.uri(), "Feed Double", credit_id, now);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let nonce = "double-assert-nonce";

    // Create challenge
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/v1/proofs/challenge",
            &serde_json::json!({
                "feed_guid": "feed-double",
                "scope": "feed:write",
                "requester_nonce": nonce,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body = body_json(resp).await;
    let challenge_id = body["challenge_id"].as_str().unwrap().to_string();
    let token_binding = body["token_binding"].as_str().unwrap();

    // Mount RSS with the correct token_binding
    let rss = rss_with_podcast_txt(&format!("stophammer-proof {token_binding}"));
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    // First assert: should succeed
    let resp2 = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/v1/proofs/assert",
            &serde_json::json!({
                "challenge_id": &challenge_id,
                "requester_nonce": nonce,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp2.status(), 200, "first assert should succeed");

    // Second assert: should fail because challenge is already resolved
    let resp3 = app
        .oneshot(json_request(
            "POST",
            "/v1/proofs/assert",
            &serde_json::json!({
                "challenge_id": &challenge_id,
                "requester_nonce": nonce,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp3.status(), 400,
        "PROTECTED: second assert returns 400 (challenge already resolved)");
}

// ============================================================================
// ATTACK 3: Cross-feed token
//
// Can a token issued for feed-A be used to PATCH feed-B?
//
// FINDING: PROTECTED. check_admin_or_bearer_with_conn compares
// subject_feed_guid from the token against the expected_feed_guid
// passed by the handler (which is the URL path parameter).
// ============================================================================

#[test]
fn attack3_cross_feed_token_rejected() {
    let conn = common::test_db();
    let token_a = issue_token_for_feed(&conn, "feed-A");

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "Authorization",
        format!("Bearer {token_a}").parse().expect("header value"),
    );

    // Try to use feed-A's token to authorize feed-B
    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn, &headers, "admin-secret", "feed:write", "feed-B",
    );
    assert!(result.is_err(), "PROTECTED: cross-feed token must be rejected");

    let err = result.unwrap_err();
    assert_eq!(err.status, axum::http::StatusCode::FORBIDDEN);
}

// ============================================================================
// ATTACK 4: Admin token timing attack
//
// The admin token comparison uses `==` on strings (line 965 of api.rs):
//   if provided == expected { Ok(()) }
//
// This is a non-constant-time comparison. However, the practical impact
// depends on the token entropy. The admin token is set at startup and is
// typically a high-entropy secret. Timing attacks against `==` on
// high-entropy tokens are extremely difficult in practice over a network
// (noise in network latency vastly exceeds any timing signal from
// byte-by-byte comparison).
//
// FINDING: PARTIALLY_PROTECTED.
// The comparison is non-constant-time, which is a defense-in-depth
// concern. For a production API exposed to the internet, using
// constant-time comparison would be best practice. However, exploitation
// is practically infeasible given network jitter and token entropy.
// ============================================================================

#[test]
fn attack4_admin_token_uses_non_constant_time_comparison() {
    // This test documents the finding. The comparison on api.rs line 965:
    //   if provided == expected { Ok(()) }
    // is non-constant-time.
    //
    // We verify the correct behavior but note the timing side-channel.
    let conn = common::test_db();

    // Correct token: passes
    let mut headers_ok = axum::http::HeaderMap::new();
    headers_ok.insert("X-Admin-Token", "test-admin-token".parse().unwrap());
    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn, &headers_ok, "test-admin-token", "feed:write", "feed-1",
    );
    assert!(result.is_ok());

    // Wrong token: fails
    let mut headers_bad = axum::http::HeaderMap::new();
    headers_bad.insert("X-Admin-Token", "wrong-token".parse().unwrap());
    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn, &headers_bad, "test-admin-token", "feed:write", "feed-1",
    );
    assert!(result.is_err());

    // Partial match: fails (not vulnerable to prefix attack in practice
    // because network jitter >> CPU comparison timing)
    let mut headers_partial = axum::http::HeaderMap::new();
    headers_partial.insert("X-Admin-Token", "test-admin-toke".parse().unwrap());
    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn, &headers_partial, "test-admin-token", "feed:write", "feed-1",
    );
    assert!(result.is_err());
}

// ============================================================================
// ATTACK 5: Bearer token format bypass
//
// 5a: Empty bearer token
// 5b: Bearer token with only whitespace (becomes empty after trim)
// 5c: Extremely long bearer token (SQLite handles it fine)
// 5d: Bearer token with special characters
//
// FINDING: PROTECTED. extract_bearer_token returns None for empty tokens.
// The SQL query simply won't match any row for non-existent tokens.
// ============================================================================

#[test]
fn attack5a_empty_bearer_token_rejected() {
    // "Bearer " with nothing after is stripped by strip_prefix("Bearer ")
    // which yields "", then trim() yields "", and the is_empty() check catches it.
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("Authorization", "Bearer ".parse().unwrap());

    let result = stophammer::api::extract_bearer_token(&headers);
    assert!(result.is_none(), "PROTECTED: empty bearer token returns None");
}

#[test]
fn attack5b_whitespace_only_bearer_rejected() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("Authorization", "Bearer    ".parse().unwrap());

    let result = stophammer::api::extract_bearer_token(&headers);
    assert!(result.is_none(),
        "PROTECTED: whitespace-only bearer token returns None after trim");
}

#[test]
fn attack5c_long_bearer_token_harmless() {
    let conn = common::test_db();
    // Generate a 1MB token -- this should not crash or cause issues,
    // just fail to match any row in proof_tokens.
    let long_token = "A".repeat(1_000_000);

    let result = stophammer::proof::validate_token(&conn, &long_token, "feed:write").unwrap();
    assert!(result.is_none(),
        "PROTECTED: extremely long token simply doesn't match any row");
}

#[test]
fn attack5d_special_chars_in_bearer_harmless() {
    let conn = common::test_db();

    // SQL injection attempt
    let malicious = "'; DROP TABLE proof_tokens; --";
    let result = stophammer::proof::validate_token(&conn, malicious, "feed:write").unwrap();
    assert!(result.is_none(), "PROTECTED: parameterized queries prevent SQL injection");

    // Verify the table still exists
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proof_tokens", [], |r| r.get(0),
    ).unwrap();
    assert_eq!(count, 0, "table should still exist after SQL injection attempt");
}

// ============================================================================
// ATTACK 6: Challenge expiry bypass
//
// Can an expired challenge be used for assert?
//
// FINDING: PROTECTED. get_challenge uses WHERE expires_at > ?now,
// so expired challenges return None, and the handler returns 404.
// ============================================================================

#[test]
fn attack6_expired_challenge_returns_none() {
    let conn = common::test_db();
    let past = common::now() - 1;

    conn.execute(
        "INSERT INTO proof_challenges (challenge_id, feed_guid, scope, token_binding, state, expires_at, created_at) \
         VALUES ('expired-challenge', 'feed-x', 'feed:write', 'tok.hash', 'pending', ?1, ?2)",
        params![past, past - 86400],
    ).unwrap();

    let result = stophammer::proof::get_challenge(&conn, "expired-challenge").unwrap();
    assert!(result.is_none(), "PROTECTED: expired challenge returns None");
}

#[tokio::test]
async fn attack6_expired_challenge_assert_returns_404() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        let past = common::now() - 1;
        conn.execute(
            "INSERT INTO proof_challenges (challenge_id, feed_guid, scope, token_binding, state, expires_at, created_at) \
             VALUES ('expired-via-api', 'feed-x', 'feed:write', 'tok.hash', 'pending', ?1, ?2)",
            params![past, past - 86400],
        ).unwrap();
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(json_request(
            "POST",
            "/v1/proofs/assert",
            &serde_json::json!({
                "challenge_id": "expired-via-api",
                "requester_nonce": "some-nonce-16-chars",
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), 404,
        "PROTECTED: expired challenge returns 404 from assert endpoint");
}

// ============================================================================
// ATTACK 7: Scope confusion -- can a "feed:write" token be used with a
// different required scope? (e.g., if a future scope "track:write" exists)
//
// FINDING: PROTECTED. validate_token uses WHERE scope = ?required_scope,
// so scope must match exactly. Currently only "feed:write" is supported,
// and all handlers pass "feed:write" as required_scope. Track mutations
// are authorized via the parent feed's scope.
// ============================================================================

#[test]
fn attack7_scope_confusion_rejected() {
    let conn = common::test_db();
    let token = issue_token_for_feed(&conn, "feed-A");

    // Token was issued with scope "feed:write"
    // Try validating with a different scope
    let result = stophammer::proof::validate_token(&conn, &token, "track:write").unwrap();
    assert!(result.is_none(), "PROTECTED: wrong scope does not validate");

    let result2 = stophammer::proof::validate_token(&conn, &token, "admin").unwrap();
    assert!(result2.is_none(), "PROTECTED: admin scope does not validate");

    let result3 = stophammer::proof::validate_token(&conn, &token, "").unwrap();
    assert!(result3.is_none(), "PROTECTED: empty scope does not validate");

    // Correct scope works
    let result4 = stophammer::proof::validate_token(&conn, &token, "feed:write").unwrap();
    assert_eq!(result4, Some("feed-A".to_string()), "correct scope validates");
}

// ============================================================================
// ATTACK 7b: Track PATCH uses parent feed guid for auth (scope confusion)
//
// The handle_patch_track handler looks up the track, gets its feed_guid,
// then validates the bearer token against that feed_guid. This means a
// token for feed-A can only modify tracks belonging to feed-A.
// ============================================================================

#[tokio::test]
async fn attack7b_track_patch_requires_parent_feed_token() {
    let db = common::test_db_arc();
    let token_a;
    {
        let conn = db.lock().expect("lock");
        let (credit_id, now) = seed_two_feeds(&conn);
        insert_track(&conn, "track-B1", "feed-B", credit_id, "Track in Feed B", now);
        token_a = issue_token_for_feed(&conn, "feed-A");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Try to PATCH track-B1 (belongs to feed-B) with token for feed-A
    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/v1/tracks/track-B1")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token_a}"))
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "enclosure_url": "https://evil.com/song.mp3"
            })).expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(patch_req).await.expect("call handler");
    assert_eq!(resp.status(), 403,
        "PROTECTED: token for feed-A cannot patch tracks in feed-B");
}

// ============================================================================
// ATTACK 8: X-Admin-Token header presence triggers admin-only path
//
// When X-Admin-Token header is present, check_admin_or_bearer_with_conn
// bypasses the bearer token path entirely and uses check_admin_token.
// An attacker who knows the admin token header name could potentially
// send an empty X-Admin-Token to trigger the admin path (which would
// fail with 403).
//
// FINDING: PROTECTED. Empty/wrong admin tokens are rejected.
// ============================================================================

#[test]
fn attack8_empty_admin_token_header_rejected() {
    let conn = common::test_db();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("X-Admin-Token", "".parse().unwrap());

    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn, &headers, "real-admin-token", "feed:write", "feed-1",
    );
    assert!(result.is_err(), "PROTECTED: empty admin token rejected");
}

// ============================================================================
// ATTACK 9: Challenge scope validation at creation time
//
// Only "feed:write" is accepted as a scope. Other scopes are rejected
// at challenge creation time.
// ============================================================================

#[tokio::test]
async fn attack9_unsupported_scope_rejected_at_challenge_creation() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(json_request(
            "POST",
            "/v1/proofs/challenge",
            &serde_json::json!({
                "feed_guid": "feed-abc",
                "scope": "admin",
                "requester_nonce": "nonce-for-scope-test",
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), 400,
        "PROTECTED: unsupported scope rejected at challenge creation");
}

// ============================================================================
// ATTACK 10: Missing Authorization header type prefix
//
// What if someone sends "Basic ..." or just the token without "Bearer "?
// ============================================================================

#[test]
fn attack10_non_bearer_auth_scheme_rejected() {
    // "Basic" scheme
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("Authorization", "Basic dXNlcjpwYXNz".parse().unwrap());

    let result = stophammer::api::extract_bearer_token(&headers);
    assert!(result.is_none(), "PROTECTED: Basic auth scheme not accepted");

    // Raw token without scheme
    let mut headers2 = axum::http::HeaderMap::new();
    headers2.insert("Authorization", "some-raw-token".parse().unwrap());

    let result2 = stophammer::api::extract_bearer_token(&headers2);
    assert!(result2.is_none(), "PROTECTED: raw token without Bearer prefix not accepted");
}
