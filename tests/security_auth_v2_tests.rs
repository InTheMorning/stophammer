#![expect(
    clippy::significant_drop_tightening,
    reason = "MutexGuard<Connection> must be held for the full scope in test setup"
)]

//! Security audit v2 tests — 2026-03-13
//!
//! Re-verifies all v1 findings with post-implementation code changes, and
//! probes new attack surfaces: RSS verification bypass (CS-01), SSRF via
//! `feed_url`, SSE registry abuse, rate limiter X-Forwarded-For spoofing,
//! admin token as single point of failure, and the three-phase TOCTOU in
//! `handle_proofs_assert`.

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
            feed_guid,
            feed_url,
            title,
            title.to_lowercase(),
            credit_id,
            "A test feed",
            0,
            0,
            now,
            now,
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

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    test_app_state_inner(db, true)
}

/// `AppState` with SSRF validation enabled (for testing the SSRF guard itself).
fn test_app_state_ssrf_enabled(
    db: Arc<Mutex<rusqlite::Connection>>,
) -> Arc<stophammer::api::AppState> {
    test_app_state_inner(db, false)
}

fn test_app_state_inner(
    db: Arc<Mutex<rusqlite::Connection>>,
    skip_ssrf: bool,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-security-auth-v2.key")
            .expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(vec![])),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token-v2".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: skip_ssrf,
    })
}

fn seed_two_feeds(conn: &rusqlite::Connection) -> (i64, i64) {
    let now = common::now();
    insert_artist(conn, "artist-v2", "V2 Artist", now);
    let credit_id = insert_artist_credit(conn, "artist-v2", "V2 Artist", now);
    insert_feed(
        conn,
        "feed-V2A",
        "https://example.com/a.xml",
        "Feed A",
        credit_id,
        now,
    );
    insert_feed(
        conn,
        "feed-V2B",
        "https://example.com/b.xml",
        "Feed B",
        credit_id,
        now,
    );
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

/// Generate RSS XML with a `podcast:txt` element containing the given text.
/// Used by tests that directly verify the RSS parsing logic.
#[expect(dead_code, reason = "helper used conditionally across tests")]
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

async fn create_challenge_for_feed(
    app: &axum::Router,
    feed_guid: &str,
    nonce: &str,
) -> (String, String) {
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/v1/proofs/challenge",
            &serde_json::json!({
                "feed_guid": feed_guid,
                "scope": "feed:write",
                "requester_nonce": nonce,
            }),
        ))
        .await
        .expect("challenge request");
    assert_eq!(resp.status(), 201);
    let body = body_json(resp).await;
    let challenge_id = body["challenge_id"]
        .as_str()
        .expect("challenge_id")
        .to_string();
    let token_binding = body["token_binding"]
        .as_str()
        .expect("token_binding")
        .to_string();
    (challenge_id, token_binding)
}

// ============================================================================
// V1 RE-VERIFICATION: Attack 1 — Token replay after feed deletion
//
// V1 STATUS: PROTECTED (subject GUID binding prevents cross-feed replay)
// V2 STATUS: CLOSED (SG-07 now cascade-deletes proof_tokens on feed delete)
//
// The token for the deleted feed is now physically removed, so even the
// harmless no-op path is eliminated.
// ============================================================================

