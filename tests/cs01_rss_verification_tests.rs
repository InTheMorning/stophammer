#![expect(clippy::significant_drop_tightening, reason = "MutexGuard<Connection> must be held for the full scope in test setup")]

// CS-01 pod:txt RSS verification tests — 2026-03-12

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use http_body_util::BodyExt;
use rusqlite::params;
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
            feed_guid, feed_url, title,
            title.to_lowercase(),
            credit_id, "A test feed", 0, 0, now, now,
        ],
    )
    .expect("insert feed");
}

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-cs01-signer.key")
            .expect("signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(vec![])),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token:      None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

fn json_request(method_str: &str, uri: &str, body: &serde_json::Value) -> Request<axum::body::Body> {
    Request::builder()
        .method(method_str)
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(body).expect("json"),
        ))
        .expect("build request")
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let collected = resp.into_body().collect().await.expect("collect body");
    let bytes = collected.to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

/// Generate RSS XML with a podcast:txt element containing the given text.
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

/// Generate RSS XML without any podcast:txt element.
fn rss_without_podcast_txt() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Test Podcast</title>
  </channel>
</rss>"#
        .to_string()
}

/// Set up a challenge and return the `challenge_id` and `token_binding`.
/// The feed must already exist in the DB with the given `feed_guid`.
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
    let challenge_id = body["challenge_id"].as_str().expect("challenge_id").to_string();
    let token_binding = body["token_binding"].as_str().expect("token_binding").to_string();
    (challenge_id, token_binding)
}

// ============================================================================
// CS-01 Unit tests: verify_podcast_txt function
// ============================================================================

// ---------------------------------------------------------------------------
// CS-01-01: verify_podcast_txt returns Ok(true) when RSS contains matching txt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_valid_match() {
    let mock_server = MockServer::start().await;
    let token_binding = "abc123.hashpart";
    let rss = rss_with_podcast_txt(&format!("stophammer-proof {token_binding}"));

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let result = stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), token_binding)
        .await;
    assert_eq!(result, Ok(true), "should return Ok(true) when RSS contains matching podcast:txt");
}

// ---------------------------------------------------------------------------
// CS-01-02: verify_podcast_txt returns Ok(false) when RSS has no podcast:txt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_no_txt_element() {
    let mock_server = MockServer::start().await;
    let rss = rss_without_podcast_txt();

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let result = stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), "abc123.hash")
        .await;
    assert_eq!(result, Ok(false), "should return Ok(false) when no podcast:txt in RSS");
}

// ---------------------------------------------------------------------------
// CS-01-03: verify_podcast_txt returns Ok(false) when podcast:txt has wrong token
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_wrong_token() {
    let mock_server = MockServer::start().await;
    let rss = rss_with_podcast_txt("stophammer-proof wrong-token.wrong-hash");

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let result = stophammer::proof::verify_podcast_txt(
        &client,
        &mock_server.uri(),
        "correct-token.correct-hash",
    )
    .await;
    assert_eq!(result, Ok(false), "should return Ok(false) when podcast:txt has wrong token");
}

// ---------------------------------------------------------------------------
// CS-01-04: verify_podcast_txt returns Err when server is unreachable
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_unreachable_server() {
    let client = reqwest::Client::new();
    // Use a port that is almost certainly not listening.
    let result = stophammer::proof::verify_podcast_txt(
        &client,
        "http://127.0.0.1:1",
        "token.hash",
    )
    .await;
    assert!(result.is_err(), "should return Err when server is unreachable");
}

// ---------------------------------------------------------------------------
// CS-01-05: verify_podcast_txt handles multiple podcast:txt elements (match on any)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_podcast_txt_multiple_txt_elements() {
    let mock_server = MockServer::start().await;
    let token_binding = "multi-token.multi-hash";
    let rss = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
  <channel>
    <title>Test Podcast</title>
    <podcast:txt>some-other-verification abc123</podcast:txt>
    <podcast:txt>stophammer-proof {token_binding}</podcast:txt>
    <podcast:txt>yet-another-thing</podcast:txt>
  </channel>
</rss>"#
    );

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let result = stophammer::proof::verify_podcast_txt(&client, &mock_server.uri(), token_binding)
        .await;
    assert_eq!(result, Ok(true), "should find matching txt among multiple elements");
}

// ============================================================================
// CS-01 Integration tests: handle_proofs_assert with RSS verification
// ============================================================================

// ---------------------------------------------------------------------------
// CS-01-06: assert without matching RSS podcast:txt returns 400
// ---------------------------------------------------------------------------

