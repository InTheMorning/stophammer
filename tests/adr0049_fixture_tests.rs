// ADR 0049 Task 002: real-feed fixtures and the ingest probe.
//
// These tests load the stored parser output for real publisher and album
// feeds (see `tests/fixtures/adr0049/SOURCES.md` for each source) and prove
// the facts that ADR 0049 §9 names. No test in this file touches the
// network — every fixture was fetched once, ahead of time, and the parser
// JSON was made with `stophammer-parse` and stored on disk.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

/// Every fixture name the task packet names in "The Feeds".
const FIXTURE_NAMES: &[&str] = &[
    "detox-artist",
    "detox-album",
    "rssblue-publisher",
    "rssblue-album",
    "sirlibre-label",
    "sirlibre-album",
    "jimmyv-publisher",
    "jimmyv-produced-album",
    "no-publisher-album",
];

fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/adr0049")
}

/// Returns the `remote_items` array of a fixture's parser JSON.
fn remote_items(feed: &serde_json::Value) -> &Vec<serde_json::Value> {
    feed["remote_items"]
        .as_array()
        .expect("remote_items must be a JSON array")
}

#[test]
fn each_fixture_has_an_xml_file_and_a_feed_data_file() {
    for name in FIXTURE_NAMES {
        let dir = fixture_dir();
        let xml_path = dir.join(format!("{name}.xml"));
        let json_path = dir.join(format!("{name}.feed_data.json"));

        assert!(
            xml_path.is_file(),
            "fixture {name} must have an xml file at {}",
            xml_path.display()
        );
        assert!(
            json_path.is_file(),
            "fixture {name} must have a feed_data.json file at {}",
            json_path.display()
        );

        let xml_len = std::fs::metadata(&xml_path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", xml_path.display()))
            .len();
        let json_len = std::fs::metadata(&json_path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", json_path.display()))
            .len();

        assert!(xml_len > 0, "fixture {name}.xml must not be empty");
        assert!(
            json_len > 0,
            "fixture {name}.feed_data.json must not be empty"
        );
    }
}

#[test]
fn detox_artist_is_a_publisher_feed_with_a_music_remote_item() {
    let feed = common::adr0049_feed_data("detox-artist");

    assert_eq!(
        feed["raw_medium"].as_str(),
        Some("publisher"),
        "detox-artist must report raw_medium == publisher, got {:?}",
        feed["raw_medium"]
    );

    let has_music_child = remote_items(&feed)
        .iter()
        .any(|item| item["medium"].as_str() == Some("music"));
    assert!(
        has_music_child,
        "detox-artist must list at least one remote item with medium == music"
    );
}

#[test]
fn detox_album_names_its_guid_and_its_publisher() {
    let feed = common::adr0049_feed_data("detox-album");

    assert_eq!(
        feed["feed_guid"].as_str(),
        Some("e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9"),
        "detox-album must report the DETOX album podcast:guid, got {:?}",
        feed["feed_guid"]
    );

    let names_the_publisher = remote_items(&feed).iter().any(|item| {
        item["medium"].as_str() == Some("publisher")
            && item["remote_feed_guid"].as_str() == Some("137aaa9c-75ff-4916-9f23-e02968b2d15e")
    });
    assert!(
        names_the_publisher,
        "detox-album must list a remote item with medium == publisher and \
         remote_feed_guid == 137aaa9c-75ff-4916-9f23-e02968b2d15e, got {:?}",
        remote_items(&feed)
    );
}

#[test]
fn sirlibre_label_lists_a_remote_item_with_rel_label() {
    let feed = common::adr0049_feed_data("sirlibre-label");

    let has_label_rel = remote_items(&feed)
        .iter()
        .any(|item| item["rel"].as_str() == Some("label"));
    assert!(
        has_label_rel,
        "sirlibre-label must list at least one remote item with rel == label, got {:?}",
        remote_items(&feed)
    );
}

#[test]
fn jimmyv_publisher_lists_a_remote_item_with_rel_producer() {
    let feed = common::adr0049_feed_data("jimmyv-publisher");

    let has_producer_rel = remote_items(&feed)
        .iter()
        .any(|item| item["rel"].as_str() == Some("producer"));
    assert!(
        has_producer_rel,
        "jimmyv-publisher must list at least one remote item with rel == producer, got {:?}",
        remote_items(&feed)
    );
}

#[test]
fn no_publisher_album_names_no_publisher() {
    let feed = common::adr0049_feed_data("no-publisher-album");

    let names_a_publisher = remote_items(&feed)
        .iter()
        .any(|item| item["medium"].as_str() == Some("publisher"));
    assert!(
        !names_a_publisher,
        "no-publisher-album must list no remote item with medium == publisher, got {:?}",
        remote_items(&feed)
    );
}

// ── The probe ────────────────────────────────────────────────────────────

/// Builds an `AppState` running the full default verifier chain
/// (`stophammer::verify::ChainSpec::DEFAULT`), the same chain a production
/// primary node runs when `VERIFIER_CHAIN` is unset.
fn test_app_state_with_default_chain(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0049-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let names: Vec<String> = stophammer::verify::ChainSpec::DEFAULT
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse ingest response json")
}

/// The probe: the default verifier chain must accept the Wavlake artist
/// (publisher) feed `detox-artist`. No Wavlake publisher feed was indexed
/// before this task, so this was not known ahead of time.
#[tokio::test]
async fn default_chain_accepts_the_wavlake_artist_feed() {
    let crawl_token = "adr0049-probe-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_default_chain(Arc::clone(&db), crawl_token);

    let feed_data = common::adr0049_feed_data("detox-artist");
    let payload = serde_json::json!({
        "canonical_url": "https://wavlake.com/feed/artist/137aaa9c-75ff-4916-9f23-e02968b2d15e",
        "source_url": "https://wavlake.com/feed/artist/137aaa9c-75ff-4916-9f23-e02968b2d15e",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "adr0049-detox-artist-fixture-hash",
        "feed_data": feed_data,
    });

    let body = ingest_response(stophammer::api::build_router(state), &payload).await;

    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "the default verifier chain must accept detox-artist; reason: {:?}, warnings: {:?}",
        body["reason"],
        body["warnings"]
    );
}
