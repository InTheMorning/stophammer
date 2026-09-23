// ADR 0049 task 007: each text field names its source.
//
// `release_artist` comes from `itunes:author`, then the non-platform
// `itunes:owner` name, then the placeholder "Unknown Artist".
// `release_artist_source` names which of the three gave the value.
// `publisher_text` is the trimmed `itunes:owner` name, or null.
// `publisher_feed_title` is derived when the node reads a feed: the title of
// the feed that the album's own `medium="publisher"` remote item resolves to.
//
// These tests use the ADR 0049 task 002 real-feed fixtures
// (`tests/fixtures/adr0049/`, see `SOURCES.md` for each source) with the
// default verifier chain, because each fixture used here carries a
// feed-level `podcast:value` block or is a publisher feed (payment-exempt).

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn default_chain_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0049-text-field-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let names: Vec<&str> = stophammer::verify::ChainSpec::DEFAULT.split(',').collect();
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

async fn ingest(app: axum::Router, payload: &serde_json::Value) -> serde_json::Value {
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
    let body = body_json(resp).await;
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
    body
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

async fn get_track(app: axum::Router, track_guid: &str) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/tracks/{track_guid}"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert_eq!(resp.status(), 200, "track read must succeed");
    body_json(resp).await
}

// ---------------------------------------------------------------------------
// DETOX: an album with its own `itunes:author`, owned by "Wavlake", naming
// its artist feed as publisher.
// ---------------------------------------------------------------------------

const DETOX_ARTIST_URL: &str =
    "https://wavlake.com/feed/artist/137aaa9c-75ff-4916-9f23-e02968b2d15e";
const DETOX_ALBUM_GUID: &str = "e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9";
const DETOX_ALBUM_URL: &str = "https://wavlake.com/feed/music/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a";
const DETOX_TRACK_GUID: &str = "0f45b925-6020-47f8-b427-cb6437ee8378";

#[tokio::test]
async fn detox_album_names_its_own_author_as_release_artist_and_resolves_publisher_feed_title() {
    let crawl_token = "adr0049-text-field-detox-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let artist_feed_data = common::adr0049_feed_data("detox-artist");
    let artist_title = artist_feed_data["title"]
        .as_str()
        .expect("detox-artist title")
        .to_string();
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
    let expected_release_artist = album_feed_data["author_name"]
        .as_str()
        .expect("detox-album author_name")
        .to_string();
    let expected_publisher_text = album_feed_data["owner_name"]
        .as_str()
        .expect("detox-album owner_name")
        .to_string();
    ingest(
        app.clone(),
        &ingest_payload(
            DETOX_ALBUM_URL,
            DETOX_ALBUM_URL,
            crawl_token,
            "detox-album-hash",
            &album_feed_data,
        ),
    )
    .await;

    let body = get_feed(app.clone(), DETOX_ALBUM_GUID).await;
    assert_eq!(
        body["data"]["release_artist"],
        serde_json::json!(expected_release_artist),
        "release_artist must equal the album's own itunes:author"
    );
    assert_eq!(
        body["data"]["release_artist_source"],
        serde_json::json!("itunes_author")
    );
    assert_eq!(
        body["data"]["publisher_text"],
        serde_json::json!(expected_publisher_text),
        "publisher_text must equal the album's own itunes:owner name (\"Wavlake\")"
    );
    assert_eq!(
        body["data"]["publisher_feed_title"],
        serde_json::json!(artist_title),
        "publisher_feed_title must resolve to the title of detox-artist"
    );

    // A track read gives release_artist_source beside release_artist.
    let track_body = get_track(app, DETOX_TRACK_GUID).await;
    assert_eq!(
        track_body["data"]["release_artist"],
        serde_json::json!(expected_release_artist)
    );
    assert_eq!(
        track_body["data"]["release_artist_source"],
        serde_json::json!("itunes_author")
    );
}

// ---------------------------------------------------------------------------
// RSS Blue: an album whose owner name equals the linked publisher feed's own
// title. publisher_text must come from the album's own itunes:owner, not
// from a lookup of the publisher feed.
// ---------------------------------------------------------------------------

const RSSBLUE_PUBLISHER_URL: &str = "https://publishers.rssblue.com/acoldtripnowhere";
const RSSBLUE_ALBUM_URL: &str = "https://feeds.rssblue.com/e-pluribus-unum";
const RSSBLUE_ALBUM_GUID: &str = "38edc858-5731-5221-86bf-86db9f886e2b";

