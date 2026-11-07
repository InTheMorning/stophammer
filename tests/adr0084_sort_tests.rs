// ADR 0084 §8, task 001: a track stores its sort key.
//
// Checks that `tracks.title_sort` equals `model::sort_key(title)`:
// - after the fill in `open_db` sets a null value,
// - after `db::upsert_track`, for an insert and for an update,
// - after an applied signed `TrackUpserted` event, which calls `upsert_track`,
// - after `POST /ingest/feed`, which writes through `ingest_transaction`.
// The event format does not change: `Track` has no `title_sort` field.
//
// ADR 0084 §3 to §7 and §9, task 002: `reverse=true` on `GET /v1/feeds/recent`,
// `GET /v1/feeds/by-name` and `GET /v1/artist-names`. Checks that:
// - each route gives the reversed list over all pages, with no row that
//   repeats or goes missing (criterion 1),
// - a request with no `reverse` gives a fixed sequence and the cursor form of
//   the release before this ADR (criterion 3),
// - a feed with no `newest_item_at` comes last in the two directions, also
//   across a page break (criterion 5),
// - `from` in Z to A starts at the last row whose key starts with the key of
//   `from`, with the edge cases of a prefix at the highest code point and of
//   an empty key (criterion 6),
// - `reverse` accepts only `true` and `false`, and a cursor with another
//   direction, or a malformed cursor, gives `400` while a cursor from before
//   this ADR continues (criterion 7),
// - the filtered list of ADR 0068 §4 follows the direction and keeps its scan
//   limit.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::body::Body;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use rusqlite::Connection;
use tower::ServiceExt;

use stophammer::event::{
    Event, EventPayload, EventType, FeedUpsertedPayload, TrackUpsertedPayload,
};
use stophammer::model::{Feed, Track, sort_key};

const MESSY_TITLE: &str = "  The  \u{c9}lan\tSession ";

fn sample_feed(feed_guid: &str) -> Feed {
    Feed {
        feed_guid: feed_guid.into(),
        feed_url: format!("https://example.com/{feed_guid}.xml"),
        title: "Sort Feed".into(),
        title_lower: "sort feed".into(),
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
        created_at: 1_000_000,
        updated_at: 1_000_000,
        raw_medium: None,
        last_build_date: None,
        release_artist_source: None,
    }
}

fn sample_track(feed_guid: &str, track_guid: &str, title: &str) -> Track {
    Track {
        track_guid: track_guid.into(),
        feed_guid: feed_guid.into(),
        title: title.into(),
        title_lower: title.to_lowercase(),
        pub_date: None,
        duration_secs: None,
        enclosure_url: None,
        enclosure_type: None,
        enclosure_bytes: None,
        track_number: None,
        season: None,
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        description: None,
        track_artist: None,
        track_artist_sort: None,
        created_at: 1_000_000,
        updated_at: 1_000_000,
    }
}

fn stored_sort(conn: &Connection, track_guid: &str) -> Option<String> {
    conn.query_row(
        "SELECT title_sort FROM tracks WHERE track_guid = ?1",
        [track_guid],
        |r| r.get(0),
    )
    .expect("the track row must exist")
}

#[test]
fn fill_sets_each_null_title_sort_on_open() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("fill.db");
    let path_str = path.to_str().expect("utf-8 path");

    let conn = stophammer::db::open_db(path_str);
    stophammer::db::upsert_feed(&conn, &sample_feed("fill-feed")).expect("upsert feed");
    for (guid, title) in [("fill-a", MESSY_TITLE), ("fill-b", "Plain")] {
        stophammer::db::upsert_track(&conn, &sample_track("fill-feed", guid, title))
            .expect("upsert track");
    }
    conn.execute("UPDATE tracks SET title_sort = NULL", [])
        .expect("clear the column");
    drop(conn);

    let conn = stophammer::db::open_db(path_str);
    for (guid, title) in [("fill-a", MESSY_TITLE), ("fill-b", "Plain")] {
        assert_eq!(
            stored_sort(&conn, guid),
            Some(sort_key(title)),
            "ADR 0084 §8: the fill in open_db must set tracks.title_sort to sort_key(title). \
             Fix ensure_track_title_sort_filled in src/db.rs."
        );
    }
}

