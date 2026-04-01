// Ingest-to-query end-to-end tests.

#![recursion_limit = "256"]

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helper: build AppState with a crawl-token-only verifier chain
// ---------------------------------------------------------------------------

fn test_app_state_with_crawl_token(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-tc05-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    // Build a verifier chain with only crawl_token (skip content_hash, medium_music, etc.)
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

fn json_request(method: &str, uri: &str, body: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(body).expect("serialize")))
        .expect("build request")
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

// ---------------------------------------------------------------------------
// TC-05: Full ingest-to-query E2E golden path
// ---------------------------------------------------------------------------

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "integration test exercises full ingest-to-query golden path"
)]
async fn test_e2e_ingest_to_query_golden_path() {
    let crawl_token = "tc05-crawl-token-secret";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "feed-tc05-golden";
    let track_guid = "track-tc05-golden-01";
    let feed_title = "TC05 Golden Path Album";
    let track_title = "TC05 Golden Track One";

    // ── Step 1: POST /ingest/feed ──────────────────────────────────────────
    let ingest_payload = serde_json::json!({
        "canonical_url": "https://example.com/tc05-golden.xml",
        "source_url": "https://example.com/tc05-golden.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "tc05-hash-unique-001",
        "feed_data": {
            "feed_guid": feed_guid,
            "title": feed_title,
            "description": "A test album for TC-05 golden path",
            "image_url": "https://img.example.com/tc05.jpg",
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "music",
            "author_name": "TC05 Artist",
            "owner_name": "TC05 Artist",
            "pub_date": null,
            "feed_payment_routes": [{
                "recipient_name": "TC05 Artist",
                "route_type": "node",
                "address": "03tc05feedrouteaddress",
                "custom_key": null,
                "custom_value": null,
                "split": 95,
                "fee": false
            }],
            "tracks": [
                {
                    "track_guid": track_guid,
                    "title": track_title,
                    "pub_date": 1_700_000_000,
                    "duration_secs": 240,
                    "enclosure_url": "https://cdn.example.com/tc05-track-01.mp3",
                    "enclosure_type": "audio/mpeg",
                    "enclosure_bytes": 5_000_000,
                    "track_number": 1,
                    "season": null,
                    "explicit": false,
                    "description": "First track on the golden path album",
                    "author_name": null,
                    "payment_routes": [{
                        "recipient_name": "TC05 Artist",
                        "route_type": "node",
                        "address": "03tc05trackrouteaddress",
                        "custom_key": "7629169",
                        "custom_value": "podcast-tc05",
                        "split": 95,
                        "fee": false
                    }],
                    "value_time_splits": []
                },
                {
                    "track_guid": "track-tc05-golden-02",
                    "title": "TC05 Golden Track Two",
                    "pub_date": 1_700_001_000,
                    "duration_secs": 180,
                    "enclosure_url": "https://cdn.example.com/tc05-track-02.mp3",
                    "enclosure_type": "audio/mpeg",
                    "enclosure_bytes": 3_000_000,
                    "track_number": 2,
                    "season": null,
                    "explicit": false,
                    "description": "Second track on the golden path album",
                    "author_name": null,
                    "payment_routes": [{
                        "recipient_name": "TC05 Artist",
                        "route_type": "node",
                        "address": "03tc05trackrouteaddress",
                        "custom_key": "7629169",
                        "custom_value": "podcast-tc05",
                        "split": 95,
                        "fee": false
                    }],
                    "value_time_splits": []
                }
            ]
        }
    });

    let resp = app
        .clone()
        .oneshot(json_request("POST", "/ingest/feed", &ingest_payload))
        .await
        .expect("ingest request should not panic");

    let status = resp.status().as_u16();
    let raw_bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let raw_text = String::from_utf8_lossy(&raw_bytes);
    assert!(
        status == 200 || status == 207,
        "ingest should return 200 or 207, got {status}: {raw_text}"
    );
    let ingest_body: serde_json::Value =
        serde_json::from_slice(&raw_bytes).expect("parse ingest json");
    assert!(
        ingest_body["accepted"].as_bool().expect("accepted field"),
        "ingest should be accepted"
    );

    // ── Step 2: GET /v1/feeds/{feed_guid} → verify feed title ──────────────
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/feeds/{feed_guid}"))
                .body(Body::empty())
                .expect("build get feed request"),
        )
        .await
        .expect("get feed should not panic");

    assert_eq!(resp.status(), 200, "GET feed should return 200");
    let feed_body = body_json(resp).await;
    assert_eq!(
        feed_body["data"]["title"].as_str().expect("title field"),
        feed_title,
        "feed title should match"
    );

    // ── Step 3: GET /v1/tracks/{track_guid} → verify track title ───────────
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/tracks/{track_guid}"))
                .body(Body::empty())
                .expect("build get track request"),
        )
        .await
        .expect("get track should not panic");

    assert_eq!(resp.status(), 200, "GET track should return 200");
    let track_body = body_json(resp).await;
    assert_eq!(
        track_body["data"]["title"].as_str().expect("title field"),
        track_title,
        "track title should match"
    );

    let resolver_pool = stophammer::db_pool::DbPool::from_writer_only(Arc::clone(&db));
    let resolver_summary =
        stophammer::resolver::worker::run_batch(&resolver_pool, "test-worker", 10)
            .expect("run resolver batch");
    assert_eq!(resolver_summary.claimed, 1);
    assert_eq!(resolver_summary.resolved, 1);

    // ── Step 4: GET /v1/search?q=Golden → verify canonical-first search results
    // Issue-FTS5-CONTENT — 2026-03-14
    // The search endpoint now JOINs through the `search_entities` companion
    // table to resolve (entity_type, entity_id) from contentless FTS5 rowids.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/search?q=Golden")
                .body(Body::empty())
                .expect("build search request"),
        )
        .await
        .expect("search should not panic");

    assert_eq!(resp.status(), 200, "GET /v1/search should return 200");
    let search_body = body_json(resp).await;
    let search_data = search_body["data"]
        .as_array()
        .expect("search data should be an array");
    assert!(
        !search_data.is_empty(),
        "search for 'Golden' should return at least one result"
    );

    // Default search now includes feeds alongside canonical artist/release/
    // recording rows.
    assert!(search_data.iter().all(|r| {
        matches!(
            r["entity_type"].as_str(),
            Some("artist" | "release" | "recording" | "feed")
        )
    }));
    let default_feed_hit = search_data.iter().find(|r| {
        r["entity_type"].as_str() == Some("feed") && r["entity_id"].as_str() == Some(feed_guid)
    });
    assert!(
        default_feed_hit.is_some(),
        "default search results should include the feed with entity_type='feed' and entity_id='{feed_guid}'"
    );

    // Explicit feed search should still surface the source feed hit.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/search?q=Golden&type=feed")
                .body(Body::empty())
                .expect("build source feed search request"),
        )
        .await
        .expect("feed search should not panic");

    assert_eq!(
        resp.status(),
        200,
        "GET /v1/search?type=feed should return 200"
    );
    let feed_search_body = body_json(resp).await;
    let feed_search_data = feed_search_body["data"]
        .as_array()
        .expect("feed search data should be an array");

    let feed_hit = feed_search_data.iter().find(|r| {
        r["entity_type"].as_str() == Some("feed") && r["entity_id"].as_str() == Some(feed_guid)
    });
    assert!(
        feed_hit.is_some(),
        "feed search results should include the feed with entity_type='feed' and entity_id='{feed_guid}'"
    );

    // ── Step 5: GET /v1/feeds/{guid}?include=payment_routes → verify routes ─
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/feeds/{feed_guid}?include=payment_routes"))
                .body(Body::empty())
                .expect("build get feed routes request"),
        )
        .await
        .expect("get feed routes should not panic");

    assert_eq!(resp.status(), 200, "GET feed with routes should return 200");
    let routes_body = body_json(resp).await;
    let routes = routes_body["data"]["payment_routes"]
        .as_array()
        .expect("payment_routes should be present");
    assert!(
        !routes.is_empty(),
        "feed should have at least one payment route"
    );
    assert_eq!(
        routes[0]["address"].as_str().expect("address"),
        "03tc05feedrouteaddress",
        "feed route address should match"
    );
}
