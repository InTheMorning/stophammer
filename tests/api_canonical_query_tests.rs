#![recursion_limit = "256"]
#![allow(
    clippy::too_many_lines,
    reason = "canonical API regression test keeps the full response matrix in one golden-path scenario"
)]
#![allow(
    clippy::unreadable_literal,
    reason = "fixture payloads preserve raw timestamp and byte-count literals for readability against JSON"
)]

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state_with_crawl_token(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
    signer_path: &std::path::Path,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(
        stophammer::signing::NodeSigner::load_or_create(signer_path).expect("create signer"),
    );
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

#[tokio::test]
async fn source_first_query_endpoints_expose_feed_track_and_source_links() {
    let crawl_token = "canonical-query-crawl-token";
    let db = common::test_db_arc();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("canonical-query.key");
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token, &signer_path);
    let app = stophammer::api::build_router(state);

    let feed_guid = "feed-canonical-query";
    let track_guid = "track-canonical-query";
    let ingest_payload = serde_json::json!({
        "canonical_url": "https://example.com/canonical-query.xml",
        "source_url": "https://example.com/canonical-query.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "canonical-query-hash-001",
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "Canonical Query Release",
            "description": "A release used to test canonical read endpoints",
            "image_url": "https://img.example.com/canonical-query.jpg",
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "music",
            "author_name": "Canonical Query Artist",
            "owner_name": "Wavlake",
            "pub_date": 1700000000,
            "persons": [],
            "entity_ids": [{
                "position": 0,
                "scheme": "nostr_npub",
                "value": "npub1canonicalqueryartist"
            }],
            "links": [{
                "position": 0,
                "link_type": "website",
                "url": "https://artist.example.com/canonical-query",
                "extraction_path": "feed.link"
            }],
            "feed_payment_routes": [],
            "tracks": [{
                "track_guid": track_guid,
                "title": "Canonical Query Song",
                "pub_date": 1700000000,
                "duration_secs": 222,
                "enclosure_url": "https://cdn.example.com/canonical-query.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 5000000,
                "alternate_enclosures": [{
                    "position": 1,
                    "url": "https://cdn.example.com/canonical-query.flac",
                    "mime_type": "audio/flac",
                    "bytes": 15000000,
                    "rel": "alternate",
                    "title": "Lossless",
                    "extraction_path": "track.podcast:alternateEnclosure[0]"
                }],
                "track_number": 1,
                "season": null,
                "explicit": false,
                "description": "Canonical Query Song Description",
                "author_name": null,
                "persons": [{
                    "position": 0,
                    "name": "Canonical Query Artist",
                    "role": "Vocals",
                    "group_name": null,
                    "href": null,
                    "img": null
                }],
                "entity_ids": [],
                "links": [{
                    "position": 0,
                    "link_type": "web_page",
                    "url": "https://artist.example.com/canonical-query/song",
                    "extraction_path": "entity.link"
                }],
                "payment_routes": [],
                "value_time_splits": []
            }]
        }
    });

    let ingest_resp = app
        .clone()
        .oneshot(json_request("POST", "/ingest/feed", &ingest_payload))
        .await
        .expect("ingest");
    assert_eq!(ingest_resp.status(), 200);

    let feed_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/feeds/{feed_guid}?include=source_links,source_ids,source_platforms,source_release_claims"))
                .body(Body::empty())
                .expect("feed request"),
        )
        .await
        .expect("feed response");
    assert_eq!(feed_resp.status(), 200);
    let feed_json = body_json(feed_resp).await;
    assert_eq!(
        feed_json["data"]["release_artist"],
        "Canonical Query Artist"
    );
    assert_eq!(
        feed_json["data"]["source_links"][0]["url"],
        "https://artist.example.com/canonical-query"
    );
    assert_eq!(feed_json["data"]["source_ids"][0]["scheme"], "nostr_npub");
    assert_eq!(
        feed_json["data"]["source_platforms"][0]["platform_key"],
        "wavlake"
    );
    let feed_claim_types = feed_json["data"]["source_release_claims"]
        .as_array()
        .expect("feed source release claims array")
        .iter()
        .filter_map(|claim| claim["claim_type"].as_str())
        .collect::<Vec<_>>();
    assert!(feed_claim_types.contains(&"release_date"));

    let track_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/tracks/{track_guid}?include=source_links,source_contributors,source_enclosures"))
                .body(Body::empty())
                .expect("track request"),
        )
        .await
        .expect("track response");
    assert_eq!(track_resp.status(), 200);
    let track_json = body_json(track_resp).await;
    assert_eq!(track_json["data"]["publisher_text"], "Wavlake");
    assert_eq!(track_json["data"]["track_artist"], "Canonical Query Artist");
    assert_eq!(
        track_json["data"]["source_links"][0]["link_type"],
        "web_page"
    );
    assert_eq!(
        track_json["data"]["source_contributors"][0]["role_norm"],
        "vocals"
    );
    let track_enclosure_urls = track_json["data"]["source_enclosures"]
        .as_array()
        .expect("track source enclosures array")
        .iter()
        .filter_map(|enclosure| enclosure["url"].as_str())
        .collect::<Vec<_>>();
    assert!(track_enclosure_urls.contains(&"https://cdn.example.com/canonical-query.flac"));

    let publisher_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/publishers/Wavlake")
                .body(Body::empty())
                .expect("publisher detail request"),
        )
        .await
        .expect("publisher detail response");
    assert_eq!(publisher_resp.status(), 200);
    let publisher_json = body_json(publisher_resp).await;
    assert_eq!(publisher_json["data"]["publisher_text"], "Wavlake");
    assert_eq!(publisher_json["data"]["feeds"][0]["feed_guid"], feed_guid);
    assert_eq!(
        publisher_json["data"]["tracks"][0]["track_guid"],
        track_guid
    );
    assert_eq!(publisher_json["data"]["tracks"][0]["feed_guid"], feed_guid);

    let search_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/search?q=Canonical")
                .body(Body::empty())
                .expect("search request"),
        )
        .await
        .expect("search response");
    assert_eq!(search_resp.status(), 200);
    let search_json = body_json(search_resp).await;
    let search_results = search_json["data"]
        .as_array()
        .expect("search results array");
    let search_types = search_results
        .iter()
        .filter_map(|row| row["entity_type"].as_str())
        .collect::<Vec<_>>();
    assert!(
        search_types
            .iter()
            .all(|kind| matches!(*kind, "feed" | "track"))
    );
    assert!(
        search_results
            .iter()
            .any(|row| row["entity_type"] == "feed" && row["entity_id"] == feed_guid)
    );
    assert!(
        search_results
            .iter()
            .any(|row| row["entity_type"] == "track" && row["entity_id"] == track_guid)
    );

    let track_search_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/search?q=Canonical&type=track")
                .body(Body::empty())
                .expect("track search request"),
        )
        .await
        .expect("track search response");
    assert_eq!(track_search_resp.status(), 200);
    let track_search_json = body_json(track_search_resp).await;
    assert_eq!(track_search_json["data"][0]["entity_type"], "track");
    assert_eq!(track_search_json["data"][0]["entity_id"], track_guid);

    let recent_feeds_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/feeds/recent")
                .body(Body::empty())
                .expect("recent feeds request"),
        )
        .await
        .expect("recent feeds response");
    assert_eq!(recent_feeds_resp.status(), 200);
    let recent_feeds_json = body_json(recent_feeds_resp).await;
    assert_eq!(recent_feeds_json["data"][0]["feed_guid"], feed_guid);
}

