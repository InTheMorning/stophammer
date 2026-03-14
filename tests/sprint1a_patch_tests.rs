#![expect(clippy::significant_drop_tightening, reason = "MutexGuard<Connection> must be held for the full scope in test assertions")]

// Sprint 1A: Issue-12 PATCH emits signed events + Issue-13 PATCH 404 for unknown GUID
// Issue-12 PATCH emits events — 2026-03-13
// Issue-13 PATCH 404 check — 2026-03-13

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-sprint1a.key")
            .expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db,
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
            "feed-1", "https://example.com/feed.xml", "Test Album",
            "test album", credit_id, "A test feed", 0, 0, now, now,
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
            track_guid, feed_guid, credit_id, title,
            title.to_lowercase(), "A test track", 0, now, now,
        ],
    )
    .expect("insert track");
}

fn issue_token_for_feed(conn: &rusqlite::Connection, feed_guid: &str) -> String {
    stophammer::proof::issue_token(conn, "feed:write", feed_guid)
        .expect("issue token")
}

fn count_events(conn: &rusqlite::Connection, event_type: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM events WHERE event_type = ?1",
        rusqlite::params![event_type],
        |row| row.get(0),
    )
    .expect("count events")
}

fn get_latest_event_payload(conn: &rusqlite::Connection, event_type: &str) -> String {
    conn.query_row(
        "SELECT payload_json FROM events WHERE event_type = ?1 ORDER BY seq DESC LIMIT 1",
        rusqlite::params![event_type],
        |row| row.get(0),
    )
    .expect("get latest event payload")
}

// ---------------------------------------------------------------------------
// Issue-12: PATCH /v1/feeds/{guid} must emit a FeedUpserted event
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_feed_emits_feed_upserted_event() {
    let db = common::test_db_arc();
    let token;
    {
        let conn = db.lock().expect("lock db");
        seed_feed(&conn);
        token = issue_token_for_feed(&conn, "feed-1");

        // Verify no events exist yet.
        assert_eq!(count_events(&conn, "feed_upserted"), 0);
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
                "feed_url": "https://updated.example.com/feed.xml"
            }))
            .expect("serialize JSON"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 204);

    // After PATCH, a FeedUpserted event should exist.
    let conn = db.lock().expect("lock db");
    assert_eq!(
        count_events(&conn, "feed_upserted"), 1,
        "PATCH /feeds must emit a FeedUpserted event"
    );

    // The event payload should contain the updated feed_url.
    let payload = get_latest_event_payload(&conn, "feed_upserted");
    assert!(
        payload.contains("https://updated.example.com/feed.xml"),
        "FeedUpserted event payload must reflect the patched feed_url"
    );
}

// ---------------------------------------------------------------------------
// Issue-12: PATCH /v1/tracks/{guid} must emit a TrackUpserted event
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_track_emits_track_upserted_event() {
    let db = common::test_db_arc();
    let token;
    {
        let conn = db.lock().expect("lock db");
        let (credit_id, now) = seed_feed(&conn);
        insert_track(&conn, "track-1", "feed-1", credit_id, "Song One", now);
        token = issue_token_for_feed(&conn, "feed-1");

        assert_eq!(count_events(&conn, "track_upserted"), 0);
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
                "enclosure_url": "https://cdn.example.com/updated-song.mp3"
            }))
            .expect("serialize JSON"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 204);

    let conn = db.lock().expect("lock db");
    assert_eq!(
        count_events(&conn, "track_upserted"), 1,
        "PATCH /tracks must emit a TrackUpserted event"
    );

    let payload = get_latest_event_payload(&conn, "track_upserted");
    assert!(
        payload.contains("https://cdn.example.com/updated-song.mp3"),
        "TrackUpserted event payload must reflect the patched enclosure_url"
    );
}

// ---------------------------------------------------------------------------
// Issue-12: The emitted events must have valid signatures
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_feed_event_has_valid_signature() {
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
                "feed_url": "https://signed.example.com/feed.xml"
            }))
            .expect("serialize JSON"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 204);

    // Read the raw event from the events table and verify its signature.
    let conn = db.lock().expect("lock db");
    let (event_id, event_type_str, payload_json, subject_guid, signed_by, signature, created_at): (String, String, String, String, String, String, i64) =
        conn.query_row(
            "SELECT event_id, event_type, payload_json, subject_guid, signed_by, signature, created_at \
             FROM events WHERE event_type = 'feed_upserted' ORDER BY seq DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
        )
        .expect("read event from DB");

    assert_eq!(event_type_str, "feed_upserted");
    assert_eq!(subject_guid, "feed-1");
    assert!(!signed_by.is_empty(), "signed_by must not be empty");
    assert!(!signature.is_empty(), "signature must not be empty");

    // Reconstruct event and verify signature.
    let tagged = format!(r#"{{"type":"feed_upserted","data":{payload_json}}}"#);
    let ev_payload: stophammer::event::EventPayload =
        serde_json::from_str(&tagged).expect("deserialize event payload");

    let ev = stophammer::event::Event {
        event_id,
        event_type: stophammer::event::EventType::FeedUpserted,
        payload: ev_payload,
        subject_guid,
        signed_by,
        signature,
        seq: 0,
        created_at,
        warnings: vec![],
        payload_json,
    };

    stophammer::signing::verify_event_signature(&ev)
        .expect("FeedUpserted event from PATCH must have a valid ed25519 signature");
}

// ---------------------------------------------------------------------------
// Issue-13: PATCH /v1/feeds/{guid} returns 404 for unknown feed
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_feed_unknown_guid_returns_404() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/nonexistent-feed")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "test-admin-token")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "feed_url": "https://updated.example.com/feed.xml"
            }))
            .expect("serialize JSON"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(), 404,
        "PATCH /feeds with unknown GUID must return 404, not 204"
    );
}

// ---------------------------------------------------------------------------
// Issue-13: PATCH /v1/tracks/{guid} returns 404 for unknown track
// (Note: handle_patch_track already does a get_track_by_guid lookup, so this
// test verifies the existing 404 behaviour is preserved.)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_track_unknown_guid_returns_404() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/tracks/nonexistent-track")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", "test-admin-token")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "enclosure_url": "https://cdn.example.com/nope.mp3"
            }))
            .expect("serialize JSON"),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(), 404,
        "PATCH /tracks with unknown GUID must return 404"
    );
}

// ---------------------------------------------------------------------------
// Issue-12: PATCH /v1/feeds/{guid} with empty body and existing feed returns 204
// but does NOT emit an event (no mutation happened)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn patch_feed_empty_body_does_not_emit_event() {
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
    assert_eq!(resp.status(), 204);

    let conn = db.lock().expect("lock db");
    assert_eq!(
        count_events(&conn, "feed_upserted"), 0,
        "PATCH with no fields should not emit an event"
    );
}
