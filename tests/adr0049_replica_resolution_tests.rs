// ADR 0049 task 004b: db::resolve_listed_feed also accepts the stored
// feeds.feed_url of an indexed feed, so a new community node — which holds
// no feed_url_observations copied by migration 0036 — resolves a listed
// link the same way the primary node does.
//
// Section 1 proves the resolver's step 3 directly against the database,
// with feeds inserted by raw SQL. The helpers are copied from
// tests/adr0049_resolver_tests.rs, per the task packet, rather than
// imported from that file.
//
// Section 2 is the replica test. Database A ingests detox-artist and
// detox-album over HTTP, the same fixtures tests/adr0049_resolver_tests.rs
// and tests/adr0049_link_stats_tests.rs use. Database B applies every event
// of A except each FeedUrlObserved event, so B holds no observation — the
// state of a new community node for a feed ingested before task 004. The
// two databases must give the same publisher views, artist count and link
// statistics.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use rusqlite::params;
use tower::ServiceExt;

use stophammer::db::ListedFeedResolution;

// ---------------------------------------------------------------------------
// Section 1: db::resolve_listed_feed step 3, direct against the database.
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
         explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            feed_guid,
            feed_url,
            title,
            title.to_lowercase(),
            credit_id,
            0,
            0,
            now,
            now
        ],
    )
    .expect("insert feed");
}

/// Inserts a minimal indexed feed, with its own artist and artist-credit row.
fn seed_indexed_feed(conn: &rusqlite::Connection, feed_guid: &str, feed_url: &str, now: i64) {
    let artist_id = format!("{feed_guid}-artist");
    insert_artist(conn, &artist_id, "Seed Artist", now);
    let credit_id = insert_artist_credit(conn, &artist_id, "Seed Artist", now);
    insert_feed(conn, feed_guid, feed_url, "Seed Feed", credit_id, now);
}

#[test]
fn resolve_listed_feed_by_stored_feed_url_when_no_observation_exists() {
    let conn = common::test_db();
    let now = common::now();
    seed_indexed_feed(
        &conn,
        "replica-stored-url-guid",
        "https://example.com/replica-stored-url.xml",
        now,
    );

    let resolution = stophammer::db::resolve_listed_feed(
        &conn,
        "replica-listed-guid-not-indexed",
        Some("https://example.com/replica-stored-url.xml"),
    )
    .expect("resolve_listed_feed must not error");

    assert_eq!(
        resolution,
        ListedFeedResolution::FeedUrl {
            feed_guid: "replica-stored-url-guid".to_string(),
            observed_at: now,
        },
        "a stored feed_url with no observation must resolve by feed_url, \
         with observed_at equal to the feed's created_at"
    );
}

#[test]
fn resolve_listed_feed_observation_wins_over_a_different_feeds_stored_url() {
    let conn = common::test_db();
    let now = common::now();
    // X: an indexed feed whose own stored feed_url is the listed url.
    // Y: a different indexed feed, at its own (different) stored url.
    // feeds.feed_url is UNIQUE, so X and Y cannot share a stored url. Each
    // gets its own artist display name, since artist_credit.display_name is
    // also unique per normalized value.
    insert_artist(&conn, "replica-feed-x-artist", "Seed Artist X", now);
    let credit_x = insert_artist_credit(&conn, "replica-feed-x-artist", "Seed Artist X", now);
    insert_feed(
        &conn,
        "replica-feed-x",
        "https://example.com/replica-x-own-url.xml",
        "Seed Feed X",
        credit_x,
        now,
    );
    insert_artist(&conn, "replica-feed-y-artist", "Seed Artist Y", now);
    let credit_y = insert_artist_credit(&conn, "replica-feed-y-artist", "Seed Artist Y", now);
    insert_feed(
        &conn,
        "replica-feed-y",
        "https://example.com/replica-y-own-url.xml",
        "Seed Feed Y",
        credit_y,
        now,
    );
    // An observation of X's stored url names Y instead of X, inserted
    // directly since this row need not come from an ingest.
    let later = now + 100;
    stophammer::db::record_feed_url_observation(
        &conn,
        "https://example.com/replica-x-own-url.xml",
        "replica-feed-y",
        later,
    )
    .expect("record observation");

    let resolution = stophammer::db::resolve_listed_feed(
        &conn,
        "replica-listed-guid-not-indexed",
        Some("https://example.com/replica-x-own-url.xml"),
    )
    .expect("resolve_listed_feed must not error");

    assert_eq!(
        resolution,
        ListedFeedResolution::FeedUrl {
            feed_guid: "replica-feed-y".to_string(),
            observed_at: later,
        },
        "an observation naming Y must win over X's own stored feed_url at \
         the same listed url, even though X is indexed too"
    );
}