#[test]
fn upsert_track_derives_title_sort_for_insert_and_update() {
    let conn = common::test_db();
    stophammer::db::upsert_feed(&conn, &sample_feed("up-feed")).expect("upsert feed");

    stophammer::db::upsert_track(&conn, &sample_track("up-feed", "up-a", MESSY_TITLE))
        .expect("insert track");
    assert_eq!(
        stored_sort(&conn, "up-a"),
        Some(sort_key(MESSY_TITLE)),
        "ADR 0084 §8: an insert through upsert_track must derive title_sort from title."
    );

    stophammer::db::upsert_track(&conn, &sample_track("up-feed", "up-a", "Changed  Title"))
        .expect("update track");
    assert_eq!(
        stored_sort(&conn, "up-a"),
        Some("changed title".to_string()),
        "ADR 0084 §8: an update through upsert_track must derive title_sort again. \
         Add title_sort to the DO UPDATE clause in src/db.rs."
    );
}

#[test]
fn applied_track_upserted_event_derives_title_sort() {
    let pool = common::test_db_pool_memory();
    let signer = common::temp_signer("adr0084-sort-apply-signer");
    let feed_guid = "ev-feed";
    let track_guid = "ev-track";

    let feed_payload = FeedUpsertedPayload {
        feed: sample_feed(feed_guid),
        reason: None,
    };
    let track_payload = TrackUpsertedPayload {
        track: sample_track(feed_guid, track_guid, MESSY_TITLE),
        routes: vec![],
        value_time_splits: vec![],
    };
    let feed_json = serde_json::to_string(&feed_payload).expect("feed json");
    let track_json = serde_json::to_string(&track_payload).expect("track json");

    let make =
        |id: &str, ty: EventType, payload: EventPayload, json: String, subject: &str, seq| {
            let (signed_by, signature) = signer.sign_event(id, &ty, &json, subject, 1_000_000, seq);
            Event {
                event_id: id.into(),
                event_type: ty,
                payload,
                payload_json: json,
                subject_guid: subject.into(),
                signed_by,
                signature,
                seq,
                created_at: 1_000_000,
                warnings: vec![],
            }
        };
    let events = [
        make(
            "ev-sort-feed",
            EventType::FeedUpserted,
            EventPayload::FeedUpserted(feed_payload),
            feed_json,
            feed_guid,
            1,
        ),
        make(
            "ev-sort-track",
            EventType::TrackUpserted,
            EventPayload::TrackUpserted(track_payload),
            track_json,
            track_guid,
            2,
        ),
    ];
    for ev in &events {
        stophammer::apply::apply_single_event(&pool, ev)
            .unwrap_or_else(|e| panic!("apply {} must succeed: {e}", ev.event_id));
    }

    let conn = pool.reader().expect("reader connection");
    assert_eq!(
        stored_sort(&conn, track_guid),
        Some(sort_key(MESSY_TITLE)),
        "ADR 0084 §8: a node that applies a signed TrackUpserted event must derive \
         title_sort itself. The event format does not carry it."
    );
}

#[tokio::test]
async fn ingest_feed_derives_title_sort_for_insert_and_update() {
    let crawl_token = "adr0084-sort-crawl-token";
    let signer = Arc::new(common::temp_signer("adr0084-sort-ingest-signer"));
    let chain = stophammer::verify::build_chain(
        &stophammer::verify::ChainSpec {
            names: vec!["content_hash".to_string()],
        },
        crawl_token.to_string(),
    );
    let db = common::test_db_arc();
    let state = Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(Arc::clone(&db)),
        chain: Arc::new(chain),
        node_pubkey_hex: signer.pubkey_hex().to_string(),
        signer,
        admin_token: "adr0084-sort-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    });

    for (hash, title, expected) in [
        ("hash-1", MESSY_TITLE, sort_key(MESSY_TITLE)),
        ("hash-2", "Changed  Title", "changed title".to_string()),
    ] {
        let url = "https://example.com/ingest-sort.xml";
        let payload = serde_json::json!({
            "canonical_url": url,
            "source_url": url,
            "crawl_token": crawl_token,
            "http_status": 200,
            "content_hash": hash,
            "feed_data": {
                "feed_guid": "ingest-sort-feed",
                "title": "Ingest Sort Feed",
                "raw_medium": "music",
                "explicit": false,
                "tracks": [{
                    "track_guid": "ingest-sort-track",
                    "title": title,
                    "explicit": false,
                }],
            },
        });
        let resp = stophammer::api::build_router(Arc::clone(&state))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/ingest/feed")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
                    .expect("build request"),
            )
            .await
            .expect("send request");
        assert!(
            resp.status().is_success(),
            "ingest must succeed, got {}",
            resp.status()
        );

        let conn = db.lock().expect("db lock");
        assert_eq!(
            stored_sort(&conn, "ingest-sort-track"),
            Some(expected),
            "ADR 0084 §8: ingest_transaction must derive tracks.title_sort at each write. \
             Add title_sort to both INSERT statements in src/db.rs."
        );
    }
}