#[tokio::test]
async fn assert_without_rss_txt_returns_400() {
    let mock_server = MockServer::start().await;
    let rss = rss_without_podcast_txt();
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        let now = common::now();
        insert_artist(&conn, "artist-1", "Test Artist", now);
        let credit_id = insert_artist_credit(&conn, "artist-1", "Test Artist", now);
        // Feed URL points to mock server
        insert_feed(&conn, "feed-rss-1", &mock_server.uri(), "Test Album", credit_id, now);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let nonce = "rss-test-nonce-16ch";
    let (challenge_id, _token_binding) = create_challenge_for_feed(&app, "feed-rss-1", nonce).await;

    // Assert -- should fail because RSS does not have podcast:txt
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
        .expect("assert request");

    assert_eq!(resp.status(), 400, "should return 400 when RSS has no matching podcast:txt");
    let body = body_json(resp).await;
    assert!(
        body["error"]
            .as_str()
            .expect("error field")
            .contains("token_binding not found"),
        "error message should mention token_binding not found"
    );
}

// ---------------------------------------------------------------------------
// CS-01-07: assert with valid RSS podcast:txt returns 200 + token
// ---------------------------------------------------------------------------

#[tokio::test]
async fn assert_with_valid_rss_txt_succeeds() {
    let mock_server = MockServer::start().await;

    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        let now = common::now();
        insert_artist(&conn, "artist-2", "Test Artist", now);
        let credit_id = insert_artist_credit(&conn, "artist-2", "Test Artist", now);
        insert_feed(&conn, "feed-rss-2", &mock_server.uri(), "Test Album 2", credit_id, now);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let nonce = "rss-valid-nonce-16ch";
    let (challenge_id, token_binding) = create_challenge_for_feed(&app, "feed-rss-2", nonce).await;

    // Now set up the mock to serve RSS with the correct token_binding.
    let rss = rss_with_podcast_txt(&format!("stophammer-proof {token_binding}"));
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    // Assert -- should succeed
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
        .expect("assert request");

    assert_eq!(resp.status(), 200, "should return 200 when RSS has matching podcast:txt");
    let body = body_json(resp).await;
    assert!(body["access_token"].as_str().is_some(), "response should contain access_token");
    assert_eq!(body["scope"].as_str().expect("scope"), "feed:write");
    assert_eq!(body["subject_feed_guid"].as_str().expect("guid"), "feed-rss-2");
}

// ---------------------------------------------------------------------------
// CS-01-08: assert with wrong token in RSS returns 400
// ---------------------------------------------------------------------------

#[tokio::test]
async fn assert_with_wrong_rss_token_returns_400() {
    let mock_server = MockServer::start().await;

    // Serve RSS with a WRONG token_binding
    let rss = rss_with_podcast_txt("stophammer-proof completely-wrong-token.wrong-hash");
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rss))
        .mount(&mock_server)
        .await;

    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        let now = common::now();
        insert_artist(&conn, "artist-3", "Test Artist", now);
        let credit_id = insert_artist_credit(&conn, "artist-3", "Test Artist", now);
        insert_feed(&conn, "feed-rss-3", &mock_server.uri(), "Test Album 3", credit_id, now);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let nonce = "rss-wrong-nonce-16ch";
    let (challenge_id, _token_binding) = create_challenge_for_feed(&app, "feed-rss-3", nonce).await;

    // Assert -- should fail because RSS has wrong token
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
        .expect("assert request");

    assert_eq!(resp.status(), 400, "should return 400 when RSS has wrong token_binding");
}

// ---------------------------------------------------------------------------
// CS-01-09: assert when feed not in DB returns 404
// ---------------------------------------------------------------------------

#[tokio::test]
async fn assert_feed_not_in_db_returns_404() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    // Create a challenge for a feed_guid that does NOT have a corresponding
    // feeds row (only proof_challenges entry exists).
    let nonce = "nofeed-nonce-16ch-x";
    let (challenge_id, _token_binding) =
        create_challenge_for_feed(&app, "nonexistent-feed", nonce).await;

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
        .expect("assert request");

    assert_eq!(resp.status(), 404, "should return 404 when feed not found in DB");
}

// ---------------------------------------------------------------------------
// CS-01-10: assert when RSS server is down returns 503
// ---------------------------------------------------------------------------

#[tokio::test]
async fn assert_rss_server_down_returns_503() {
    // Use a non-routable address (RFC 5737 TEST-NET) that will always fail to connect.
    let unreachable_url = "http://192.0.2.1:1/feed.xml";

    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        let now = common::now();
        insert_artist(&conn, "artist-5", "Test Artist", now);
        let credit_id = insert_artist_credit(&conn, "artist-5", "Test Artist", now);
        insert_feed(&conn, "feed-rss-5", unreachable_url, "Test Album 5", credit_id, now);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let nonce = "rss-down-nonce-16ch";
    let (challenge_id, _token_binding) =
        create_challenge_for_feed(&app, "feed-rss-5", nonce).await;

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
        .expect("assert request");

    assert_eq!(resp.status(), 503, "should return 503 when RSS server is unreachable");
}
