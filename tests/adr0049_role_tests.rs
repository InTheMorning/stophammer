// ADR 0049 task 006: the role fields on the `publisher` view.
//
// `publisher_rel` and `music_rel` carry the raw `rel` of the two items that
// task 005 matched. `role` and `role_source` follow the table in
// `docs/tasks/adr-0049-task-006-role-fields.md` "Constraints". The pure
// function that derives them, and a unit test for each table row, live in
// `src/query.rs` (`query::tests`).
//
// The Sir Libre and DETOX cases use the ADR 0049 task 002 real-feed
// fixtures with the default verifier chain, because each of those feeds
// carries a feed-level `podcast:value` block, or is payment-exempt as a
// publisher feed. The producer and list cases use inline payloads with a
// short chain (`content_hash, medium_music`), because those
// synthetic feeds carry no payment route at all. Helper functions here are
// copied from `tests/adr0049_resolver_tests.rs` rather than shared with it,
// per the task's "Do Not Touch" list.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn app_state_with_chain(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
    names: &[&str],
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0049-role-signer"));
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

/// The full default verifier chain. Every real-feed fixture ingest in this
/// file either carries a feed-level `podcast:value` block, or is
/// payment-exempt as a publisher feed (`medium::payment_exempt`).
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

// ---------------------------------------------------------------------------
// Sir Libre: both sides state "label", so the role comes from the
// publisher side and there is no conflict.
// ---------------------------------------------------------------------------

const SIRLIBRE_LABEL_GUID: &str = "34bf795f-f551-4db1-8877-ad19aa5f89c8";
const SIRLIBRE_LABEL_URL: &str =
    "https://sirlibre.com/publisher/sir-libre-records-publisher-rss.xml";
const SIRLIBRE_ALBUM_GUID: &str = "2960ba9d-be5a-5fe4-b4e0-18b527e2519c";
const SIRLIBRE_ALBUM_URL: &str = "https://cdn.kolomona.com/podcasts/lightning-thrashes/faces-pale/reap-what-you-sow/reap-what-you-sow.xml";

#[tokio::test]
async fn sirlibre_album_row_reports_label_from_both_sides() {
    let crawl_token = "adr0049-role-sirlibre-token";
    let db = common::test_db_arc();
    let state = default_chain_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let label_feed_data = common::adr0049_feed_data("sirlibre-label");
    ingest(
        app.clone(),
        &ingest_payload(
            SIRLIBRE_LABEL_URL,
            SIRLIBRE_LABEL_URL,
            crawl_token,
            "sirlibre-label-hash",
            &label_feed_data,
        ),
    )
    .await;

    let album_feed_data = common::adr0049_feed_data("sirlibre-album");
    ingest(
        app.clone(),
        &ingest_payload(
            SIRLIBRE_ALBUM_URL,
            SIRLIBRE_ALBUM_URL,
            crawl_token,
            "sirlibre-album-hash",
            &album_feed_data,
        ),
    )
    .await;

    let body = get_feed_with_publisher(app, SIRLIBRE_LABEL_GUID).await;
    let rows = body["data"]["publisher"]
        .as_array()
        .expect("publisher must be an array");
    let row = rows
        .iter()
        .find(|row| row["music_feed_guid"] == serde_json::json!(SIRLIBRE_ALBUM_GUID))
        .unwrap_or_else(|| panic!("no row for the album, got {rows:?}"));

    assert_eq!(row["publisher_rel"], serde_json::json!("label"));
    assert_eq!(row["music_rel"], serde_json::json!("label"));
    assert_eq!(row["role"], serde_json::json!("label"));
    assert_eq!(row["role_source"], serde_json::json!("publisher_rel"));
}

// ---------------------------------------------------------------------------
// Producer and list cases: inline payloads with a short chain
// (`content_hash, medium_music`), because neither synthetic
// feed carries a payment route.
// ---------------------------------------------------------------------------

fn publisher_lists_one_album(
    publisher_guid: &str,
    album_guid: &str,
    album_url: &str,
    rel: &str,
) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": publisher_guid,
        "title": "Test Publisher",
        "raw_medium": "publisher",
        "explicit": false,
        "remote_items": [{
            "position": 0,
            "medium": "music",
            "remote_feed_guid": album_guid,
            "remote_feed_url": album_url,
            "rel": rel,
        }]
    })
}

fn album_with_no_publisher_link(album_guid: &str, title: &str) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": album_guid,
        "title": title,
        "raw_medium": "music",
        "explicit": false,
        "remote_items": []
    })
}