// ── Task 002: `reverse` on the three list routes ────────────────────────────

fn list_state(db: Arc<std::sync::Mutex<Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("adr0084-list-signer"));
    let chain = stophammer::verify::build_chain(
        &stophammer::verify::ChainSpec { names: vec![] },
        "adr0084-list-crawl-token".to_string(),
    );
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        node_pubkey_hex: signer.pubkey_hex().to_string(),
        signer,
        admin_token: "adr0084-list-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn get(st: &Arc<stophammer::api::AppState>, uri: &str) -> (StatusCode, serde_json::Value) {
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

/// Reads each page of `uri` with `limit`, and gives the value of `field` of
/// each row in sequence, with the cursor of each page that has more.
async fn walk(
    st: &Arc<stophammer::api::AppState>,
    uri: &str,
    limit: usize,
    field: &str,
) -> (Vec<String>, Vec<String>) {
    let mut rows = Vec::new();
    let mut cursors = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..200 {
        let mut full = format!(
            "{uri}{}limit={limit}",
            if uri.contains('?') { '&' } else { '?' }
        );
        if let Some(c) = &cursor {
            full = format!("{full}&cursor={c}");
        }
        let (status, body) = get(st, &full).await;
        assert_eq!(status, StatusCode::OK, "GET {full} gave {body}");
        for row in body["data"].as_array().expect("data array") {
            rows.push(row[field].as_str().expect("string field").to_string());
        }
        if body["pagination"]["has_more"] == true {
            let next = body["pagination"]["cursor"]
                .as_str()
                .expect("a page with has_more must give a cursor")
                .to_string();
            cursors.push(next.clone());
            cursor = Some(next);
        } else {
            return (rows, cursors);
        }
    }
    panic!("the walk of {uri} did not end");
}

fn b64(text: &str) -> String {
    URL_SAFE_NO_PAD.encode(text.as_bytes())
}

fn music_feed(guid: &str, title: &str, newest: Option<i64>) -> Feed {
    let mut feed = sample_feed(guid);
    feed.title = title.into();
    feed.title_lower = title.to_lowercase();
    feed.raw_medium = Some("music".into());
    feed.newest_item_at = newest;
    feed
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn reversed(items: &[&str]) -> Vec<String> {
    items.iter().rev().map(|s| (*s).to_string()).collect()
}

// ── GET /v1/feeds/recent ────────────────────────────────────────────────────

fn recent_state() -> Arc<stophammer::api::AppState> {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("db lock");
        for (guid, ts) in [
            ("d-a1", Some(100)),
            ("d-a2", Some(100)),
            ("d-b", Some(200)),
            ("d-c", Some(300)),
            ("u-1", None),
            ("u-2", None),
            ("u-3", None),
        ] {
            stophammer::db::upsert_feed(&conn, &music_feed(guid, guid, ts)).expect("upsert feed");
        }
    }
    list_state(db)
}