#[tokio::test]
async fn track_contributor_views_inherit_feed_people_only_when_track_people_are_absent() {
    let crawl_token = "canonical-query-feed-people-crawl-token";
    let db = common::test_db_arc();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("canonical-query-feed-people.key");
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token, &signer_path);
    let app = stophammer::api::build_router(state);

    let feed_guid = "feed-canonical-query-feed-people";
    let track_guid = "track-canonical-query-feed-people";
    let ingest_payload = serde_json::json!({
        "canonical_url": "https://example.com/canonical-query-feed-people.xml",
        "source_url": "https://example.com/canonical-query-feed-people.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "canonical-query-feed-people-hash-001",
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "Canonical Query Feed People Release",
            "description": "A release used to test contributor inheritance",
            "image_url": "https://img.example.com/canonical-query-feed-people.jpg",
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "music",
            "author_name": "Canonical Query Feed People Artist",
            "owner_name": "Independent",
            "pub_date": 1700000000,
            "persons": [{
                "position": 0,
                "name": "Feed Host",
                "role": "Host",
                "group_name": null,
                "href": null,
                "img": null
            }],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [],
            "tracks": [{
                "track_guid": track_guid,
                "title": "Canonical Query Feed People Song",
                "pub_date": 1700000000,
                "duration_secs": 180,
                "enclosure_url": "https://cdn.example.com/canonical-query-feed-people.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 4000000,
                "alternate_enclosures": [],
                "track_number": 1,
                "season": null,
                "explicit": false,
                "description": "Canonical Query Feed People Song Description",
                "author_name": null,
                "persons": [],
                "entity_ids": [],
                "links": [],
                "payment_routes": [],
                "value_time_splits": []
            }]
        }
    });

    let ingest_resp = app
        .clone()
        .oneshot(json_request("POST", "/ingest/feed", &ingest_payload))
        .await
        .expect("ingest");
    assert_eq!(ingest_resp.status(), 200);

    let track_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/v1/tracks/{track_guid}?include=source_contributors"
                ))
                .body(Body::empty())
                .expect("track request"),
        )
        .await
        .expect("track response");
    assert_eq!(track_resp.status(), 200);
    let track_json = body_json(track_resp).await;
    assert_eq!(
        track_json["data"]["source_contributors"][0]["name"],
        "Feed Host"
    );
    assert_eq!(
        track_json["data"]["source_contributors"][0]["entity_type"],
        "feed"
    );
}