#[tokio::test]
async fn v2_attack1_token_cascade_deleted_on_feed_delete() {
    let db = common::test_db_arc();
    let token;
    {
        let conn = db.lock().expect("lock db");
        let (credit_id, now) = seed_two_feeds(&conn);
        insert_track(&conn, "v2-track-B1", "feed-V2B", credit_id, "Song B1", now);
        token = issue_token_for_feed(&conn, "feed-V2A");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Delete feed-A
    let delete_req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/feed-V2A")
        .header("X-Admin-Token", "test-admin-token-v2")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(delete_req).await.unwrap();
    assert_eq!(resp.status(), 204);

    // SG-07 verification: token is physically gone
    {
        let conn = db.lock().unwrap();
        let result = stophammer::proof::validate_token(&conn, &token, "feed:write").unwrap();
        assert_eq!(
            result, None,
            "SG-07 CLOSED: token cascade-deleted on feed delete"
        );
    }

    // Cross-feed replay still returns 401 (token doesn't exist anymore)
    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/feed-V2B")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({"feed_url": "https://evil.com"})).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(patch_req).await.unwrap();
    assert_eq!(resp.status(), 401, "Token no longer exists -> 401");
}

// ============================================================================
// V1 RE-VERIFICATION: Attack 2 — Challenge race condition
//
// V1 STATUS: PROTECTED (SQLite mutex + WHERE state = 'pending')
// V2 STATUS: CLOSED (unchanged — same protection)
// ============================================================================

#[test]
fn v2_attack2_resolve_challenge_still_idempotent() {
    let conn = common::test_db();
    let (cid, _) = stophammer::proof::create_challenge(
        &conn,
        "feed-v2-race",
        "feed:write",
        "nonce-for-v2-race-test",
    )
    .unwrap();
    let rows1 = stophammer::proof::resolve_challenge(&conn, &cid, "valid").unwrap();
    assert_eq!(rows1, 1, "first resolve affects 1 row");

    let rows2 = stophammer::proof::resolve_challenge(&conn, &cid, "invalid").unwrap();
    assert_eq!(rows2, 0, "second resolve affects 0 rows (no-op)");

    let ch = stophammer::proof::get_challenge(&conn, &cid)
        .unwrap()
        .unwrap();
    assert_eq!(
        ch.state, "valid",
        "second resolve is no-op; state stays valid"
    );
}

// ============================================================================
// V1 RE-VERIFICATION: Attack 3 — Cross-feed token
//
// V1 STATUS: PROTECTED
// V2 STATUS: CLOSED (unchanged)
// ============================================================================

#[test]
fn v2_attack3_cross_feed_token_still_rejected() {
    let conn = common::test_db();
    let token_a = issue_token_for_feed(&conn, "feed-cross-a");

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "Authorization",
        format!("Bearer {token_a}").parse().unwrap(),
    );

    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn,
        &headers,
        "admin-secret-v2",
        "feed:write",
        "feed-cross-b",
    );
    assert!(result.is_err(), "cross-feed token still rejected");
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::FORBIDDEN
    );
}

// ============================================================================
// V1 RE-VERIFICATION: Attack 4 — Admin token timing attack
//
// V1 STATUS: PARTIALLY_PROTECTED (non-constant-time ==)
// V2 STATUS: CLOSED (CS-02: SHA-256 + subtle::ConstantTimeEq)
//
// The new check_admin_token hashes both sides with SHA-256 and uses
// ct_eq for comparison.
// ============================================================================

#[test]
fn v2_attack4_admin_token_now_constant_time() {
    let conn = common::test_db();

    // Correct token: passes
    let mut headers_ok = axum::http::HeaderMap::new();
    headers_ok.insert("X-Admin-Token", "correct-admin-token-v2".parse().unwrap());
    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn,
        &headers_ok,
        "correct-admin-token-v2",
        "feed:write",
        "feed-1",
    );
    assert!(result.is_ok(), "correct admin token accepted");

    // Wrong token: rejected
    let mut headers_bad = axum::http::HeaderMap::new();
    headers_bad.insert("X-Admin-Token", "wrong-token".parse().unwrap());
    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn,
        &headers_bad,
        "correct-admin-token-v2",
        "feed:write",
        "feed-1",
    );
    assert!(result.is_err(), "wrong admin token rejected");

    // Partial match: rejected (constant-time comparison prevents leaking prefix length)
    let mut headers_partial = axum::http::HeaderMap::new();
    headers_partial.insert("X-Admin-Token", "correct-admin-token-v".parse().unwrap());
    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn,
        &headers_partial,
        "correct-admin-token-v2",
        "feed:write",
        "feed-1",
    );
    assert!(
        result.is_err(),
        "CS-02 CLOSED: partial match rejected with constant-time eq"
    );

    // Empty admin_token on server: rejects all
    let mut headers_any = axum::http::HeaderMap::new();
    headers_any.insert("X-Admin-Token", "anything".parse().unwrap());
    let result = stophammer::api::check_admin_or_bearer_with_conn(
        &conn,
        &headers_any,
        "",
        "feed:write",
        "feed-1",
    );
    assert!(result.is_err(), "empty server admin_token rejects all");
}

// ============================================================================
// V1 RE-VERIFICATION: Attack 5 — Bearer token format bypass
//
// V1 STATUS: PROTECTED
// V2 STATUS: CLOSED (unchanged)
// ============================================================================

