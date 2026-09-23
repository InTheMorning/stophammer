// ADR 0049 task 009: `GET /v1/publisher-links/stats` and
// `db::get_publisher_link_stats`.
//
// Section 1 proves the counting function directly against the database, with
// feeds and remote items inserted by raw SQL. It proves the empty case, the
// join filter that keeps a music-feed item out of the count, and that the
// `raw_medium` comparison is case-insensitive.
//
// Section 2 proves the HTTP route through real-feed ingest, using the ADR
// 0049 task 002 fixtures (`tests/fixtures/adr0049/`; see `SOURCES.md`). The
// helper functions here are copied from `tests/adr0049_resolver_tests.rs`,
// per the task packet, rather than importing from that file.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use rusqlite::params;
use tower::ServiceExt;

use stophammer::db::PublisherLinkStats;

// ---------------------------------------------------------------------------
// Section 1: db::get_publisher_link_stats, direct against the database.
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

/// Inserts a minimal indexed feed, with its own artist and artist-credit row,
/// and an explicit `raw_medium` (unlike the resolver test's `insert_feed`,
/// which leaves it null).
fn seed_feed(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    feed_url: &str,
    raw_medium: Option<&str>,
    now: i64,
) {
    let artist_id = format!("{feed_guid}-artist");
    insert_artist(conn, &artist_id, "Seed Artist", now);
    let credit_id = insert_artist_credit(conn, &artist_id, "Seed Artist", now);
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         explicit, episode_count, created_at, updated_at, raw_medium) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            feed_guid,
            feed_url,
            "Seed Feed",
            "seed feed",
            credit_id,
            0,
            0,
            now,
            now,
            raw_medium
        ],
    )
    .expect("insert feed");
}

fn insert_remote_item(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    position: i64,
    medium: &str,
    remote_feed_guid: &str,
    remote_feed_url: Option<&str>,
) {
    conn.execute(
        "INSERT INTO feed_remote_items_raw \
         (feed_guid, position, medium, remote_feed_guid, remote_feed_url) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            feed_guid,
            position,
            medium,
            remote_feed_guid,
            remote_feed_url
        ],
    )
    .expect("insert remote item");
}

#[test]
fn empty_database_gives_four_zeros() {
    let conn = common::test_db();

    let stats = stophammer::db::get_publisher_link_stats(&conn).expect("stats must not error");

    assert_eq!(
        stats,
        PublisherLinkStats {
            listed_links: 0,
            resolved_by_guid: 0,
            resolved_by_feed_url: 0,
            unresolved: 0,
        },
        "an empty database must give four zeros"
    );
}

#[test]
fn a_medium_music_item_on_a_music_feed_is_not_counted() {
    let conn = common::test_db();
    let now = common::now();
    seed_feed(
        &conn,
        "link-stats-music-feed",
        "https://example.com/link-stats-music-feed.xml",
        Some("music"),
        now,
    );
    // A medium="music" remote item on a feed whose own raw_medium is
    // "music", not "publisher". The publisher view would not treat this
    // feed as a publisher either, so the stats route must not count it.
    insert_remote_item(
        &conn,
        "link-stats-music-feed",
        0,
        "music",
        "link-stats-not-a-publisher-link",
        None,
    );

    let stats = stophammer::db::get_publisher_link_stats(&conn).expect("stats must not error");

    assert_eq!(
        stats.listed_links, 0,
        "a medium=music item on a non-publisher feed must not count as a \
         listed link"
    );
}

#[test]
fn publisher_raw_medium_is_matched_case_insensitively() {
    let conn = common::test_db();
    let now = common::now();
    // "Publisher", not "publisher". db::medium::is_publisher compares
    // case-insensitively, and the task requires the same comparison here.
    seed_feed(
        &conn,
        "link-stats-mixed-case-publisher",
        "https://example.com/link-stats-mixed-case.xml",
        Some("Publisher"),
        now,
    );
    insert_remote_item(
        &conn,
        "link-stats-mixed-case-publisher",
        0,
        "music",
        "link-stats-mixed-case-unresolved",
        None,
    );

    let stats = stophammer::db::get_publisher_link_stats(&conn).expect("stats must not error");

    assert_eq!(
        stats.listed_links, 1,
        "a raw_medium of \"Publisher\" must still count as a publisher feed"
    );
    assert_eq!(stats.unresolved, 1);
}

// ---------------------------------------------------------------------------
// Section 2: the HTTP route, through real-feed ingest.
// ---------------------------------------------------------------------------

fn app_state_with_chain(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
    names: &[&str],
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0049-link-stats-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: names.iter().map(ToString::to_string).collect(),
    };
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

/// The full default verifier chain. Every fixture ingest here carries a
/// feed-level `podcast:value` block, so this chain accepts each of them, the
/// same as `tests/adr0049_resolver_tests.rs`.
fn default_chain_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let names: Vec<&str> = stophammer::verify::ChainSpec::DEFAULT.split(',').collect();
    app_state_with_chain(db, crawl_token, &names)
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

