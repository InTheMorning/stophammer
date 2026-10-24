//! ADR 0078 §3: a retire deletes each index row of the feed, and a track
//! removal deletes the index row of the track.

mod common;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const CRAWL_TOKEN: &str = "adr0078-retire-crawl-token";
const ADMIN_TOKEN: &str = "adr0078-retire-admin-token";
const GONE_HOST: &str = "gone-adr0078.example.com";
const ONE_DAY_SECS: i64 = 24 * 60 * 60;

/// The GUID namespace that the ADR 0058 §1b rule derives a GUID from a URL with.
const GUID_ORIGIN_NAMESPACE: uuid::Uuid = uuid::uuid!("ead4c236-bf58-58c6-a2c6-a6b28d128cb6");

type Db = Arc<Mutex<rusqlite::Connection>>;

fn uuidv5_of_url(url: &str) -> String {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let name = without_scheme.strip_suffix('/').unwrap_or(without_scheme);
    uuid::Uuid::new_v5(&GUID_ORIGIN_NAMESPACE, name.as_bytes()).to_string()
}

fn state(db: Db) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0078-retire-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec {
        names: vec!["content_hash".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, CRAWL_TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: ADMIN_TOKEN.into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: vec![GONE_HOST.to_string()],
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

/// The medium of a feed that the index holds, with the words that find it.
#[derive(Clone, Copy)]
struct Case {
    medium: &'static str,
    title: &'static str,
    artist: &'static str,
    track_titles: &'static [&'static str],
}

const MUSIC: Case = Case {
    medium: "music",
    title: "Zephyrine",
    artist: "Quillon",
    track_titles: &["Marmoset", "Narwhal"],
};

const PUBLISHER: Case = Case {
    medium: "publisher",
    title: "Yarrowmere",
    artist: "Pelagius",
    track_titles: &[],
};

fn feed_data(case: Case, feed_guid: &str, keep_tracks: usize) -> serde_json::Value {
    let tracks: Vec<serde_json::Value> = case
        .track_titles
        .iter()
        .take(keep_tracks)
        .enumerate()
        .map(|(i, title)| {
            serde_json::json!({
                "track_guid": format!("{feed_guid}-track-{i}"),
                "title": title,
                "author_name": case.artist,
                "explicit": false
            })
        })
        .collect();
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": case.title,
        "author_name": case.artist,
        "raw_medium": case.medium,
        "explicit": false,
        "tracks": tracks
    })
}

fn track_guids(case: Case, feed_guid: &str) -> Vec<String> {
    (0..case.track_titles.len())
        .map(|i| format!("{feed_guid}-track-{i}"))
        .collect()
}

async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    admin: bool,
    body: Option<&serde_json::Value>,
) -> (u16, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if admin {
        builder = builder.header("X-Admin-Token", ADMIN_TOKEN);
    }
    let request = if let Some(body) = body {
        builder
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(body).expect("serialize")))
            .expect("build request")
    } else {
        builder.body(Body::empty()).expect("build request")
    };
    let resp = app.oneshot(request).await.expect("send request");
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn ingest(
    app: axum::Router,
    url: &str,
    hash: &str,
    data: &serde_json::Value,
) -> serde_json::Value {
    let payload = serde_json::json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": CRAWL_TOKEN,
        "http_status": 200,
        "content_hash": hash,
        "feed_data": data,
    });
    let (status, body) = send(app, "POST", "/ingest/feed", false, Some(&payload)).await;
    assert_eq!(status, 200, "ingest must answer 200: {body:?}");
    body
}

async fn report_gone(app: axum::Router, url: &str) -> serde_json::Value {
    let payload = serde_json::json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": CRAWL_TOKEN,
        "http_status": 404,
        "content_hash": "",
        "redirects": [],
        "feed_data": null,
    });
    let (status, body) = send(app, "POST", "/ingest/feed", false, Some(&payload)).await;
    assert_eq!(status, 200, "gone report must answer 200: {body:?}");
    body
}

