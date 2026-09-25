// ADR 0049 task 008: the derived artist count on a publisher feed read.
//
// A feed read of a publisher feed gives `distinct_release_artist_count` and
// `distinct_release_artists`. Both are derived from the music feeds that
// name the publisher feed through a `medium="publisher"` remote item that
// resolves back to it (`db::resolve_listed_feed`). Only an album with
// `release_artist_source = "itunes_author"` counts.
//
// These tests use inline payloads and the short chain
// (`content_hash, medium_music`), the same pattern
// `tests/adr0049_resolver_tests.rs` uses for its "listed by" test, because
// none of these synthetic feeds carries a payment route.

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
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0049-artist-count-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: ["content_hash", "medium_music"]
            .iter()
            .map(ToString::to_string)
            .collect(),
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

async fn ingest(app: axum::Router, payload: &serde_json::Value) {
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

/// Three albums name one publisher: `"X"`, `" x "` (equal to `"X"` after
/// normalization) and `"Y"`. The count is 2, and the list gives one raw
/// value for each normalized group, sorted by the normalized value.
///
/// The album guids are chosen so `artist-count-album-1-x` (author `"X"`)
/// sorts before `artist-count-album-2-x-spaced` (author `" x "`): digit "1"
/// sorts before digit "2". The list must give `"X"`, the raw value of the
/// album with the lower `feed_guid`, not `" x "`.
#[tokio::test]
async fn three_named_artists_count_distinct_and_keep_the_lowest_guid() {
    let crawl_token = "adr0049-artist-count-three-token";
    let db = common::test_db_arc();
    let state = app_state_with_chain(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let p_guid = "artist-count-p-three";
    let p_url = "https://example.com/artist-count/p-three.xml";
    let album_one_guid = "artist-count-album-1-x";
    let album_one_url = "https://example.com/artist-count/album-1-x.xml";
    let album_two_guid = "artist-count-album-2-x-spaced";
    let album_two_url = "https://example.com/artist-count/album-2-x-spaced.xml";
    let album_three_guid = "artist-count-album-3-y";
    let album_three_url = "https://example.com/artist-count/album-3-y.xml";

    let p_payload = serde_json::json!({
        "feed_guid": p_guid,
        "title": "Artist Count Publisher Three",
        "raw_medium": "publisher",
        "explicit": false,
        "remote_items": [{
            "position": 0,
            "medium": "music",
            "remote_feed_guid": album_one_guid,
            "remote_feed_url": album_one_url
        }]
    });
    ingest(
        app.clone(),
        &ingest_payload(p_url, p_url, crawl_token, "p-three-hash", &p_payload),
    )
    .await;

    for (guid, url, author) in [
        (album_one_guid, album_one_url, "X"),
        (album_two_guid, album_two_url, " x "),
        (album_three_guid, album_three_url, "Y"),
    ] {
        let album_payload = serde_json::json!({
            "feed_guid": guid,
            "title": format!("Album {author}"),
            "raw_medium": "music",
            "explicit": false,
            "author_name": author,
            "remote_items": [{
                "position": 0,
                "medium": "publisher",
                "remote_feed_guid": p_guid,
                "remote_feed_url": p_url
            }]
        });
        ingest(
            app.clone(),
            &ingest_payload(
                url,
                url,
                crawl_token,
                &format!("{guid}-hash"),
                &album_payload,
            ),
        )
        .await;
    }

    let body = get_feed(app, p_guid).await;
    assert_eq!(
        body["data"]["distinct_release_artist_count"],
        serde_json::json!(2),
        "\"X\" and \" x \" must normalize to one value, \"Y\" to another: {body:?}"
    );
    assert_eq!(
        body["data"]["distinct_release_artists"],
        serde_json::json!(["X", "Y"]),
        "the list must give the raw value of the lower-feed_guid album in \
         each group, sorted by the normalized value: {body:?}"
    );
}

/// An album whose `release_artist_source` is `"itunes_owner"` — no
/// `itunes:author`, and a non-platform `itunes:owner` name — is not
/// counted.
#[tokio::test]
async fn itunes_owner_album_is_not_counted() {
    let crawl_token = "adr0049-artist-count-owner-token";
    let db = common::test_db_arc();
    let state = app_state_with_chain(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let p_guid = "artist-count-p-owner";
    let p_url = "https://example.com/artist-count/p-owner.xml";
    let author_album_guid = "artist-count-owner-album-author";
    let author_album_url = "https://example.com/artist-count/owner-album-author.xml";
    let owner_album_guid = "artist-count-owner-album-owner-only";
    let owner_album_url = "https://example.com/artist-count/owner-album-owner-only.xml";

    let p_payload = serde_json::json!({
        "feed_guid": p_guid,
        "title": "Artist Count Publisher Owner",
        "raw_medium": "publisher",
        "explicit": false,
        "remote_items": [{
            "position": 0,
            "medium": "music",
            "remote_feed_guid": author_album_guid,
            "remote_feed_url": author_album_url
        }]
    });
    ingest(
        app.clone(),
        &ingest_payload(p_url, p_url, crawl_token, "p-owner-hash", &p_payload),
    )
    .await;

    let author_album_payload = serde_json::json!({
        "feed_guid": author_album_guid,
        "title": "Album With Author",
        "raw_medium": "music",
        "explicit": false,
        "author_name": "Solo Artist",
        "remote_items": [{
            "position": 0,
            "medium": "publisher",
            "remote_feed_guid": p_guid,
            "remote_feed_url": p_url
        }]
    });
    ingest(
        app.clone(),
        &ingest_payload(
            author_album_url,
            author_album_url,
            crawl_token,
            "author-album-hash",
            &author_album_payload,
        ),
    )
    .await;

    // "Indie Collective" is the non-platform owner name `src/api.rs`'s own
    // unit tests use for the same fallback rule (task 007's
    // `derive_release_artist`), so this reuses a known non-platform name.
    let owner_album_payload = serde_json::json!({
        "feed_guid": owner_album_guid,
        "title": "Album With Owner Only",
        "raw_medium": "music",
        "explicit": false,
        "owner_name": "Indie Collective",
        "remote_items": [{
            "position": 0,
            "medium": "publisher",
            "remote_feed_guid": p_guid,
            "remote_feed_url": p_url
        }]
    });
    ingest(
        app.clone(),
        &ingest_payload(
            owner_album_url,
            owner_album_url,
            crawl_token,
            "owner-album-hash",
            &owner_album_payload,
        ),
    )
    .await;

    let body = get_feed(app, p_guid).await;
    assert_eq!(
        body["data"]["distinct_release_artist_count"],
        serde_json::json!(1),
        "the itunes_owner album must not count: {body:?}"
    );
    assert_eq!(
        body["data"]["distinct_release_artists"],
        serde_json::json!(["Solo Artist"]),
        "only the itunes_author album's raw value must appear: {body:?}"
    );
}

/// An album that names the publisher only through a `feed_url` observation
/// is counted. The album's declared `remote_feed_guid` is not indexed, but
/// its declared `remote_feed_url` matches a URL the node observed for the
/// publisher (its `source_url`, recorded because it differs from the
/// `canonical_url` at ingest, ADR 0049 §1).
#[tokio::test]
async fn feed_url_observation_album_is_counted() {
    let crawl_token = "adr0049-artist-count-feed-url-token";
    let db = common::test_db_arc();
    let state = app_state_with_chain(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let r_guid = "artist-count-r";
    let r_canonical_url = "https://example.com/artist-count/r-canonical.xml";
    let r_source_url = "https://example.com/artist-count/r-source.xml";
    let z_guid = "artist-count-z";
    let z_url = "https://example.com/artist-count/z.xml";
    let z_bogus_remote_guid = "artist-count-z-bogus-remote-guid";

    let r_payload = serde_json::json!({
        "feed_guid": r_guid,
        "title": "Artist Count Publisher R",
        "raw_medium": "publisher",
        "explicit": false,
        "remote_items": [{
            "position": 0,
            "medium": "music",
            "remote_feed_guid": z_guid,
            "remote_feed_url": z_url
        }]
    });
    ingest(
        app.clone(),
        &ingest_payload(
            r_canonical_url,
            r_source_url,
            crawl_token,
            "r-hash",
            &r_payload,
        ),
    )
    .await;

    let z_payload = serde_json::json!({
        "feed_guid": z_guid,
        "title": "Album Z",
        "raw_medium": "music",
        "explicit": false,
        "author_name": "Feed Url Artist",
        "remote_items": [{
            "position": 0,
            "medium": "publisher",
            "remote_feed_guid": z_bogus_remote_guid,
            "remote_feed_url": r_source_url
        }]
    });
    ingest(
        app.clone(),
        &ingest_payload(z_url, z_url, crawl_token, "z-hash", &z_payload),
    )
    .await;

    let body = get_feed(app, r_guid).await;
    assert_eq!(
        body["data"]["distinct_release_artist_count"],
        serde_json::json!(1),
        "an album resolved through a feed_url observation must count: {body:?}"
    );
    assert_eq!(
        body["data"]["distinct_release_artists"],
        serde_json::json!(["Feed Url Artist"]),
        "the counted album's raw release_artist must appear: {body:?}"
    );
}

/// A music feed read has neither field: the keys are absent, not null.
#[tokio::test]
async fn music_feed_read_has_neither_field() {
    let crawl_token = "adr0049-artist-count-music-token";
    let db = common::test_db_arc();
    let state = app_state_with_chain(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "artist-count-plain-music";
    let feed_url = "https://example.com/artist-count/plain-music.xml";
    let payload = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Plain Music Feed",
        "raw_medium": "music",
        "explicit": false,
        "author_name": "Some Artist"
    });
    ingest(
        app.clone(),
        &ingest_payload(
            feed_url,
            feed_url,
            crawl_token,
            "plain-music-hash",
            &payload,
        ),
    )
    .await;

    let body = get_feed(app, feed_guid).await;
    let data = body["data"].as_object().expect("data must be an object");
    assert!(
        !data.contains_key("distinct_release_artist_count"),
        "a music feed read must not have distinct_release_artist_count: {body:?}"
    );
    assert!(
        !data.contains_key("distinct_release_artists"),
        "a music feed read must not have distinct_release_artists: {body:?}"
    );
}

/// A publisher feed with no album gives `0` and `[]`, not a missing field.
///
/// The publisher still declares one `medium="music"` remote item, to a guid
/// that is never ingested, so the ingest-time `medium_music` verifier
/// accepts it. No music feed names it back, so no album counts.
#[tokio::test]
async fn publisher_with_no_album_gives_zero_and_empty_list() {
    let crawl_token = "adr0049-artist-count-empty-token";
    let db = common::test_db_arc();
    let state = app_state_with_chain(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let p_guid = "artist-count-p-empty";
    let p_url = "https://example.com/artist-count/p-empty.xml";
    let payload = serde_json::json!({
        "feed_guid": p_guid,
        "title": "Artist Count Publisher With No Album",
        "raw_medium": "publisher",
        "explicit": false,
        "remote_items": [{
            "position": 0,
            "medium": "music",
            "remote_feed_guid": "artist-count-p-empty-never-ingested",
            "remote_feed_url": "https://example.com/artist-count/never-ingested.xml"
        }]
    });
    ingest(
        app.clone(),
        &ingest_payload(p_url, p_url, crawl_token, "p-empty-hash", &payload),
    )
    .await;

    let body = get_feed(app, p_guid).await;
    assert_eq!(
        body["data"]["distinct_release_artist_count"],
        serde_json::json!(0),
        "a publisher feed with no album must give 0: {body:?}"
    );
    assert_eq!(
        body["data"]["distinct_release_artists"],
        serde_json::json!([]),
        "a publisher feed with no album must give an empty list: {body:?}"
    );
}
