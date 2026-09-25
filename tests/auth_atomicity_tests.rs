mod common;

use rusqlite::params;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

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
    let signer = Arc::new(common::temp_signer("test-auth-atomicity"));
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
    insert_artist(conn, "artist-1", "Test Artist", now);
    let credit_id = insert_artist_credit(conn, "artist-1", "Test Artist", now);
    insert_feed(
        conn,
        "feed-1",
        "https://example.com/feed.xml",
        "Test Album",
        credit_id,
        now,
    );
    (credit_id, now)
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

use http::Request;
use tower::ServiceExt;

// ============================================================================
// DELETE /feeds/{guid} with admin token uses atomic auth+write
//
// End-to-end test: the handler must succeed with admin auth, proving
// the single-lock-scope path works through the full handler. ADR 0056
// removed the bearer path and its `check_admin_or_bearer_with_conn` helper,
// so this is the one auth path this file now covers.
// ============================================================================

#[tokio::test]
async fn retire_feed_admin_atomic() {
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
        "retire with admin token should return 204"
    );

    // Verify feed actually deleted.
    let count: i64 = {
        let conn = db.lock().expect("lock db");
        conn.query_row(
            "SELECT COUNT(*) FROM feeds WHERE feed_guid = 'feed-1'",
            [],
            |r| r.get(0),
        )
        .expect("count feeds")
    };
    assert_eq!(count, 0, "feed should be deleted after retire");
}
