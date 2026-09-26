// Client Requests Work Plan, item 3 and 6: search row fields and deleted album.
//
// Item 3 (ADR 0042): A search result gives the artist, the track count and
// the duration of its row.
//
// Item 6 (ADR 0049 section 3): A test proves that a publisher read gives
// `unresolved` for an album feed that is deleted.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-search-fields-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
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
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    assert!(
        !bytes.is_empty(),
        "empty response body with status {status}"
    );
    serde_json::from_slice(&bytes).expect("parse json")
}

async fn post(app: axum::Router, uri: &str, payload: &serde_json::Value) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(payload).expect("serialize")))
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = resp.status();
    assert!(
        status.is_success(),
        "POST {uri} failed with status {status}"
    );
    body_json(resp).await
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
    let status = resp.status();
    assert!(status.is_success(), "GET {uri} failed with status {status}");
    body_json(resp).await
}

async fn delete_with_admin(
    app: axum::Router,
    uri: &str,
    admin_token: &str,
) -> axum::http::StatusCode {
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(uri)
                .header("X-Admin-Token", admin_token)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    resp.status()
}

#[tokio::test]
async fn a_search_for_a_feed_gives_the_artist_and_episode_count() {
    let token = "search-fields-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);
    let feed_guid = "search-feed-artist";

    let payload = serde_json::json!({
        "canonical_url": "https://example.com/search-feed.xml",
        "source_url": "https://example.com/search-feed.xml",
        "crawl_token": token,
        "http_status": 200,
        "content_hash": "hash-search-feed",
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "Search Test Album",
            "raw_medium": "music",
            "explicit": false,
            "author_name": "Test Artist",
            "tracks": [
                {
                    "track_guid": "search-track-1",
                    "title": "Track One",
                    "pub_date": 1_700_000_000,
                    "explicit": false,
                    "payment_routes": [], "value_time_splits": []
                },
                {
                    "track_guid": "search-track-2",
                    "title": "Track Two",
                    "pub_date": 1_700_000_100,
                    "explicit": false,
                    "payment_routes": [], "value_time_splits": []
                },
                {
                    "track_guid": "search-track-3",
                    "title": "Track Three",
                    "pub_date": 1_700_000_200,
                    "explicit": false,
                    "payment_routes": [], "value_time_splits": []
                },
                {
                    "track_guid": "search-track-4",
                    "title": "Track Four",
                    "pub_date": 1_700_000_300,
                    "explicit": false,
                    "payment_routes": [], "value_time_splits": []
                },
                {
                    "track_guid": "search-track-5",
                    "title": "Track Five",
                    "pub_date": 1_700_000_400,
                    "explicit": false,
                    "payment_routes": [], "value_time_splits": []
                }
            ]
        }
    });

    let _ingest_resp = post(
        stophammer::api::build_router(Arc::clone(&st)),
        "/ingest/feed",
        &payload,
    )
    .await;

    let body = get(
        stophammer::api::build_router(st),
        "/v1/search?q=Search+Test+Album&type=feed&limit=5",
    )
    .await;
    let items = body["data"].as_array().expect("search data");
    let hit = items
        .iter()
        .find(|i| i["entity_id"] == feed_guid)
        .expect("the seeded feed must appear in the results");

    assert_eq!(
        hit["release_artist"].as_str(),
        Some("Test Artist"),
        "musicindex request 3: a search result must carry release_artist"
    );
    assert_eq!(
        hit["release_artist_source"].as_str(),
        Some("itunes_author"),
        "a search result must carry release_artist_source"
    );
    assert_eq!(
        hit["episode_count"].as_i64(),
        Some(5),
        "a search result must carry episode_count"
    );
}