/// The count of `search_entities` rows for the feed (both types) and its tracks.
fn entity_rows_on(conn: &rusqlite::Connection, feed_guid: &str, tracks: &[String]) -> i64 {
    let mut total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_entities \
             WHERE entity_type IN ('feed', 'publisher') AND entity_id = ?1",
            [feed_guid],
            |row| row.get(0),
        )
        .expect("count feed rows");
    for track_guid in tracks {
        let id = stophammer::db::canonical_track_entity_id(feed_guid, track_guid);
        total += conn
            .query_row(
                "SELECT COUNT(*) FROM search_entities \
                 WHERE entity_type = 'track' AND entity_id = ?1",
                [id],
                |row| row.get::<_, i64>(0),
            )
            .expect("count track rows");
    }
    total
}

fn hits(conn: &rusqlite::Connection, word: &str) -> usize {
    stophammer::search::search(conn, word, None, 20, None, None)
        .expect("search")
        .len()
}

/// Expects the index to hold the rows of the case and a hit for each word.
fn assert_indexed(conn: &rusqlite::Connection, case: Case, feed_guid: &str) {
    let tracks = track_guids(case, feed_guid);
    let expected = 1 + i64::try_from(tracks.len()).expect("small count");
    assert_eq!(
        entity_rows_on(conn, feed_guid, &tracks),
        expected,
        "ADR 0078 §3: the control feed must be in the index before the retire"
    );
    for word in [case.title, case.artist]
        .into_iter()
        .chain(case.track_titles.iter().copied())
    {
        assert!(
            hits(conn, word) > 0,
            "ADR 0078 §3: the control search for {word} must have a hit"
        );
    }
}

/// Expects no row and no hit for the feed, its artist and its tracks.
fn assert_unindexed(conn: &rusqlite::Connection, case: Case, feed_guid: &str, path: &str) {
    let tracks = track_guids(case, feed_guid);
    assert_eq!(
        entity_rows_on(conn, feed_guid, &tracks),
        0,
        "ADR 0078 §3: {path} must delete each search_entities row of a {} feed",
        case.medium
    );
    for word in [case.title, case.artist]
        .into_iter()
        .chain(case.track_titles.iter().copied())
    {
        assert_eq!(
            hits(conn, word),
            0,
            "ADR 0078 §3: after {path} a search for {word} must give no hit ({})",
            case.medium
        );
    }
}

fn check_unindexed(db: &Db, case: Case, feed_guid: &str, path: &str) {
    let conn = db.lock().expect("lock db");
    assert_unindexed(&conn, case, feed_guid, path);
}

async fn seed(case: Case, name: &str) -> (Db, Arc<stophammer::api::AppState>, String, String) {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let url = format!("https://{GONE_HOST}/{name}/{}.xml", case.medium);
    let feed_guid = format!("adr0078-{name}-{}", case.medium);
    let body = ingest(
        stophammer::api::build_router(Arc::clone(&st)),
        &url,
        "hash-seed",
        &feed_data(case, &feed_guid, 2),
    )
    .await;
    assert_eq!(body["accepted"], true, "seed must be accepted: {body:?}");
    {
        let conn = db.lock().expect("lock db");
        assert_indexed(&conn, case, &feed_guid);
    }
    (db, st, url, feed_guid)
}

fn router(st: &Arc<stophammer::api::AppState>) -> axum::Router {
    stophammer::api::build_router(Arc::clone(st))
}

#[tokio::test]
async fn admin_retire_deletes_each_index_row() {
    for case in [MUSIC, PUBLISHER] {
        let (db, st, _url, guid) = seed(case, "admin").await;
        let (status, body) = send(
            router(&st),
            "DELETE",
            &format!("/v1/feeds/{guid}"),
            true,
            None,
        )
        .await;
        assert_eq!(status, 204, "admin retire must succeed: {body:?}");
        check_unindexed(&db, case, &guid, "DELETE /v1/feeds/{guid}");
    }
}

#[tokio::test]
async fn podcast_block_retire_deletes_each_index_row() {
    for case in [MUSIC, PUBLISHER] {
        let (db, st, url, guid) = seed(case, "block").await;
        let mut data = feed_data(case, &guid, 2);
        data["blocks"] = serde_json::json!([{"id": null, "value": "yes"}]);
        let body = ingest(router(&st), &url, "hash-block", &data).await;
        assert_eq!(body["reason"], "source_blocked", "{body:?}");
        check_unindexed(&db, case, &guid, "a podcast:block ingest");
    }
}

