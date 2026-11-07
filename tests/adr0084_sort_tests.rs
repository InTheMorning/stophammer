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
//
// ADR 0084 §2 to §5 and §9 to §11, task 003: `sort` and `reverse` on
// `GET /v1/search`. Checks that:
// - each sort, in each direction, with a word and with no word, for each
//   `type` and for no `type`, gives the expected sequence over all pages at
//   each page size, with no row that repeats or goes missing (criteria 1
//   and 2),
// - a request with neither parameter gives the sequence, the fields and the
//   cursor forms of the release before this ADR, and no `sort_name` or
//   `sort_date` (criterion 3),
// - each error code has its condition, and the checks of ADR 0080 §4 run
//   first (criterion 4),
// - a row with no key comes last in both directions, also on the page break
//   between keyed and unkeyed rows (criterion 5),
// - a cursor with another sort or direction gives `400`, each malformed form
//   gives `400`, and a cursor from before this ADR continues the default sort
//   (criterion 7),
// - the capability list ends with `sort`, and the contract and the field
//   fixture state the new parameters and fields (criteria 9 and 10).

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

// ── Task 003: `sort` and `reverse` on `GET /v1/search` ──────────────────────

#[derive(Clone, Debug)]
struct SeedItem {
    /// `feed`, `track` or `publisher`.
    kind: &'static str,
    /// The `entity_id` that a response row gives.
    id: String,
    /// `None` for a row whose sort key is null.
    name_key: Option<String>,
    date: Option<i64>,
    /// The tie key of the row in the index.
    rowid: i64,
}