#[tokio::test]
async fn feed_query_exposes_publisher_rss_truth() {
    let crawl_token = "publisher-truth-crawl-token";
    let db = common::test_db_arc();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("publisher-truth.key");
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token, &signer_path);
    let app = stophammer::api::build_router(state);

    let publisher_feed_guid = "feed-publisher-truth-publisher";
    let child_feed_guid = "feed-publisher-truth-child";

    let publisher_payload = serde_json::json!({
        "canonical_url": "https://wavlake.com/feed/artist/publisher-truth",
        "source_url": "https://wavlake.com/feed/artist/publisher-truth",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "publisher-truth-publisher-hash",
        "feed_data": {
            "feed_guid": publisher_feed_guid,
            "title": "Publisher Truth Artist",
            "description": "Publisher feed for rss truth query coverage",
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "publisher",
            "author_name": "Publisher Truth Artist",
            "owner_name": "Wavlake",
            "pub_date": null,
            "remote_items": [{
                "position": 0,
                "medium": "music",
                "remote_feed_guid": child_feed_guid,
                "remote_feed_url": "https://wavlake.com/feed/music/publisher-truth-child"
            }],
            "persons": [],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [],
            "tracks": []
        }
    });

    let publisher_resp = app
        .clone()
        .oneshot(json_request("POST", "/ingest/feed", &publisher_payload))
        .await
        .expect("publisher ingest");
    assert_eq!(publisher_resp.status(), 200);

    let child_payload = serde_json::json!({
        "canonical_url": "https://wavlake.com/feed/music/publisher-truth-child",
        "source_url": "https://wavlake.com/feed/music/publisher-truth-child",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "publisher-truth-child-hash",
        "feed_data": {
            "feed_guid": child_feed_guid,
            "title": "Publisher Truth Release",
            "description": "Child music feed for rss truth query coverage",
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "music",
            "author_name": null,
            "owner_name": "Wavlake",
            "pub_date": null,
            "remote_items": [{
                "position": 0,
                "medium": "publisher",
                "remote_feed_guid": publisher_feed_guid,
                "remote_feed_url": "https://wavlake.com/feed/artist/publisher-truth"
            }],
            "persons": [],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [],
            "tracks": [{
                "track_guid": "track-publisher-truth-child",
                "title": "Publisher Truth Song",
                "pub_date": 1700000100,
                "duration_secs": 180,
                "enclosure_url": "https://cdn.example.com/publisher-truth.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 1234567,
                "alternate_enclosures": [],
                "track_number": 1,
                "season": null,
                "explicit": false,
                "description": null,
                "author_name": null,
                "persons": [],
                "entity_ids": [],
                "links": [],
                "payment_routes": [],
                "value_time_splits": []
            }]
        }
    });

    let child_resp = app
        .clone()
        .oneshot(json_request("POST", "/ingest/feed", &child_payload))
        .await
        .expect("child ingest");
    assert_eq!(child_resp.status(), 200);

    let publisher_feed_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/v1/feeds/{publisher_feed_guid}?include=remote_items,publisher"
                ))
                .body(Body::empty())
                .expect("publisher feed request"),
        )
        .await
        .expect("publisher feed response");
    assert_eq!(publisher_feed_resp.status(), 200);
    let publisher_feed_json = body_json(publisher_feed_resp).await;
    assert_eq!(publisher_feed_json["data"]["raw_medium"], "publisher");
    assert_eq!(
        publisher_feed_json["data"]["remote_items"][0]["medium"],
        "music"
    );
    assert_eq!(
        publisher_feed_json["data"]["publisher"][0]["direction"],
        "publisher_to_music"
    );
    assert_eq!(
        publisher_feed_json["data"]["publisher"][0]["publisher_feed_guid"],
        publisher_feed_guid
    );
    assert_eq!(
        publisher_feed_json["data"]["publisher"][0]["music_feed_guid"],
        child_feed_guid
    );
    assert_eq!(
        publisher_feed_json["data"]["publisher"][0]["two_way_validated"],
        true
    );
    assert!(
        publisher_feed_json["data"]["publisher"][0]
            .get("artist_signal")
            .is_none()
    );

    let child_feed_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/v1/feeds/{child_feed_guid}?include=remote_items,publisher"
                ))
                .body(Body::empty())
                .expect("child feed request"),
        )
        .await
        .expect("child feed response");
    assert_eq!(child_feed_resp.status(), 200);
    let child_feed_json = body_json(child_feed_resp).await;
    assert_eq!(child_feed_json["data"]["raw_medium"], "music");
    assert_eq!(child_feed_json["data"]["publisher_text"], "Wavlake");
    assert_eq!(
        child_feed_json["data"]["release_artist"],
        "Publisher Truth Artist"
    );
    assert_eq!(
        child_feed_json["data"]["remote_items"][0]["medium"],
        "publisher"
    );
    assert_eq!(
        child_feed_json["data"]["publisher"][0]["direction"],
        "music_to_publisher"
    );
    assert_eq!(
        child_feed_json["data"]["publisher"][0]["two_way_validated"],
        true
    );
    assert!(
        child_feed_json["data"]["publisher"][0]
            .get("artist_signal")
            .is_none()
    );

    let child_track_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/tracks/track-publisher-truth-child")
                .body(Body::empty())
                .expect("child track request"),
        )
        .await
        .expect("child track response");
    assert_eq!(child_track_resp.status(), 200);
    let child_track_json = body_json(child_track_resp).await;
    assert_eq!(
        child_track_json["data"]["track_artist"],
        "Publisher Truth Artist"
    );
}