#[tokio::test]
async fn gone_source_retire_deletes_each_index_row() {
    for case in [MUSIC, PUBLISHER] {
        let (db, st, url, guid) = seed(case, "gone").await;
        let first = report_gone(router(&st), &url).await;
        assert_eq!(first["reason"], "source_gone_observed", "{first:?}");
        {
            let conn = db.lock().expect("lock db");
            let now = stophammer::db::unix_now();
            conn.execute(
                "UPDATE source_gone_answers SET first_gone_at = ?1 WHERE feed_guid = ?2",
                rusqlite::params![now - ONE_DAY_SECS, guid],
            )
            .expect("backdate");
        }
        let second = report_gone(router(&st), &url).await;
        assert_eq!(second["reason"], "source_gone", "{second:?}");
        check_unindexed(&db, case, &guid, "two gone answers");
    }
}

#[tokio::test]
async fn guid_change_retire_deletes_each_index_row() {
    for case in [MUSIC, PUBLISHER] {
        let (db, st, url, old_guid) = seed(case, "guidchange").await;
        let new_guid = uuidv5_of_url(&url);
        // The new record keeps the title, so only the old rows must go. The
        // new GUID gives other track GUIDs, so the old rows are separate.
        let mut data = feed_data(case, &new_guid, 2);
        data["title"] = serde_json::json!(case.title);
        let body = ingest(router(&st), &url, "hash-g2", &data).await;
        assert_eq!(body["accepted"], true, "GUID change must apply: {body:?}");
        let conn = db.lock().expect("lock db");
        let tracks = track_guids(case, &old_guid);
        assert_eq!(
            entity_rows_on(&conn, &old_guid, &tracks),
            0,
            "ADR 0078 §3: a GUID change must delete each index row of the old {} feed",
            case.medium
        );
        let new_tracks = track_guids(case, &new_guid);
        assert_eq!(
            entity_rows_on(&conn, &new_guid, &new_tracks),
            1 + i64::try_from(new_tracks.len()).expect("small count"),
            "ADR 0078 §3: the new record must hold its own index rows"
        );
    }
}

/// Returns the events with a seq after `after`.
fn events_after(db: &Db, after: usize) -> Vec<stophammer::event::Event> {
    let conn = db.lock().expect("lock db");
    let all = stophammer::db::get_events_since(&conn, 0, 10_000).expect("events");
    all.into_iter().skip(after).collect()
}

fn apply_all(replica: &Db, events: &[stophammer::event::Event]) {
    let pool = common::wrap_pool(Arc::clone(replica));
    for ev in events {
        stophammer::apply::apply_single_event(&pool, ev).expect("apply event");
    }
}

#[tokio::test]
async fn apply_of_feed_retired_deletes_each_index_row() {
    for case in [MUSIC, PUBLISHER] {
        let (db, st, _url, guid) = seed(case, "apply").await;
        let before = events_after(&db, 0);
        let replica = common::test_db_arc();
        apply_all(&replica, &before);
        {
            let conn = replica.lock().expect("lock replica");
            assert_indexed(&conn, case, &guid);
        }
        let (status, _) = send(
            router(&st),
            "DELETE",
            &format!("/v1/feeds/{guid}"),
            true,
            None,
        )
        .await;
        assert_eq!(status, 204);
        let retire = events_after(&db, before.len());
        assert!(!retire.is_empty(), "the retire must sign an event");
        apply_all(&replica, &retire);
        check_unindexed(&replica, case, &guid, "the apply of FeedRetired");
    }
}

fn track_rows(db: &Db, feed_guid: &str, track_guid: &str) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'track' AND entity_id = ?1",
        [stophammer::db::canonical_track_entity_id(
            feed_guid, track_guid,
        )],
        |row| row.get(0),
    )
    .expect("count track rows")
}

fn track_hits(db: &Db, word: &str) -> usize {
    let conn = db.lock().expect("lock db");
    hits(&conn, word)
}

