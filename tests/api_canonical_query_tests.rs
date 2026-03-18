#![recursion_limit = "256"]

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
async fn canonical_query_endpoints_expose_release_recording_and_source_links() {
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

    let (release_id, recording_id, artist_id) = {
        let conn = db.lock().expect("lock db");
        let release_id: String = conn
            .query_row(
                "SELECT release_id FROM source_feed_release_map WHERE feed_guid = ?1",
                [feed_guid],
                |row| row.get(0),
            )
            .expect("release id");
        let recording_id: String = conn
            .query_row(
                "SELECT recording_id FROM source_item_recording_map WHERE track_guid = ?1",
                [track_guid],
                |row| row.get(0),
            )
            .expect("recording id");
        let artist_id: String = conn
            .query_row(
                "SELECT acn.artist_id \
                 FROM feeds f \
                 JOIN artist_credit_name acn ON acn.artist_credit_id = f.artist_credit_id \
                 WHERE f.feed_guid = ?1 \
                 ORDER BY acn.position LIMIT 1",
                [feed_guid],
                |row| row.get(0),
            )
            .expect("artist id");
        (release_id, recording_id, artist_id)
    };

    let feed_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/feeds/{feed_guid}?include=canonical,source_links,source_ids,source_platforms,source_release_claims"))
                .body(Body::empty())
                .expect("feed request"),
        )
        .await
        .expect("feed response");
    assert_eq!(feed_resp.status(), 200);
    let feed_json = body_json(feed_resp).await;
    assert_eq!(feed_json["data"]["canonical"]["release_id"], release_id);
    assert_eq!(feed_json["data"]["source_links"][0]["url"], "https://artist.example.com/canonical-query");
    assert_eq!(feed_json["data"]["source_ids"][0]["scheme"], "nostr_npub");
    assert_eq!(feed_json["data"]["source_platforms"][0]["platform_key"], "wavlake");
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
                .uri(format!("/v1/tracks/{track_guid}?include=canonical,source_links,source_contributors,source_enclosures"))
                .body(Body::empty())
                .expect("track request"),
        )
        .await
        .expect("track response");
    assert_eq!(track_resp.status(), 200);
    let track_json = body_json(track_resp).await;
    assert_eq!(track_json["data"]["canonical"]["recording_id"], recording_id);
    assert_eq!(track_json["data"]["source_links"][0]["link_type"], "web_page");
    assert_eq!(track_json["data"]["source_contributors"][0]["role_norm"], "vocals");
    let track_enclosure_urls = track_json["data"]["source_enclosures"]
        .as_array()
        .expect("track source enclosures array")
        .iter()
        .filter_map(|enclosure| enclosure["url"].as_str())
        .collect::<Vec<_>>();
    assert!(track_enclosure_urls.contains(&"https://cdn.example.com/canonical-query.flac"));

    let release_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/releases/{release_id}?include=tracks,sources"))
                .body(Body::empty())
                .expect("release request"),
        )
        .await
        .expect("release response");
    assert_eq!(release_resp.status(), 200);
    let release_json = body_json(release_resp).await;
    assert_eq!(release_json["data"]["title"], "Canonical Query Release");
    assert_eq!(release_json["data"]["tracks"][0]["recording_id"], recording_id);
    assert_eq!(release_json["data"]["sources"][0]["feed_guid"], feed_guid);

    let recording_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/recordings/{recording_id}?include=sources,releases"))
                .body(Body::empty())
                .expect("recording request"),
        )
        .await
        .expect("recording response");
    assert_eq!(recording_resp.status(), 200);
    let recording_json = body_json(recording_resp).await;
    assert_eq!(recording_json["data"]["title"], "Canonical Query Song");
    assert_eq!(recording_json["data"]["sources"][0]["track_guid"], track_guid);
    assert_eq!(recording_json["data"]["releases"][0]["release_id"], release_id);

    let artist_releases_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/artists/{artist_id}/releases"))
                .body(Body::empty())
                .expect("artist releases request"),
        )
        .await
        .expect("artist releases response");
    assert_eq!(artist_releases_resp.status(), 200);
    let artist_releases_json = body_json(artist_releases_resp).await;
    assert_eq!(artist_releases_json["data"][0]["release_id"], release_id);

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
    assert!(search_types.iter().all(|kind| matches!(*kind, "artist" | "release" | "recording")));
    assert!(search_results.iter().any(|row| row["entity_type"] == "release" && row["entity_id"] == release_id));
    assert!(search_results.iter().any(|row| row["entity_type"] == "recording" && row["entity_id"] == recording_id));

    let release_search_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/search?q=Canonical&type=release")
                .body(Body::empty())
                .expect("release search request"),
        )
        .await
        .expect("release search response");
    assert_eq!(release_search_resp.status(), 200);
    let release_search_json = body_json(release_search_resp).await;
    assert_eq!(release_search_json["data"][0]["entity_type"], "release");
    assert_eq!(release_search_json["data"][0]["entity_id"], release_id);
}