#[tokio::test]
async fn non_wavlake_reciprocal_publisher_remote_item_sets_publisher_text() {
    let crawl_token = "publisher-remoteitem-crawl-token";
    let db = common::test_db_arc();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("publisher-remoteitem.key");
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token, &signer_path);
    let app = stophammer::api::build_router(state);

    let publisher_feed_guid = "feed-remoteitem-publisher";
    let child_feed_guid = "feed-remoteitem-child";

    let publisher_payload = serde_json::json!({
        "canonical_url": "https://label.example.com/publisher.xml",
        "source_url": "https://label.example.com/publisher.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "remoteitem-publisher-hash",
        "feed_data": {
            "feed_guid": publisher_feed_guid,
            "title": "Indie Collective",
            "description": "Publisher feed for reciprocal publisher mapping",
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "publisher",
            "author_name": null,
            "owner_name": null,
            "pub_date": null,
            "remote_items": [{
                "position": 0,
                "medium": "music",
                "remote_feed_guid": child_feed_guid,
                "remote_feed_url": "https://artist.example.com/release.xml"
            }],
            "persons": [],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [],
            "tracks": [{
                "track_guid": "track-remoteitem-child",
                "title": "Remote Item Song",
                "pub_date": 1700000200,
                "duration_secs": 181,
                "enclosure_url": "https://cdn.example.com/remote-item-song.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 7654321,
                "alternate_enclosures": [],
                "track_number": 1,
                "season": null,
                "explicit": false,
                "description": null,
                "author_name": null,
                "persons": [],
                "entity_ids": [],
                "links": [],
                "payment_routes": [],
                "value_time_splits": []
            }]
        }
    });

    let publisher_resp = app
        .clone()
        .oneshot(json_request("POST", "/ingest/feed", &publisher_payload))
        .await
        .expect("publisher ingest");
    assert_eq!(publisher_resp.status(), 200);

    let child_payload = serde_json::json!({
        "canonical_url": "https://artist.example.com/release.xml",
        "source_url": "https://artist.example.com/release.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "remoteitem-child-hash",
        "feed_data": {
            "feed_guid": child_feed_guid,
            "title": "Remote Item Release",
            "description": "Music feed linked to a non-Wavlake publisher",
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "music",
            "author_name": "Remote Item Artist",
            "owner_name": null,
            "pub_date": null,
            "remote_items": [{
                "position": 0,
                "medium": "publisher",
                "remote_feed_guid": publisher_feed_guid,
                "remote_feed_url": "https://label.example.com/publisher.xml"
            }],
            "persons": [],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [],
            "tracks": [{
                "track_guid": "track-remoteitem-child",
                "title": "Remote Item Song",
                "pub_date": 1700000200,
                "duration_secs": 181,
                "enclosure_url": "https://cdn.example.com/remote-item-song.mp3",
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": 7654321,
                "alternate_enclosures": [],
                "track_number": 1,
                "season": null,
                "explicit": false,
                "description": null,
                "author_name": null,
                "persons": [],
                "entity_ids": [],
                "links": [],
                "payment_routes": [],
                "value_time_splits": []
            }]
        }
    });

    let child_resp = app
        .clone()
        .oneshot(json_request("POST", "/ingest/feed", &child_payload))
        .await
        .expect("child ingest");
    assert_eq!(child_resp.status(), 200);

    let child_feed_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/v1/feeds/{child_feed_guid}?include=remote_items,publisher"
                ))
                .body(Body::empty())
                .expect("child feed request"),
        )
        .await
        .expect("child feed response");
    assert_eq!(child_feed_resp.status(), 200);
    let child_feed_json = body_json(child_feed_resp).await;
    assert_eq!(
        child_feed_json["data"]["publisher_text"],
        "Indie Collective"
    );
    assert_eq!(
        child_feed_json["data"]["release_artist"],
        "Remote Item Artist"
    );
    assert_eq!(
        child_feed_json["data"]["publisher"][0]["direction"],
        "music_to_publisher"
    );
    assert_eq!(
        child_feed_json["data"]["publisher"][0]["two_way_validated"],
        true
    );

    let child_track_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/tracks/track-remoteitem-child")
                .body(Body::empty())
                .expect("child track request"),
        )
        .await
        .expect("child track response");
    assert_eq!(child_track_resp.status(), 200);
    let child_track_json = body_json(child_track_resp).await;
    assert_eq!(
        child_track_json["data"]["publisher_text"],
        "Indie Collective"
    );
    assert_eq!(
        child_track_json["data"]["track_artist"],
        "Remote Item Artist"
    );
}

