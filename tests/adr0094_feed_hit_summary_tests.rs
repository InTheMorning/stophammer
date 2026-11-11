//! ADR 0094: the medium, kind and date of each search feed hit.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0094-feed-summary-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0094-feed-summary-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0094-feed-summary-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn get(st: &Arc<stophammer::api::AppState>, uri: &str) -> (u16, Value) {
    let response = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("read response body")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn ok(st: &Arc<stophammer::api::AppState>, uri: &str) -> Value {
    let (status, body) = get(st, uri).await;
    assert_eq!(status, 200, "GET {uri} gave {status}: {body}");
    body
}

async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    title: &str,
    medium: &str,
    tracks: &[(&str, i64)],
) {
    let track_rows: Vec<Value> = tracks
        .iter()
        .map(|(track_title, pub_date)| {
            json!({
                "track_guid": format!("{guid}-track"),
                "title": track_title,
                "pub_date": pub_date,
                "explicit": false,
            })
        })
        .collect();
    let url = format!("https://summary.example/{guid}.xml");
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": title,
            "raw_medium": medium,
            "explicit": false,
            "author_name": "Summary Artist",
            "tracks": track_rows,
        }
    });
    let response = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&payload).expect("serialize payload"),
                ))
                .expect("build request"),
        )
        .await
        .expect("send ingest request");
    assert!(
        response.status().is_success(),
        "ingest of {guid} gave {}",
        response.status()
    );
}

fn recent_row(body: &Value, guid: &str) -> Value {
    body["data"]
        .as_array()
        .expect("recent data array")
        .iter()
        .find(|row| row["feed_guid"] == guid)
        .unwrap_or_else(|| panic!("recent response has no feed {guid}: {body}"))
        .clone()
}

fn search_row(body: &Value, entity_type: &str, guid: &str) -> Value {
    body["data"]
        .as_array()
        .expect("search data array")
        .iter()
        .find(|row| row["entity_type"] == entity_type && row["entity_id"] == guid)
        .unwrap_or_else(|| panic!("search response has no {entity_type} {guid}: {body}"))
        .clone()
}

fn assert_feed_summary_matches_recent(search: &Value, recent: &Value) {
    for (search_key, recent_key) in [
        ("medium", "raw_medium"),
        ("release_kind", "release_kind"),
        ("newest_item_at", "newest_item_at"),
    ] {
        assert!(
            search.get(search_key).is_some(),
            "search row omits {search_key}: {search}"
        );
        assert_eq!(
            search[search_key], recent[recent_key],
            "search row differs for {search_key}"
        );
    }
}

#[tokio::test]
async fn feed_and_publisher_hits_give_their_recent_summary() {
    let db = Arc::new(Mutex::new(common::test_db()));
    let st = state(db);
    ingest(
        &st,
        "summary-music-guid",
        "SummaryFeedNeedle Release",
        "MuSiC",
        &[("SummaryTrackNeedle Track", 1_700_000_123)],
    )
    .await;
    ingest(
        &st,
        "summary-publisher-guid",
        "SummaryPublisherNeedle Label",
        "publisher",
        &[("Publisher Track", 1_700_000_456)],
    )
    .await;
    ingest(
        &st,
        "summary-null-guid",
        "SummaryNullNeedle Release",
        "music",
        &[],
    )
    .await;

    let feed_search = ok(&st, "/v1/search?q=SummaryFeedNeedle&type=feed").await;
    let feed_hit = search_row(&feed_search, "feed", "summary-music-guid");
    let feed_recent = recent_row(
        &ok(&st, "/v1/feeds/recent?medium=music").await,
        "summary-music-guid",
    );
    assert_feed_summary_matches_recent(&feed_hit, &feed_recent);
    assert_eq!(feed_hit["medium"], json!("MuSiC"));

    let publisher_search = ok(&st, "/v1/search?q=SummaryPublisherNeedle&type=publisher").await;
    let publisher_hit = search_row(&publisher_search, "publisher", "summary-publisher-guid");
    let publisher_recent = recent_row(
        &ok(&st, "/v1/feeds/recent?medium=publisher").await,
        "summary-publisher-guid",
    );
    assert_feed_summary_matches_recent(&publisher_hit, &publisher_recent);

    let null_search = ok(&st, "/v1/search?q=SummaryNullNeedle&type=feed").await;
    let null_hit = search_row(&null_search, "feed", "summary-null-guid");
    let null_recent = recent_row(
        &ok(&st, "/v1/feeds/recent?medium=music").await,
        "summary-null-guid",
    );
    assert_feed_summary_matches_recent(&null_hit, &null_recent);
    assert_eq!(null_hit["newest_item_at"], Value::Null);

    let track_search = ok(&st, "/v1/search?q=SummaryTrackNeedle&type=track").await;
    let track_hit = search_row(&track_search, "track", "summary-music-guid-track");
    for key in ["medium", "release_kind", "newest_item_at"] {
        assert!(
            track_hit.get(key).is_none(),
            "track hit gives {key}: {track_hit}"
        );
    }

    let capabilities = ok(&st, "/v1/node/capabilities").await;
    assert!(
        capabilities["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|value| value == "search_feed_summary")
    );
}
