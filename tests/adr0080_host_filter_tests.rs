//! ADR 0080 §5, the `host` row of §2 and `not_host` of §3: `feeds.feed_host`
//! and the host filters of `GET /v1/search`.

mod common;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const CRAWL_TOKEN: &str = "adr0080-host-crawl-token";
const ADMIN_TOKEN: &str = "adr0080-host-admin-token";

fn app(db: Arc<Mutex<rusqlite::Connection>>) -> axum::Router {
    let signer = Arc::new(common::temp_signer("test-adr0080-host-filter"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec {
        names: vec!["content_hash".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, CRAWL_TOKEN.to_string());
    stophammer::api::build_router(Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: ADMIN_TOKEN.into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    }))
}

/// Sends a request and returns the status and the JSON body.
async fn send(
    db: &Arc<Mutex<rusqlite::Connection>>,
    method: &str,
    uri: &str,
    admin: bool,
    body: Option<&serde_json::Value>,
) -> (u16, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if admin {
        builder = builder.header("X-Admin-Token", ADMIN_TOKEN);
    }
    let request = match body {
        Some(body) => builder
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(body).expect("serialize")))
            .expect("build request"),
        None => builder.body(Body::empty()).expect("build request"),
    };
    let resp = app(Arc::clone(db))
        .oneshot(request)
        .await
        .expect("call handler");
    let status = resp.status().as_u16();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// The sorted `entity_id` values of a `200` response.
async fn ids(db: &Arc<Mutex<rusqlite::Connection>>, uri: &str) -> Vec<String> {
    let (status, body) = send(db, "GET", uri, false, None).await;
    assert_eq!(status, 200, "{uri} must give 200, got {body}");
    let mut ids: Vec<String> = body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|row| row["entity_id"].as_str().expect("entity_id").to_owned())
        .collect();
    ids.sort();
    ids
}

fn stored_host(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> Option<String> {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT feed_host FROM feeds WHERE feed_guid = ?1",
        [feed_guid],
        |row| row.get(0),
    )
    .expect("read feed_host")
}

fn feed_data(feed_guid: &str) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Host Feed",
        "raw_medium": "music",
        "explicit": false,
        "tracks": [{"track_guid": "host-track", "title": "Track", "explicit": false}]
    })
}

/// Ingests a body at `url` and returns the response body.
async fn ingest(
    db: &Arc<Mutex<rusqlite::Connection>>,
    url: &str,
    hash: &str,
    guid: &str,
) -> serde_json::Value {
    let payload = serde_json::json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": CRAWL_TOKEN,
        "http_status": 200,
        "content_hash": hash,
        "feed_data": feed_data(guid),
    });
    let (status, body) = send(db, "POST", "/ingest/feed", false, Some(&payload)).await;
    assert_eq!(status, 200, "{body}");
    body
}

// ---------------------------------------------------------------------------
// The host function
// ---------------------------------------------------------------------------

#[test]
fn feed_host_of_gives_the_lower_case_host_or_the_empty_text() {
    assert_eq!(
        stophammer::db::feed_host_of("https://Feeds.Wavlake.com/feed/x"),
        "feeds.wavlake.com",
        "the host is in lower case"
    );
    assert_eq!(
        stophammer::db::feed_host_of("http://user@example.com:8080/a"),
        "example.com",
        "the user and the port are not part of the host"
    );
    assert_eq!(
        stophammer::db::feed_host_of("not a url"),
        "",
        "a text that is not a URL has no host"
    );
    assert_eq!(
        stophammer::db::feed_host_of("https://example.com./a"),
        "example.com",
        "the host has no trailing dot"
    );
    assert_eq!(
        stophammer::db::feed_host_of("mailto:a@example.com"),
        "",
        "a URL with no host gives the empty text"
    );
}

// ---------------------------------------------------------------------------
// The writers (check 11)
// ---------------------------------------------------------------------------

