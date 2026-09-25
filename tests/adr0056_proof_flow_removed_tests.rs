//! ADR 0056 task 001: the public proof flow is gone.
//!
//! These tests are the acceptance criteria of
//! `docs/tasks/adr-0056-task-001-remove-proof-flow.md`, through the router.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "adr0056-test-admin-token";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("adr0056-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(
            "test-token".into(),
            vec![],
        )),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: ADMIN_TOKEN.into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

/// Seeds one feed with one track and returns `(feed_guid, track_guid)`.
fn seed_feed_with_track(conn: &rusqlite::Connection) -> (&'static str, &'static str) {
    let now = common::now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES ('adr0056-artist', 'ADR 0056 Artist', 'adr0056 artist', ?1, ?2)",
        rusqlite::params![now, now],
    )
    .expect("insert artist");
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES ('ADR 0056 Artist', ?1)",
        rusqlite::params![now],
    )
    .expect("insert artist_credit");
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, 'adr0056-artist', 0, 'ADR 0056 Artist', '')",
        rusqlite::params![credit_id],
    )
    .expect("insert artist_credit_name");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         description, explicit, episode_count, created_at, updated_at) \
         VALUES ('adr0056-feed', 'https://example.com/adr0056-feed.xml', 'ADR 0056 Feed', \
         'adr0056 feed', ?1, 'A test feed', 0, 0, ?2, ?3)",
        rusqlite::params![credit_id, now, now],
    )
    .expect("insert feed");
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         description, explicit, created_at, updated_at) \
         VALUES ('adr0056-track', 'adr0056-feed', ?1, 'ADR 0056 Track', 'adr0056 track', \
         'A test track', 0, ?2, ?3)",
        rusqlite::params![credit_id, now, now],
    )
    .expect("insert track");
    ("adr0056-feed", "adr0056-track")
}

fn feeds_row_count(conn: &rusqlite::Connection, feed_guid: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM feeds WHERE feed_guid = ?1",
        rusqlite::params![feed_guid],
        |row| row.get(0),
    )
    .expect("count feeds")
}

fn tracks_row_count(conn: &rusqlite::Connection, track_guid: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM tracks WHERE track_guid = ?1",
        rusqlite::params![track_guid],
        |row| row.get(0),
    )
    .expect("count tracks")
}

fn feed_url(conn: &rusqlite::Connection, feed_guid: &str) -> String {
    conn.query_row(
        "SELECT feed_url FROM feeds WHERE feed_guid = ?1",
        rusqlite::params![feed_guid],
        |row| row.get(0),
    )
    .expect("get feed_url")
}

fn json_body(body: &serde_json::Value) -> axum::body::Body {
    axum::body::Body::from(serde_json::to_vec(body).expect("serialize"))
}

// ---------------------------------------------------------------------------
// The two proof routes are gone.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn proofs_challenge_route_answers_404() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/proofs/challenge")
        .header("Content-Type", "application/json")
        .body(json_body(&serde_json::json!({
            "feed_guid": "adr0056-feed",
            "scope": "feed:write",
            "requester_nonce": "at-least-16-chars-x"
        })))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 404, "POST /v1/proofs/challenge must be gone");
}

#[tokio::test]
async fn proofs_assert_route_answers_404() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/proofs/assert")
        .header("Content-Type", "application/json")
        .body(json_body(&serde_json::json!({
            "challenge_id": "nonexistent",
            "requester_nonce": "at-least-16-chars-x"
        })))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 404, "POST /v1/proofs/assert must be gone");
}

// ---------------------------------------------------------------------------
// A bearer header alone answers 403 on each write route, and changes nothing.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retire_feed_with_only_bearer_header_is_forbidden_and_changes_nothing() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed_with_track(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/adr0056-feed")
        .header("Authorization", "Bearer x")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "a bearer header alone must not retire a feed"
    );

    let conn = db.lock().expect("lock db");
    assert_eq!(
        feeds_row_count(&conn, "adr0056-feed"),
        1,
        "the feed must still exist"
    );
}

#[tokio::test]
async fn patch_feed_with_only_bearer_header_is_forbidden_and_changes_nothing() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed_with_track(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/adr0056-feed")
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer x")
        .body(json_body(
            &serde_json::json!({ "feed_url": "https://evil.example.com/feed.xml" }),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "a bearer header alone must not patch a feed"
    );

    let conn = db.lock().expect("lock db");
    assert_eq!(
        feed_url(&conn, "adr0056-feed"),
        "https://example.com/adr0056-feed.xml",
        "the feed_url must be unchanged"
    );
}

#[tokio::test]
async fn remove_track_with_only_bearer_header_is_forbidden_and_changes_nothing() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed_with_track(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/adr0056-feed/tracks/adr0056-track")
        .header("Authorization", "Bearer x")
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "a bearer header alone must not remove a track"
    );

    let conn = db.lock().expect("lock db");
    assert_eq!(
        tracks_row_count(&conn, "adr0056-track"),
        1,
        "the track must still exist"
    );
}

