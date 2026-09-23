// ADR 0049 task 005: db::resolve_listed_feed, and the resolver-backed
// relationship fields on the feed and track `publisher` views.
//
// Section 1 proves the resolver's three branches directly against the
// database. No HTTP ingest is involved there.
//
// Section 2 proves the publisher view through HTTP ingest and read. It uses
// the ADR 0049 task 002 real-feed fixtures (`tests/fixtures/adr0049/`, see
// `SOURCES.md` for each source) with the default verifier chain, because
// every fixture used here carries the feed-level `podcast:value` block that
// chain needs. The "listed by" test uses inline payloads and a short chain
// (`crawl_token, content_hash, medium_music`) because its synthetic feeds
// carry no payment routes at all.
//
// Section 3 is the guard: only `db::resolve_listed_feed` may compare a
// listed `feedGuid` with an indexed `feed_guid` directly.

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
// Section 1: db::resolve_listed_feed, direct against the database.
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
fn resolve_listed_feed_by_guid_when_indexed() {
    let conn = common::test_db();
    let now = common::now();
    seed_indexed_feed(
        &conn,
        "resolver-guid-a",
        "https://example.com/resolver-guid-a.xml",
        now,
    );

    let resolution = stophammer::db::resolve_listed_feed(&conn, "resolver-guid-a", None)
        .expect("resolve_listed_feed must not error");

    assert_eq!(
        resolution,
        ListedFeedResolution::Guid {
            feed_guid: "resolver-guid-a".to_string()
        },
        "an indexed feed_guid must resolve by guid, regardless of the listed url"
    );
}

#[test]
fn resolve_listed_feed_by_feed_url_when_guid_not_indexed() {
    let conn = common::test_db();
    let now = common::now();
    seed_indexed_feed(
        &conn,
        "resolver-real-guid",
        "https://example.com/resolver-real.xml",
        now,
    );

    let listed_url = "https://example.com/listed-at-a-different-url.xml";
    stophammer::db::record_feed_url_observation(&conn, listed_url, "resolver-real-guid", now)
        .expect("record observation");

    let resolution =
        stophammer::db::resolve_listed_feed(&conn, "resolver-wrong-guid", Some(listed_url))
            .expect("resolve_listed_feed must not error");

    assert_eq!(
        resolution,
        ListedFeedResolution::FeedUrl {
            feed_guid: "resolver-real-guid".to_string(),
            observed_at: now,
        },
        "an observation for the listed url that names an indexed guid must \
         resolve by feed_url, even though the listed guid itself is not indexed"
    );
}

#[test]
fn resolve_listed_feed_unresolved_when_nothing_matches() {
    let conn = common::test_db();

    let resolution = stophammer::db::resolve_listed_feed(
        &conn,
        "resolver-guid-that-does-not-exist",
        Some("https://example.com/never-observed.xml"),
    )
    .expect("resolve_listed_feed must not error");

    assert_eq!(
        resolution,
        ListedFeedResolution::Unresolved,
        "no feed_guid match and no url observation must give Unresolved"
    );
}

#[test]
fn resolve_listed_feed_unresolved_when_observation_names_a_guid_not_indexed() {
    let conn = common::test_db();
    let now = common::now();
    let listed_url = "https://example.com/observed-but-ghost.xml";
    stophammer::db::record_feed_url_observation(&conn, listed_url, "ghost-guid-never-indexed", now)
        .expect("record observation");

    let resolution = stophammer::db::resolve_listed_feed(
        &conn,
        "resolver-guid-not-indexed-either",
        Some(listed_url),
    )
    .expect("resolve_listed_feed must not error");

    assert_eq!(
        resolution,
        ListedFeedResolution::Unresolved,
        "an observation that names a guid feeds does not hold must give \
         Unresolved, not FeedUrl"
    );
}

// ---------------------------------------------------------------------------
// Section 2: the publisher view, through HTTP ingest and read.
// ---------------------------------------------------------------------------

