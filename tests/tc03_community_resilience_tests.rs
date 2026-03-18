// TC-03 community resilience — 2026-03-13

#![expect(
    clippy::significant_drop_tightening,
    reason = "MutexGuard<Connection> must be held for the full scope in test assertions"
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicI64;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// 1. Community poll handles empty response without crash
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_community_poll_handles_empty_response() {
    let mock_server = MockServer::start().await;

    // Primary returns an empty events list.
    Mock::given(method("GET"))
        .and(path("/sync/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "events": [],
            "has_more": false,
            "next_seq": 0
        })))
        .mount(&mock_server)
        .await;

    // Build a community config pointing at the mock primary.
    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let last_push_at = Arc::new(AtomicI64::new(0));

    let config = stophammer::community::CommunityConfig {
        primary_url: mock_server.uri(),
        tracker_url: mock_server.uri(),
        node_address: "http://localhost:9999".into(),
        poll_interval_secs: 1,
        push_timeout_secs: 0, // immediately trigger poll
    };

    // Run the sync loop in a background task; it should not panic.
    let db2 = pool.clone();
    let lp = Arc::clone(&last_push_at);
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/tc03-community-sync.key")
            .expect("create signer"),
    );

    // Also mount tracker and primary register mocks so they don't fail the test.
    Mock::given(method("POST"))
        .and(path("/nodes/register"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/sync/register"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let handle = tokio::spawn(async move {
        stophammer::community::run_community_sync(config, db2, signer, lp, None).await;
    });

    // Let it run briefly, then abort (it's an infinite loop).
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    handle.abort();
    let _ = handle.await;

    // Verify cursor stays at 0: no events applied, so sync state should be absent
    // or at 0.
    let conn = db.lock().expect("lock for verification");
    let cursor: i64 = conn
        .query_row(
            "SELECT COALESCE((SELECT last_seq FROM node_sync_state WHERE node_pubkey = 'primary_sync_cursor'), 0)",
            [],
            |row| row.get(0),
        )
        .expect("query cursor");
    assert_eq!(
        cursor, 0,
        "cursor should stay at 0 when no events are returned"
    );
}