#[tokio::test]
async fn non_wavlake_one_way_publisher_remote_item_does_not_set_publisher_text() {
    let crawl_token = "publisher-one-way-crawl-token";
    let db = common::test_db_arc();
    let tmp = tempfile::tempdir().expect("tempdir");
    let signer_path = tmp.path().join("publisher-one-way.key");
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token, &signer_path);
    let app = stophammer::api::build_router(state);

    let publisher_feed_guid = "feed-one-way-publisher";
    let child_feed_guid = "feed-one-way-child";

    let publisher_payload = serde_json::json!({
        "canonical_url": "https://label.example.com/one-way-publisher.xml",
        "source_url": "https://label.example.com/one-way-publisher.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "one-way-publisher-hash",
        "feed_data": {
            "feed_guid": publisher_feed_guid,
            "title": "One Way Label",
            "description": "Publisher feed with no reciprocal remote item",
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "publisher",
            "author_name": null,
            "owner_name": null,
            "pub_date": null,
            "remote_items": [],
            "persons": [],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [],
            "tracks": []
        }
    });

    let publisher_resp = app
        .clone()
        .oneshot(json_request("POST", "/ingest/feed", &publisher_payload))
        .await
        .expect("publisher ingest");
    assert_eq!(publisher_resp.status(), 200);

    let child_payload = serde_json::json!({
        "canonical_url": "https://artist.example.com/one-way-release.xml",
        "source_url": "https://artist.example.com/one-way-release.xml",
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "one-way-child-hash",
        "feed_data": {
            "feed_guid": child_feed_guid,
            "title": "One Way Release",
            "description": "Music feed with a one-way publisher link",
            "image_url": null,
            "language": "en",
            "explicit": false,
            "itunes_type": null,
            "raw_medium": "music",
            "author_name": "One Way Artist",
            "owner_name": null,
            "pub_date": null,
            "remote_items": [{
                "position": 0,
                "medium": "publisher",
                "remote_feed_guid": publisher_feed_guid,
                "remote_feed_url": "https://label.example.com/one-way-publisher.xml"
            }],
            "persons": [],
            "entity_ids": [],
            "links": [],
            "feed_payment_routes": [],
            "tracks": []
        }
    });

    let child_resp = app
        .clone()
        .oneshot(json_request("POST", "/ingest/feed", &child_payload))
        .await
        .expect("child ingest");
    assert_eq!(child_resp.status(), 200);

    let child_feed_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/feeds/{child_feed_guid}"))
                .body(Body::empty())
                .expect("child feed request"),
        )
        .await
        .expect("child feed response");
    assert_eq!(child_feed_resp.status(), 200);
    let child_feed_json = body_json(child_feed_resp).await;
    assert!(child_feed_json["data"]["publisher_text"].is_null());
}