fn app_state_with_chain(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
    names: &[&str],
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0049-resolver-signer"));
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

/// The full default verifier chain, the same one a production primary node
/// runs when `VERIFIER_CHAIN` is unset. Every fixture ingest in this file
/// carries a feed-level `podcast:value` block, so this chain accepts each of
/// them (the album fixtures are refused by `v4v_payment` only when a feed has
/// no feed-level block at all; none of these do).
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

const DETOX_ARTIST_URL: &str =
    "https://wavlake.com/feed/artist/137aaa9c-75ff-4916-9f23-e02968b2d15e";
const DETOX_ALBUM_GUID: &str = "e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9";
/// The `feedUrl` that `detox-artist` lists for the DETOX album, at position
/// 10 of its remote items.
const DETOX_ALBUM_LISTED_URL: &str =
    "https://wavlake.com/feed/music/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a";
/// The task's named "other URL" for the same `podcast:guid`, one path
/// segment shorter, without the `music/` term.
const DETOX_ALBUM_OTHER_URL: &str = "https://wavlake.com/feed/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a";

#[tokio::test]
async fn detox_album_two_way_validates_through_a_feed_url_resolution() {
    let crawl_token = "adr0049-resolver-detox-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let artist_feed_data = common::adr0049_feed_data("detox-artist");
    ingest(
        app.clone(),
        &ingest_payload(
            DETOX_ARTIST_URL,
            DETOX_ARTIST_URL,
            crawl_token,
            "detox-artist-hash",
            &artist_feed_data,
        ),
    )
    .await;

    let album_feed_data = common::adr0049_feed_data("detox-album");
    // The canonical form differs from the exact url the artist feed lists,
    // but the source_url is that exact listed url.
    ingest(
        app.clone(),
        &ingest_payload(
            DETOX_ALBUM_OTHER_URL,
            DETOX_ALBUM_LISTED_URL,
            crawl_token,
            "detox-album-hash",
            &album_feed_data,
        ),
    )
    .await;

    let body = get_feed_with_publisher(app, DETOX_ALBUM_GUID).await;
    let rows = body["data"]["publisher"]
        .as_array()
        .expect("publisher must be an array");
    assert_eq!(
        rows.len(),
        1,
        "the album names exactly one publisher: {rows:?}"
    );
    let row = &rows[0];

    assert_eq!(row["music_names_publisher"], serde_json::json!(true));
    assert_eq!(row["publisher_lists_music"], serde_json::json!(true));
    assert_eq!(
        row["publisher_link_resolution"],
        serde_json::json!("feed_url")
    );
    assert_eq!(row["two_way_validated"], serde_json::json!(true));
}

#[tokio::test]
async fn detox_album_alone_at_the_other_url_gives_unresolved() {
    let crawl_token = "adr0049-resolver-detox-lone-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let album_feed_data = common::adr0049_feed_data("detox-album");
    ingest(
        app.clone(),
        &ingest_payload(
            DETOX_ALBUM_OTHER_URL,
            DETOX_ALBUM_OTHER_URL,
            crawl_token,
            "detox-album-lone-hash",
            &album_feed_data,
        ),
    )
    .await;

    let body = get_feed_with_publisher(app, DETOX_ALBUM_GUID).await;
    let rows = body["data"]["publisher"]
        .as_array()
        .expect("publisher must be an array");
    assert_eq!(
        rows.len(),
        1,
        "the album still declares its publisher: {rows:?}"
    );
    let row = &rows[0];

    assert_eq!(
        row["publisher_link_resolution"],
        serde_json::json!("unresolved")
    );
    assert_eq!(row["two_way_validated"], serde_json::json!(false));
}

#[tokio::test]
async fn rssblue_publisher_and_album_resolve_by_guid() {
    let crawl_token = "adr0049-resolver-rssblue-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let publisher_url = "https://publishers.rssblue.com/acoldtripnowhere";
    let album_url = "https://feeds.rssblue.com/e-pluribus-unum";
    let publisher_guid = "844e0b32-7c69-5850-bf62-5abb28b3830b";
    let album_guid = "38edc858-5731-5221-86bf-86db9f886e2b";

    let publisher_feed_data = common::adr0049_feed_data("rssblue-publisher");
    ingest(
        app.clone(),
        &ingest_payload(
            publisher_url,
            publisher_url,
            crawl_token,
            "rssblue-publisher-hash",
            &publisher_feed_data,
        ),
    )
    .await;

    let album_feed_data = common::adr0049_feed_data("rssblue-album");
    ingest(
        app.clone(),
        &ingest_payload(
            album_url,
            album_url,
            crawl_token,
            "rssblue-album-hash",
            &album_feed_data,
        ),
    )
    .await;

    let publisher_body = get_feed_with_publisher(app.clone(), publisher_guid).await;
    let publisher_rows = publisher_body["data"]["publisher"]
        .as_array()
        .expect("publisher must be an array");
    let publisher_row_for_album = publisher_rows
        .iter()
        .find(|row| row["music_feed_guid"] == serde_json::json!(album_guid))
        .unwrap_or_else(|| panic!("no row for the album, got {publisher_rows:?}"));
    assert_eq!(
        publisher_row_for_album["publisher_link_resolution"],
        serde_json::json!("guid")
    );

    let album_body = get_feed_with_publisher(app, album_guid).await;
    let album_rows = album_body["data"]["publisher"]
        .as_array()
        .expect("publisher must be an array");
    assert_eq!(
        album_rows.len(),
        1,
        "the album names exactly one publisher: {album_rows:?}"
    );
    assert_eq!(
        album_rows[0]["publisher_link_resolution"],
        serde_json::json!("guid")
    );
}

#[tokio::test]
async fn jimmyv_publisher_is_refused_with_the_medium_music_reason() {
    let crawl_token = "adr0049-resolver-jimmyv-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_data = common::adr0049_feed_data("jimmyv-publisher");
    let url = "https://music.jimmyv4v.com/publisher/jimmy-v-publisher-rss.xml";
    let body = ingest_response(
        app,
        &ingest_payload(url, url, crawl_token, "jimmyv-publisher-hash", &feed_data),
    )
    .await;

    assert_eq!(
        body["accepted"].as_bool(),
        Some(false),
        "jimmyv-publisher has no remote item with medium == music and must \
         be refused: {body:?}"
    );
    let reason = body["reason"].as_str().unwrap_or_default();
    assert!(
        reason.contains("medium_music"),
        "the refusal reason must name the medium_music verifier, got {reason:?}"
    );
}

#[tokio::test]
async fn no_publisher_album_gives_no_publisher_row() {
    let crawl_token = "adr0049-resolver-no-publisher-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_data = common::adr0049_feed_data("no-publisher-album");
    let feed_guid = feed_data["feed_guid"]
        .as_str()
        .expect("fixture must carry feed_guid")
        .to_string();
    let url = "https://feed.justcast.com/shows/into-the-blue/audioposts.rss";
    ingest(
        app.clone(),
        &ingest_payload(url, url, crawl_token, "no-publisher-album-hash", &feed_data),
    )
    .await;

    let body = get_feed_with_publisher(app, &feed_guid).await;
    let rows = body["data"]["publisher"]
        .as_array()
        .expect("publisher must be an array");
    assert!(
        rows.is_empty(),
        "an album that names no publisher must give no publisher row, got {rows:?}"
    );
}

/// A "listed by" relationship, named after the Jimmy V case: a publisher
/// feed lists an album that names a different publisher. Inline payloads,
/// with a short chain (`crawl_token, content_hash, medium_music`) because
/// neither synthetic feed carries a payment route.
#[tokio::test]
async fn publisher_lists_an_album_that_names_a_different_publisher() {
    let crawl_token = "adr0049-resolver-listed-by-token";
    let db = common::test_db_arc();
    let state = app_state_with_chain(
        Arc::clone(&db),
        crawl_token,
        &["crawl_token", "content_hash", "medium_music"],
    );
    let app = stophammer::api::build_router(state);

    let p_guid = "listed-by-publisher-p";
    let q_guid = "listed-by-publisher-q";
    let a_guid = "listed-by-album-a";

    let p_payload = serde_json::json!({
        "canonical_url": "https://example.com/listed-by/p.xml",
        "source_url": "https://example.com/listed-by/p.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "listed-by-p-hash",
        "feed_data": {
            "feed_guid": p_guid,
            "title": "Publisher P",
            "raw_medium": "publisher",
            "explicit": false,
            "remote_items": [{
                "position": 0,
                "medium": "music",
                "remote_feed_guid": a_guid,
                "remote_feed_url": "https://example.com/listed-by/a.xml"
            }]
        }
    });
    ingest(app.clone(), &p_payload).await;

    let a_payload = serde_json::json!({
        "canonical_url": "https://example.com/listed-by/a.xml",
        "source_url": "https://example.com/listed-by/a.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "listed-by-a-hash",
        "feed_data": {
            "feed_guid": a_guid,
            "title": "Album A",
            "raw_medium": "music",
            "explicit": false,
            "remote_items": [{
                "position": 0,
                "medium": "publisher",
                "remote_feed_guid": q_guid,
                "remote_feed_url": "https://example.com/listed-by/q.xml"
            }]
        }
    });
    ingest(app.clone(), &a_payload).await;

    let body = get_feed_with_publisher(app, p_guid).await;
    let rows = body["data"]["publisher"]
        .as_array()
        .expect("publisher must be an array");
    assert_eq!(rows.len(), 1, "P lists exactly one album: {rows:?}");
    let row = &rows[0];

    assert_eq!(row["publisher_lists_music"], serde_json::json!(true));
    assert_eq!(row["music_names_publisher"], serde_json::json!(false));
}

// ---------------------------------------------------------------------------
// Section 3: the guard. Only db::resolve_listed_feed may compare a listed
// feedGuid with an indexed feed_guid.
// ---------------------------------------------------------------------------

#[test]
fn query_rs_holds_no_direct_remote_feed_guid_match() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/query.rs");
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));

    assert!(
        !contents.contains("remote_feed_guid =="),
        "ADR 0049 §3: a shell must not compare a listed feedGuid with an \
         indexed feed_guid directly. Resolve a listed feed with \
         db::resolve_listed_feed."
    );
}