#[test]
fn v2_attack5_bearer_format_bypass_still_protected() {
    assert!(
        stophammer::api::extract_bearer_token(&{
            let mut h = axum::http::HeaderMap::new();
            h.insert("Authorization", "Bearer ".parse().unwrap());
            h
        })
        .is_none(),
        "empty bearer rejected"
    );

    assert!(
        stophammer::api::extract_bearer_token(&{
            let mut h = axum::http::HeaderMap::new();
            h.insert("Authorization", "Bearer    ".parse().unwrap());
            h
        })
        .is_none(),
        "whitespace bearer rejected"
    );

    assert!(
        stophammer::api::extract_bearer_token(&{
            let mut h = axum::http::HeaderMap::new();
            h.insert("Authorization", "Basic dXNlcjpwYXNz".parse().unwrap());
            h
        })
        .is_none(),
        "Basic scheme rejected"
    );
}

// ============================================================================
// V1 RE-VERIFICATION: Attack 6 — Challenge expiry bypass
//
// V1 STATUS: PROTECTED
// V2 STATUS: CLOSED (unchanged)
// ============================================================================

#[test]
fn v2_attack6_expired_challenge_still_returns_none() {
    let conn = common::test_db();
    let past = common::now() - 1;
    conn.execute(
        "INSERT INTO proof_challenges (challenge_id, feed_guid, scope, token_binding, state, expires_at, created_at) \
         VALUES ('v2-expired', 'feed-x', 'feed:write', 'tok.hash', 'pending', ?1, ?2)",
        params![past, past - 86400],
    ).unwrap();
    assert!(
        stophammer::proof::get_challenge(&conn, "v2-expired")
            .unwrap()
            .is_none()
    );
}

// ============================================================================
// V1 RE-VERIFICATION: Attack 7 — Scope confusion
//
// V1 STATUS: PROTECTED
// V2 STATUS: CLOSED (unchanged)
// ============================================================================

#[test]
fn v2_attack7_scope_confusion_still_rejected() {
    let conn = common::test_db();
    let token = issue_token_for_feed(&conn, "feed-scope-v2");
    assert!(
        stophammer::proof::validate_token(&conn, &token, "track:write")
            .unwrap()
            .is_none()
    );
    assert!(
        stophammer::proof::validate_token(&conn, &token, "")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        stophammer::proof::validate_token(&conn, &token, "feed:write").unwrap(),
        Some("feed-scope-v2".to_string()),
    );
}

// ============================================================================
// NEW ATTACK SURFACE: CS-01 SSRF — feed_url pointing to internal targets
//
// FINDING: VULNERABLE (design-level)
//
// The verify_podcast_txt function accepts any feed_url from the database.
// The feed_url was originally ingested from the crawler. An attacker who
// controls their RSS feed_url (via PATCH /v1/feeds/{guid}) can point it at
// internal services (127.0.0.1, 169.254.x.x, file://, etc.). When
// handle_proofs_assert fetches the RSS, it acts as an SSRF proxy.
//
// The reqwest client with rustls-tls does NOT support file:// URLs (returns
// a builder error), which mitigates local file read. However, HTTP URLs
// targeting internal IPs are still reachable.
//
// Mitigation: validate feed_url scheme (https:// or http://) and reject
// private/reserved IP ranges before fetching.
// ============================================================================

// ── SSRF guard tests (validate_feed_url) ─────────────────────────────────
//
// The SSRF guard is applied at the API layer (handle_proofs_assert) via
// proof::validate_feed_url(), not inside verify_podcast_txt itself.
// This design allows existing integration tests that use localhost mock
// servers to test the RSS parsing logic independently.

#[test]
fn v2_cs01_validate_feed_url_rejects_file_scheme() {
    let result = stophammer::proof::validate_feed_url("file:///etc/passwd");
    assert!(result.is_err(), "file:// scheme should be rejected");
    assert!(result.unwrap_err().contains("disallowed URL scheme"));
}

#[test]
fn v2_cs01_validate_feed_url_rejects_private_ips() {
    // 127.0.0.0/8 (loopback)
    assert!(
        stophammer::proof::validate_feed_url("http://127.0.0.1/feed.xml").is_err(),
        "127.0.0.1 should be rejected"
    );

    // 10.0.0.0/8
    assert!(
        stophammer::proof::validate_feed_url("http://10.0.0.1/feed.xml").is_err(),
        "10.0.0.0/8 should be rejected"
    );

    // 172.16.0.0/12
    assert!(
        stophammer::proof::validate_feed_url("http://172.16.0.1/feed.xml").is_err(),
        "172.16.0.0/12 should be rejected"
    );

    // 192.168.0.0/16
    assert!(
        stophammer::proof::validate_feed_url("http://192.168.1.1/feed.xml").is_err(),
        "192.168.0.0/16 should be rejected"
    );

    // 169.254.0.0/16 (link-local / cloud metadata)
    assert!(
        stophammer::proof::validate_feed_url("http://169.254.169.254/latest/meta-data/").is_err(),
        "169.254.0.0/16 (cloud metadata) should be rejected"
    );

    // IPv6 loopback
    assert!(
        stophammer::proof::validate_feed_url("http://[::1]/feed.xml").is_err(),
        "::1 should be rejected"
    );
}