#[tokio::test]
async fn recent_without_reverse_keeps_its_sequence_and_cursor_form() {
    let st = recent_state();
    let (rows, cursors) = walk(&st, "/v1/feeds/recent", 2, "feed_guid").await;
    assert_eq!(
        rows,
        strs(&["d-c", "d-b", "d-a2", "d-a1", "u-3", "u-2", "u-1"]),
        "ADR 0084 criterion 3: with no reverse, GET /v1/feeds/recent must give newest first, \
         then the undated rows, with feed_guid descending. Do not change the usual SQL."
    );
    assert_eq!(
        cursors,
        vec![b64("200\0d-b"), b64("100\0d-a1"), b64("-1\0u-2"),],
        "ADR 0084 §9: the cursor of the usual direction must keep the form \
         base64url(\"<key>\\0<feed_guid>\"), with -1 for a row with no date."
    );
}

#[tokio::test]
async fn recent_reverse_gives_oldest_first_and_undated_last_at_each_page_size() {
    let st = recent_state();
    let expected = strs(&["d-a1", "d-a2", "d-b", "d-c", "u-1", "u-2", "u-3"]);
    for limit in 1..=8 {
        let (rows, cursors) = walk(&st, "/v1/feeds/recent?reverse=true", limit, "feed_guid").await;
        assert_eq!(
            rows, expected,
            "ADR 0084 §6 and criterion 5: with reverse=true and limit={limit}, GET /v1/feeds/recent \
             must give the dated rows oldest first, then the undated rows, each page break \
             included. Fix the reversed key in recent_feed_rows in src/query.rs."
        );
        for cursor in cursors {
            assert!(
                cursor.starts_with("r."),
                "ADR 0084 §9: a cursor of a reversed page must carry the direction mark"
            );
        }
    }
    let (_, cursors) = walk(&st, "/v1/feeds/recent?reverse=true", 2, "feed_guid").await;
    assert_eq!(
        cursors,
        vec![
            format!("r.{}", b64("100\0d-a2")),
            format!("r.{}", b64("300\0d-c")),
            format!("r.{}", b64("-1\0u-2")),
        ],
        "ADR 0084 §9: the reversed cursor must be the mark `r.` and the usual encoding"
    );
}

#[tokio::test]
async fn recent_reverse_false_equals_no_reverse() {
    let st = recent_state();
    let (plain, _) = walk(&st, "/v1/feeds/recent", 3, "feed_guid").await;
    let (explicit, _) = walk(&st, "/v1/feeds/recent?reverse=false", 3, "feed_guid").await;
    assert_eq!(
        plain, explicit,
        "ADR 0084 §5: reverse=false is the usual direction"
    );
}

#[tokio::test]
async fn recent_link_filter_follows_the_direction_and_keeps_its_scan_limit() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("db lock");
        for (guid, ts) in [
            ("p-1", 10),
            ("p-2", 20),
            ("p-3", 30),
            ("p-4", 40),
            ("p-5", 50),
        ] {
            let mut feed = music_feed(guid, guid, Some(ts));
            feed.raw_medium = Some("publisher".into());
            stophammer::db::upsert_feed(&conn, &feed).expect("upsert feed");
        }
    }
    let st = list_state(db);
    let base = "/v1/feeds/recent?medium=publisher&two_way_links=none";
    let (usual, _) = walk(&st, base, 2, "feed_guid").await;
    assert_eq!(usual, strs(&["p-5", "p-4", "p-3", "p-2", "p-1"]));
    let (rows, _) = walk(&st, &format!("{base}&reverse=true"), 2, "feed_guid").await;
    assert_eq!(
        rows,
        strs(&["p-1", "p-2", "p-3", "p-4", "p-5"]),
        "ADR 0084 §3: the filtered list of ADR 0068 §4 must follow reverse. Pass the \
         direction through filtered_recent_rows in src/query.rs."
    );
}

