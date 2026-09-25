#![expect(
    clippy::significant_drop_tightening,
    reason = "MutexGuard<Connection> must be held for the full scope in test setup"
)]

//! Security audit v2 tests — 2026-03-13
//!
//! Re-verifies v1 findings that outlive ADR 0056. The proof flow and its
//! bearer path are gone, so this file keeps the SSRF guard tests (moved to
//! `fetch_guard`), sync/register auth, SSE registry abuse, rate limiter
//! X-Forwarded-For spoofing, and CORS checks.

mod common;

use rusqlite::params;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
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

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    test_app_state_inner(db, true)
}

fn test_app_state_inner(
    db: Arc<Mutex<rusqlite::Connection>>,
    skip_ssrf: bool,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-security-auth-v2"));
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(
            "test-token".into(),
            vec![],
        )),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token-v2".into(),
        sync_token: Some("test-sync-token-v2".into()),
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: skip_ssrf,
    })
}

async fn signed_register_body_for_mock_peer(
    signer: &stophammer::signing::NodeSigner,
) -> (MockServer, serde_json::Value) {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::path("/node/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "node_pubkey": signer.pubkey_hex()
        })))
        .mount(&mock_server)
        .await;

    let body =
        common::signed_sync_register_body(signer, &format!("{}/sync/push", mock_server.uri()));
    (mock_server, body)
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
// CS-01 SSRF guard — `fetch_guard::validate_feed_url`
//
// A feed_url can point at an internal target: 127.0.0.1, 169.254.x.x,
// file://, and so on. The guard rejects a disallowed scheme and a
// private or reserved IP before any code fetches the URL.
//
// ADR 0056 removed the proof flow, which was the caller that fetched a
// feed_url on behalf of a public request. The guard itself stays. Sync
// registration and the ADR 0054 crawl fetch still call it.
// ============================================================================

#[test]
fn v2_cs01_validate_feed_url_rejects_file_scheme() {
    let result = stophammer::fetch_guard::validate_feed_url("file:///etc/passwd");
    assert!(result.is_err(), "file:// scheme should be rejected");
    assert!(result.unwrap_err().contains("disallowed URL scheme"));
}

#[test]
fn v2_cs01_validate_feed_url_rejects_private_ips() {
    // 127.0.0.0/8 (loopback)
    assert!(
        stophammer::fetch_guard::validate_feed_url("http://127.0.0.1/feed.xml").is_err(),
        "127.0.0.1 should be rejected"
    );

    // 10.0.0.0/8
    assert!(
        stophammer::fetch_guard::validate_feed_url("http://10.0.0.1/feed.xml").is_err(),
        "10.0.0.0/8 should be rejected"
    );

    // 172.16.0.0/12
    assert!(
        stophammer::fetch_guard::validate_feed_url("http://172.16.0.1/feed.xml").is_err(),
        "172.16.0.0/12 should be rejected"
    );

    // 192.168.0.0/16
    assert!(
        stophammer::fetch_guard::validate_feed_url("http://192.168.1.1/feed.xml").is_err(),
        "192.168.0.0/16 should be rejected"
    );

    // 169.254.0.0/16 (link-local / cloud metadata)
    assert!(
        stophammer::fetch_guard::validate_feed_url("http://169.254.169.254/latest/meta-data/")
            .is_err(),
        "169.254.0.0/16 (cloud metadata) should be rejected"
    );

    // IPv6 loopback
    assert!(
        stophammer::fetch_guard::validate_feed_url("http://[::1]/feed.xml").is_err(),
        "::1 should be rejected"
    );
}

#[test]
fn v2_cs01_validate_feed_url_accepts_public_urls() {
    assert!(stophammer::fetch_guard::validate_feed_url("https://93.184.216.34/feed.xml").is_ok());
    assert!(stophammer::fetch_guard::validate_feed_url("http://93.184.216.34/feed.xml").is_ok());
    assert!(stophammer::fetch_guard::validate_feed_url("https://1.1.1.1/podcast.xml").is_ok());
}

#[test]
fn v2_cs01_validate_feed_url_rejects_disallowed_schemes() {
    assert!(stophammer::fetch_guard::validate_feed_url("ftp://example.com/feed.xml").is_err());
    assert!(stophammer::fetch_guard::validate_feed_url("gopher://example.com/").is_err());
    assert!(stophammer::fetch_guard::validate_feed_url("data:text/xml,<rss/>").is_err());
}

#[test]
fn v2_cs01_validate_feed_url_rejects_malformed() {
    assert!(stophammer::fetch_guard::validate_feed_url("not-a-url").is_err());
    assert!(stophammer::fetch_guard::validate_feed_url("").is_err());
}

// ============================================================================
// NEW ATTACK SURFACE: CS-03 — sync/register requires dedicated sync token
//
// V1 STATUS: N/A (was unauthenticated)
// V2 STATUS: CLOSED (CS-03 implemented)
//
// Verify that POST /sync/register without sync token returns 403.
// ============================================================================

#[tokio::test]
async fn v2_cs03_sync_register_requires_sync_token() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);
    let signer = common::temp_signer("test-security-auth-v2-register");
    let (_peer_server, register_body) = signed_register_body_for_mock_peer(&signer).await;

    // Without admin token
    let resp = app
        .clone()
        .oneshot(json_request("POST", "/sync/register", &register_body))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "CS-03: register without sync token returns 403"
    );

    // With wrong admin token
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "wrong-token")
        .body(axum::body::Body::from(
            serde_json::to_vec(&register_body).unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        403,
        "CS-03: register with admin token returns 403"
    );

    // With correct sync token
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Sync-Token", "test-sync-token-v2")
        .body(axum::body::Body::from(
            serde_json::to_vec(&register_body).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "CS-03: register with correct sync token succeeds"
    );
}

// ============================================================================
// NEW ATTACK SURFACE: CS-03 — admin token must not grant sync access
//
// FINDING: CLOSED
//
// Admin compromise is still bad, but it must not grant access to sync/register.
// This test verifies the sync least-privilege boundary.
// ============================================================================

#[tokio::test]
async fn v2_cs03_admin_token_cannot_register_push_peer() {
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
    let signer = common::temp_signer("test-security-auth-v2-blast");
    let (_peer_server, register_body) = signed_register_body_for_mock_peer(&signer).await;

    // With leaked admin token: cannot register push peer
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "test-admin-token-v2")
        .body(axum::body::Body::from(
            serde_json::to_vec(&register_body).unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        403,
        "admin token must not register push peer"
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