impl SeedItem {
    fn label(&self) -> String {
        format!("{}:{}", self.kind, self.id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortKind {
    Name,
    Date,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum KeyValue {
    Text(String),
    Date(i64),
}

impl SortKind {
    fn name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Date => "date",
        }
    }

    fn key(self, item: &SeedItem) -> Option<KeyValue> {
        match self {
            Self::Name => item.name_key.clone().map(KeyValue::Text),
            Self::Date => item.date.map(KeyValue::Date),
        }
    }
}

/// The sequence that ADR 0084 §2, §3 and §6 give, built in the test from the
/// seed. A name goes up and a date goes down in the usual direction. A row
/// with no key is last. The tie key `rowid` follows the direction.
fn expected_sequence(
    items: &[SeedItem],
    kinds: &[&str],
    sort: SortKind,
    reverse: bool,
) -> Vec<String> {
    let mut rows: Vec<&SeedItem> = items.iter().filter(|i| kinds.contains(&i.kind)).collect();
    let key_descends = (sort == SortKind::Date) != reverse;
    rows.sort_by(|a, b| {
        let by_key = match (sort.key(a), sort.key(b)) {
            (Some(x), Some(y)) if key_descends => y.cmp(&x),
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        by_key.then_with(|| {
            if reverse {
                b.rowid.cmp(&a.rowid)
            } else {
                a.rowid.cmp(&b.rowid)
            }
        })
    });
    rows.iter().map(|i| i.label()).collect()
}

fn kinds_of(kind: Option<&str>) -> Vec<&'static str> {
    match kind {
        None => vec!["feed", "track"],
        Some("feed") => vec!["feed"],
        Some("track") => vec!["track"],
        Some("publisher") => vec!["publisher"],
        Some(other) => panic!("unknown type {other}"),
    }
}

/// Twenty indexed rows. Each one has `common` in its description, so `q=common`
/// and `host=example.com` select the same rows.
///
/// - Two feeds and one track share a key and a date (a tie across types).
/// - Four feeds and two tracks have no date. One feed, one publisher and one
///   track have a null `title_sort`.
/// - The title of a feed is `s.mark`, and the title of a track is `r.mark`.
fn seeded_search() -> (Arc<stophammer::api::AppState>, Vec<SeedItem>) {
    let db = common::test_db_arc();
    let mut items = Vec::new();
    {
        let conn = db.lock().expect("db lock");
        let feeds: [(&str, &str, Option<i64>, bool); 12] = [
            ("fd-1", "Bravo", Some(300), false),
            ("fd-2", "alpha", Some(100), false),
            ("fd-3", "Charlie", Some(200), false),
            ("fd-4", "Same", Some(150), false),
            ("fd-5", "Same", Some(150), false),
            ("fd-6", "Delta", None, false),
            ("fd-7", "echo", None, false),
            ("fd-8", "s.mark", Some(250), false),
            ("fd-9", "Nokey", Some(120), false),
            ("pb-1", "Pub One", None, true),
            ("pb-2", "Pub Two", Some(400), true),
            ("pb-3", "alpha pub", None, true),
        ];
        let null_key = ["fd-9", "pb-3"];
        for (guid, title, newest, publisher) in feeds {
            let mut feed = music_feed(guid, title, newest);
            if publisher {
                feed.raw_medium = Some("publisher".into());
            }
            stophammer::db::upsert_feed(&conn, &feed).expect("upsert feed");
            let kind = if publisher { "publisher" } else { "feed" };
            stophammer::search::populate_search_index(&conn, kind, guid, title, title, "common")
                .expect("populate");
            items.push(SeedItem {
                kind,
                id: guid.into(),
                name_key: (!null_key.contains(&guid)).then(|| sort_key(title)),
                date: newest,
                rowid: stophammer::search::rowid_for(kind, guid),
            });
        }
        let tracks: [(&str, &str, &str, Option<i64>, bool); 8] = [
            ("tr-1", "fd-1", "Zulu", Some(310), false),
            ("tr-2", "fd-2", "alpha", Some(100), false),
            ("tr-3", "fd-4", "Same", Some(150), false),
            ("tr-4", "fd-6", "mike", None, false),
            ("tr-5", "fd-7", "Yankee", None, false),
            ("tr-6", "fd-8", "r.mark", Some(260), false),
            ("tr-7", "fd-9", "Notrack", Some(50), true),
            ("tr-8", "fd-1", "Quebec", Some(500), false),
        ];
        for (guid, feed, title, pub_date, null) in tracks {
            let mut track = sample_track(feed, guid, title);
            track.pub_date = pub_date;
            stophammer::db::upsert_track(&conn, &track).expect("upsert track");
            let entity_id = stophammer::db::canonical_track_entity_id(feed, guid);
            stophammer::search::populate_search_index(
                &conn, "track", &entity_id, title, title, "common",
            )
            .expect("populate track");
            items.push(SeedItem {
                kind: "track",
                id: guid.into(),
                name_key: (!null).then(|| sort_key(title)),
                date: pub_date,
                rowid: stophammer::search::rowid_for("track", &entity_id),
            });
        }
        conn.execute(
            "UPDATE feeds SET title_sort = NULL WHERE feed_guid IN ('fd-9', 'pb-3')",
            [],
        )
        .expect("clear feed keys");
        conn.execute(
            "UPDATE tracks SET title_sort = NULL WHERE track_guid = 'tr-7'",
            [],
        )
        .expect("clear track key");
        conn.execute("UPDATE feeds SET feed_host = 'example.com'", [])
            .expect("set host");
    }
    (list_state(db), items)
}

/// Reads each page of a search. Gives each row, and the cursor of each page
/// that has more.
async fn search_walk(
    st: &Arc<stophammer::api::AppState>,
    base: &str,
    limit: usize,
) -> (Vec<serde_json::Value>, Vec<String>) {
    let mut rows = Vec::new();
    let mut cursors = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..200 {
        let mut uri = format!("{base}&limit={limit}");
        if let Some(c) = &cursor {
            uri = format!("{uri}&cursor={c}");
        }
        let (status, body) = get(st, &uri).await;
        assert_eq!(status, StatusCode::OK, "GET {uri} gave {body}");
        rows.extend(body["data"].as_array().expect("data array").iter().cloned());
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
    panic!("the walk of {base} did not end");
}

fn row_label(row: &serde_json::Value) -> String {
    format!(
        "{}:{}",
        row["entity_type"].as_str().expect("entity_type"),
        row["entity_id"].as_str().expect("entity_id")
    )
}

fn labels(rows: &[serde_json::Value]) -> Vec<String> {
    rows.iter().map(row_label).collect()
}

fn decoded(cursor: &str) -> String {
    String::from_utf8(URL_SAFE_NO_PAD.decode(cursor).expect("base64url")).expect("utf-8")
}

/// The base of a request: `type`, the word or the filter, `sort`, `reverse`.
fn search_base(
    kind: Option<&str>,
    word: bool,
    sort: Option<&str>,
    reverse: Option<&str>,
) -> String {
    let mut uri = String::from(if word {
        "/v1/search?q=common"
    } else {
        "/v1/search?host=example.com"
    });
    if let Some(kind) = kind {
        uri = format!("{uri}&type={kind}");
    }
    if let Some(sort) = sort {
        uri = format!("{uri}&sort={sort}");
    }
    if let Some(reverse) = reverse {
        uri = format!("{uri}&reverse={reverse}");
    }
    uri
}

const TYPES: [Option<&str>; 4] = [None, Some("feed"), Some("track"), Some("publisher")];

#[tokio::test]
async fn each_sort_gives_its_sequence_for_each_type_and_page_size() {
    let (st, items) = seeded_search();
    for word in [true, false] {
        for kind in TYPES {
            // The rank of a row with `sort=relevance`, for the check that a
            // sort does not change it.
            let ranks: HashMap<String, f64> = if word {
                let (rows, _) =
                    search_walk(&st, &search_base(kind, true, Some("relevance"), None), 100).await;
                rows.iter()
                    .map(|r| (row_label(r), r["rank"].as_f64().expect("rank")))
                    .collect()
            } else {
                HashMap::new()
            };
            for sort in [SortKind::Name, SortKind::Date] {
                for reverse in [false, true] {
                    let base = search_base(
                        kind,
                        word,
                        Some(sort.name()),
                        Some(if reverse { "true" } else { "false" }),
                    );
                    let want = expected_sequence(&items, &kinds_of(kind), sort, reverse);
                    for limit in [1, 3, 7, 100] {
                        let (rows, _) = search_walk(&st, &base, limit).await;
                        assert_eq!(
                            labels(&rows),
                            want,
                            "ADR 0084 §2, §3 and §6: {base} at limit {limit} must give the sort \
                             sequence with no repeat and no loss. Fix search_sorted in \
                             src/search.rs or the cursor in src/query.rs."
                        );
                    }
                    let (rows, _) = search_walk(&st, &base, 100).await;
                    for row in &rows {
                        let item = items
                            .iter()
                            .find(|i| i.label() == row_label(row))
                            .expect("a seeded row");
                        let rank = row["rank"].as_f64().expect("rank");
                        if word {
                            assert_eq!(
                                Some(&rank),
                                ranks.get(&item.label()),
                                "ADR 0084 §1: {base} must keep the rank of {}",
                                item.label()
                            );
                        } else {
                            assert!(
                                rank == 0.0,
                                "ADR 0080 §6: a search with no word gives rank 0 on {base}"
                            );
                        }
                        // ADR 0084 §10: only the sort of the request gives its value.
                        let (name_field, date_field) = (&row["sort_name"], &row["sort_date"]);
                        match sort {
                            SortKind::Name => {
                                assert!(
                                    date_field.is_null(),
                                    "ADR 0084 §10: {base} must not give sort_date"
                                );
                                assert_eq!(
                                    name_field.as_str().map(str::to_owned),
                                    item.name_key.clone(),
                                    "ADR 0084 §10: sort_name of {} on {base}",
                                    item.label()
                                );
                            }
                            SortKind::Date => {
                                assert!(
                                    name_field.is_null(),
                                    "ADR 0084 §10: {base} must not give sort_name"
                                );
                                assert_eq!(
                                    date_field.as_i64(),
                                    item.date,
                                    "ADR 0084 §10: sort_date of {} on {base}, absent with no date",
                                    item.label()
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn every_page_break_of_a_merged_search_gives_each_row_one_time() {
    // The break falls on each position, so it also falls on the boundary
    // between keyed and unkeyed rows, and on a tie across a feed and a track.
    let (st, items) = seeded_search();
    for word in [true, false] {
        for sort in [SortKind::Name, SortKind::Date] {
            for reverse in ["false", "true"] {
                let base = search_base(None, word, Some(sort.name()), Some(reverse));
                let want = expected_sequence(&items, &kinds_of(None), sort, reverse == "true");
                for limit in 1..=want.len() + 1 {
                    let (rows, _) = search_walk(&st, &base, limit).await;
                    assert_eq!(
                        labels(&rows),
                        want,
                        "ADR 0084 §6 and §9: {base} at limit {limit} must give each row one \
                         time. Fix the keyset condition in search_sorted in src/search.rs."
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn a_row_with_no_key_comes_last_in_both_directions_and_the_cursor_names_it() {
    let (st, items) = seeded_search();
    for sort in [SortKind::Name, SortKind::Date] {
        for reverse in ["false", "true"] {
            let base = search_base(None, false, Some(sort.name()), Some(reverse));
            let (rows, _) = search_walk(&st, &base, 100).await;
            let want = labels(&rows);
            let keyed = items
                .iter()
                .filter(|i| kinds_of(None).contains(&i.kind) && sort.key(i).is_some())
                .count();
            let tail: Vec<&SeedItem> = want
                .iter()
                .skip(keyed)
                .map(|l| items.iter().find(|i| &i.label() == l).expect("seeded row"))
                .collect();
            assert!(
                !tail.is_empty() && tail.iter().all(|i| sort.key(i).is_none()),
                "ADR 0084 §6: each row with no key must come last on {base}"
            );
            // The first page ends on the last keyed row. Its cursor holds a
            // key. The next page starts with a row with no key.
            let (status, page) = get(&st, &format!("{base}&limit={keyed}")).await;
            assert_eq!(status, StatusCode::OK, "{base}: {page}");
            let cursor = page["pagination"]["cursor"].as_str().expect("cursor");
            // The usual date order with no word keeps the `d` cursor.
            let legacy = sort == SortKind::Date && reverse == "false";
            let body = |c: &str| {
                if legacy {
                    decoded(c)
                } else {
                    decoded(c.strip_prefix("s.").expect("keyed mark"))
                }
            };
            assert!(
                if legacy {
                    body(cursor).starts_with('d') && !body(cursor).starts_with("d\0")
                } else {
                    body(cursor).contains("\0k\0")
                },
                "ADR 0084 §9: the cursor of a keyed row holds its key on {base}"
            );
            // A page of one row ends on a row with no key. Its cursor names that state.
            let (_, page2) = get(&st, &format!("{base}&limit=1&cursor={cursor}")).await;
            let cursor2 = page2["pagination"]["cursor"].as_str().expect("cursor 2");
            assert!(
                if legacy {
                    body(cursor2).starts_with("d\0")
                } else {
                    body(cursor2).contains("\0n\0")
                },
                "ADR 0084 §9: the cursor of a row with no key names that state on {base}"
            );
        }
    }
}

#[tokio::test]
async fn a_default_request_keeps_its_sequence_fields_and_cursor_form() {
    let (st, items) = seeded_search();

    // With a word: the rank order, as the function of the release before this
    // ADR gives it, and the rank cursor.
    let (status, page) = get(&st, "/v1/search?q=common&type=feed&limit=2").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let first_rows = labels(page["data"].as_array().expect("data"));
    let (all, _) = search_walk(&st, "/v1/search?q=common&type=feed", 100).await;
    assert_eq!(
        first_rows,
        labels(&all)[..2].to_vec(),
        "ADR 0084 §4: a request with neither parameter keeps the rank order"
    );
    let (explicit, _) = search_walk(&st, "/v1/search?q=common&type=feed&sort=relevance", 100).await;
    assert_eq!(
        labels(&explicit),
        labels(&all),
        "ADR 0084 §4: sort=relevance is the default sort with a word"
    );
    let cursor = page["pagination"]["cursor"].as_str().expect("rank cursor");
    let body = decoded(cursor);
    let parts: Vec<&str> = body.split('\0').collect();
    assert!(
        parts.len() == 2
            && parts[0].parse::<u64>().is_ok()
            && parts[1].parse::<i64>().is_ok()
            && !cursor.contains('.'),
        "ADR 0084 §9: the rank cursor keeps its form, got {body:?}"
    );
    for row in &all {
        assert!(
            row["sort_name"].is_null() && row["sort_date"].is_null(),
            "ADR 0084 §10: a request with no sort gives neither sort_name nor sort_date: {row}"
        );
    }

    // With no word: newest first, rowid order for the rows with no date, and
    // the `d` cursor.
    let feeds_dated: Vec<String> = ["fd-1", "fd-8", "fd-3", "fd-4", "fd-5", "fd-9", "fd-2"]
        .iter()
        .map(|g| format!("feed:{g}"))
        .collect();
    let mut undated = [
        ("fd-6", stophammer::search::rowid_for("feed", "fd-6")),
        ("fd-7", stophammer::search::rowid_for("feed", "fd-7")),
    ];
    undated.sort_by_key(|(_, rowid)| *rowid);
    let mut want = feeds_dated;
    // `fd-4` and `fd-5` share a date, so they come in rowid order.
    let (i4, i5) = (
        stophammer::search::rowid_for("feed", "fd-4"),
        stophammer::search::rowid_for("feed", "fd-5"),
    );
    if i4 > i5 {
        want.swap(3, 4);
    }
    want.extend(undated.iter().map(|(g, _)| format!("feed:{g}")));
    let (rows, cursors) = search_walk(&st, "/v1/search?host=example.com&type=feed", 3).await;
    assert_eq!(
        labels(&rows),
        want,
        "ADR 0084 §4: a request with no word keeps the date order"
    );
    for row in &rows {
        assert!(
            row["sort_name"].is_null() && row["sort_date"].is_null(),
            "ADR 0084 §10: a no-word request with no sort gives no sort_date: {row}"
        );
        assert!(row["rank"].as_f64() == Some(0.0), "rank stays 0: {row}");
    }
    let first_cursor = decoded(&cursors[0]);
    let last_of_page = &want[2];
    let guid = last_of_page.trim_start_matches("feed:");
    let date = items.iter().find(|i| i.id == guid).expect("seed").date;
    assert_eq!(
        first_cursor,
        format!(
            "d{}\0{}",
            date.expect("a dated row"),
            stophammer::search::rowid_for("feed", guid)
        ),
        "ADR 0084 §9: the d cursor keeps its form"
    );

    // `reverse=false` is the usual direction.
    let (plain, _) = search_walk(&st, "/v1/search?q=common&reverse=false", 100).await;
    let (usual, _) = search_walk(&st, "/v1/search?q=common", 100).await;
    assert_eq!(
        labels(&plain),
        labels(&usual),
        "ADR 0084 §3: reverse=false is the usual direction, also with relevance"
    );
}

#[tokio::test]
async fn the_rank_order_of_a_default_request_equals_the_function_of_the_search_module() {
    let (st, _items) = seeded_search();
    let direct: Vec<String> = {
        let writer = st.db.writer();
        let db = writer.lock().expect("db lock");
        stophammer::search::search_filtered(
            &db,
            "common",
            &stophammer::search::SearchFilters::default(),
            Some("feed"),
            100,
            None,
            None,
        )
        .expect("search")
        .into_iter()
        .map(|r| format!("{}:{}", r.entity_type, r.entity_id))
        .collect()
    };
    let (rows, _) = search_walk(&st, "/v1/search?q=common&type=feed", 4).await;
    assert_eq!(
        labels(&rows),
        direct,
        "ADR 0084 §4: the default sort with a word keeps the query of search_filtered"
    );
}

#[tokio::test]
async fn the_defaults_with_reverse_pick_the_expected_sort() {
    let (st, items) = seeded_search();
    // No word, reverse=true, no sort: the date sort, oldest first.
    let (rows, _) = search_walk(&st, "/v1/search?host=example.com&reverse=true", 4).await;
    assert_eq!(
        labels(&rows),
        expected_sequence(&items, &kinds_of(None), SortKind::Date, true),
        "ADR 0084 §3: no word, reverse=true and no sort gives the date sort, oldest first"
    );
    for row in &rows {
        assert!(
            row["sort_date"].is_null() && row["sort_name"].is_null(),
            "ADR 0084 §10: a request with no sort gives neither field: {row}"
        );
    }
    // Explicit sort=date with no word keeps the default cursor and gives the value.
    let (rows, cursors) =
        search_walk(&st, "/v1/search?host=example.com&sort=date&type=feed", 3).await;
    assert_eq!(
        labels(&rows),
        expected_sequence(&items, &["feed"], SortKind::Date, false),
        "sort=date with no word is the default sort"
    );
    assert!(
        cursors.iter().all(|c| !c.contains('.')),
        "ADR 0084 §9: the default sort keeps the d cursor form"
    );
    assert!(
        rows.iter().any(|r| r["sort_date"].is_i64())
            && rows.iter().any(|r| r["sort_date"].is_null()),
        "ADR 0084 §10: an explicit sort=date gives sort_date, absent on an undated row"
    );
    // A default cursor continues the explicit default sort, and the reverse.
    let (_, page) = get(&st, "/v1/search?host=example.com&type=feed&limit=2").await;
    let c = page["pagination"]["cursor"].as_str().expect("cursor");
    let (status, next) = get(
        &st,
        &format!("/v1/search?host=example.com&type=feed&sort=date&limit=2&cursor={c}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{next}");
    let (_, page) = get(&st, "/v1/search?q=common&type=feed&limit=2").await;
    let c = page["pagination"]["cursor"].as_str().expect("cursor");
    let (status, next) = get(
        &st,
        &format!("/v1/search?q=common&type=feed&sort=relevance&limit=2&cursor={c}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{next}");
}

#[tokio::test]
async fn the_publisher_type_sorts_by_the_key_of_its_feed() {
    let (st, items) = seeded_search();
    let (rows, _) = search_walk(
        &st,
        "/v1/search?host=example.com&type=publisher&sort=date",
        1,
    )
    .await;
    let got = labels(&rows);
    assert_eq!(
        got[0], "publisher:pb-2",
        "ADR 0084 §2: the only publisher with a date comes first on sort=date"
    );
    assert_eq!(
        got,
        expected_sequence(&items, &["publisher"], SortKind::Date, false),
        "undated publishers come in rowid order"
    );
}

async fn error_of(st: &Arc<stophammer::api::AppState>, uri: &str) -> (String, Option<String>) {
    let (status, body) = get(st, uri).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "GET {uri} gave {body}");
    (
        body["code"].as_str().expect("a code").to_owned(),
        body["filter"].as_str().map(str::to_owned),
    )
}

#[tokio::test]
async fn each_error_code_has_its_condition() {
    let (st, _items) = seeded_search();
    for (uri, code, filter) in [
        (
            "/v1/search?q=common&sort=newest",
            "unsupported_sort",
            Some("sort"),
        ),
        (
            "/v1/search?q=common&sort=",
            "unsupported_sort",
            Some("sort"),
        ),
        (
            "/v1/search?q=common&sort=name&reverse=yes",
            "unsupported_sort",
            Some("reverse"),
        ),
        (
            "/v1/search?q=common&reverse=",
            "unsupported_sort",
            Some("reverse"),
        ),
        (
            "/v1/search?host=example.com&reverse=1",
            "unsupported_sort",
            Some("reverse"),
        ),
        (
            "/v1/search?host=example.com&sort=relevance",
            "relevance_needs_word",
            Some("sort"),
        ),
        (
            "/v1/search?q=%22%22&host=example.com&sort=relevance",
            "relevance_needs_word",
            Some("sort"),
        ),
        (
            "/v1/search?q=common&reverse=true",
            "relevance_cannot_reverse",
            Some("reverse"),
        ),
        (
            "/v1/search?q=common&sort=relevance&reverse=true",
            "relevance_cannot_reverse",
            Some("reverse"),
        ),
        // The ADR 0080 §4 checks come first.
        ("/v1/search?sort=name", "missing_positive_term", None),
        ("/v1/search?sort=relevance", "missing_positive_term", None),
        (
            "/v1/search?sort=bogus&reverse=maybe",
            "missing_positive_term",
            None,
        ),
        (
            "/v1/search?q=common&type=publisher&release=x&sort=bogus",
            "unsupported_filter",
            Some("release"),
        ),
        (
            "/v1/search?q=common&kind=album&kind=ep&sort=bogus",
            "invalid_filter",
            Some("kind"),
        ),
        // The sort value comes before the direction and before the word checks.
        (
            "/v1/search?host=example.com&sort=bogus&reverse=maybe",
            "unsupported_sort",
            Some("sort"),
        ),
    ] {
        let (got_code, got_filter) = error_of(&st, uri).await;
        assert_eq!(
            (got_code.as_str(), got_filter.as_deref()),
            (code, filter),
            "ADR 0084 §5 and ADR 0080 §4: GET {uri} must give this code. Fix \
             parse_search_request in src/query.rs."
        );
    }
    // These requests are valid.
    for uri in [
        "/v1/search?q=common&reverse=false",
        "/v1/search?q=common&sort=name&reverse=true",
        "/v1/search?host=example.com&reverse=true",
        "/v1/search?host=example.com&sort=date&reverse=true",
        "/v1/search?q=common&sort=relevance&reverse=false",
    ] {
        let (status, body) = get(&st, uri).await;
        assert_eq!(status, StatusCode::OK, "GET {uri} must give 200: {body}");
    }
    // An unknown name is still refused, and `sort` and `reverse` are not.
    let (code, _) = error_of(&st, "/v1/search?q=common&order=asc").await;
    assert_eq!(code, "unsupported_filter", "ADR 0080 §4: an unknown name");
    let (status, body) = get(&st, "/v1/search?q=common&sort=name&sort=date").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "duplicate sort: {body}");
}

fn key_cursor(sort: &str, direction: &str, rowid: i64, state: &str, key: &str) -> String {
    format!(
        "s.{}",
        b64(&format!("{sort}\0{direction}\0{rowid}\0{state}\0{key}"))
    )
}

async fn cursor_of(st: &Arc<stophammer::api::AppState>, base: &str) -> String {
    let (status, page) = get(st, &format!("{base}&limit=2")).await;
    assert_eq!(status, StatusCode::OK, "{base}: {page}");
    page["pagination"]["cursor"]
        .as_str()
        .expect("a cursor")
        .to_owned()
}

#[tokio::test]
async fn a_cursor_with_another_sort_or_direction_gives_400() {
    let (st, _items) = seeded_search();
    // A cursor of each request form, with its own base.
    let forms = [
        ("name", "/v1/search?q=common&type=feed&sort=name"),
        (
            "name reverse",
            "/v1/search?q=common&type=feed&sort=name&reverse=true",
        ),
        ("date word", "/v1/search?q=common&type=feed&sort=date"),
        (
            "date word reverse",
            "/v1/search?q=common&type=feed&sort=date&reverse=true",
        ),
        (
            "name no word",
            "/v1/search?host=example.com&type=feed&sort=name",
        ),
        (
            "date reverse no word",
            "/v1/search?host=example.com&type=feed&reverse=true",
        ),
        ("rank", "/v1/search?q=common&type=feed"),
        ("d", "/v1/search?host=example.com&type=feed"),
    ];
    let mut cursors = Vec::new();
    for (name, base) in forms {
        cursors.push((name, base, cursor_of(&st, base).await));
    }
    // Two requests with the same sort and direction share a cursor form.
    let same_order = |a: &str, b: &str| {
        let order = |n: &str| match n {
            "name" | "name no word" => 1,
            "date word reverse" | "date reverse no word" => 2,
            other => other.len() + 10,
        };
        order(a) == order(b)
    };
    for (own_name, own_base, cursor) in &cursors {
        // The own base continues.
        let (status, body) = get(&st, &format!("{own_base}&limit=2&cursor={cursor}")).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "ADR 0084 §9: the cursor of `{own_name}` must continue its own request: {body}"
        );
        for (other_name, other_base, _) in &cursors {
            if same_order(other_name, own_name) {
                continue;
            }
            // `date word` and `name` differ in sort. A pair with the same
            // form of cursor and the same parameters is not tested here.
            let (status, body) = get(&st, &format!("{other_base}&limit=2&cursor={cursor}")).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "ADR 0084 §9: the cursor of `{own_name}` on the request `{other_name}` must \
                 give 400, got {body}. Fix decode_search_key_cursor in src/query.rs."
            );
        }
    }
    // Explicit sort=name on the cursors of the default sorts.
    let rank = &cursors[6].2;
    let d = &cursors[7].2;
    for uri in [
        format!("/v1/search?q=common&sort=name&cursor={rank}"),
        format!("/v1/search?q=common&sort=date&cursor={rank}"),
        format!("/v1/search?host=example.com&sort=name&cursor={d}"),
        format!("/v1/search?host=example.com&reverse=true&cursor={d}"),
        format!("/v1/search?q=common&sort=date&reverse=true&cursor={d}"),
    ] {
        let (status, body) = get(&st, &uri).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "ADR 0084 §9: a cursor from before ADR 0084 is valid only for the default sort: \
             GET {uri} gave {body}"
        );
    }
}

#[tokio::test]
async fn a_malformed_search_cursor_gives_400() {
    let (st, _items) = seeded_search();
    let good_rowid = 5;
    let bad = [
        "s.".to_owned(),
        "s.!!!".to_owned(),
        "s.AAAA".to_owned(),
        format!("s.{}", b64("name\0usual\0x")),
        key_cursor("name", "usual", good_rowid, "k", "a").replace("s.", "s.s."),
        key_cursor("sideways", "usual", good_rowid, "k", "a"),
        key_cursor("name", "up", good_rowid, "k", "a"),
        key_cursor("name", "usual", 0, "q", "a"),
        key_cursor("name", "usual", 0, "n", "a"),
        key_cursor("name", "usual", 0, "k", "a").replace(&b64("name\0usual\0"), "!"),
        format!("s.{}", b64("name\0usual\0notanumber\0k\0a")),
        format!("s.{}", b64("name\0usual\0\0k\0a")),
        format!("s.{}", b64("name\0usual\0-9223372036854775809\0k\0a")),
        format!("s.{}", b64("name\0usual\x001\0k")),
        format!("s.{}", b64("")),
        // A date key that is not an integer.
        key_cursor("date", "usual", 0, "k", "yesterday"),
        key_cursor("date", "usual", 0, "k", ""),
        // The usual-form legacy shapes with the mark in front.
        format!("s.{}", b64(&format!("{}\x001", 1.5_f64.to_bits()))),
        format!("s.{}", b64("d100\x001")),
    ];
    for cursor in &bad {
        for base in [
            "/v1/search?q=common&sort=name",
            "/v1/search?host=example.com&sort=date&reverse=true",
            "/v1/search?host=example.com&sort=name&reverse=true",
        ] {
            let (status, body) = get(&st, &format!("{base}&cursor={cursor}")).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "ADR 0084 §9: GET {base} with the cursor {cursor:?} must give a clean 400, \
                 got {body}. Fix decode_search_key_cursor in src/query.rs."
            );
        }
    }
    // The mark is never the start of a cursor of the default sort.
    for uri in [
        format!(
            "/v1/search?q=common&cursor={}",
            key_cursor("name", "usual", 1, "k", "a")
        ),
        format!(
            "/v1/search?host=example.com&cursor={}",
            key_cursor("date", "usual", 1, "k", "1")
        ),
    ] {
        let (status, body) = get(&st, &uri).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "ADR 0084 §9: a keyed cursor on the default sort must give 400: {body}"
        );
    }
    // A legacy cursor on a keyed request.
    for uri in [
        format!("/v1/search?q=common&sort=name&cursor={}", b64("1\x001")),
        format!("/v1/search?q=common&sort=name&cursor={}", b64("d100\x001")),
        "/v1/search?q=common&sort=name&cursor=not-base64!".to_owned(),
    ] {
        let (status, _) = get(&st, &uri).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "legacy cursor on a keyed sort: {uri}"
        );
    }
}

#[tokio::test]
async fn a_key_that_looks_like_a_mark_or_holds_a_nul_is_only_text() {
    let (st, items) = seeded_search();
    // The titles `s.mark` and `r.mark` round-trip through the cursor, at each
    // page size.
    for sort in ["name"] {
        for reverse in ["false", "true"] {
            let base = format!("/v1/search?q=common&sort={sort}&reverse={reverse}");
            let want =
                expected_sequence(&items, &kinds_of(None), SortKind::Name, reverse == "true");
            for limit in [1, 2] {
                let (rows, _) = search_walk(&st, &base, limit).await;
                assert_eq!(labels(&rows), want, "{base} at limit {limit}");
            }
        }
    }
    // A hand-made cursor with a NUL in the key is read as the key. It admits
    // each row after that key.
    let cursor = key_cursor("name", "usual", 1, "k", "alpha\0b");
    let (status, body) = get(
        &st,
        &format!("/v1/search?q=common&type=feed&sort=name&cursor={cursor}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a key with a NUL is text: {body}");
    let got = labels(body["data"].as_array().expect("data"));
    assert!(
        !got.is_empty() && !got.contains(&"feed:fd-2".to_owned()),
        "the row with the key `alpha` is before the key `alpha NUL b`: {got:?}"
    );
}

#[tokio::test]
async fn capabilities_list_sort_last() {
    let (st, _items) = seeded_search();
    let (status, body) = get(&st, "/v1/node/capabilities").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let caps: Vec<&str> = body["capabilities"]
        .as_array()
        .expect("capabilities")
        .iter()
        .map(|c| c.as_str().expect("string"))
        .collect();
    assert_eq!(
        caps.last(),
        Some(&"sort"),
        "ADR 0084 §11: `sort` is the last capability, got {caps:?}. Add it last in \
         handle_capabilities in src/query.rs."
    );
    for old in ["query", "search", "sync", "push", "name_lists"] {
        assert!(caps.contains(&old), "ADR 0084 §11: {old} stays listed");
    }
}

#[test]
fn the_contract_and_the_fixture_state_the_new_parameters_and_fields() {
    let doc = serde_json::to_value(stophammer::openapi::primary_document()).expect("serialize");
    let params: Vec<&str> = doc["paths"]["/v1/search"]["get"]["parameters"]
        .as_array()
        .expect("parameters")
        .iter()
        .map(|p| p["name"].as_str().expect("name"))
        .collect();
    for name in ["sort", "reverse"] {
        assert!(
            params.contains(&name),
            "ADR 0084 §2 and §3: /v1/search must declare {name} in src/openapi.rs"
        );
    }
    let props = &doc["components"]["schemas"]["SearchResponseItem"]["properties"];
    for field in ["sort_name", "sort_date"] {
        assert!(
            props.get(field).is_some(),
            "ADR 0084 §10: SearchResponseItem must declare {field}"
        );
    }
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/v1-schema-fields.json")).expect("fixture json");
    let listed: Vec<&str> = fixture["SearchResponseItem"]["properties"]
        .as_array()
        .expect("fixture properties")
        .iter()
        .map(|v| v.as_str().expect("name"))
        .collect();
    for field in ["sort_name", "sort_date"] {
        assert!(
            listed.contains(&field),
            "ADR 0084 criterion 10: add {field} to SearchResponseItem in \
             tests/fixtures/v1-schema-fields.json"
        );
    }
}
