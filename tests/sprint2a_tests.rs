mod common;

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

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-sprint2a.key")
            .expect("create signer"),
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

fn seed_feed_at_url(conn: &rusqlite::Connection, feed_url: &str) -> (i64, i64) {
    let now = common::now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params!["artist-s2a", "Test Artist", "test artist", now, now],
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
        rusqlite::params![credit_id, "artist-s2a", 0, "Test Artist", ""],
    )
    .expect("insert artist_credit_name");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         description, explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            "feed-abc",
            feed_url,
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

// ============================================================================
// Issue #3: Mutation endpoints must respond at /v1/ prefix
// ============================================================================

// ---------------------------------------------------------------------------
// Test: DELETE /v1/feeds/{guid} responds with 204
// ---------------------------------------------------------------------------
#[tokio::test]
async fn delete_feed_at_v1_prefix_returns_204() {
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
    assert_eq!(
        resp.status(),
        204,
        "DELETE /v1/feeds/feed-1 should return 204"
    );
}

// ---------------------------------------------------------------------------
// Test: PATCH /v1/feeds/{guid} responds with 204 No Content
// REST semantics compliant (RFC 7396) — 2026-03-12
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_feed_at_v1_prefix_returns_204() {
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
        "PATCH /v1/feeds/feed-1 should return 204"
    );
}

// ---------------------------------------------------------------------------
// Test: PATCH /v1/tracks/{guid} responds with 204 No Content
// REST semantics compliant (RFC 7396) — 2026-03-12
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_track_at_v1_prefix_returns_204() {
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
        "PATCH /v1/tracks/track-1 should return 204"
    );
}

// ---------------------------------------------------------------------------
// Test: POST /v1/proofs/challenge responds with 201
// ---------------------------------------------------------------------------
#[tokio::test]
async fn proofs_challenge_at_v1_prefix_returns_201() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed_at_url(&conn, "https://example.com/proof-feed.xml");
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/proofs/challenge")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "feed_guid": "feed-abc",
                "scope": "feed:write",
                "requester_nonce": "test-nonce-1234x"
            }))
            .expect("serialize JSON"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        201,
        "POST /v1/proofs/challenge should return 201"
    );
}

// ---------------------------------------------------------------------------
// Test: POST /v1/proofs/assert responds with 200
// ---------------------------------------------------------------------------
#[tokio::test]
async fn proofs_assert_at_v1_prefix_returns_200() {
    let mock_server = MockServer::start().await;
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed_at_url(&conn, &mock_server.uri());
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // First create a challenge to assert against.
    let nonce = "test-nonce-1234x";
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/proofs/challenge")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "feed_guid": "feed-abc",
                        "scope": "feed:write",
                        "requester_nonce": nonce
                    }))
                    .expect("serialize JSON"),
                ))
                .expect("build request"),
        )
        .await
        .expect("call handler");
    assert_eq!(resp.status(), 201);

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("parse JSON");
    let challenge_id = body["challenge_id"].as_str().expect("challenge_id");
    let token_binding = body["token_binding"].as_str().expect("token_binding");

    // Mount RSS with the correct token_binding
    let rss = rss_with_podcast_txt(&format!("stophammer-proof {token_binding}"));
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/proofs/assert")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "challenge_id": challenge_id,
                "requester_nonce": nonce
            }))
            .expect("serialize JSON"),
        ))
        .expect("build request");

    let resp2 = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp2.status(),
        200,
        "POST /v1/proofs/assert should return 200"
    );
}

// ---------------------------------------------------------------------------
// Test: DELETE /v1/feeds/{guid}/tracks/{track_guid} responds with 204
// ---------------------------------------------------------------------------
#[tokio::test]
async fn delete_track_at_v1_prefix_returns_204() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        let (credit_id, now) = seed_feed(&conn);
        insert_track(&conn, "track-1", "feed-1", credit_id, "Song One", now);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/feed-1/tracks/track-1")
        .header("X-Admin-Token", "test-admin-token")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        204,
        "DELETE /v1/feeds/feed-1/tracks/track-1 should return 204"
    );
}