// ---------------------------------------------------------------------------
// 2. Community poll handles 503 gracefully (returns error, no panic)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_community_poll_handles_503_gracefully() {
    let mock_server = MockServer::start().await;

    // Primary returns 503 for sync/events.
    Mock::given(method("GET"))
        .and(path("/sync/events"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&mock_server)
        .await;

    // Mount tracker and register mocks.
    Mock::given(method("POST"))
        .and(path("/nodes/register"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/sync/register"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let last_push_at = Arc::new(AtomicI64::new(0));

    let config = stophammer::community::CommunityConfig {
        primary_url: mock_server.uri(),
        tracker_url: mock_server.uri(),
        node_address: "http://localhost:9999".into(),
        poll_interval_secs: 1,
        push_timeout_secs: 0,
    };

    let db2 = pool.clone();
    let lp = Arc::clone(&last_push_at);
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/tc03-community-sync-503.key")
            .expect("create signer"),
    );

    // The sync loop should log the error and keep going, not panic.
    let handle = tokio::spawn(async move {
        stophammer::community::run_community_sync(config, db2, signer, lp, None)
            .await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    handle.abort();
    let result = handle.await;

    // The task should have been aborted (JoinError::Cancelled), NOT panicked.
    assert!(result.is_err(), "task should have been aborted");
    let err = result.unwrap_err();
    assert!(
        err.is_cancelled(),
        "task should be cancelled, not panicked: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. Push handler rejects events signed by unknown key
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_community_push_handler_rejects_wrong_signer() {
    use axum::body::Body;
    use http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let primary_pubkey = "aaaa1111bbbb2222cccc3333dddd4444eeee5555ffff6666aaaa1111bbbb2222";

    let state = Arc::new(stophammer::community::CommunityState {
        db: pool.clone(),
        primary_pubkey_hex: primary_pubkey.to_string(),
        last_push_at: Arc::new(AtomicI64::new(0)),
        sse_registry: None,
    });

    let app = stophammer::community::build_community_push_router(state);

    // Push an event signed by a DIFFERENT key (wrong signer).
    let wrong_signer = "1111aaaa2222bbbb3333cccc4444dddd5555eeee6666ffff1111aaaa2222bbbb";
    let payload = serde_json::json!({
        "events": [{
            "event_id": "evt-bad-signer",
            "event_type": "artist_upserted",
            "payload": {
                "type": "artist_upserted",
                "data": {
                    "artist": {
                        "artist_id": "art-bad",
                        "name": "Bad Artist",
                        "name_lower": "bad artist",
                        "created_at": 1000,
                        "updated_at": 1000
                    }
                }
            },
            "subject_guid": "art-bad",
            "signed_by": wrong_signer,
            "signature": "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "seq": 1,
            "created_at": 1000,
            "warnings": [],
            "payload_json": "{\"artist\":{\"artist_id\":\"art-bad\",\"name\":\"Bad Artist\",\"name_lower\":\"bad artist\",\"created_at\":1000,\"updated_at\":1000}}"
        }]
    });

    let req = Request::builder()
        .method("POST")
        .uri("/sync/push")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("request should not panic");
    assert_eq!(resp.status(), 200);

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");

    // The event should be rejected (wrong signer), not applied.
    assert_eq!(body["applied"].as_u64().expect("applied"), 0);
    assert_eq!(body["rejected"].as_u64().expect("rejected"), 1);

    // Verify the artist was NOT inserted into the DB.
    let conn = db.lock().expect("lock for verification");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE artist_id = 'art-bad'",
            [],
            |row| row.get(0),
        )
        .expect("query artists");
    assert_eq!(
        count, 0,
        "artist from wrong-signer event should not be in DB"
    );
}

// ---------------------------------------------------------------------------
// 4. Push handler accepts valid events signed by primary key
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_community_push_handler_accepts_valid_events() {
    use axum::body::Body;
    use http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());

    // Create a real signer so we can produce valid signatures.
    let signer = stophammer::signing::NodeSigner::load_or_create("/tmp/test-tc03-signer.key")
        .expect("create signer");
    let primary_pubkey = signer.pubkey_hex().to_string();
    let now = stophammer::db::unix_now();

    // Build a valid ArtistUpserted event.
    let event_id = uuid::Uuid::new_v4().to_string();
    let artist_payload = serde_json::json!({
        "artist": {
            "artist_id": "art-valid-tc03",
            "name": "Valid TC03 Artist",
            "name_lower": "valid tc03 artist",
            "created_at": now,
            "updated_at": now
        }
    });
    let payload_json = serde_json::to_string(&artist_payload).expect("serialize payload");

    let (signed_by, signature) = signer.sign_event(
        &event_id,
        &stophammer::event::EventType::ArtistUpserted,
        &payload_json,
        "art-valid-tc03",
        now,
        1, // Issue-SEQ-INTEGRITY — 2026-03-14
    );

    let push_body = serde_json::json!({
        "events": [{
            "event_id": event_id,
            "event_type": "artist_upserted",
            "payload": {
                "type": "artist_upserted",
                "data": artist_payload
            },
            "subject_guid": "art-valid-tc03",
            "signed_by": signed_by,
            "signature": signature,
            "seq": 1,
            "created_at": now,
            "warnings": [],
            "payload_json": payload_json
        }]
    });

    let state = Arc::new(stophammer::community::CommunityState {
        db: pool.clone(),
        primary_pubkey_hex: primary_pubkey,
        last_push_at: Arc::new(AtomicI64::new(0)),
        sse_registry: None,
    });

    let app = stophammer::community::build_community_push_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/sync/push")
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&push_body).expect("serialize"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("request should not panic");
    assert_eq!(resp.status(), 200);

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");

    assert_eq!(body["applied"].as_u64().expect("applied"), 1);
    assert_eq!(body["rejected"].as_u64().expect("rejected"), 0);

    // Verify the artist WAS inserted into the DB.
    let conn = db.lock().expect("lock for verification");
    let name: String = conn
        .query_row(
            "SELECT name FROM artists WHERE artist_id = 'art-valid-tc03'",
            [],
            |row| row.get(0),
        )
        .expect("artist should exist in DB");
    assert_eq!(name, "Valid TC03 Artist");
}