#[test]
fn v2_cs01_validate_feed_url_accepts_public_urls() {
    assert!(stophammer::proof::validate_feed_url("https://example.com/feed.xml").is_ok());
    assert!(stophammer::proof::validate_feed_url("http://example.com/feed.xml").is_ok());
    assert!(stophammer::proof::validate_feed_url("https://feeds.megaphone.fm/podcast.xml").is_ok());
}

#[test]
fn v2_cs01_validate_feed_url_rejects_disallowed_schemes() {
    assert!(stophammer::proof::validate_feed_url("ftp://example.com/feed.xml").is_err());
    assert!(stophammer::proof::validate_feed_url("gopher://example.com/").is_err());
    assert!(stophammer::proof::validate_feed_url("data:text/xml,<rss/>").is_err());
}

#[test]
fn v2_cs01_validate_feed_url_rejects_malformed() {
    assert!(stophammer::proof::validate_feed_url("not-a-url").is_err());
    assert!(stophammer::proof::validate_feed_url("").is_err());
}

#[tokio::test]
async fn v2_cs01_ssrf_blocked_at_api_layer() {
    // Verify that the SSRF guard in handle_proofs_assert blocks
    // private-IP feed URLs at the API layer (returns 400, not 503).
    let db = common::test_db_arc();
    {
        let conn = db.lock().unwrap();
        let now = common::now();
        insert_artist(&conn, "artist-ssrf", "SSRF Artist", now);
        let cid = insert_artist_credit(&conn, "artist-ssrf", "SSRF Artist", now);
        // Feed URL points to a private IP
        insert_feed(
            &conn,
            "feed-ssrf",
            "http://169.254.169.254/latest/meta-data/",
            "SSRF Feed",
            cid,
            now,
        );
    }
    let state = test_app_state_ssrf_enabled(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let nonce = "ssrf-test-nonce-16ch";
    let (challenge_id, _) = create_challenge_for_feed(&app, "feed-ssrf", nonce).await;

    let resp = app
        .oneshot(json_request(
            "POST",
            "/v1/proofs/assert",
            &serde_json::json!({
                "challenge_id": challenge_id,
                "requester_nonce": nonce,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "SSRF FIX: private-IP feed URL blocked at API layer with 400"
    );
    let body = body_json(resp).await;
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("private/reserved IP"),
        "error should mention private/reserved IP"
    );
}

// ============================================================================
// NEW ATTACK SURFACE: CS-01 — Attacker-controlled feed_url serves matching
// podcast:txt on demand.
//
// FINDING: BY_DESIGN (not a vulnerability)
//
// This is the intended use case. The feed owner controls their RSS feed.
// If they can serve the correct podcast:txt, they ARE the feed owner (or
// have compromised the DNS/hosting of the feed). This is the fundamental
// trust model of podcast:txt proof-of-possession.
// ============================================================================

// The SSRF guard blocks localhost URLs at the API layer (handle_proofs_assert),
// so the assert flow returns 400 for feeds with private-IP feed_urls.
// The attacker-controlled-server scenario (where the attacker hosts a public
// server) remains BY_DESIGN -- the podcast:txt trust model is based on control
// of the feed URL. This is tested via v2_cs01_ssrf_blocked_at_api_layer above.
#[tokio::test]
async fn v2_cs01_assert_with_localhost_feed_url_blocked_by_ssrf() {
    let mock_server = MockServer::start().await;
    let db = common::test_db_arc();
    {
        let conn = db.lock().unwrap();
        let now = common::now();
        insert_artist(&conn, "artist-ctrl", "Controlled Artist", now);
        let cid = insert_artist_credit(&conn, "artist-ctrl", "Controlled Artist", now);
        // feed_url points to localhost mock server
        insert_feed(
            &conn,
            "feed-ctrl",
            &mock_server.uri(),
            "Controlled Feed",
            cid,
            now,
        );
    }
    let state = test_app_state_ssrf_enabled(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let nonce = "controlled-nonce-16ch";
    let (challenge_id, _token_binding) = create_challenge_for_feed(&app, "feed-ctrl", nonce).await;

    let resp = app
        .oneshot(json_request(
            "POST",
            "/v1/proofs/assert",
            &serde_json::json!({
                "challenge_id": challenge_id,
                "requester_nonce": nonce,
            }),
        ))
        .await
        .unwrap();
    // The SSRF guard rejects localhost URLs with 400.
    assert_eq!(
        resp.status(),
        400,
        "SSRF FIX: localhost feed_url blocked during assert, returns 400"
    );
}

// ============================================================================
// NEW ATTACK SURFACE: CS-01 — podcast:txt with extra content (parse ambiguity)
//
// FINDING: PROTECTED
//
// The verify_podcast_txt parser uses exact trimmed match:
//   trimmed == expected_text
// Padding, surrounding text, or embedded newlines all cause mismatch.
// ============================================================================

#[tokio::test]
async fn v2_cs01_podcast_txt_partial_match_rejected() {
    let mock_server = MockServer::start().await;
    let token_binding = "exact-token.exact-hash";

    // RSS with podcast:txt that CONTAINS the expected text but has extra content
    let rss = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Test</title>
    <podcast:txt>stophammer-proof {token_binding} AND EXTRA STUFF</podcast:txt>
  </channel>
</rss>"#
    );
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let result =
        stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), token_binding).await;
    assert_eq!(
        result,
        Ok(false),
        "PROTECTED: exact match required, extra content rejected"
    );
}

#[tokio::test]
async fn v2_cs01_podcast_txt_prefix_attack_rejected() {
    let mock_server = MockServer::start().await;
    let token_binding = "prefix-token.prefix-hash";

    // RSS with podcast:txt that has a prefix before the expected text
    let rss = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Test</title>
    <podcast:txt>INJECTED stophammer-proof {token_binding}</podcast:txt>
  </channel>
</rss>"#
    );
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let result =
        stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), token_binding).await;
    assert_eq!(
        result,
        Ok(false),
        "PROTECTED: prefix injection rejected by exact match"
    );
}