/// Sends `GET /v1/publisher-links/stats` with no headers at all, so this
/// also proves the route needs no credential whenever it is called.
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
    assert_eq!(
        resp.status(),
        200,
        "the route must need no credential and must succeed"
    );
    body_json(resp).await
}

const DETOX_ARTIST_URL: &str =
    "https://wavlake.com/feed/artist/137aaa9c-75ff-4916-9f23-e02968b2d15e";
/// The `feedUrl` that `detox-artist` lists for the DETOX album, at position
/// 10 of its remote items.
const DETOX_ALBUM_LISTED_URL: &str =
    "https://wavlake.com/feed/music/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a";
/// A different URL for the same `podcast:guid`, one path segment shorter.
/// `detox-album` is ingested at this URL as its canonical URL, so its stored
/// `feed_url` differs from `DETOX_ALBUM_LISTED_URL`.
const DETOX_ALBUM_OTHER_URL: &str = "https://wavlake.com/feed/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a";

const RSSBLUE_PUBLISHER_URL: &str = "https://publishers.rssblue.com/acoldtripnowhere";
const RSSBLUE_ALBUM_URL: &str = "https://feeds.rssblue.com/e-pluribus-unum";

#[tokio::test]
async fn the_route_needs_no_credential() {
    let crawl_token = "adr0049-link-stats-no-cred-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let body = get_publisher_link_stats(app).await;

    assert_eq!(
        body["data"],
        serde_json::json!({
            "listed_links": 0,
            "resolved_by_guid": 0,
            "resolved_by_feed_url": 0,
            "unresolved": 0
        }),
        "an empty database read with no credential must give four zeros"
    );
}

#[tokio::test]
async fn fixture_counts_agree_with_the_ingested_rows() {
    let crawl_token = "adr0049-link-stats-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    // detox-artist is a publisher feed with 30 medium="music" remote items
    // (tests/fixtures/adr0049/detox-artist.feed_data.json). Each one names
    // a Wavlake URL identifier as its feedGuid, never the album's real
    // podcast:guid (ADR 0049 context), so none of the 30 can resolve by
    // guid in this test.
    let artist_feed_data = common::adr0049_feed_data("detox-artist");
    ingest(
        app.clone(),
        &ingest_payload(
            DETOX_ARTIST_URL,
            DETOX_ARTIST_URL,
            crawl_token,
            "link-stats-detox-artist-hash",
            &artist_feed_data,
        ),
    )
    .await;

    // detox-album is ingested at DETOX_ALBUM_OTHER_URL (its canonical URL),
    // with source_url DETOX_ALBUM_LISTED_URL: the exact URL that
    // detox-artist lists at position 10. ADR 0049 decision 1 records an
    // observation for source_url too, so the position-10 link resolves by
    // feed_url. The other 29 detox-artist links name Wavlake feeds this
    // test never ingests, so they stay unresolved.
    let album_feed_data = common::adr0049_feed_data("detox-album");
    ingest(
        app.clone(),
        &ingest_payload(
            DETOX_ALBUM_OTHER_URL,
            DETOX_ALBUM_LISTED_URL,
            crawl_token,
            "link-stats-detox-album-hash",
            &album_feed_data,
        ),
    )
    .await;

    // rssblue-publisher lists two medium="music" items (SOURCES.md). The
    // first names the album's real podcast:guid, so it resolves by guid
    // once rssblue-album is ingested. The second names a Wavlake feed this
    // test never ingests, so it stays unresolved.
    let rssblue_publisher_data = common::adr0049_feed_data("rssblue-publisher");
    ingest(
        app.clone(),
        &ingest_payload(
            RSSBLUE_PUBLISHER_URL,
            RSSBLUE_PUBLISHER_URL,
            crawl_token,
            "link-stats-rssblue-publisher-hash",
            &rssblue_publisher_data,
        ),
    )
    .await;

    let rssblue_album_data = common::adr0049_feed_data("rssblue-album");
    ingest(
        app.clone(),
        &ingest_payload(
            RSSBLUE_ALBUM_URL,
            RSSBLUE_ALBUM_URL,
            crawl_token,
            "link-stats-rssblue-album-hash",
            &rssblue_album_data,
        ),
    )
    .await;

    let body = get_publisher_link_stats(app).await;

    // detox-artist contributes 30 listed links: 1 resolves by feed_url
    // (position 10), 29 stay unresolved, 0 resolve by guid.
    // rssblue-publisher contributes 2 listed links: 1 resolves by guid
    // (position 0, the album's real podcast:guid), 1 stays unresolved
    // (position 1, an un-ingested Wavlake feed), 0 resolve by feed_url.
    // Totals: listed_links = 30 + 2 = 32; resolved_by_guid = 0 + 1 = 1;
    // resolved_by_feed_url = 1 + 0 = 1; unresolved = 29 + 1 = 30.
    assert_eq!(
        body["data"],
        serde_json::json!({
            "listed_links": 32,
            "resolved_by_guid": 1,
            "resolved_by_feed_url": 1,
            "unresolved": 30
        }),
        "the counts must agree with the rows the fixtures ingest: {body:?}"
    );
}
