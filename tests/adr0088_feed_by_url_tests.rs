//! ADR 0088: `GET /v1/feeds/by-url` gives the feed that one RSS URL names.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use rusqlite::params;
use stophammer::db;
use stophammer::model::Feed;
use tower::ServiceExt;

fn app(db: Arc<Mutex<rusqlite::Connection>>) -> axum::Router {
    let signer = Arc::new(common::temp_signer("test-adr0088-by-url"));
    let pubkey = signer.pubkey_hex().to_string();
    stophammer::api::build_router(Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(
            "test-token".into(),
            vec![],
        )),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    }))
}

/// Sends `GET` to `path` and returns the status and the JSON body.
async fn get(db: &Arc<Mutex<rusqlite::Connection>>, path: &str) -> (u16, serde_json::Value) {
    let req = Request::builder()
        .method("GET")
        .uri(path)
        .body(axum::body::Body::empty())
        .expect("build request");
    let resp = app(Arc::clone(db))
        .oneshot(req)
        .await
        .expect("call handler");
    let status = resp.status().as_u16();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, body)
}

/// Sends `GET /v1/feeds/by-url` with `url`.
async fn by_url(db: &Arc<Mutex<rusqlite::Connection>>, url: &str) -> (u16, serde_json::Value) {
    let query: String = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("url", url)
        .finish();
    get(db, &format!("/v1/feeds/by-url?{query}")).await
}

fn feed(guid: &str, feed_url: &str) -> Feed {
    Feed {
        feed_guid: guid.into(),
        feed_url: feed_url.into(),
        title: format!("Title {guid}"),
        title_lower: format!("title {guid}"),
        description: None,
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        itunes_type: None,
        release_artist: Some("Artist".into()),
        release_artist_sort: None,
        release_date: None,
        release_kind: None,
        episode_count: 0,
        newest_item_at: None,
        oldest_item_at: None,
        created_at: 1_000_000,
        updated_at: 1_000_000,
        raw_medium: Some("music".into()),
        last_build_date: None,
        release_artist_source: None,
    }
}

fn put_feed(conn: &rusqlite::Connection, guid: &str, feed_url: &str) {
    db::upsert_feed(conn, &feed(guid, feed_url)).expect("write feed");
}

fn put_copy(conn: &rusqlite::Connection, guid: &str, url: &str) {
    conn.execute(
        "INSERT INTO feed_copies (feed_guid, url, first_seen, title, item_guids, \
         feed_recipients, track_recipients, summary_digest) \
         VALUES (?1, ?2, 1000, 'Copy', '[]', '[]', '{}', 'digest')",
        params![guid, url],
    )
    .expect("write copy row");
}

/// The `feed_guid` and `match` of a 200 body.
fn found(status: u16, body: &serde_json::Value) -> (String, String) {
    assert_eq!(status, 200, "expected a feed, got {body}");
    (
        body["data"]["feed_guid"]
            .as_str()
            .expect("feed_guid")
            .to_owned(),
        body["data"]["match"].as_str().expect("match").to_owned(),
    )
}

#[tokio::test]
async fn the_feed_url_gives_the_feed_and_its_href() {
    let db = common::test_db_arc();
    put_feed(
        &db.lock().expect("lock"),
        "f-1",
        "https://example.com/music/feed.xml",
    );

    let (status, body) = by_url(&db, "https://example.com/music/feed.xml").await;
    assert_eq!(
        found(status, &body),
        ("f-1".to_owned(), "feed_url".to_owned()),
        "ADR 0088 §2: the feed_url names its feed"
    );
    assert_eq!(body["data"]["href"], "/v1/feeds/f-1", "ADR 0088 §3: href");
    assert_eq!(
        body["data"]["matched_url"], "https://example.com/music/feed.xml",
        "ADR 0088 §3: the stored URL that matched"
    );
}

#[tokio::test]
async fn the_scheme_the_host_case_a_slash_a_fragment_and_white_space_do_not_matter() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        put_feed(&conn, "f-https", "https://example.com/a/feed.xml");
        put_feed(&conn, "f-slash", "https://example.com/b/");
    }

    for url in [
        "http://example.com/a/feed.xml",
        "HTTPS://EXAMPLE.COM/a/feed.xml",
        "https://example.com:443/a/feed.xml",
        "https://example.com/a/feed.xml/",
        "https://example.com/a/feed.xml#top",
        "  https://example.com/a/feed.xml\n",
    ] {
        let (status, body) = by_url(&db, url).await;
        assert_eq!(
            found(status, &body).0,
            "f-https",
            "ADR 0088 §1: {url:?} names the stored URL"
        );
    }
    let (status, body) = by_url(&db, "http://example.com/b").await;
    assert_eq!(
        found(status, &body).0,
        "f-slash",
        "ADR 0088 §1: a slash added and the other scheme"
    );
}

