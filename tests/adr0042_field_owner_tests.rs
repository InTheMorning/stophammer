// ADR 0042: a query response field names the owner of its value.
//
// `image_url` is resolved display artwork on every route that returns a track.
// `track_image_url` and `feed_image_url` each come from one column. A search
// result carries enough to draw a row.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const FEED_ART: &str = "https://img.example.com/adr0042-feed.jpg";
const TRACK_ART: &str = "https://img.example.com/adr0042-track.jpg";

fn state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0042-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec {
        names: vec!["crawl_token".to_string()],
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

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

async fn get(app: axum::Router, uri: &str) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert!(resp.status().is_success(), "{uri} must succeed");
    body_json(resp).await
}

/// One feed with its own artwork, one track that asserts its own and one that
/// does not, plus a third whose artwork equals the feed's.
async fn seed(app: axum::Router, crawl_token: &str, feed_guid: &str) {
    let payload = serde_json::json!({
        "canonical_url": format!("https://example.com/{feed_guid}.xml"),
        "source_url": format!("https://example.com/{feed_guid}.xml"),
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": format!("hash-{feed_guid}"),
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "ADR 0042 Album",
            "raw_medium": "music",
            "explicit": false,
            "image_url": FEED_ART,
            "tracks": [
                {
                    "track_guid": "adr0042-own-art",
                    "title": "Track With Own Art",
                    "image_url": TRACK_ART,
                    "pub_date": 1_700_000_000,
                    "explicit": false,
                    "payment_routes": [], "value_time_splits": []
                },
                {
                    "track_guid": "adr0042-no-art",
                    "title": "Track Without Own Art",
                    "pub_date": 1_700_000_100,
                    "explicit": false,
                    "payment_routes": [], "value_time_splits": []
                },
                {
                    "track_guid": "adr0042-same-art",
                    "title": "Track Asserting The Feed Art",
                    "image_url": FEED_ART,
                    "pub_date": 1_700_000_200,
                    "explicit": false,
                    "payment_routes": [], "value_time_splits": []
                }
            ]
        }
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert!(resp.status().is_success(), "seed ingest must succeed");
}

#[tokio::test]
async fn a_track_without_its_own_artwork_names_the_owner() {
    let token = "adr0042-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);
    let feed_guid = "adr0042-owner";
    seed(
        stophammer::api::build_router(Arc::clone(&st)),
        token,
        feed_guid,
    )
    .await;

    let body = get(
        stophammer::api::build_router(st),
        &format!("/v1/feeds/{feed_guid}/tracks/adr0042-no-art"),
    )
    .await;
    let track = &body["data"];

    assert_eq!(
        track["track_image_url"],
        serde_json::Value::Null,
        "a track with no artwork of its own must report null"
    );
    assert_eq!(
        track["feed_image_url"].as_str(),
        Some(FEED_ART),
        "the feed artwork must be named as the feed's"
    );
    assert_eq!(
        track["image_url"].as_str(),
        Some(FEED_ART),
        "image_url is resolved display artwork and falls back to the feed"
    );
}

#[tokio::test]
async fn matching_artwork_still_reports_both_owners() {
    let token = "adr0042-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);
    let feed_guid = "adr0042-same";
    seed(
        stophammer::api::build_router(Arc::clone(&st)),
        token,
        feed_guid,
    )
    .await;

    let body = get(
        stophammer::api::build_router(st),
        &format!("/v1/feeds/{feed_guid}/tracks/adr0042-same-art"),
    )
    .await;
    let track = &body["data"];

    assert_eq!(
        track["track_image_url"].as_str(),
        Some(FEED_ART),
        "the track asserts this artwork itself, so it must be reported as its own"
    );
    assert_eq!(
        track["feed_image_url"].as_str(),
        Some(FEED_ART),
        "the feed asserts the same artwork"
    );
}

#[tokio::test]
async fn image_url_is_the_same_on_every_route_that_returns_a_track() {
    let token = "adr0042-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);
    let feed_guid = "adr0042-routes";
    seed(
        stophammer::api::build_router(Arc::clone(&st)),
        token,
        feed_guid,
    )
    .await;

    let scoped = get(
        stophammer::api::build_router(Arc::clone(&st)),
        &format!("/v1/feeds/{feed_guid}/tracks/adr0042-no-art"),
    )
    .await;
    let embedded = get(
        stophammer::api::build_router(st),
        &format!("/v1/feeds/{feed_guid}?include=tracks"),
    )
    .await;

    let from_list = embedded["data"]["tracks"]
        .as_array()
        .expect("tracks include")
        .iter()
        .find(|t| t["track_guid"] == "adr0042-no-art")
        .expect("seeded track present")
        .clone();

    assert_eq!(
        from_list["image_url"], scoped["data"]["image_url"],
        "the embedded list and the track detail must agree on image_url"
    );
    assert_eq!(
        from_list["track_image_url"],
        serde_json::Value::Null,
        "the embedded list must also report that the track owns no artwork"
    );
}

#[tokio::test]
async fn a_search_result_carries_enough_to_draw_a_row() {
    let token = "adr0042-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);
    let feed_guid = "adr0042-search";
    seed(
        stophammer::api::build_router(Arc::clone(&st)),
        token,
        feed_guid,
    )
    .await;

    let body = get(
        stophammer::api::build_router(st),
        "/v1/search?q=%22Track+With+Own+Art%22&type=track&limit=5",
    )
    .await;
    let items = body["data"].as_array().expect("search data");
    assert!(!items.is_empty(), "the seeded track must be found");

    let hit = items
        .iter()
        .find(|i| i["entity_id"] == "adr0042-own-art")
        .expect("the seeded track must appear in the results");

    assert_eq!(
        hit["title"].as_str(),
        Some("Track With Own Art"),
        "a search result must carry a title"
    );
    assert_eq!(
        hit["feed_title"].as_str(),
        Some("ADR 0042 Album"),
        "a track result must name its feed"
    );
    assert_eq!(
        hit["track_image_url"].as_str(),
        Some(TRACK_ART),
        "a search result must name the artwork owner"
    );
}