#[tokio::test]
async fn admin_track_removal_deletes_the_index_row() {
    let (db, st, _url, guid) = seed(MUSIC, "rmtrack").await;
    let track = format!("{guid}-track-0");
    let (status, body) = send(
        router(&st),
        "DELETE",
        &format!("/v1/feeds/{guid}/tracks/{track}"),
        true,
        None,
    )
    .await;
    assert_eq!(status, 204, "track removal must succeed: {body:?}");
    assert_eq!(
        track_rows(&db, &guid, &track),
        0,
        "ADR 0078 §3: the admin track removal must delete the search_entities row"
    );
    assert_eq!(
        track_hits(&db, "Marmoset"),
        0,
        "ADR 0078 §3: a search for the removed track title must give no hit"
    );
    assert!(
        track_hits(&db, "Narwhal") > 0,
        "the other track must stay in the index"
    );
}

#[tokio::test]
async fn stale_track_removal_deletes_the_index_row() {
    let (db, st, url, guid) = seed(MUSIC, "stale").await;
    let body = ingest(
        router(&st),
        &url,
        "hash-one-track",
        &feed_data(MUSIC, &guid, 1),
    )
    .await;
    assert_eq!(body["accepted"], true, "{body:?}");
    let removed = format!("{guid}-track-1");
    assert_eq!(
        track_rows(&db, &guid, &removed),
        0,
        "ADR 0078 §3: the stale track removal of an ingest must delete the row"
    );
    assert_eq!(track_hits(&db, "Narwhal"), 0, "ADR 0078 §3: no hit");
    assert!(track_hits(&db, "Marmoset") > 0, "the kept track stays");
}

#[tokio::test]
async fn apply_of_track_removed_deletes_the_index_row() {
    let (db, st, _url, guid) = seed(MUSIC, "applytrack").await;
    let before = events_after(&db, 0);
    let replica = common::test_db_arc();
    apply_all(&replica, &before);
    let track = format!("{guid}-track-0");
    assert_eq!(track_rows(&replica, &guid, &track), 1, "control row");
    let (status, _) = send(
        router(&st),
        "DELETE",
        &format!("/v1/feeds/{guid}/tracks/{track}"),
        true,
        None,
    )
    .await;
    assert_eq!(status, 204);
    apply_all(&replica, &events_after(&db, before.len()));
    assert_eq!(
        track_rows(&replica, &guid, &track),
        0,
        "ADR 0078 §3: the apply of TrackRemoved must delete the row"
    );
    assert_eq!(track_hits(&replica, "Marmoset"), 0, "ADR 0078 §3: no hit");
}

#[tokio::test]
async fn a_failed_index_delete_fails_the_retire_and_writes_no_row() {
    let (db, st, _url, guid) = seed(MUSIC, "fail").await;
    {
        let conn = db.lock().expect("lock db");
        conn.execute("DROP TABLE search_index", [])
            .expect("drop search_index");
    }
    let (status, _) = send(
        router(&st),
        "DELETE",
        &format!("/v1/feeds/{guid}"),
        true,
        None,
    )
    .await;
    assert_eq!(
        status, 500,
        "ADR 0078 §3: a retire that receives an error from a delete gives that error"
    );
    let conn = db.lock().expect("lock db");
    let feeds: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feeds WHERE feed_guid = ?1",
            [&guid],
            |r| r.get(0),
        )
        .expect("count feeds");
    let retired: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'feed_retired'",
            [],
            |r| r.get(0),
        )
        .expect("count events");
    let blocks: i64 = conn
        .query_row("SELECT COUNT(*) FROM feed_blocks", [], |r| r.get(0))
        .expect("count blocks");
    assert_eq!(
        feeds, 1,
        "ADR 0078 §3: the failed retire must keep the feed"
    );
    assert_eq!(
        retired, 0,
        "ADR 0078 §3: the failed retire must write no event"
    );
    assert_eq!(
        blocks, 0,
        "ADR 0078 §3: the failed retire must write no block"
    );
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_source_file_discards_the_result_of_an_index_delete() {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    assert!(!files.is_empty(), "the scan must find source files");
    for file in files {
        let text = std::fs::read_to_string(&file).expect("read source");
        for (n, line) in text.lines().enumerate() {
            let discards = line.contains("let _ = crate::search::delete_from_search_index")
                || line.contains("let _ = delete_from_search_index");
            assert!(
                !discards,
                "ADR 0078 §3: {}:{} discards the result of delete_from_search_index. \
                 Use `?`, or call delete_feed_sql or delete_track_sql, which delete the index rows.",
                file.display(),
                n + 1
            );
        }
    }
}