fn sample_feed(feed_guid: &str, feed_url: &str) -> stophammer::model::Feed {
    let now = common::now();
    stophammer::model::Feed {
        feed_guid: feed_guid.into(),
        feed_url: feed_url.into(),
        title: "Host Feed".into(),
        title_lower: "host feed".into(),
        description: None,
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        itunes_type: None,
        release_artist: None,
        release_artist_sort: None,
        release_date: None,
        release_kind: None,
        episode_count: 0,
        newest_item_at: None,
        oldest_item_at: None,
        created_at: now,
        updated_at: now,
        raw_medium: Some("music".into()),
        last_build_date: None,
        release_artist_source: None,
    }
}

#[test]
fn upsert_feed_sets_feed_host_from_the_new_feed_url() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        stophammer::db::upsert_feed(
            &conn,
            &sample_feed("host-upsert", "https://A.example.com/x"),
        )
        .expect("insert");
    }
    assert_eq!(
        stored_host(&db, "host-upsert").as_deref(),
        Some("a.example.com"),
        "an insert must set feed_host"
    );
    {
        let conn = db.lock().expect("lock");
        stophammer::db::upsert_feed(
            &conn,
            &sample_feed("host-upsert", "https://b.example.org/x"),
        )
        .expect("update");
    }
    assert_eq!(
        stored_host(&db, "host-upsert").as_deref(),
        Some("b.example.org"),
        "an update must set feed_host from the new feed_url"
    );
}

#[tokio::test]
async fn ingest_of_a_new_feed_sets_feed_host_and_relocation_sets_it_again() {
    let db = common::test_db_arc();
    let guid = "host-ingest";
    let first = ingest(&db, "https://old.example.com/a.xml", "hash-a", guid).await;
    assert_eq!(first["accepted"].as_bool(), Some(true), "accepted: {first}");
    assert_eq!(
        stored_host(&db, guid).as_deref(),
        Some("old.example.com"),
        "an ingest of a new feed must set feed_host"
    );

    // A second body at another URL is a copy, not accepted. The operator
    // relocates to it.
    ingest(&db, "https://new.example.net/b.xml", "hash-b", guid).await;
    let (status, body) = send(
        &db,
        "POST",
        &format!("/v1/feeds/{guid}/copies/resolve"),
        true,
        Some(&serde_json::json!({
            "url": "https://new.example.net/b.xml",
            "decision": "relocate",
            "reason": "the feed moved"
        })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        stored_host(&db, guid).as_deref(),
        Some("new.example.net"),
        "a relocation must set feed_host from the new feed_url"
    );
}

// ---------------------------------------------------------------------------
// The repairs (check 11)
// ---------------------------------------------------------------------------

fn seed_hosts(conn: &rusqlite::Connection) {
    for (guid, url) in [
        ("r1", "https://Feeds.Wavlake.com/a"),
        ("r2", "https://example.com/b"),
        ("r3", "not a url"),
    ] {
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, created_at, updated_at) \
             VALUES (?1, ?2, ?1, ?1, 1, 1)",
            rusqlite::params![guid, url],
        )
        .expect("insert feed");
    }
}

fn hosts(conn: &rusqlite::Connection) -> Vec<(String, Option<String>)> {
    conn.prepare("SELECT feed_guid, feed_host FROM feeds ORDER BY feed_guid")
        .expect("prepare")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("collect")
}