#[tokio::test]
async fn a_search_for_a_track_gives_the_artist_and_duration() {
    let token = "search-fields-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);
    let feed_guid = "search-track-feed";
    let track_guid = "search-track-test";

    let payload = serde_json::json!({
        "canonical_url": "https://example.com/search-track.xml",
        "source_url": "https://example.com/search-track.xml",
        "crawl_token": token,
        "http_status": 200,
        "content_hash": "hash-search-track",
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "Album With Track",
            "raw_medium": "music",
            "explicit": false,
            "author_name": "Album Artist",
            "tracks": [
                {
                    "track_guid": track_guid,
                    "title": "Track With Duration",
                    "author_name": "Track Artist Name",
                    "duration_secs": 240,
                    "pub_date": 1_700_000_100,
                    "explicit": false,
                    "payment_routes": [], "value_time_splits": []
                }
            ]
        }
    });

    let _ingest_resp = post(
        stophammer::api::build_router(Arc::clone(&st)),
        "/ingest/feed",
        &payload,
    )
    .await;

    let body = get(
        stophammer::api::build_router(st),
        "/v1/search?q=Track+With+Duration&type=track&limit=5",
    )
    .await;
    let items = body["data"].as_array().expect("search data");
    let hit = items
        .iter()
        .find(|i| i["entity_id"] == track_guid)
        .expect("the seeded track must appear in the results");

    assert_eq!(
        hit["track_artist"].as_str(),
        Some("Track Artist Name"),
        "a search result must carry track_artist"
    );
    assert_eq!(
        hit["duration_secs"].as_i64(),
        Some(240),
        "a search result must carry duration_secs"
    );
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the test ingests a publisher and an album, deletes the album and reads the publisher in one sequence"
)]
async fn a_deleted_album_feed_gives_unresolved_on_publisher_read() {
    let token = "publisher-delete-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);

    let publisher_guid = "test-publisher-feed";
    let album_guid = "test-album-to-delete";

    // Ingest the publisher feed first
    let publisher_payload = serde_json::json!({
        "canonical_url": "https://example.com/publisher.xml",
        "source_url": "https://example.com/publisher.xml",
        "crawl_token": token,
        "http_status": 200,
        "content_hash": "hash-publisher",
        "feed_data": {
            "feed_guid": publisher_guid,
            "title": "Publisher Feed",
            "raw_medium": "publisher",
            "explicit": false,
            "remote_items": [
                {
                    "position": 1,
                    "remote_feed_guid": album_guid,
                    "remote_feed_url": "https://example.com/album.xml",
                    "medium": "music"
                }
            ]
        }
    });

    let _ingest_resp = post(
        stophammer::api::build_router(Arc::clone(&st)),
        "/ingest/feed",
        &publisher_payload,
    )
    .await;

    // Now ingest the album feed
    let album_payload = serde_json::json!({
        "canonical_url": "https://example.com/album.xml",
        "source_url": "https://example.com/album.xml",
        "crawl_token": token,
        "http_status": 200,
        "content_hash": "hash-album",
        "feed_data": {
            "feed_guid": album_guid,
            "title": "Album to Delete",
            "raw_medium": "music",
            "explicit": false,
            "remote_items": [
                {
                    "position": 1,
                    "remote_feed_guid": publisher_guid,
                    "remote_feed_url": "https://example.com/publisher.xml",
                    "medium": "publisher"
                }
            ]
        }
    });

    let _ingest_resp = post(
        stophammer::api::build_router(Arc::clone(&st)),
        "/ingest/feed",
        &album_payload,
    )
    .await;

    // Read the publisher with include=publisher to verify the album is resolved
    let body_before_delete = get(
        stophammer::api::build_router(Arc::clone(&st)),
        &format!("/v1/feeds/{publisher_guid}?include=publisher"),
    )
    .await;
    let publisher_links_before = body_before_delete["data"]["publisher"]
        .as_array()
        .expect("publisher include");
    let album_entry_before = publisher_links_before
        .iter()
        .find(|e| e["music_feed_guid"] == album_guid)
        .expect("album should be listed");

    assert_ne!(
        album_entry_before["publisher_link_resolution"].as_str(),
        Some("unresolved"),
        "before delete, the album should be resolved"
    );

    // Delete the album feed
    let delete_status = delete_with_admin(
        stophammer::api::build_router(Arc::clone(&st)),
        &format!("/v1/feeds/{album_guid}"),
        "test-admin-token",
    )
    .await;
    assert_eq!(
        delete_status,
        axum::http::StatusCode::NO_CONTENT,
        "delete must succeed with 204 No Content"
    );

    // Read the publisher again with include=publisher
    let body_after_delete = get(
        stophammer::api::build_router(st),
        &format!("/v1/feeds/{publisher_guid}?include=publisher"),
    )
    .await;
    let publisher_links_after = body_after_delete["data"]["publisher"]
        .as_array()
        .expect("publisher include");
    let album_entry_after = publisher_links_after
        .iter()
        .find(|e| e["music_feed_guid"] == album_guid)
        .expect("album entry should still exist");

    assert_eq!(
        album_entry_after["publisher_link_resolution"].as_str(),
        Some("unresolved"),
        "ADR 0049 section 3: after delete, the album entry must give unresolved"
    );
}