// ---------------------------------------------------------------------------
// Section 2: the replica test.
// ---------------------------------------------------------------------------

fn app_state_with_chain(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
    signer_label: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer(signer_label));
    let pubkey = signer.pubkey_hex().to_string();

    let names: Vec<String> = stophammer::verify::ChainSpec::DEFAULT
        .split(',')
        .map(ToString::to_string)
        .collect();
    let spec = stophammer::verify::ChainSpec { names };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());

    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
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

fn ingest_payload(
    canonical_url: &str,
    source_url: &str,
    crawl_token: &str,
    content_hash: &str,
    feed_data: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": canonical_url,
        "source_url": source_url,
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": content_hash,
        "feed_data": feed_data,
    })
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

async fn ingest_response(app: axum::Router, payload: &serde_json::Value) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(payload).expect("serialize")))
                .expect("build request"),
        )
        .await
        .expect("send request");
    body_json(resp).await
}

async fn ingest(app: axum::Router, payload: &serde_json::Value) {
    let body = ingest_response(app, payload).await;
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
}

async fn get_feed_with_publisher(app: axum::Router, feed_guid: &str) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/feeds/{feed_guid}?include=publisher"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert_eq!(resp.status(), 200, "feed read must succeed");
    body_json(resp).await
}

async fn get_feed(app: axum::Router, feed_guid: &str) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/feeds/{feed_guid}"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert_eq!(resp.status(), 200, "feed read must succeed");
    body_json(resp).await
}

async fn get_publisher_link_stats(app: axum::Router) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/publisher-links/stats")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert_eq!(resp.status(), 200, "stats read must succeed");
    body_json(resp).await
}

const DETOX_ARTIST_URL: &str =
    "https://wavlake.com/feed/artist/137aaa9c-75ff-4916-9f23-e02968b2d15e";
const DETOX_ARTIST_GUID: &str = "137aaa9c-75ff-4916-9f23-e02968b2d15e";
const DETOX_ALBUM_GUID: &str = "e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9";

/// Reads the `feedUrl` that `detox-artist` lists for the DETOX album, at
/// position 10 of its remote items (`tests/fixtures/adr0049/SOURCES.md`
/// names this as the album's real fetch URL).
fn detox_album_listed_url(artist_feed_data: &serde_json::Value) -> String {
    artist_feed_data["remote_items"][10]["remote_feed_url"]
        .as_str()
        .expect("detox-artist fixture must list a remote item at position 10")
        .to_string()
}

/// Ingests `detox-artist` then `detox-album` into database `db`, with the
/// album's `canonical_url` and `source_url` both equal to the `feedUrl` that
/// the artist feed lists for it. Returns that shared URL.
async fn ingest_detox_fixtures(db: Arc<Mutex<rusqlite::Connection>>, crawl_token: &str) -> String {
    let state = app_state_with_chain(Arc::clone(&db), crawl_token, "test-adr0049-replica-signer");
    let app = stophammer::api::build_router(state);

    let artist_feed_data = common::adr0049_feed_data("detox-artist");
    let listed_url = detox_album_listed_url(&artist_feed_data);
    ingest(
        app.clone(),
        &ingest_payload(
            DETOX_ARTIST_URL,
            DETOX_ARTIST_URL,
            crawl_token,
            "replica-detox-artist-hash",
            &artist_feed_data,
        ),
    )
    .await;

    // The album's canonical_url and source_url are both the exact url the
    // artist feed lists, so the album's stored feeds.feed_url equals that
    // listed url exactly (ADR 0049 task 004b, step 3).
    let album_feed_data = common::adr0049_feed_data("detox-album");
    ingest(
        app,
        &ingest_payload(
            &listed_url,
            &listed_url,
            crawl_token,
            "replica-detox-album-hash",
            &album_feed_data,
        ),
    )
    .await;

    listed_url
}