#[tokio::test]
async fn a_query_string_and_the_path_case_stay_part_of_the_url() {
    let db = common::test_db_arc();
    put_feed(
        &db.lock().expect("lock"),
        "f-q",
        "https://example.com/feed?id=7",
    );

    let (status, _) = by_url(&db, "https://example.com/feed?id=7").await;
    assert_eq!(status, 200, "ADR 0088 §1: the same query string matches");
    for url in [
        "https://example.com/feed?id=8",
        "https://example.com/feed",
        "https://example.com/FEED?id=7",
    ] {
        let (status, body) = by_url(&db, url).await;
        assert_eq!(
            status, 404,
            "ADR 0088 §1: {url} is a different URL, got {body}"
        );
    }
}

#[tokio::test]
async fn an_observed_url_gives_its_feed() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        put_feed(
            &conn,
            "f-wl",
            "https://wavlake.com/feed/music/0d1aa48a-3e7b-4d0a-ad8c-7d47e57b1a6f",
        );
        db::upsert_feed_url_observation(
            &conn,
            "https://wavlake.com/feed/0d1aa48a-3e7b-4d0a-ad8c-7d47e57b1a6f",
            "f-wl",
            1000,
        )
        .expect("write observation");
    }

    let (status, body) = by_url(
        &db,
        "https://wavlake.com/feed/0d1aa48a-3e7b-4d0a-ad8c-7d47e57b1a6f",
    )
    .await;
    assert_eq!(
        found(status, &body),
        ("f-wl".to_owned(), "observed_url".to_owned()),
        "ADR 0088 §2: an observed URL names the feed of its GUID"
    );
}

#[tokio::test]
async fn a_copy_url_gives_the_indexed_record() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        put_feed(&conn, "f-src", "https://source.example.com/feed.xml");
        put_copy(&conn, "f-src", "https://mirror.example.com/feed.xml");
        db::upsert_feed_url_observation(
            &conn,
            "https://mirror.example.com/feed.xml",
            "f-src",
            1000,
        )
        .expect("write observation");
    }

    let (status, body) = by_url(&db, "https://mirror.example.com/feed.xml").await;
    assert_eq!(
        found(status, &body),
        ("f-src".to_owned(), "copy".to_owned()),
        "ADR 0088 §2: a copy URL names the record, and copy comes before observed_url"
    );
}

#[tokio::test]
async fn a_feed_url_comes_before_an_observation_of_another_feed() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        put_feed(&conn, "f-now", "https://example.com/shared.xml");
        put_feed(&conn, "f-old", "https://example.com/old.xml");
        db::upsert_feed_url_observation(&conn, "https://example.com/shared.xml", "f-old", 1)
            .expect("write observation");
    }

    let (status, body) = by_url(&db, "https://example.com/shared.xml").await;
    assert_eq!(
        found(status, &body),
        ("f-now".to_owned(), "feed_url".to_owned()),
        "ADR 0088 §2: the feed_url of a feed comes first"
    );
}

#[tokio::test]
async fn the_form_as_given_comes_before_another_form() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        put_feed(&conn, "f-http", "http://example.com/feed.xml");
        put_feed(&conn, "f-https", "https://example.com/feed.xml");
    }

    let (status, body) = by_url(&db, "http://example.com/feed.xml").await;
    assert_eq!(
        found(status, &body).0,
        "f-http",
        "ADR 0088 §1: the URL as given comes before the other scheme"
    );
}

#[tokio::test]
async fn a_retired_guid_gives_404_with_superseded_by() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        put_feed(&conn, "f-new", "https://example.com/new.xml");
        db::upsert_feed_url_observation(&conn, "https://example.com/gone.xml", "f-gone", 1)
            .expect("write observation");
        conn.execute(
            "INSERT INTO feed_guid_supersessions (old_guid, new_guid, source_url, superseded_at) \
             VALUES ('f-gone', 'f-new', 'https://example.com/gone.xml', 2)",
            [],
        )
        .expect("write supersession");
    }

    let (status, body) = by_url(&db, "https://example.com/gone.xml").await;
    assert_eq!(status, 404, "ADR 0088 §2: the node holds no such feed");
    assert_eq!(
        body,
        serde_json::json!({ "error": "feed not found", "superseded_by": "f-new" }),
        "ADR 0088 §2: the 404 has the form of the feed read"
    );
}