#[tokio::test]
async fn recent_filter_scan_limit_is_the_same_in_the_reversed_direction() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("db lock");
        for n in 0..1_005_i64 {
            let mut feed = music_feed(&format!("s-{n:05}"), "scan", Some(n));
            feed.raw_medium = Some("publisher".into());
            stophammer::db::upsert_feed(&conn, &feed).expect("upsert feed");
        }
    }
    let st = list_state(db);
    // No row keeps `stated_rel=nomatch`. The request stops at 1,000 examined rows.
    for reverse in ["", "&reverse=true"] {
        let uri = format!("/v1/feeds/recent?medium=publisher&stated_rel=nomatch&limit=10{reverse}");
        let (status, body) = get(&st, &uri).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"].as_array().map(Vec::len), Some(0), "{uri}");
        assert_eq!(
            body["pagination"]["has_more"], true,
            "ADR 0068 §4 and ADR 0084 §3: the scan limit of 1,000 rows must hold with reverse \
             ({uri}). The first page ends with a cursor after the last examined row."
        );
        let cursor = body["pagination"]["cursor"].as_str().expect("cursor");
        let (status, body) = get(&st, &format!("{uri}&cursor={cursor}")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["pagination"]["has_more"], false,
            "the second request must reach the end of the 1,005 rows ({uri})"
        );
    }
}

// ── GET /v1/feeds/by-name ───────────────────────────────────────────────────

/// Titles in code point order of their sort keys. `m\u{10FFFF}z` sorts after
/// `mz`. Two feeds have the same title, so the tie key decides.
const NAME_FEEDS: [(&str, &str); 10] = [
    ("n-alpha", "Alpha"),
    ("n-lemon", "Lemon"),
    ("n-m", "M"),
    ("n-madonna", "Madonna"),
    ("n-mango", "Mango"),
    ("n-mz", "Mz"),
    ("n-hi", "m\u{10FFFF}z"),
    ("n-zeta1", "Zeta"),
    ("n-zeta2", "Zeta"),
    ("n-emoji", "\u{1F596}"),
];

const NAME_ASC: [&str; 10] = [
    "n-alpha",
    "n-lemon",
    "n-m",
    "n-madonna",
    "n-mango",
    "n-mz",
    "n-hi",
    "n-zeta1",
    "n-zeta2",
    "n-emoji",
];

fn name_state() -> Arc<stophammer::api::AppState> {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("db lock");
        for (guid, title) in NAME_FEEDS {
            stophammer::db::upsert_feed(&conn, &music_feed(guid, title, Some(5)))
                .expect("upsert feed");
        }
    }
    list_state(db)
}

#[tokio::test]
async fn by_name_without_reverse_keeps_its_sequence_and_cursor_form() {
    let st = name_state();
    let (rows, cursors) = walk(&st, "/v1/feeds/by-name", 4, "feed_guid").await;
    assert_eq!(
        rows,
        strs(&NAME_ASC),
        "ADR 0084 criterion 3: with no reverse, GET /v1/feeds/by-name must keep A to Z, \
         then feed_guid ascending."
    );
    assert_eq!(
        cursors,
        vec![b64("madonna\0n-madonna"), b64("zeta\0n-zeta1")],
        "ADR 0084 §9: the cursor of the usual direction must keep its form"
    );
}

#[tokio::test]
async fn by_name_reverse_gives_z_to_a_at_each_page_size() {
    let st = name_state();
    // The tie key follows the direction: n-zeta2 comes before n-zeta1.
    let expected = reversed(&NAME_ASC);
    for limit in [1, 2, 3, 4, 10, 11] {
        let (rows, cursors) = walk(&st, "/v1/feeds/by-name?reverse=true", limit, "feed_guid").await;
        assert_eq!(
            rows, expected,
            "ADR 0084 criterion 1: reverse=true with limit={limit} must give the full reversed \
             list with feed_guid descending. Fix by_name_feed_rows in src/query.rs."
        );
        assert!(cursors.iter().all(|c| c.starts_with("r.")));
    }
}