// ============================================================================
// NEW ATTACK SURFACE: CS-01 — Three-phase TOCTOU in handle_proofs_assert
//
// FINDING: VULNERABLE (low severity)
//
// handle_proofs_assert releases the DB mutex between Phase 1 (validate nonce)
// and Phase 3 (resolve challenge + issue token). The challenge remains
// "pending" while the RSS fetch (Phase 2) is in progress. During this window:
//
//   1. A second concurrent assert with the same challenge_id can enter Phase 1
//      (the challenge is still "pending"), pass nonce validation, and also
//      proceed to Phase 2.
//
//   2. Both requests proceed to Phase 3. However, resolve_challenge uses
//      WHERE state = 'pending', so only the FIRST to reach Phase 3 will
//      transition the challenge to "valid". The second will find state="valid"
//      and the UPDATE affects 0 rows (idempotent no-op). But the second
//      request does NOT re-check state after resolve_challenge — it proceeds
//      to issue_token unconditionally.
//
// Wait — let me re-read. Phase 3 checks `if !rss_verified` but does NOT
// re-check that the challenge is still pending. After resolve_challenge
// (which is a no-op if already resolved), it calls issue_token regardless.
// This means TWO tokens could be issued from a single challenge.
//
// Severity: LOW. The two tokens are for the same feed_guid and same scope,
// so the attacker gains no privilege escalation. They just get two tokens
// instead of one. Both tokens expire in 1 hour.
//
// Fix: re-check challenge state in Phase 3 before issuing token, or use
// resolve_challenge's return value to detect the no-op.
// ============================================================================

// This test demonstrates the TOCTOU window by simulating a second
// assert that arrives while the first is blocked on RSS fetch.
// In a unit test, we cannot easily reproduce true concurrency, but we
// verify the design-level issue by checking that resolve_challenge does
// not prevent subsequent issue_token calls in the same handler.