#[tokio::test]
async fn an_unknown_url_gives_404() {
    let db = common::test_db_arc();
    let (status, body) = by_url(&db, "https://example.com/none.xml").await;
    assert_eq!(status, 404, "ADR 0088 §2: no feed has the URL");
    assert_eq!(
        body["error"], "feed not found",
        "ADR 0088 §2: the 404 of the feed read"
    );
    assert!(
        body.get("superseded_by").is_none(),
        "no GUID change applies, got {body}"
    );
}

#[tokio::test]
async fn an_invalid_url_gives_400() {
    let db = common::test_db_arc();
    let (status, _) = get(&db, "/v1/feeds/by-url").await;
    assert_eq!(status, 400, "ADR 0088 §1: url is required");
    for url in [
        "",
        "example.com/feed.xml",
        "ftp://example.com/feed.xml",
        "https://user:secret@example.com/feed.xml",
        &format!("https://example.com/{}", "a".repeat(2048)),
    ] {
        let (status, _) = by_url(&db, url).await;
        assert_eq!(status, 400, "ADR 0088 §1: {url:.40} is not valid");
    }
}

#[tokio::test]
async fn the_capabilities_name_the_route() {
    let db = common::test_db_arc();
    let (status, body) = get(&db, "/v1/node/capabilities").await;
    assert_eq!(status, 200, "capabilities answer");
    assert!(
        body["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|flag| flag == "feed_by_url"),
        "ADR 0088 §4: the flag feed_by_url, got {body}"
    );
}

const WL_ID: &str = "0d1aa48a-3e7b-4d0a-ad8c-7d47e57b1a6f";

fn wl_music() -> String {
    format!("https://wavlake.com/feed/music/{WL_ID}")
}

#[tokio::test]
async fn the_wavlake_short_url_finds_the_music_url_with_no_observation() {
    let db = common::test_db_arc();
    put_feed(&db.lock().expect("lock"), "f-wl", &wl_music());

    let (status, body) = by_url(&db, &format!("https://wavlake.com/feed/{WL_ID}")).await;
    assert_eq!(
        found(status, &body),
        ("f-wl".to_owned(), "feed_url".to_owned()),
        "ADR 0088 §1a: the short URL names the feed of the music URL"
    );
    assert_eq!(
        body["data"]["matched_url"],
        wl_music(),
        "ADR 0088 §1a: matched_url is the stored music URL"
    );
}

#[tokio::test]
async fn the_wavlake_alias_ignores_the_scheme_a_slash_and_the_host_case() {
    let db = common::test_db_arc();
    put_feed(&db.lock().expect("lock"), "f-wl", &wl_music());

    for url in [
        format!("http://wavlake.com/feed/{WL_ID}"),
        format!("https://wavlake.com/feed/{WL_ID}/"),
        format!("https://WAVLAKE.COM/feed/{WL_ID}"),
    ] {
        let (status, body) = by_url(&db, &url).await;
        assert_eq!(
            found(status, &body),
            ("f-wl".to_owned(), "feed_url".to_owned()),
            "ADR 0088 §1a: {url} finds the music URL, got {body}"
        );
    }
}

#[tokio::test]
async fn the_wavlake_alias_comes_after_an_observation_of_the_short_url() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        put_feed(&conn, "f-a", "https://example.com/a.xml");
        put_feed(&conn, "f-b", &wl_music());
        db::upsert_feed_url_observation(
            &conn,
            &format!("https://wavlake.com/feed/{WL_ID}"),
            "f-a",
            1000,
        )
        .expect("write observation");
    }

    let (status, body) = by_url(&db, &format!("https://wavlake.com/feed/{WL_ID}")).await;
    assert_eq!(
        found(status, &body),
        ("f-a".to_owned(), "observed_url".to_owned()),
        "ADR 0088 §1a: the alias form is last, so the observation matches first"
    );
}

#[tokio::test]
async fn the_wavlake_alias_does_not_apply_to_another_host_or_path() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        put_feed(&conn, "f-wl", &wl_music());
        put_feed(
            &conn,
            "f-ex",
            &format!("https://example.com/feed/music/{WL_ID}"),
        );
    }

    for url in [
        format!("https://www.wavlake.com/feed/{WL_ID}"),
        format!("https://feeds.wavlake.com/feed/{WL_ID}"),
        "https://wavlake.com/feed/not-a-uuid".to_owned(),
        format!("https://wavlake.com/feed/{WL_ID}/extra"),
        format!("https://example.com/feed/{WL_ID}"),
    ] {
        let (status, body) = by_url(&db, &url).await;
        assert_eq!(status, 404, "ADR 0088 §1a: {url} has no alias, got {body}");
    }
}