// ============================================================================
// Issue #7: RSS-fetch skip logic (content hash no-change) unit test
// ============================================================================

// ---------------------------------------------------------------------------
// Test: ContentHashVerifier returns NO_CHANGE when hash matches cached value
// ---------------------------------------------------------------------------
#[test]
fn content_hash_skip_returns_no_change_when_cached_hash_matches() {
    let conn = common::test_db();
    let now = common::now();
    let feed_url = "https://example.com/feed.xml";
    let hash = "abc123def456";

    // Pre-populate the crawl cache with a known hash.
    conn.execute(
        "INSERT INTO feed_crawl_cache (feed_url, content_hash, crawled_at) \
         VALUES (?1, ?2, ?3)",
        rusqlite::params![feed_url, hash, now],
    )
    .expect("insert crawl cache");

    // Build a request with the same hash — should trigger skip.
    let request = stophammer::ingest::IngestFeedRequest {
        canonical_url: feed_url.to_string(),
        source_url: feed_url.to_string(),
        crawl_token: String::new(),
        http_status: 304,
        content_hash: hash.to_string(),
        feed_data: None,
    };

    let verifier = stophammer::verifiers::content_hash::ContentHashVerifier;
    let ctx = stophammer::verify::IngestContext {
        request: &request,
        db: &conn,
        existing: None,
    };

    let result = stophammer::verify::Verifier::verify(&verifier, &ctx);
    match result {
        stophammer::verify::VerifyResult::Fail(reason) => {
            assert_eq!(
                reason,
                stophammer::verifiers::content_hash::NO_CHANGE_SENTINEL,
                "should return NO_CHANGE sentinel when hash matches"
            );
        }
        other => panic!("expected Fail(NO_CHANGE), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Test: ContentHashVerifier passes when hash differs from cached value
// ---------------------------------------------------------------------------
#[test]
fn content_hash_passes_when_hash_differs() {
    let conn = common::test_db();
    let now = common::now();
    let feed_url = "https://example.com/feed.xml";

    // Pre-populate the crawl cache with one hash.
    conn.execute(
        "INSERT INTO feed_crawl_cache (feed_url, content_hash, crawled_at) \
         VALUES (?1, ?2, ?3)",
        rusqlite::params![feed_url, "old-hash-value", now],
    )
    .expect("insert crawl cache");

    // Build a request with a different hash — should NOT skip.
    let request = stophammer::ingest::IngestFeedRequest {
        canonical_url: feed_url.to_string(),
        source_url: feed_url.to_string(),
        crawl_token: String::new(),
        http_status: 200,
        content_hash: "new-hash-value".to_string(),
        feed_data: None,
    };

    let verifier = stophammer::verifiers::content_hash::ContentHashVerifier;
    let ctx = stophammer::verify::IngestContext {
        request: &request,
        db: &conn,
        existing: None,
    };

    let result = stophammer::verify::Verifier::verify(&verifier, &ctx);
    match result {
        stophammer::verify::VerifyResult::Pass => {}
        other => panic!("expected Pass when hash differs, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Test: ContentHashVerifier passes when no cached entry exists (first crawl)
// ---------------------------------------------------------------------------
#[test]
fn content_hash_passes_on_first_crawl() {
    let conn = common::test_db();

    // No crawl cache entry — first time crawling this feed.
    let request = stophammer::ingest::IngestFeedRequest {
        canonical_url: "https://example.com/new-feed.xml".to_string(),
        source_url: "https://example.com/new-feed.xml".to_string(),
        crawl_token: String::new(),
        http_status: 200,
        content_hash: "first-hash".to_string(),
        feed_data: None,
    };

    let verifier = stophammer::verifiers::content_hash::ContentHashVerifier;
    let ctx = stophammer::verify::IngestContext {
        request: &request,
        db: &conn,
        existing: None,
    };

    let result = stophammer::verify::Verifier::verify(&verifier, &ctx);
    match result {
        stophammer::verify::VerifyResult::Pass => {}
        other => panic!("expected Pass on first crawl, got {other:?}"),
    }
}
