// Issue-SSE-EXHAUSTION — 2026-03-15
//
// Tests for the SSE registry exhaustion fix: unknown artist IDs must NOT create
// channels, while real artist IDs must still work.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use rusqlite::params;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create("/tmp/test-sse-exhaustion.key")
            .expect("create signer"),
    );
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(vec![])),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: String::new(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

fn insert_artist(conn: &rusqlite::Connection, artist_id: &str, name: &str) {
    let now = stophammer::db::unix_now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![artist_id, name, name.to_lowercase(), now, now],
    )
    .expect("insert artist");
}

// ---------------------------------------------------------------------------
// Test: subscribing with a fake artist ID does NOT create a registry channel
// ---------------------------------------------------------------------------
#[tokio::test]
async fn fake_artist_id_does_not_create_channel() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(Arc::clone(&state));

    assert_eq!(
        state.sse_registry.artist_count(),
        0,
        "registry starts empty"
    );

    let req = Request::builder()
        .method("GET")
        .uri("/v1/events?artists=bogus-id-1,bogus-id-2,bogus-id-3")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        200,
        "SSE endpoint returns 200 even with unknown IDs"
    );

    // The registry must NOT have created channels for the fake artist IDs.
    assert_eq!(
        state.sse_registry.artist_count(),
        0,
        "fake artist IDs must not create channels in the registry"
    );
}

// ---------------------------------------------------------------------------
// Test: subscribing with a real artist ID creates a channel and events flow
// ---------------------------------------------------------------------------
#[tokio::test]
async fn real_artist_id_creates_channel() {
    let db = common::test_db_arc();

    // Insert a real artist into the database.
    {
        let conn = db.lock().expect("lock db");
        insert_artist(&conn, "real-artist-1", "Test Artist");
    }

    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(Arc::clone(&state));

    assert_eq!(
        state.sse_registry.artist_count(),
        0,
        "registry starts empty"
    );

    let req = Request::builder()
        .method("GET")
        .uri("/v1/events?artists=real-artist-1")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        200,
        "SSE endpoint returns 200 for real artist"
    );

    // The registry should have created a channel for the real artist.
    assert_eq!(
        state.sse_registry.artist_count(),
        1,
        "real artist ID must create a channel in the registry"
    );

    // Verify events can flow through the channel.
    let mut rx = state
        .sse_registry
        .subscribe("real-artist-1")
        .expect("subscribe to existing channel");

    let frame = stophammer::api::SseFrame {
        event_type: "track_upserted".to_string(),
        subject_guid: "track-abc".to_string(),
        payload: serde_json::json!({"title": "Hello"}),
        seq: 1,
    };
    state.sse_registry.publish("real-artist-1", frame);

    let received = rx.recv().await.expect("receive event");
    assert_eq!(received.event_type, "track_upserted");
    assert_eq!(received.subject_guid, "track-abc");
}

// ---------------------------------------------------------------------------
// Test: mixed real and fake IDs — only real ones get channels
// ---------------------------------------------------------------------------
#[tokio::test]
async fn mixed_real_and_fake_ids_only_real_get_channels() {
    let db = common::test_db_arc();

    {
        let conn = db.lock().expect("lock db");
        insert_artist(&conn, "real-a", "Artist A");
        insert_artist(&conn, "real-b", "Artist B");
    }

    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(Arc::clone(&state));

    let req = Request::builder()
        .method("GET")
        .uri("/v1/events?artists=real-a,bogus-x,real-b,bogus-y")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 200);

    assert_eq!(
        state.sse_registry.artist_count(),
        2,
        "only the 2 real artist IDs should create channels"
    );
}

// ---------------------------------------------------------------------------
// Test: db::artist_exists returns false for nonexistent, true for real
// ---------------------------------------------------------------------------
#[test]
fn artist_exists_db_function() {
    let conn = common::test_db();

    assert!(
        !stophammer::db::artist_exists(&conn, "nonexistent").expect("query"),
        "nonexistent artist should return false"
    );

    let now = stophammer::db::unix_now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES ('exists-1', 'Name', 'name', ?1, ?2)",
        params![now, now],
    )
    .expect("insert");

    assert!(
        stophammer::db::artist_exists(&conn, "exists-1").expect("query"),
        "inserted artist should return true"
    );
}