#[tokio::test]
async fn by_name_reverse_from_starts_at_the_last_title_with_the_prefix() {
    let st = name_state();
    // The last key that starts with "m" is `m\u{10FFFF}z`, after "mz".
    let cases: [(&str, &str); 6] = [
        ("M", "n-hi"),
        ("ma", "n-mango"),
        ("mb", "n-mango"),
        ("m%F4%8F%BF%BF", "n-hi"),
        ("%F4%8F%BF%BF", "n-emoji"),
        ("", "n-emoji"),
    ];
    for (from, first) in cases {
        let (status, body) = get(
            &st,
            &format!("/v1/feeds/by-name?reverse=true&from={from}&limit=1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["data"][0]["feed_guid"], first,
            "ADR 0084 §7 and criterion 6: from={from:?} with reverse=true must start at the last \
             row whose key starts with it, or at the last row before its position. Use \
             reverse_from_bound in src/query.rs."
        );
    }
    // A key after every row: the list starts at the last row.
    let (_, body) = get(&st, "/v1/feeds/by-name?reverse=true&from=zzzz&limit=1").await;
    assert_eq!(
        body["data"][0]["feed_guid"], "n-zeta2",
        "no key starts with zzzz, so the list starts at the last row before its position"
    );
    let (_, body) = get(&st, "/v1/feeds/by-name?reverse=true&from=Z&limit=1").await;
    assert_eq!(body["data"][0]["feed_guid"], "n-zeta2");
    // A key before every row: the list is empty.
    let (_, body) = get(&st, "/v1/feeds/by-name?reverse=true&from=%20%20&limit=1").await;
    assert_eq!(
        body["data"][0]["feed_guid"], "n-emoji",
        "an empty key starts at the last row"
    );
    // The usual direction still starts at the first row at or after the key.
    let (_, body) = get(&st, "/v1/feeds/by-name?from=M&limit=1").await;
    assert_eq!(body["data"][0]["feed_guid"], "n-m");
}

#[tokio::test]
async fn by_name_reverse_from_with_no_lower_row_gives_an_empty_list() {
    let st = name_state();
    let (status, body) = get(&st, "/v1/feeds/by-name?reverse=true&from=%01&limit=5").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["data"].as_array().map(Vec::len),
        Some(0),
        "ADR 0084 §7: a prefix before every key has no row at or before its position"
    );
}

// ── GET /v1/artist-names ────────────────────────────────────────────────────

fn artist_state() -> Arc<stophammer::api::AppState> {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("db lock");
        for (guid, artist) in [
            ("a-1", "Alpha"),
            ("a-2", "Madonna"),
            ("a-3", "Mango"),
            ("a-4", "M"),
            ("a-5", "m\u{10FFFF}z"),
            ("a-6", "Tie"),
            ("a-7", "Zed"),
        ] {
            let mut feed = music_feed(guid, guid, Some(5));
            feed.release_artist = Some(artist.into());
            stophammer::db::upsert_feed(&conn, &feed).expect("upsert feed");
        }
        // A publisher feed whose sort key equals the key of the `Tie` name row.
        let mut publisher = music_feed("pub-tie", "Tie", None);
        publisher.raw_medium = Some("publisher".into());
        stophammer::db::upsert_feed(&conn, &publisher).expect("upsert publisher");
    }
    list_state(db)
}

/// `kind:key` of each row, in the usual sequence.
const ARTIST_ASC: [&str; 8] = [
    "name:alpha",
    "name:m",
    "name:madonna",
    "name:mango",
    "name:m\u{10FFFF}z",
    "name:tie",
    "publisher:pub-tie",
    "name:zed",
];

async fn artist_rows(
    st: &Arc<stophammer::api::AppState>,
    uri: &str,
    limit: usize,
) -> (Vec<String>, Vec<String>) {
    let (kinds, cursors) = walk(st, uri, limit, "kind").await;
    let (keys, _) = walk(st, uri, limit, "key").await;
    (
        kinds
            .into_iter()
            .zip(keys)
            .map(|(k, key)| format!("{k}:{key}"))
            .collect(),
        cursors,
    )
}

#[tokio::test]
async fn artist_names_without_reverse_keeps_its_sequence_and_cursor_form() {
    let st = artist_state();
    let (rows, cursors) = artist_rows(&st, "/v1/artist-names", 3).await;
    assert_eq!(
        rows,
        strs(&ARTIST_ASC),
        "ADR 0084 criterion 3: with no reverse, GET /v1/artist-names must keep its sequence \
         (sort_name, kind, key). `name` comes before `publisher`."
    );
    assert_eq!(
        cursors,
        vec![b64("madonna\0name\0madonna"), b64("tie\0name\0tie")],
        "ADR 0084 §9: the cursor of the usual direction must keep its form"
    );
}