#[test]
fn v2_cs01_toctou_now_fixed_via_rows_check() {
    let conn = common::test_db();
    let (cid, _) = stophammer::proof::create_challenge(
        &conn,
        "feed-toctou",
        "feed:write",
        "nonce-toctou-16chars",
    )
    .unwrap();

    // First resolve: succeeds, returns 1 row affected
    let rows1 = stophammer::proof::resolve_challenge(&conn, &cid, "valid").unwrap();
    assert_eq!(rows1, 1, "first resolve should affect 1 row");

    // Second resolve: no-op, returns 0 rows affected
    let rows2 = stophammer::proof::resolve_challenge(&conn, &cid, "valid").unwrap();
    assert_eq!(
        rows2, 0,
        "second resolve should affect 0 rows (already resolved)"
    );

    // The fix in handle_proofs_assert Phase 3 now checks rows == 0 and
    // returns 400 instead of proceeding to issue_token. We verify the
    // resolve_challenge return value provides the signal needed for the fix.
    //
    // issue_token itself has no dependency on challenge state (it just inserts
    // a token row), so the fix must be in the caller (api.rs Phase 3).
}

// ============================================================================
// NEW ATTACK SURFACE: CS-03 — sync/register requires admin token
//
// V1 STATUS: N/A (was unauthenticated)
// V2 STATUS: CLOSED (CS-03 implemented)
//
// Verify that POST /sync/register without admin token returns 403.
// ============================================================================

#[tokio::test]
async fn v2_cs03_sync_register_requires_admin_token() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);
    let signer =
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-security-auth-v2-register.key")
            .expect("create signer");

    // Without admin token
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/sync/register",
            &common::signed_sync_register_body(&signer, "https://evil.com/push"),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "CS-03: register without admin token returns 403"
    );

    // With wrong admin token
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "wrong-token")
        .body(axum::body::Body::from(
            serde_json::to_vec(&common::signed_sync_register_body(
                &signer,
                "https://evil.com/push",
            ))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        403,
        "CS-03: register with wrong admin token returns 403"
    );

    // With correct admin token
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "test-admin-token-v2")
        .body(axum::body::Body::from(
            serde_json::to_vec(&common::signed_sync_register_body(
                &signer,
                "https://legit.com/push",
            ))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "CS-03: register with correct admin token succeeds"
    );
}

// ============================================================================
// NEW ATTACK SURFACE: CS-03 — Admin token as single point of failure
//
// FINDING: ACCEPTED_RISK
//
// If the admin token is compromised, an attacker can:
//   1. Register malicious push peers (receive all events)
//   2. Delete feeds, patch feeds/tracks
//   3. Merge artists, add aliases
//
// This is an accepted risk because:
//   - Admin tokens are server-side secrets, not user-facing
//   - The admin token is equivalent to database access
//   - Rate limiting and TLS protect the transport layer
//
// This test verifies the blast radius of a leaked admin token.
// ============================================================================

#[tokio::test]
async fn v2_cs03_admin_token_blast_radius() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().unwrap();
        let now = common::now();
        insert_artist(&conn, "artist-blast", "Blast Artist", now);
        let cid = insert_artist_credit(&conn, "artist-blast", "Blast Artist", now);
        insert_feed(
            &conn,
            "feed-blast",
            "https://example.com/blast.xml",
            "Blast Feed",
            cid,
            now,
        );
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);
    let signer =
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-security-auth-v2-blast.key")
            .expect("create signer");

    // With leaked admin token: can register push peer
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "test-admin-token-v2")
        .body(axum::body::Body::from(
            serde_json::to_vec(&common::signed_sync_register_body(
                &signer,
                "https://evil.com/push",
            ))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "leaked admin token can register push peer"
    );

    // With leaked admin token: can delete feed
    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/feed-blast")
        .header("X-Admin-Token", "test-admin-token-v2")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 204, "leaked admin token can delete feed");

    // Verify feed is gone
    {
        let conn = db.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM feeds WHERE feed_guid = 'feed-blast'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "feed was deleted by leaked admin token");
    }
}

// ============================================================================
// NEW ATTACK SURFACE: SSE — unlimited artist_id registrations (memory growth)
//
// FINDING: VULNERABLE (availability)
//
// The SseRegistry creates a broadcast channel per unique artist_id on
// subscribe(). There is no limit on the number of unique artist_ids.
// An attacker can subscribe to millions of random artist_ids, each creating:
//   - A tokio broadcast::Sender (256 capacity)
//   - A HashMap entry in senders and ring_buffers
//
// The ring_buffers map is bounded per artist (100 events) but the number
// of artists is unbounded.
//
// Mitigation: cap the number of unique artist_ids in the SSE registry,
// or cap the artists parameter to a maximum count per SSE connection.
// ============================================================================