#[test]
fn open_db_fills_each_null_feed_host_and_a_second_open_writes_no_row() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("host-fill.db");
    {
        let conn = stophammer::db::open_db(&path);
        seed_hosts(&conn);
        assert!(
            hosts(&conn).iter().all(|(_, host)| host.is_none()),
            "the seed writes no feed_host"
        );
    }

    let conn = stophammer::db::open_db(&path);
    assert_eq!(
        hosts(&conn),
        vec![
            ("r1".to_owned(), Some("feeds.wavlake.com".to_owned())),
            ("r2".to_owned(), Some("example.com".to_owned())),
            ("r3".to_owned(), Some(String::new())),
        ],
        "open_db must give each row a host, and the empty text for no host"
    );
    conn.execute(
        "UPDATE feeds SET feed_host = 'kept' WHERE feed_guid IN ('r1', 'r3')",
        [],
    )
    .expect("mark rows");
    drop(conn);

    let conn = stophammer::db::open_db(&path);
    assert_eq!(
        hosts(&conn),
        vec![
            ("r1".to_owned(), Some("kept".to_owned())),
            ("r2".to_owned(), Some("example.com".to_owned())),
            ("r3".to_owned(), Some("kept".to_owned())),
        ],
        "a second open must write no row, also for the empty host"
    );
}

#[test]
fn a_database_past_migration_0057_with_no_feed_host_column_is_repaired() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("host-repair.db");
    {
        let conn = stophammer::db::open_db(&path);
        seed_hosts(&conn);
        // The watermark records version 51 (migration 0057) already, and the
        // column is missing (ADR 0046).
        conn.execute_batch("ALTER TABLE feeds DROP COLUMN feed_host;")
            .expect("drop the column");
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .expect("read version");
        assert!(version >= 51, "the watermark must reach migration 0057");
    }

    let conn = stophammer::db::open_db(&path);
    assert_eq!(
        hosts(&conn),
        vec![
            ("r1".to_owned(), Some("feeds.wavlake.com".to_owned())),
            ("r2".to_owned(), Some("example.com".to_owned())),
            ("r3".to_owned(), Some(String::new())),
        ],
        "open_db must add feed_host and fill each row"
    );
}

// ---------------------------------------------------------------------------
// The filters (checks 10 and 12)
// ---------------------------------------------------------------------------

/// Feeds `f1` to `f4` with tracks `t1` to `t4`, and publishers `p1` and `p2`.
/// Each indexed row has the word `common`.
///
/// | Row | Host |
/// |---|---|
/// | `f1`, `p1` | `wavlake.com` |
/// | `f2` | `feeds.wavlake.com` |
/// | `f3`, `p2` | `notwavlake.com` |
/// | `f4` | none (empty text) |
fn seeded() -> Arc<Mutex<rusqlite::Connection>> {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        let rows = [
            ("f1", "https://wavlake.com/a"),
            ("f2", "https://Feeds.Wavlake.com/b"),
            ("f3", "https://notwavlake.com/c"),
            ("f4", "not a url"),
            ("p1", "https://wavlake.com/p"),
            ("p2", "https://notwavlake.com/p"),
        ];
        for (feed, url) in rows {
            conn.execute(
                "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, feed_host, \
                 created_at, updated_at) VALUES (?1, ?2, ?1, ?1, ?3, 1, 1)",
                rusqlite::params![feed, url, stophammer::db::feed_host_of(url)],
            )
            .expect("insert feed");
            if feed.starts_with('p') {
                stophammer::search::populate_search_index(
                    &conn,
                    "publisher",
                    feed,
                    feed,
                    feed,
                    "common",
                )
                .expect("populate publisher");
                continue;
            }
            let track = format!("t{}", &feed[1..]);
            conn.execute(
                "INSERT INTO tracks (track_guid, feed_guid, title, title_lower, created_at, \
                 updated_at) VALUES (?1, ?2, ?1, ?1, 1, 1)",
                rusqlite::params![track, feed],
            )
            .expect("insert track");
            stophammer::search::populate_search_index(&conn, "feed", feed, feed, feed, "common")
                .expect("populate feed");
            stophammer::search::populate_search_index(
                &conn,
                "track",
                &stophammer::db::canonical_track_entity_id(feed, &track),
                feed,
                &track,
                "common",
            )
            .expect("populate track");
        }
    }
    db
}

