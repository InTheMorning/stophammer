// ADR 0054 section 4: the node serves only web URLs in URL fields.
//
// This file covers the read-route and ingest-warning behavior of a URL field.
// The address cases of section 1 and section 5 are in the tests of
// `musicindex-fetch-guard` (musicindex ADR 0104 R-LYRIC-9 item 2). The unit tests of
// `web_url_or_none` are in stophammer-parser `tests/web_link.rs`.

#![recursion_limit = "256"]

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

// ── Ingest-to-query end-to-end: ADR 0054 section 4 ──────────────────────────

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("adr0054-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());

    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "adr0054-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
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

/// Acceptance criteria: a feed with a non-web feed image and a non-web track
/// enclosure. The response carries the two warnings, the feed is accepted,
/// the read routes give `null` for both fields, and the raw values stay in
/// the database. A second, well-formed track shows an `https` value passes
/// through unchanged.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "integration test exercises the full ingest-to-query round trip for two tracks"
)]
async fn ingest_warns_and_read_routes_null_a_non_web_url() {
    let crawl_token = "adr0054-crawl-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "feed-adr0054-non-web-url";
    let bad_track_guid = "track-adr0054-bad-enclosure";
    let good_track_guid = "track-adr0054-good-enclosure";

    let ingest_payload = serde_json::json!({
        "canonical_url": "https://example.com/adr0054.xml",
        "source_url": "https://example.com/adr0054.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "adr0054-hash-001",
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "ADR 0054 Test Feed",
            "description": "A feed with a non-web image URL",
            "image_url": "javascript:alert(1)",
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "music",
            "author_name": "ADR 0054 Artist",
            "owner_name": "ADR 0054 Artist",
            "pub_date": null,
            "feed_payment_routes": [{
                "recipient_name": "ADR 0054 Artist",
                "route_type": "node",
                "address": "03adr0054feedrouteaddress",
                "custom_key": null,
                "custom_value": null,
                "split": 95,
                "fee": false
            }],
            "tracks": [
                {
                    "track_guid": bad_track_guid,
                    "title": "Bad Enclosure Track",
                    "pub_date": 1_700_000_000,
                    "duration_secs": 120,
                    "enclosure_url": "data:audio/mp3;base64,AA",
                    "enclosure_type": "audio/mpeg",
                    "enclosure_bytes": 1000,
                    "track_number": 1,
                    "season": null,
                    "explicit": false,
                    "description": null,
                    "author_name": null,
                    "payment_routes": [],
                    "value_time_splits": []
                },
                {
                    "track_guid": good_track_guid,
                    "title": "Good Enclosure Track",
                    "pub_date": 1_700_000_100,
                    "duration_secs": 130,
                    "enclosure_url": "https://cdn.example.com/adr0054-good.mp3",
                    "enclosure_type": "audio/mpeg",
                    "enclosure_bytes": 2000,
                    "track_number": 2,
                    "season": null,
                    "explicit": false,
                    "description": null,
                    "author_name": null,
                    "payment_routes": [],
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
    assert_eq!(resp.status(), 200, "ingest should be accepted");
    let ingest_body = body_json(resp).await;
    assert!(
        ingest_body["accepted"].as_bool().expect("accepted field"),
        "feed should be accepted despite the non-web URLs"
    );
    let warnings: Vec<&str> = ingest_body["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .map(|w| w.as_str().expect("warning is a string"))
        .collect();
    assert!(
        warnings.contains(&"non-web URL in image_url"),
        "warnings should name image_url, got {warnings:?}"
    );
    assert!(
        warnings.contains(&"non-web URL in enclosure_url"),
        "warnings should name enclosure_url, got {warnings:?}"
    );

    // GET /v1/feeds/{guid}: image_url is null.
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
    assert_eq!(resp.status(), 200);
    let feed_body = body_json(resp).await;
    assert!(
        feed_body["data"]["image_url"].is_null(),
        "image_url should be null, got {:?}",
        feed_body["data"]["image_url"]
    );

    // GET .../tracks/{bad_track_guid}: enclosure_url is null.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/feeds/{feed_guid}/tracks/{bad_track_guid}"))
                .body(Body::empty())
                .expect("build get track request"),
        )
        .await
        .expect("get track should not panic");
    assert_eq!(resp.status(), 200);
    let bad_track_body = body_json(resp).await;
    assert!(
        bad_track_body["data"]["enclosure_url"].is_null(),
        "enclosure_url should be null, got {:?}",
        bad_track_body["data"]["enclosure_url"]
    );

    // GET .../tracks/{good_track_guid}: the https enclosure passes through
    // unchanged.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/feeds/{feed_guid}/tracks/{good_track_guid}"))
                .body(Body::empty())
                .expect("build get good track request"),
        )
        .await
        .expect("get good track should not panic");
    assert_eq!(resp.status(), 200);
    let good_track_body = body_json(resp).await;
    assert_eq!(
        good_track_body["data"]["enclosure_url"].as_str(),
        Some("https://cdn.example.com/adr0054-good.mp3"),
        "an https enclosure should reach the client unchanged"
    );

    // The database still holds both raw, non-web values (ADR 0054 section 4
    // and mandate 3 of AGENTS.md: never discard raw source data).
    let conn = db.lock().expect("lock db");
    let stored_image_url: String = conn
        .query_row(
            "SELECT image_url FROM feeds WHERE feed_guid = ?1",
            [feed_guid],
            |row| row.get(0),
        )
        .expect("feed row exists");
    assert_eq!(stored_image_url, "javascript:alert(1)");

    let stored_enclosure_url: String = conn
        .query_row(
            "SELECT enclosure_url FROM tracks WHERE track_guid = ?1",
            [bad_track_guid],
            |row| row.get(0),
        )
        .expect("track row exists");
    assert_eq!(stored_enclosure_url, "data:audio/mp3;base64,AA");
}

/// ADR 0054 section 4 extension: a `podcast:person` `img` and a
/// `podcast:transcript` URL are also URL fields from RSS. A non-web value in
/// either nulls on the read route, and the raw value stays in the database.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "integration test exercises the full ingest-to-query round trip"
)]
async fn ingest_warns_and_read_routes_null_a_non_web_person_img_and_transcript_url() {
    let crawl_token = "adr0054-crawl-token-2";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "feed-adr0054-person-transcript";
    let track_guid = "track-adr0054-person-transcript";

    let ingest_payload = serde_json::json!({
        "canonical_url": "https://example.com/adr0054-person-transcript.xml",
        "source_url": "https://example.com/adr0054-person-transcript.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "adr0054-hash-002",
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "ADR 0054 Person And Transcript Feed",
            "description": null,
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "music",
            "author_name": "ADR 0054 Artist",
            "owner_name": "ADR 0054 Artist",
            "pub_date": null,
            "feed_payment_routes": [{
                "recipient_name": "ADR 0054 Artist",
                "route_type": "node",
                "address": "03adr0054personrouteaddress",
                "custom_key": null,
                "custom_value": null,
                "split": 95,
                "fee": false
            }],
            "tracks": [
                {
                    "track_guid": track_guid,
                    "title": "Person And Transcript Track",
                    "pub_date": 1_700_000_200,
                    "duration_secs": 140,
                    "enclosure_url": "https://cdn.example.com/adr0054-person-transcript.mp3",
                    "enclosure_type": "audio/mpeg",
                    "enclosure_bytes": 3000,
                    "track_number": 1,
                    "season": null,
                    "explicit": false,
                    "description": null,
                    "author_name": null,
                    "persons": [{
                        "position": 0,
                        "name": "A Voice Actor",
                        "role": "voice actor",
                        "group_name": null,
                        "href": null,
                        "img": "javascript:alert(1)",
                        "npub": null
                    }],
                    "transcripts": [{
                        "position": 0,
                        "url": "data:text/plain,x",
                        "mime_type": "text/plain",
                        "language": "en",
                        "rel": null
                    }],
                    "payment_routes": [],
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
    assert_eq!(resp.status(), 200, "ingest should be accepted");
    let ingest_body = body_json(resp).await;
    assert!(
        ingest_body["accepted"].as_bool().expect("accepted field"),
        "feed should be accepted despite the non-web URLs"
    );
    let warnings: Vec<&str> = ingest_body["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .map(|w| w.as_str().expect("warning is a string"))
        .collect();
    assert!(
        warnings.contains(&"non-web URL in img"),
        "warnings should name img, got {warnings:?}"
    );
    assert!(
        warnings.contains(&"non-web URL in url"),
        "warnings should name url, got {warnings:?}"
    );

    // GET .../tracks/{track_guid}?include=source_contributors,source_transcripts
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/v1/feeds/{feed_guid}/tracks/{track_guid}?include=source_contributors,source_transcripts"
                ))
                .body(Body::empty())
                .expect("build get track request"),
        )
        .await
        .expect("get track should not panic");
    assert_eq!(resp.status(), 200);
    let track_body = body_json(resp).await;
    let contributors = track_body["data"]["source_contributors"]
        .as_array()
        .expect("source_contributors array");
    assert_eq!(contributors.len(), 1, "one contributor claim expected");
    assert!(
        contributors[0]["img"].is_null(),
        "img should be null, got {:?}",
        contributors[0]["img"]
    );
    let transcripts = track_body["data"]["source_transcripts"]
        .as_array()
        .expect("source_transcripts array");
    assert_eq!(transcripts.len(), 1, "one transcript expected");
    assert!(
        transcripts[0]["url"].is_null(),
        "url should be null, got {:?}",
        transcripts[0]["url"]
    );

    // The database still holds both raw, non-web values.
    let conn = db.lock().expect("lock db");
    let stored_img: String = conn
        .query_row(
            "SELECT img FROM source_contributor_claims WHERE feed_guid = ?1 AND entity_type = 'track'",
            [feed_guid],
            |row| row.get(0),
        )
        .expect("contributor claim row exists");
    assert_eq!(stored_img, "javascript:alert(1)");

    let stored_transcript_url: String = conn
        .query_row(
            "SELECT url FROM source_item_transcripts WHERE feed_guid = ?1 AND entity_type = 'track'",
            [feed_guid],
            |row| row.get(0),
        )
        .expect("transcript row exists");
    assert_eq!(stored_transcript_url, "data:text/plain,x");
}