#[tokio::test]
async fn publisher_item_with_rel_producer_gives_role_producer() {
    let crawl_token = "adr0049-role-producer-token";
    let db = common::test_db_arc();
    let state = app_state_with_chain(
        Arc::clone(&db),
        crawl_token,
        &["content_hash", "medium_music"],
    );
    let app = stophammer::api::build_router(state);

    let p_guid = "role-producer-publisher-p";
    let a_guid = "role-producer-album-a";
    let a_url = "https://example.com/role-producer/a.xml";

    ingest(
        app.clone(),
        &ingest_payload(
            "https://example.com/role-producer/p.xml",
            "https://example.com/role-producer/p.xml",
            crawl_token,
            "role-producer-p-hash",
            &publisher_lists_one_album(p_guid, a_guid, a_url, "producer"),
        ),
    )
    .await;

    ingest(
        app.clone(),
        &ingest_payload(
            a_url,
            a_url,
            crawl_token,
            "role-producer-a-hash",
            &album_with_no_publisher_link(a_guid, "Role Producer Album"),
        ),
    )
    .await;

    let body = get_feed_with_publisher(app, p_guid).await;
    let rows = body["data"]["publisher"]
        .as_array()
        .expect("publisher must be an array");
    assert_eq!(rows.len(), 1, "P lists exactly one album: {rows:?}");
    let row = &rows[0];

    assert_eq!(row["publisher_rel"], serde_json::json!("producer"));
    assert_eq!(row["music_rel"], serde_json::json!(null));
    assert_eq!(row["role"], serde_json::json!("producer"));
    assert_eq!(row["role_source"], serde_json::json!("publisher_rel"));
}

#[tokio::test]
async fn publisher_item_with_comma_rel_is_one_value() {
    let crawl_token = "adr0049-role-list-token";
    let db = common::test_db_arc();
    let state = app_state_with_chain(
        Arc::clone(&db),
        crawl_token,
        &["content_hash", "medium_music"],
    );
    let app = stophammer::api::build_router(state);

    let p_guid = "role-list-publisher-p";
    let a_guid = "role-list-album-a";
    let a_url = "https://example.com/role-list/a.xml";

    ingest(
        app.clone(),
        &ingest_payload(
            "https://example.com/role-list/p.xml",
            "https://example.com/role-list/p.xml",
            crawl_token,
            "role-list-p-hash",
            &publisher_lists_one_album(p_guid, a_guid, a_url, "Artist, Producer"),
        ),
    )
    .await;

    ingest(
        app.clone(),
        &ingest_payload(
            a_url,
            a_url,
            crawl_token,
            "role-list-a-hash",
            &album_with_no_publisher_link(a_guid, "Role List Album"),
        ),
    )
    .await;

    let body = get_feed_with_publisher(app, p_guid).await;
    let rows = body["data"]["publisher"]
        .as_array()
        .expect("publisher must be an array");
    assert_eq!(rows.len(), 1, "P lists exactly one album: {rows:?}");
    let row = &rows[0];

    assert_eq!(row["publisher_rel"], serde_json::json!("Artist, Producer"));
    assert_eq!(row["role"], serde_json::json!("artist, producer"));
    assert_eq!(row["role_source"], serde_json::json!("publisher_rel"));
}

// ---------------------------------------------------------------------------
// DETOX: neither side states a `rel`, so the row falls back to the default.
// ---------------------------------------------------------------------------

const DETOX_ARTIST_URL: &str =
    "https://wavlake.com/feed/artist/137aaa9c-75ff-4916-9f23-e02968b2d15e";
const DETOX_ALBUM_GUID: &str = "e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9";
/// The `feedUrl` that `detox-artist` lists for the DETOX album, at position
/// 10 of its remote items.
const DETOX_ALBUM_LISTED_URL: &str =
    "https://wavlake.com/feed/music/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a";
/// The other URL form that serves the same `podcast:guid`, one path
/// segment shorter, without the `music/` term.
const DETOX_ALBUM_OTHER_URL: &str = "https://wavlake.com/feed/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a";

#[tokio::test]
async fn detox_row_has_no_rel_on_either_side_and_defaults_to_artist() {
    let crawl_token = "adr0049-role-detox-token";
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
            "detox-artist-role-hash",
            &artist_feed_data,
        ),
    )
    .await;

    let album_feed_data = common::adr0049_feed_data("detox-album");
    ingest(
        app.clone(),
        &ingest_payload(
            DETOX_ALBUM_OTHER_URL,
            DETOX_ALBUM_LISTED_URL,
            crawl_token,
            "detox-album-role-hash",
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

    assert_eq!(row["publisher_rel"], serde_json::json!(null));
    assert_eq!(row["music_rel"], serde_json::json!(null));
    assert_eq!(row["role"], serde_json::json!("artist"));
    assert_eq!(row["role_source"], serde_json::json!("default"));
}