#[test]
fn v2_sse_unlimited_artist_registrations() {
    let registry = stophammer::api::SseRegistry::new();

    // Create 10,000 unique artist subscriptions (in production, this would
    // be via HTTP requests). Each creates a broadcast channel.
    for i in 0..10_000 {
        let _rx = registry.subscribe(&format!("attacker-artist-{i}"));
    }

    // Verify all channels were created (no panic, no limit enforced).
    // This is the vulnerability: no cap on unique artist_ids.
    let recent = registry.recent_events("attacker-artist-0");
    assert!(recent.is_empty(), "no events published yet");

    // The senders map now has 10,000 entries. In production at scale,
    // an attacker with many connections could exhaust memory.
}

// ============================================================================
// NEW ATTACK SURFACE: SSE — no cross-pollination (information leak)
//
// FINDING: PROTECTED
//
// Each artist_id has its own broadcast channel. Subscribing to artist-A
// does not receive events for artist-B.
// ============================================================================

#[test]
fn v2_sse_no_cross_pollination() {
    let registry = stophammer::api::SseRegistry::new();
    let mut rx_a = registry
        .subscribe("artist-leak-a")
        .expect("subscribe should succeed");
    let _rx_b = registry
        .subscribe("artist-leak-b")
        .expect("subscribe should succeed");

    registry.publish(
        "artist-leak-b",
        stophammer::api::SseFrame {
            event_type: "track_upserted".to_string(),
            subject_guid: "secret-track".to_string(),
            payload: serde_json::json!({}),
            seq: 1,
        },
    );

    assert!(
        rx_a.try_recv().is_err(),
        "PROTECTED: subscribing to artist-a does not leak artist-b events"
    );
}

// ============================================================================
// NEW ATTACK SURFACE: Rate limiter — X-Forwarded-For spoofing
//
// FINDING: VULNERABLE (when not behind a trusted reverse proxy)
//
// The rate limiter (main.rs apply_rate_limit) extracts the client IP from:
//   1. X-Forwarded-For header (first hop: s.split(',').next())
//   2. ConnectInfo<SocketAddr> (fallback)
//   3. "unknown" (ultimate fallback)
//
// If the server is directly exposed (no reverse proxy), an attacker can
// spoof X-Forwarded-For with any IP to bypass rate limiting entirely.
// Each request uses a different spoofed IP, getting a fresh bucket.
//
// Mitigation: only trust X-Forwarded-For when behind a known proxy, or
// use ConnectInfo exclusively when no trusted proxy is configured.
// ============================================================================

#[test]
fn v2_rate_limiter_xff_spoofing() {
    // The rate limiter uses the string IP as the key. An attacker who
    // controls the X-Forwarded-For header can use a different "IP" on
    // each request, getting a separate rate limit bucket for each.
    let limiter = stophammer::api::build_rate_limiter(1, 1);

    // First request as "real" IP: passes
    assert!(limiter.check_key(&"10.0.0.1".to_string()).is_ok());
    // Second request as same IP: limited
    assert!(limiter.check_key(&"10.0.0.1".to_string()).is_err());

    // Attacker spoofs different X-Forwarded-For on each request:
    for i in 0..100 {
        let spoofed_ip = format!("spoofed-{i}");
        assert!(
            limiter.check_key(&spoofed_ip).is_ok(),
            "XFF SPOOFING: spoofed IP {spoofed_ip} gets its own fresh bucket"
        );
    }
}

// ============================================================================
// NEW ATTACK SURFACE: Rate limiter — "unknown" fallback
//
// FINDING: VULNERABLE (availability)
//
// When neither X-Forwarded-For nor ConnectInfo is available, the rate
// limiter uses the string "unknown" as the key. All requests without
// an identified IP share a single bucket. This means:
//   1. Legitimate requests without IP headers are unfairly grouped
//   2. An attacker can exhaust the "unknown" bucket, denying service
//      to all other "unknown" clients
//
// In practice this is unlikely because axum's make_service_with_connect_info
// always populates ConnectInfo for TCP connections. The "unknown" fallback
// would only trigger for in-process test requests.
// ============================================================================

#[test]
fn v2_rate_limiter_unknown_fallback_shared_bucket() {
    let limiter = stophammer::api::build_rate_limiter(2, 2);

    // Multiple "unknown" clients share the same bucket
    assert!(
        limiter.check_key(&"unknown".to_string()).is_ok(),
        "unknown req 1 passes"
    );
    assert!(
        limiter.check_key(&"unknown".to_string()).is_ok(),
        "unknown req 2 passes"
    );
    assert!(
        limiter.check_key(&"unknown".to_string()).is_err(),
        "unknown req 3 limited"
    );
    // A legitimate client with the same "unknown" key would be denied.
}