#[tokio::test]
async fn replica_without_url_observations_resolves_the_same_as_the_primary() {
    let crawl_token_a = "adr0049-replica-a-token";
    let db_a = common::test_db_arc();
    ingest_detox_fixtures(Arc::clone(&db_a), crawl_token_a).await;

    // Database B applies every event of A except each FeedUrlObserved
    // event, so B never records an observation — the state of a new
    // community node for a feed ingested before task 004.
    let events: Vec<stophammer::event::Event> = {
        let conn = db_a.lock().expect("lock db_a");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events from A")
    };
    assert!(
        events
            .iter()
            .any(|ev| matches!(ev.event_type, stophammer::event::EventType::FeedUrlObserved)),
        "the ingest sequence must emit at least one FeedUrlObserved event to \
         exclude, or this test proves nothing"
    );

    let db_b = common::test_db_arc();
    let pool_b = common::wrap_pool(Arc::clone(&db_b));
    for ev in &events {
        if matches!(ev.event_type, stophammer::event::EventType::FeedUrlObserved) {
            continue;
        }
        let result = stophammer::apply::apply_single_event(&pool_b, ev);
        assert!(
            result.is_ok(),
            "applying event {:?} on the replica must succeed: {result:?}",
            ev.event_type
        );
    }

    let observation_count: i64 = {
        let conn = db_b.lock().expect("lock db_b");
        conn.query_row("SELECT COUNT(*) FROM feed_url_observations", [], |r| {
            r.get(0)
        })
        .expect("count observations")
    };
    assert_eq!(
        observation_count, 0,
        "database B must hold no feed_url_observations row, the same state \
         a new community node has for a feed ingested before task 004"
    );

    let reader_state_a = app_state_with_chain(
        Arc::clone(&db_a),
        crawl_token_a,
        "test-adr0049-replica-a-reader",
    );
    let app_a = stophammer::api::build_router(reader_state_a);
    let reader_state_b = app_state_with_chain(
        Arc::clone(&db_b),
        "unused-b-token",
        "test-adr0049-replica-b-reader",
    );
    let app_b = stophammer::api::build_router(reader_state_b);

    // 1. The publisher view of the album.
    let album_a = get_feed_with_publisher(app_a.clone(), DETOX_ALBUM_GUID).await;
    let album_b = get_feed_with_publisher(app_b.clone(), DETOX_ALBUM_GUID).await;
    assert_eq!(
        album_a["data"]["publisher"], album_b["data"]["publisher"],
        "the album's publisher view must agree between A and B: A={:?} B={:?}",
        album_a["data"]["publisher"], album_b["data"]["publisher"]
    );
    let album_rows = album_a["data"]["publisher"]
        .as_array()
        .expect("publisher must be an array");
    assert_eq!(
        album_rows.len(),
        1,
        "the album names exactly one publisher: {album_rows:?}"
    );
    assert_eq!(
        album_rows[0]["publisher_link_resolution"],
        serde_json::json!("feed_url"),
        "the album row must resolve by feed_url on both databases: {album_rows:?}"
    );

    // 2. The publisher view of the artist feed.
    let artist_a = get_feed_with_publisher(app_a.clone(), DETOX_ARTIST_GUID).await;
    let artist_b = get_feed_with_publisher(app_b.clone(), DETOX_ARTIST_GUID).await;
    assert_eq!(
        artist_a["data"]["publisher"], artist_b["data"]["publisher"],
        "the artist feed's publisher view must agree between A and B"
    );

    // 3. The artist count of the artist feed.
    let artist_count_a = get_feed(app_a.clone(), DETOX_ARTIST_GUID).await;
    let artist_count_b = get_feed(app_b.clone(), DETOX_ARTIST_GUID).await;
    assert_eq!(
        artist_count_a["data"]["distinct_release_artist_count"],
        artist_count_b["data"]["distinct_release_artist_count"],
        "the derived artist count must agree between A and B"
    );
    assert_eq!(
        artist_count_a["data"]["distinct_release_artists"],
        artist_count_b["data"]["distinct_release_artists"],
        "the derived artist list must agree between A and B"
    );

    // 4. The statistics counts.
    let stats_a = get_publisher_link_stats(app_a).await;
    let stats_b = get_publisher_link_stats(app_b).await;
    assert_eq!(
        stats_a["data"], stats_b["data"],
        "the publisher-link statistics must agree between A and B: A={:?} B={:?}",
        stats_a["data"], stats_b["data"]
    );
}