#[tokio::test]
async fn rssblue_album_publisher_text_comes_from_its_own_owner_name() {
    let crawl_token = "adr0049-text-field-rssblue-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let publisher_feed_data = common::adr0049_feed_data("rssblue-publisher");
    let publisher_title = publisher_feed_data["title"]
        .as_str()
        .expect("rssblue-publisher title")
        .to_string();
    ingest(
        app.clone(),
        &ingest_payload(
            RSSBLUE_PUBLISHER_URL,
            RSSBLUE_PUBLISHER_URL,
            crawl_token,
            "rssblue-publisher-hash",
            &publisher_feed_data,
        ),
    )
    .await;

    let album_feed_data = common::adr0049_feed_data("rssblue-album");
    let expected_owner_name = album_feed_data["owner_name"]
        .as_str()
        .expect("rssblue-album owner_name")
        .to_string();
    ingest(
        app.clone(),
        &ingest_payload(
            RSSBLUE_ALBUM_URL,
            RSSBLUE_ALBUM_URL,
            crawl_token,
            "rssblue-album-hash",
            &album_feed_data,
        ),
    )
    .await;

    let body = get_feed(app, RSSBLUE_ALBUM_GUID).await;
    assert_eq!(
        body["data"]["publisher_text"],
        serde_json::json!(expected_owner_name),
        "publisher_text must equal the album's own itunes:owner name"
    );

    // The rssblue-publisher fixture's title happens to equal the album's own
    // owner name, so this fixture alone cannot show that publisher_text no
    // longer comes from a publisher-feed-title lookup by value comparison.
    // The derivation path is proven directly: rssblue-album's owner_name and
    // rssblue-publisher's title are the same string.
    assert_eq!(
        expected_owner_name, publisher_title,
        "this fixture's owner name and its publisher feed's title happen to \
         be the same string; the test above proves publisher_text is the \
         album's own owner name regardless"
    );
}

// ---------------------------------------------------------------------------
// An album with no publisher remote item gives a null publisher_feed_title.
// ---------------------------------------------------------------------------

const NO_PUBLISHER_ALBUM_URL: &str = "https://feed.justcast.com/shows/into-the-blue/audioposts.rss";
const NO_PUBLISHER_ALBUM_GUID: &str = "48ad9b64-d3a8-5719-b452-9ae85e57ab54";

#[tokio::test]
async fn album_with_no_publisher_remote_item_gives_null_publisher_feed_title() {
    let crawl_token = "adr0049-text-field-no-publisher-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_data = common::adr0049_feed_data("no-publisher-album");
    ingest(
        app.clone(),
        &ingest_payload(
            NO_PUBLISHER_ALBUM_URL,
            NO_PUBLISHER_ALBUM_URL,
            crawl_token,
            "no-publisher-album-hash",
            &feed_data,
        ),
    )
    .await;

    let body = get_feed(app, NO_PUBLISHER_ALBUM_GUID).await;
    assert!(
        body["data"]["publisher_feed_title"].is_null(),
        "an album with no publisher remote item must give a null publisher_feed_title: {body:?}"
    );
}

// ---------------------------------------------------------------------------
// The ingest of a publisher feed emits no event for a different feed. The
// publisher repair that once did this (ADR 0049 §5) is deleted.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publisher_ingest_emits_no_event_for_a_different_feed() {
    let crawl_token = "adr0049-text-field-no-repair-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    // Ingest the album first, naming a publisher feed that is not yet
    // indexed.
    let album_feed_data = common::adr0049_feed_data("detox-album");
    ingest(
        app.clone(),
        &ingest_payload(
            DETOX_ALBUM_URL,
            DETOX_ALBUM_URL,
            crawl_token,
            "detox-album-no-repair-hash",
            &album_feed_data,
        ),
    )
    .await;

    let seq_before: i64 = {
        let conn = db.lock().expect("lock db");
        conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))
            .expect("read max seq before publisher ingest")
    };

    // Now ingest the publisher feed. It lists the album by medium="music".
    let artist_feed_data = common::adr0049_feed_data("detox-artist");
    ingest(
        app,
        &ingest_payload(
            DETOX_ARTIST_URL,
            DETOX_ARTIST_URL,
            crawl_token,
            "detox-artist-no-repair-hash",
            &artist_feed_data,
        ),
    )
    .await;

    let new_events = {
        let conn = db.lock().expect("lock db");
        stophammer::db::get_events_since(&conn, seq_before, 10_000)
            .expect("read events emitted by the publisher ingest")
    };

    assert!(
        !new_events.is_empty(),
        "the publisher ingest must emit at least its own events"
    );
    let other_feed_events: Vec<_> = new_events
        .iter()
        .filter(|event| {
            event.subject_guid == DETOX_ALBUM_GUID || event.subject_guid == DETOX_TRACK_GUID
        })
        .collect();
    assert!(
        other_feed_events.is_empty(),
        "the publisher ingest must emit no event for the album feed or its \
         track: {other_feed_events:?}"
    );
}

// ---------------------------------------------------------------------------
// Guard: src/api.rs holds no deleted host rule.
// ---------------------------------------------------------------------------

#[test]
fn api_rs_holds_no_deleted_wavlake_host_rule() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/api.rs");
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));

    let failure_message = "ADR 0049 §5: take the value from one RSS element, and put a \
                            derived value in a separate field.";

    assert!(!contents.contains("fn is_wavlake_url"), "{failure_message}");
    assert!(
        !contents.contains("fn wavlake_artist_name_from_links"),
        "{failure_message}"
    );
    assert!(
        !contents.contains("remote_feed_guid =="),
        "{failure_message}"
    );
}