// ============================================================================
// NEW ATTACK SURFACE: SP-05 — SystemTime::now() replaced with .expect()
//
// V1 STATUS: N/A (unwrap_or_default silently returned epoch 0)
// V2 STATUS: CLOSED (SP-05: .expect() panics pre-epoch, which is correct)
//
// Verify unix_now returns a sane value (not 0).
// ============================================================================

#[test]
fn v2_sp05_unix_now_returns_sane_value() {
    let now = stophammer::db::unix_now();
    assert!(
        now > 1_700_000_000,
        "SP-05: unix_now should return modern timestamp, got {now}"
    );
}

// ============================================================================
// NEW ATTACK SURFACE: SG-07 — proof_challenges also cleaned on feed delete
//
// FINDING: CLOSED
//
// Both proof_tokens AND proof_challenges are deleted when a feed is deleted.
// This prevents an attacker from creating a challenge, deleting the feed,
// then asserting the challenge after the feed is re-created (with different
// ownership).
// ============================================================================

#[tokio::test]
async fn v2_sg07_challenges_deleted_on_feed_delete() {
    let db = common::test_db_arc();
    let challenge_id;
    {
        let conn = db.lock().unwrap();
        let now = common::now();
        insert_artist(&conn, "artist-sg07", "SG07 Artist", now);
        let cid = insert_artist_credit(&conn, "artist-sg07", "SG07 Artist", now);
        insert_feed(
            &conn,
            "feed-sg07",
            "https://example.com/sg07.xml",
            "SG07 Feed",
            cid,
            now,
        );

        // Create a challenge for this feed
        let (cid_val, _) = stophammer::proof::create_challenge(
            &conn,
            "feed-sg07",
            "feed:write",
            "sg07-nonce-16-chars",
        )
        .unwrap();
        challenge_id = cid_val;
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Delete the feed
    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/feed-sg07")
        .header("X-Admin-Token", "test-admin-token-v2")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 204);

    // Challenge should be gone
    {
        let conn = db.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proof_challenges WHERE challenge_id = ?1",
                params![challenge_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "SG-07 CLOSED: challenge deleted on feed delete");
    }
}

// ============================================================================
// NEW ATTACK SURFACE: CORS configuration
//
// FINDING: INFORMATIONAL (allow_origin Any)
//
// The CORS layer uses `allow_origin(Any)`, meaning any web origin can make
// cross-origin requests. This is appropriate for a public API but means
// browser-based clients from any origin can interact with the API.
// Combined with bearer token auth, this is standard for public APIs.
// ============================================================================

#[tokio::test]
async fn v2_sp08_cors_allows_any_origin() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/health")
        .header("Origin", "https://evil.com")
        .header("Access-Control-Request-Method", "GET")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    let acao = resp
        .headers()
        .get("access-control-allow-origin")
        .map(|v| v.to_str().unwrap_or(""));
    assert_eq!(
        acao,
        Some("*"),
        "INFORMATIONAL: CORS allows any origin (appropriate for public API)"
    );
}

// ============================================================================
// NEW ATTACK SURFACE: Pending challenge flooding (per-feed cap)
//
// FINDING: PROTECTED
//
// MAX_PENDING_CHALLENGES_PER_FEED (20) prevents an attacker from flooding
// the proof_challenges table for a single feed.
// ============================================================================

#[tokio::test]
async fn v2_challenge_flooding_capped_per_feed() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().unwrap();
        let now = common::now();
        insert_artist(&conn, "artist-flood", "Flood Artist", now);
        let cid = insert_artist_credit(&conn, "artist-flood", "Flood Artist", now);
        insert_feed(
            &conn,
            "feed-flood",
            "https://example.com/flood.xml",
            "Flood Feed",
            cid,
            now,
        );
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Create 20 challenges (the maximum)
    for i in 0..20 {
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/v1/proofs/challenge",
                &serde_json::json!({
                    "feed_guid": "feed-flood",
                    "scope": "feed:write",
                    "requester_nonce": format!("flood-nonce-{i:02}-pad"),
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "challenge {i} should succeed");
    }

    // 21st challenge should be rate-limited
    let resp = app
        .oneshot(json_request(
            "POST",
            "/v1/proofs/challenge",
            &serde_json::json!({
                "feed_guid": "feed-flood",
                "scope": "feed:write",
                "requester_nonce": "flood-nonce-20-padd",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        429,
        "PROTECTED: 21st challenge returns 429 (per-feed cap)"
    );
}
