mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

use stophammer::ingest::{IngestFeedData, IngestRemoteFeedRef};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-sprint2a"));
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(
            "test-token".into(),
            vec![],
        )),
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
        force_reingest: false,
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

#[test]
fn content_hash_passes_for_publisher_feed_when_cached_hash_matches() {
    let conn = common::test_db();
    let now = common::now();
    let feed_url = "https://example.com/publisher.xml";
    let hash = "publisher-same-hash";

    conn.execute(
        "INSERT INTO feed_crawl_cache (feed_url, content_hash, crawled_at) \
         VALUES (?1, ?2, ?3)",
        rusqlite::params![feed_url, hash, now],
    )
    .expect("insert crawl cache");

    let request = stophammer::ingest::IngestFeedRequest {
        canonical_url: feed_url.to_string(),
        source_url: feed_url.to_string(),
        crawl_token: String::new(),
        http_status: 200,
        content_hash: hash.to_string(),
        force_reingest: false,
        feed_data: Some(IngestFeedData {
            feed_guid: "publisher-feed-guid".to_string(),
            title: "Publisher Feed".to_string(),
            description: None,
            image_url: None,
            language: None,
            explicit: false,
            itunes_type: None,
            raw_medium: Some("publisher".to_string()),
            last_build_date: None,
            author_name: None,
            owner_name: None,
            pub_date: None,
            remote_items: vec![IngestRemoteFeedRef {
                position: 0,
                medium: Some("music".to_string()),
                remote_feed_guid: "music-feed-guid".to_string(),
                remote_feed_url: Some("https://example.com/music.xml".to_string()),
                rel: None,
            }],
            persons: vec![],
            entity_ids: vec![],
            links: vec![],
            feed_payment_routes: vec![],
            tracks: vec![],
            live_items: vec![],
        }),
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
        other => panic!("expected Pass for publisher feed with cached hash, got {other:?}"),
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
        force_reingest: false,
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
        force_reingest: false,
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