#[tokio::test]
async fn artist_names_reverse_gives_z_to_a_at_each_page_size() {
    let st = artist_state();
    let expected = reversed(&ARTIST_ASC);
    for limit in [1, 2, 3, 7, 8, 9] {
        let (rows, cursors) = artist_rows(&st, "/v1/artist-names?reverse=true", limit).await;
        assert_eq!(
            rows, expected,
            "ADR 0084 criterion 1 and §6: reverse=true with limit={limit} must give the \
             reversed list. The tie order is sort_name, kind and key, all descending, so \
             `publisher` comes before `name`. Fix handle_get_artist_names in src/query.rs."
        );
        assert!(cursors.iter().all(|c| c.starts_with("r.")));
    }
}

#[tokio::test]
async fn artist_names_reverse_from_starts_at_the_last_key_with_the_prefix() {
    let st = artist_state();
    for (from, first) in [
        ("M", "m\u{10FFFF}z"),
        ("ma", "mango"),
        ("mb", "mango"),
        ("%F4%8F%BF%BF", "zed"),
        ("", "zed"),
        ("zzzz", "zed"),
    ] {
        let (status, body) = get(
            &st,
            &format!("/v1/artist-names?reverse=true&from={from}&limit=1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["data"][0]["key"], first,
            "ADR 0084 §7 and criterion 6: from={from:?} with reverse=true must start at the \
             last row whose key starts with it. Use reverse_from_bound in src/query.rs."
        );
    }
    let (_, body) = get(&st, "/v1/artist-names?reverse=true&from=%01&limit=3").await;
    assert_eq!(body["data"].as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn artist_names_reverse_does_not_rebuild_the_cache() {
    let st = artist_state();
    let (a, _) = walk(&st, "/v1/artist-names?reverse=true", 2, "key").await;
    {
        // A write with no event leaves the event sequence unchanged.
        let conn = st.db.writer().lock().expect("db lock");
        conn.execute(
            "UPDATE feeds SET release_artist = 'Changed' WHERE feed_guid = 'a-1'",
            [],
        )
        .expect("update");
    }
    let (b, _) = walk(&st, "/v1/artist-names?reverse=true", 2, "key").await;
    assert_eq!(
        a, b,
        "ADR 0070 §4a: a reversed page must read the copy in memory, not build it again"
    );
}

// ── Validation and cursors, three routes ────────────────────────────────────

const LIST_ROUTES: [&str; 3] = ["/v1/feeds/recent", "/v1/feeds/by-name", "/v1/artist-names"];

#[tokio::test]
async fn reverse_accepts_only_true_and_false() {
    let st = name_state();
    for route in LIST_ROUTES {
        for good in ["true", "false"] {
            let (status, body) = get(&st, &format!("{route}?reverse={good}")).await;
            assert_eq!(status, StatusCode::OK, "{route} reverse={good}: {body}");
        }
        for bad in ["", "yes", "1", "TRUE", "True", "0", "tru"] {
            let (status, body) = get(&st, &format!("{route}?reverse={bad}")).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "ADR 0084 §5: {route} must give 400 for reverse={bad:?}. Read it with \
                 ListQuery::reverse in src/query.rs."
            );
            assert!(
                body["error"]
                    .as_str()
                    .is_some_and(|m| m.contains("reverse")),
                "the 400 body of {route} must use the error form with a message that names reverse: {body}"
            );
        }
    }
}

#[tokio::test]
async fn a_cursor_with_another_direction_gives_400_on_each_route() {
    let st = artist_state();
    for route in LIST_ROUTES {
        let (_, usual) = get(&st, &format!("{route}?limit=1")).await;
        let (_, rev) = get(&st, &format!("{route}?limit=1&reverse=true")).await;
        let usual_cursor = usual["pagination"]["cursor"]
            .as_str()
            .expect("usual cursor");
        let rev_cursor = rev["pagination"]["cursor"]
            .as_str()
            .expect("reversed cursor");
        let (status, body) = get(&st, &format!("{route}?reverse=true&cursor={usual_cursor}")).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "ADR 0084 §9: a cursor of the usual direction with reverse=true must give 400 on {route}: {body}"
        );
        let (status, body) = get(&st, &format!("{route}?cursor={rev_cursor}")).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "ADR 0084 §9: a reversed cursor with no reverse must give 400 on {route}: {body}"
        );
        let (status, _) = get(&st, &format!("{route}?reverse=false&cursor={rev_cursor}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{route}");
    }
}

#[tokio::test]
async fn a_malformed_cursor_gives_400_in_each_direction_on_each_route() {
    let st = name_state();
    let marked = |body: &str| format!("r.{body}");
    let bad_cursors = [
        "r.".to_string(),
        "r.r.".to_string(),
        "r.!!".to_string(),
        "r.%00".to_string(),
        marked(&b64("no separator")),
        marked(&b64("x\0y")),
        marked(&URL_SAFE_NO_PAD.encode([0xff_u8, 0xfe])),
        "!!".to_string(),
        URL_SAFE_NO_PAD.encode([0xff_u8, 0xfe]),
        b64("no separator"),
        b64("x\0y"),
    ];
    for route in LIST_ROUTES {
        for cursor in &bad_cursors {
            for reverse in ["", "&reverse=true"] {
                let (status, body) = get(&st, &format!("{route}?cursor={cursor}{reverse}")).await;
                // `b64("x\0y")` is a well-formed cursor of the by-name list
                // and the recent list has a bad key. The artist list needs
                // three parts. Only the by-name list takes two free parts.
                let well_formed_by_name = route == "/v1/feeds/by-name"
                    && (*cursor == b64("x\0y") && reverse.is_empty()
                        || *cursor == marked(&b64("x\0y")) && !reverse.is_empty());
                if well_formed_by_name {
                    assert_eq!(status, StatusCode::OK, "{route} {cursor} {reverse}: {body}");
                } else {
                    assert_eq!(
                        status,
                        StatusCode::BAD_REQUEST,
                        "ADR 0084 §9: {route} must give 400 for cursor {cursor:?}{reverse}, not {body}"
                    );
                    assert!(
                        body["error"].is_string(),
                        "the 400 must use the error form: {body}"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn a_legacy_cursor_whose_name_looks_like_the_mark_continues_in_the_usual_direction() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("db lock");
        for (guid, title) in [("l-1", "q"), ("l-2", "r."), ("l-3", "r.a"), ("l-4", "s")] {
            stophammer::db::upsert_feed(&conn, &music_feed(guid, title, Some(1)))
                .expect("upsert feed");
        }
    }
    let st = list_state(db);
    // A cursor from before this change, made by hand, after the row `r.`.
    let legacy = b64("r.\0l-2");
    let (status, body) = get(&st, &format!("/v1/feeds/by-name?cursor={legacy}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let guids: Vec<&str> = body["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|r| r["feed_guid"].as_str().expect("guid"))
        .collect();
    assert_eq!(
        guids,
        ["l-3", "l-4"],
        "ADR 0084 §9: a cursor from before this change must continue in the usual direction, \
         even when its name looks like the mark of the reversed direction"
    );
    // The same name in a reversed cursor continues in the reversed direction.
    let (rows, _) = walk(&st, "/v1/feeds/by-name?reverse=true", 1, "feed_guid").await;
    assert_eq!(rows, strs(&["l-4", "l-3", "l-2", "l-1"]));
    let reversed_cursor = format!("r.{}", b64("r.\0l-2"));
    let (status, body) = get(
        &st,
        &format!("/v1/feeds/by-name?reverse=true&cursor={reversed_cursor}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"][0]["feed_guid"], "l-1");
    // A legacy cursor with reverse=true gives 400.
    let (status, _) = get(
        &st,
        &format!("/v1/feeds/by-name?reverse=true&cursor={legacy}"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_legacy_cursor_continues_on_recent_and_the_artist_list() {
    let st = recent_state();
    let (status, body) = get(
        &st,
        &format!("/v1/feeds/recent?cursor={}&limit=2", b64("200\0d-b")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"][0]["feed_guid"], "d-a2");
    let st = artist_state();
    let legacy = b64("m\0name\0m");
    let (status, body) = get(&st, &format!("/v1/artist-names?cursor={legacy}&limit=1")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"][0]["key"], "madonna");
}
