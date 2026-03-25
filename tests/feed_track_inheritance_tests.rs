mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use rusqlite::params;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-inheritance"));
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

fn seed_feed_and_track(conn: &rusqlite::Connection) -> i64 {
    let now = common::now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["artist-inh", "Inherit Artist", "inherit artist", now, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        params!["Inherit Artist", now],
    )
    .unwrap();
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, ?2, 0, ?3, '')",
        params![credit_id, "artist-inh", "Inherit Artist"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, ?7)",
        params![
            "feed-inh",
            "https://example.com/feed.xml",
            "Inherit Feed",
            "inherit feed",
            credit_id,
            now,
            now,
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         explicit, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7)",
        params![
            "track-inh",
            "feed-inh",
            credit_id,
            "Inherit Track",
            "inherit track",
            now,
            now,
        ],
    )
    .unwrap();
    now
}

fn insert_track_route(conn: &rusqlite::Connection, track_guid: &str, name: &str, address: &str) {
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES (?1, 'feed-inh', ?2, 'keysend', ?3, 100, 0)",
        params![track_guid, name, address],
    )
    .unwrap();
}

fn insert_feed_route(conn: &rusqlite::Connection, feed_guid: &str, name: &str, address: &str) {
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES (?1, ?2, 'keysend', ?3, 100, 0)",
        params![feed_guid, name, address],
    )
    .unwrap();
}

fn insert_contributor(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
    name: &str,
    now: i64,
) {
    conn.execute(
        "INSERT INTO source_contributor_claims \
         (feed_guid, entity_type, entity_id, position, name, role, source, extraction_path, observed_at) \
         VALUES (?1, ?2, ?3, 0, ?4, 'host', 'rss', '/rss/channel/item/podcast:person', ?5)",
        params![feed_guid, entity_type, entity_id, name, now],
    )
    .unwrap();
}

async fn get_track_json(
    state: Arc<stophammer::api::AppState>,
    track_guid: &str,
    include: &str,
) -> serde_json::Value {
    let app = stophammer::api::build_router(state);
    let uri = format!("/v1/tracks/{track_guid}?include={include}");
    let req = Request::builder().uri(&uri).body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        200,
        "GET {uri} → {}",
        String::from_utf8_lossy(&body)
    );
    let envelope: serde_json::Value = serde_json::from_slice(&body).unwrap();
    envelope["data"].clone()
}

// ---------------------------------------------------------------------------
// Payment routes inheritance
// ---------------------------------------------------------------------------

#[tokio::test]
async fn track_with_own_routes_returns_track_routes() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().unwrap();
        seed_feed_and_track(&conn);
        insert_track_route(&conn, "track-inh", "Track Artist", "aaa111");
        insert_feed_route(&conn, "feed-inh", "Feed Artist", "bbb222");
    }
    let state = test_app_state(db);
    let json = get_track_json(state, "track-inh", "payment_routes").await;
    let routes = json["payment_routes"]
        .as_array()
        .unwrap_or_else(|| panic!("payment_routes missing from: {json}"));
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0]["recipient_name"], "Track Artist");
    assert_eq!(routes[0]["address"], "aaa111");
}

#[tokio::test]
async fn track_without_routes_inherits_feed_routes() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().unwrap();
        seed_feed_and_track(&conn);
        // No track routes — only feed routes
        insert_feed_route(&conn, "feed-inh", "Feed Artist", "bbb222");
    }
    let state = test_app_state(db);
    let json = get_track_json(state, "track-inh", "payment_routes").await;
    let routes = json["payment_routes"].as_array().unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0]["recipient_name"], "Feed Artist");
    assert_eq!(routes[0]["address"], "bbb222");
}

#[tokio::test]
async fn track_without_routes_and_feed_empty_returns_empty() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().unwrap();
        seed_feed_and_track(&conn);
        // No routes at all
    }
    let state = test_app_state(db);
    let json = get_track_json(state, "track-inh", "payment_routes").await;
    let routes = json["payment_routes"].as_array().unwrap();
    assert!(routes.is_empty());
}

// ---------------------------------------------------------------------------
// Source contributors inheritance
// ---------------------------------------------------------------------------

#[tokio::test]
async fn track_with_own_contributors_returns_track_contributors() {
    let db = common::test_db_arc();
    let now;
    {
        let conn = db.lock().unwrap();
        now = seed_feed_and_track(&conn);
        insert_contributor(&conn, "feed-inh", "track", "track-inh", "Track Host", now);
        insert_contributor(&conn, "feed-inh", "feed", "feed-inh", "Feed Host", now);
    }
    let state = test_app_state(db);
    let json = get_track_json(state, "track-inh", "source_contributors").await;
    let contribs = json["source_contributors"].as_array().unwrap();
    assert_eq!(contribs.len(), 1);
    assert_eq!(contribs[0]["name"], "Track Host");
    assert_eq!(contribs[0]["entity_type"], "track");
}

#[tokio::test]
async fn track_without_contributors_inherits_feed_contributors() {
    let db = common::test_db_arc();
    let now;
    {
        let conn = db.lock().unwrap();
        now = seed_feed_and_track(&conn);
        // No track contributors — only feed-level
        insert_contributor(&conn, "feed-inh", "feed", "feed-inh", "Feed Host", now);
    }
    let state = test_app_state(db);
    let json = get_track_json(state, "track-inh", "source_contributors").await;
    let contribs = json["source_contributors"].as_array().unwrap();
    assert_eq!(contribs.len(), 1);
    assert_eq!(contribs[0]["name"], "Feed Host");
    // The inherited contributor retains its original entity_type/entity_id
    assert_eq!(contribs[0]["entity_type"], "feed");
    assert_eq!(contribs[0]["entity_id"], "feed-inh");
}