#[tokio::test]
async fn patch_track_with_only_bearer_header_is_forbidden_and_changes_nothing() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed_with_track(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/tracks/adr0056-track")
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer x")
        .body(json_body(
            &serde_json::json!({ "enclosure_url": "https://evil.example.com/track.mp3" }),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(
        resp.status(),
        403,
        "a bearer header alone must not patch a track"
    );
}

// ---------------------------------------------------------------------------
// The same four routes behave as before with the admin token.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn patch_feed_with_admin_token_still_succeeds() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed_with_track(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/feeds/adr0056-feed")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", ADMIN_TOKEN)
        .body(json_body(
            &serde_json::json!({ "feed_url": "https://updated.example.com/feed.xml" }),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 204, "admin token must still patch a feed");

    let conn = db.lock().expect("lock db");
    assert_eq!(
        feed_url(&conn, "adr0056-feed"),
        "https://updated.example.com/feed.xml"
    );
}

#[tokio::test]
async fn patch_track_with_admin_token_still_succeeds() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed_with_track(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/tracks/adr0056-track")
        .header("Content-Type", "application/json")
        .header("X-Admin-Token", ADMIN_TOKEN)
        .body(json_body(
            &serde_json::json!({ "enclosure_url": "https://cdn.example.com/updated.mp3" }),
        ))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 204, "admin token must still patch a track");
}

#[tokio::test]
async fn remove_track_with_admin_token_still_succeeds() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed_with_track(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/adr0056-feed/tracks/adr0056-track")
        .header("X-Admin-Token", ADMIN_TOKEN)
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 204, "admin token must still remove a track");

    let conn = db.lock().expect("lock db");
    assert_eq!(tracks_row_count(&conn, "adr0056-track"), 0);
}

// ---------------------------------------------------------------------------
// A retire with the admin token writes no row to proof_challenges or
// proof_tokens (ADR 0056 decision 4: no application code reads or writes
// these tables any more).
//
// NOTE: a pre-existing SQL trigger, `trg_feeds_cleanup_before_delete`
// (migration 0032, and its predecessors 0019/0026/0030), still runs
// `DELETE FROM proof_tokens` / `DELETE FROM proof_challenges` scoped to the
// retired feed_guid. That trigger is schema, not the Rust-level delete this
// task changed, and decision 4 rules out a migration in this task, so it is
// left as is. In production the tables are empty, so the trigger deletes
// zero rows either way. This test asserts the part decision 4 promises
// through Rust code: retiring a feed inserts no row into either table.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retire_feed_with_admin_token_writes_no_proof_rows() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock db");
        seed_feed_with_track(&conn);
    }
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let req = Request::builder()
        .method("DELETE")
        .uri("/v1/feeds/adr0056-feed")
        .header("X-Admin-Token", ADMIN_TOKEN)
        .body(axum::body::Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 204, "admin token must still retire a feed");

    let conn = db.lock().expect("lock db");
    let challenges: i64 = conn
        .query_row("SELECT COUNT(*) FROM proof_challenges", [], |r| r.get(0))
        .expect("count proof_challenges");
    let tokens: i64 = conn
        .query_row("SELECT COUNT(*) FROM proof_tokens", [], |r| r.get(0))
        .expect("count proof_tokens");
    assert_eq!(
        challenges, 0,
        "retire must write no row to proof_challenges"
    );
    assert_eq!(tokens, 0, "retire must write no row to proof_tokens");
}

// ---------------------------------------------------------------------------
// Static checks
// ---------------------------------------------------------------------------

/// Recursively collects every `.rs` file under `src/`, as a list of
/// `(relative path, contents)` pairs.
fn src_rust_files() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).expect("read src dir") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let contents = std::fs::read_to_string(&path).expect("read source file");
                let rel = path
                    .strip_prefix(".")
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                out.push((rel, contents));
            }
        }
    }
    let mut out = Vec::new();
    walk(std::path::Path::new("src"), &mut out);
    out
}

/// The proof tables stay in `src/schema.sql` and in the historical migration
/// entry named in `src/db.rs`'s `MIGRATIONS` array. No other source file
/// under `src/` names either table (ADR 0056 decision 4).
#[test]
fn proof_tables_are_named_only_in_schema_and_the_migration_array() {
    for (path, contents) in src_rust_files() {
        if contents.contains("proof_challenges") || contents.contains("proof_tokens") {
            assert_eq!(
                path, "src/db.rs",
                "{path} names a proof table outside src/db.rs's migration array"
            );
        }
    }
    let schema = std::fs::read_to_string("src/schema.sql").expect("read schema.sql");
    assert!(
        schema.contains("proof_challenges") && schema.contains("proof_tokens"),
        "src/schema.sql must still declare both proof tables"
    );
}

/// No source file under `src/` calls into a `proof` module: it is deleted.
#[test]
fn no_source_file_references_the_deleted_proof_module() {
    for (path, contents) in src_rust_files() {
        assert!(
            !contents.contains("proof::"),
            "{path} still references the deleted proof:: module"
        );
    }
}

/// The generated `OpenAPI` document carries no `/v1/proofs/` path.
#[tokio::test]
async fn openapi_document_has_no_proofs_path() {
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(axum::body::Body::empty())
                .expect("build request"),
        )
        .await
        .expect("call handler");
    assert_eq!(resp.status(), 200);

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("parse openapi json");
    let paths = json["paths"]
        .as_object()
        .expect("paths object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        paths.iter().all(|p| !p.contains("/v1/proofs/")),
        "openapi document still lists a /v1/proofs/ path: {paths:?}"
    );
}