#[tokio::test]
async fn host_matches_the_host_and_its_subdomains_only() {
    let db = seeded();
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=feed&host=wavlake.com").await,
        vec!["f1", "f2"],
        "host=wavlake.com must match wavlake.com and feeds.wavlake.com, not notwavlake.com"
    );
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=feed&host=feeds.wavlake.com").await,
        vec!["f2"],
        "a subdomain value must not match its parent"
    );
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=track&host=wavlake.com").await,
        vec!["t1", "t2"],
        "a track must match by the host of its feed"
    );
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=publisher&host=wavlake.com").await,
        vec!["p1"],
        "a publisher must match by its own host"
    );
    assert_eq!(
        ids(&db, "/v1/search?q=common&host=wavlake.com").await.len(),
        4,
        "a request with no type must apply host to feeds and tracks"
    );
}

#[tokio::test]
async fn host_ignores_case_and_a_trailing_dot() {
    let db = seeded();
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=feed&host=Wavlake.COM.").await,
        ids(&db, "/v1/search?q=common&type=feed&host=wavlake.com").await,
        "host=Wavlake.COM. must match the same feeds as host=wavlake.com"
    );
}

#[tokio::test]
async fn not_host_removes_the_host_and_keeps_the_empty_host() {
    let db = seeded();
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=feed&not_host=wavlake.com").await,
        vec!["f3", "f4"],
        "not_host=wavlake.com must remove wavlake.com and its subdomains, and keep the empty host"
    );
    assert_eq!(
        ids(
            &db,
            "/v1/search?q=common&type=feed&not_host=wavlake.com&not_host=notwavlake.com"
        )
        .await,
        vec!["f4"],
        "not_host can repeat"
    );
    assert_eq!(
        ids(
            &db,
            "/v1/search?q=common&type=publisher&not_host=wavlake.com"
        )
        .await,
        vec!["p2"],
        "not_host applies to a publisher"
    );
}

#[tokio::test]
async fn not_host_counts_toward_the_limit_of_ten_exclusions() {
    let db = seeded();
    let mut query = String::from("/v1/search?q=common&type=feed");
    for n in 0..5 {
        write!(query, "&not_host=h{n}.example&not_title=x{n}").expect("write query");
    }
    let (status, body) = send(&db, "GET", &query, false, None).await;
    assert_eq!(status, 200, "ten exclusions must pass: {body}");
    query.push_str("&not_host=last.example");
    let (status, body) = send(&db, "GET", &query, false, None).await;
    assert_eq!(status, 400, "the 11th exclusion must fail: {body}");
    assert_eq!(body["code"], "invalid_filter", "code: {body}");
    assert_eq!(body["filter"], "not_host", "filter: {body}");
}

#[tokio::test]
async fn a_host_value_in_the_wrong_form_gives_invalid_filter() {
    let db = seeded();
    for uri in [
        "/v1/search?q=common&type=feed&host=",
        "/v1/search?q=common&type=feed&host=https%3A%2F%2Fwavlake.com",
        "/v1/search?q=common&type=feed&host=wavlake.com%2Fx",
        "/v1/search?q=common&type=feed&host=wavlake.com%3A443",
        "/v1/search?q=common&type=feed&host=a%40b.com",
        "/v1/search?q=common&type=feed&host=a%20b",
    ] {
        let (status, body) = send(&db, "GET", uri, false, None).await;
        assert_eq!(status, 400, "{uri} must give 400, got {body}");
        assert_eq!(body["code"], "invalid_filter", "{uri}: {body}");
        assert_eq!(body["filter"], "host", "{uri}: {body}");
    }
    let (status, body) = send(
        &db,
        "GET",
        "/v1/search?q=common&type=feed&host=a&host=b",
        false,
        None,
    )
    .await;
    assert_eq!(status, 400, "host cannot repeat: {body}");
    assert_eq!(body["code"], "invalid_filter", "{body}");
}
