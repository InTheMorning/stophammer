//! ADR 0090: the order key of a name, and the lists that sort by it.
//!
//! Checks that:
//! - `model::sort_key` does not change, and the identity of a name (`key` of a
//!   `name` row, the `artist_key` filter) still uses it (§1),
//! - `model::order_key` folds accents, pads digit runs and groups a text that
//!   does not start with a letter under `#` (§2),
//! - `GET /v1/feeds/by-name`, `GET /v1/artist-names` and `GET /v1/search` with
//!   `sort=name` give the `#` group first, then A to Z with folded accents (§3),
//! - `open_db` writes the order key into each null `title_sort` (§4).

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use rusqlite::Connection;
use serde_json::{Value, json};
use tower::ServiceExt;

use stophammer::model::{Feed, Track, order_key, sort_key};

const TOKEN: &str = "adr0090-order-key-crawl-token";

// ── Pure cases ──────────────────────────────────────────────────────────────

#[test]
fn sort_key_trims_joins_white_space_lowercases_and_keeps_accents() {
    assert_eq!(
        sort_key("  Zëmar \t Red  "),
        "zëmar red",
        "ADR 0090 §1: sort_key must trim, join white space, lowercase and keep the accent."
    );
    assert_eq!(
        sort_key("Élan"),
        "élan",
        "ADR 0090 §1: sort_key must not fold an accent."
    );
}

#[test]
fn order_key_starts_with_the_sort_key() {
    assert_eq!(
        order_key("  The   Paul  "),
        "the paul",
        "ADR 0090 §2: the order key of a plain text must equal its sort key."
    );
}

#[test]
fn order_key_folds_accents_and_letters_with_no_decomposition() {
    for (text, want) in [
        ("Élan", "elan"),
        ("Øystein Berge", "oystein berge"),
        ("Straße", "strasse"),
        ("Æther", "aether"),
    ] {
        assert_eq!(
            order_key(text),
            want,
            "ADR 0090 §2: order_key({text:?}) must fold the accent."
        );
    }
}

#[test]
fn order_key_pads_each_run_of_digits_to_20() {
    let want = format!("track {}2", "0".repeat(19));
    assert_eq!(
        order_key("track 2"),
        want,
        "ADR 0090 §2: a run of digits must get zeros at its start, to 20 digits."
    );
    assert!(
        order_key("track 2") < order_key("track 10"),
        "ADR 0090 §2: track 2 must come before track 10."
    );
    let long = "1".repeat(25);
    assert_eq!(
        order_key(&format!("a{long}")),
        format!("a{long}"),
        "ADR 0090 §2: a run longer than 20 digits must not change."
    );
    assert_eq!(
        order_key("a1b22"),
        format!("a{}1b{}22", "0".repeat(19), "0".repeat(18)),
        "ADR 0090 §2: each run of digits must get its own padding."
    );
}

#[test]
fn order_key_puts_a_text_with_no_first_letter_in_the_hash_group() {
    for text in ["✳ZBD✳Chill", "1999", "Ключ", "🖖", "  (Intro)"] {
        assert!(
            order_key(text).starts_with('#'),
            "ADR 0090 §2: order_key({text:?}) = {:?} must start with #.",
            order_key(text)
        );
    }
    assert!(
        !order_key("Élan").starts_with('#'),
        "ADR 0090 §2: a text whose first folded character is a letter must not get #."
    );
}

#[test]
fn order_key_of_an_empty_or_blank_text_is_empty() {
    assert_eq!(
        order_key(""),
        "",
        "ADR 0090 §2: an empty text must give an empty key."
    );
    assert_eq!(
        order_key(" \t  "),
        "",
        "ADR 0090 §2: a text of white space must give an empty key."
    );
}

#[test]
fn each_hash_key_comes_before_each_letter_key() {
    let hash = ["✳ZBD✳Chill", "1999", "Ключ", "🖖"].map(order_key);
    let letters = ["Aaa", "Élan", "Oscar", "Zed", "Zëmar"].map(order_key);
    for h in &hash {
        for l in &letters {
            assert!(
                h < l,
                "ADR 0090 §2: the key {h:?} must come before the key {l:?} by code point."
            );
        }
    }
}

// ── Seeding helpers ─────────────────────────────────────────────────────────

fn state(db: Arc<Mutex<Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("adr0090-order-key-signer"));
    let chain = stophammer::verify::build_chain(
        &stophammer::verify::ChainSpec { names: vec![] },
        TOKEN.to_string(),
    );
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        node_pubkey_hex: signer.pubkey_hex().to_string(),
        signer,
        admin_token: "adr0090-order-key-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn get(st: &Arc<stophammer::api::AppState>, uri: &str) -> (StatusCode, Value) {
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
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Reads each page of `uri` and gives `field` of each row in sequence.
async fn walk(
    st: &Arc<stophammer::api::AppState>,
    uri: &str,
    limit: usize,
    field: &str,
) -> Vec<String> {
    let mut rows = Vec::new();
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
            cursor = Some(
                body["pagination"]["cursor"]
                    .as_str()
                    .expect("a cursor")
                    .to_string(),
            );
        } else {
            return rows;
        }
    }
    panic!("the walk of {uri} did not end");
}

fn music_feed(guid: &str, title: &str) -> Feed {
    Feed {
        feed_guid: guid.into(),
        feed_url: format!("https://example.com/{guid}.xml"),
        title: title.into(),
        title_lower: title.to_lowercase(),
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
        newest_item_at: Some(5),
        oldest_item_at: None,
        created_at: 1_000_000,
        updated_at: 1_000_000,
        raw_medium: Some("music".into()),
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

/// Names in the order of the order key: the `#` group, then A to Z.
const NAMES_IN_ORDER: [&str; 8] = [
    "1999",
    "Ключ",
    "✳ZBD✳Chill",
    "🖖",
    "Élan",
    "Oscar",
    "Øystein",
    "Paul",
];

/// A shuffled seed order, so the test does not rely on the insert order.
const NAMES_SEEDED: [&str; 8] = [
    "Paul",
    "🖖",
    "Øystein",
    "Élan",
    "1999",
    "Oscar",
    "✳ZBD✳Chill",
    "Ключ",
];

fn titles_state() -> Arc<stophammer::api::AppState> {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("db lock");
        for (i, title) in NAMES_SEEDED.iter().enumerate() {
            stophammer::db::upsert_feed(&conn, &music_feed(&format!("t-{i}"), title))
                .expect("upsert feed");
        }
    }
    state(db)
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

// ── §3: GET /v1/feeds/by-name ───────────────────────────────────────────────

#[tokio::test]
async fn by_name_gives_the_hash_group_first_then_a_to_z_with_folded_accents() {
    let st = titles_state();
    for limit in [1, 3, 100] {
        let titles = walk(&st, "/v1/feeds/by-name", limit, "title").await;
        assert_eq!(
            titles,
            strs(&NAMES_IN_ORDER),
            "ADR 0090 §3: by-name at limit {limit} must give the # group first, then A to Z. \
             Oystein must sit between Oscar and Paul."
        );
    }
}

#[tokio::test]
async fn by_name_sort_name_is_the_order_key() {
    let st = titles_state();
    let (_, body) = get(&st, "/v1/feeds/by-name?limit=100").await;
    for row in body["data"].as_array().expect("data array") {
        let title = row["title"].as_str().expect("title");
        assert_eq!(
            row["sort_name"].as_str(),
            Some(order_key(title).as_str()),
            "ADR 0090 §3: sort_name of {title:?} must be its order key."
        );
    }
}

#[tokio::test]
async fn by_name_from_hash_starts_at_the_first_row_of_the_hash_group() {
    let st = titles_state();
    let titles = walk(&st, "/v1/feeds/by-name?from=%23", 3, "title").await;
    assert_eq!(
        titles,
        strs(&NAMES_IN_ORDER),
        "ADR 0090 §3: from=# must start at the first row of the # group."
    );
}

#[tokio::test]
async fn by_name_from_a_letter_reaches_the_folded_name() {
    let st = titles_state();
    let titles = walk(&st, "/v1/feeds/by-name?from=O", 2, "title").await;
    assert_eq!(
        titles,
        strs(&["Oscar", "Øystein", "Paul"]),
        "ADR 0090 §3: from=O must start at the first key of the O letter and reach Øystein."
    );
    let titles = walk(&st, "/v1/feeds/by-name?from=Oy", 2, "title").await;
    assert_eq!(
        titles,
        strs(&["Øystein", "Paul"]),
        "ADR 0090 §3: from=Oy must start at Øystein, because its key is oystein."
    );
}

#[tokio::test]
async fn by_name_reverse_from_hash_gives_the_hash_group_only_from_its_last_row() {
    let st = titles_state();
    let titles = walk(&st, "/v1/feeds/by-name?reverse=true&from=%23", 2, "title").await;
    assert_eq!(
        titles,
        strs(&["🖖", "✳ZBD✳Chill", "Ключ", "1999"]),
        "ADR 0090 §3: reverse with from=# must give the # group only, from its last row."
    );
}

// ── §3: GET /v1/artist-names, and §1 for the key ────────────────────────────

fn artists_state() -> Arc<stophammer::api::AppState> {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("db lock");
        for (i, name) in NAMES_SEEDED
            .iter()
            .copied()
            .chain(["Zëmar Red"])
            .enumerate()
        {
            let mut feed = music_feed(&format!("a-{i}"), &format!("Release {i}"));
            feed.release_artist = Some(name.into());
            stophammer::db::upsert_feed(&conn, &feed).expect("upsert feed");
        }
    }
    state(db)
}

#[tokio::test]
async fn artist_names_group_and_fold_in_sort_name_while_key_keeps_the_sort_key() {
    let st = artists_state();
    let mut want_names: Vec<&str> = NAMES_IN_ORDER.to_vec();
    want_names.push("Zëmar Red");
    let want_sort: Vec<String> = want_names.iter().map(|n| order_key(n)).collect();
    let want_key: Vec<String> = want_names.iter().map(|n| sort_key(n)).collect();
    for limit in [1, 4, 100] {
        let sort_names = walk(&st, "/v1/artist-names", limit, "sort_name").await;
        assert_eq!(
            sort_names, want_sort,
            "ADR 0090 §3: artist-names at limit {limit} must give the # group first, \
             then A to Z in sort_name."
        );
        let keys = walk(&st, "/v1/artist-names", limit, "key").await;
        assert_eq!(
            keys, want_key,
            "ADR 0090 §1: key must stay the sort key, with the accent."
        );
    }
    assert!(
        want_key.contains(&"zëmar red".to_string()) && want_key.contains(&"øystein".to_string()),
        "ADR 0090 §1: the expected keys must keep accents."
    );
}

fn ingest_payload(guid: &str, author: &str) -> Value {
    let url = format!("https://example.com/{guid}.xml");
    json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": format!("Feed {guid}"),
            "raw_medium": "music",
            "explicit": false,
            "author_name": author,
        }
    })
}

#[tokio::test]
async fn the_artist_key_filter_still_uses_the_sort_key_with_the_accent() {
    let st = state(common::test_db_arc());
    for (guid, author) in [("z-1", "Zëmar Red"), ("z-2", "Other Artist")] {
        let resp = stophammer::api::build_router(Arc::clone(&st))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/ingest/feed")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ingest_payload(guid, author)).expect("serialize"),
                    ))
                    .expect("build request"),
            )
            .await
            .expect("send request");
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        let body: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(body["accepted"], true, "ingest must be accepted: {body}");
    }

    let (_, names) = get(&st, "/v1/artist-names?limit=100").await;
    let row = names["data"]
        .as_array()
        .expect("data array")
        .iter()
        .find(|r| r["kind"] == "name" && r["key"] == "zëmar red")
        .unwrap_or_else(|| panic!("ADR 0090 §1: no name row with key zëmar red in {names}"));
    assert_eq!(
        row["sort_name"], "zemar red",
        "ADR 0090 §3: sort_name must fold the accent while key keeps it."
    );

    let (status, body) = get(&st, "/v1/feeds/by-name?artist_key=z%C3%ABmar%20red").await;
    assert_eq!(status, StatusCode::OK, "artist_key request gave {body}");
    let guids: Vec<&str> = body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|r| r["feed_guid"].as_str().expect("feed_guid"))
        .collect();
    assert_eq!(
        guids,
        vec!["z-1"],
        "ADR 0090 §1: artist_key=zëmar red must give the release of that name."
    );
}

// ── §3: GET /v1/search with sort=name ───────────────────────────────────────

#[tokio::test]
async fn search_sort_name_gives_the_order_key_sequence() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("db lock");
        for (i, title) in NAMES_SEEDED.iter().enumerate() {
            let guid = format!("s-{i}");
            stophammer::db::upsert_feed(&conn, &music_feed(&guid, title)).expect("upsert feed");
            stophammer::search::populate_search_index(&conn, "feed", &guid, title, title, "common")
                .expect("populate");
        }
    }
    let st = state(db);
    for limit in [1, 3, 100] {
        let (status, first) = get(
            &st,
            &format!("/v1/search?type=feed&q=common&sort=name&limit={limit}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "search gave {first}");
        // Walk the pages by cursor, and read the entity id of each row.
        let mut ids = Vec::new();
        let mut body = first;
        for _ in 0..200 {
            for row in body["data"].as_array().expect("data array") {
                ids.push(row["entity_id"].as_str().expect("entity_id").to_string());
            }
            if body["pagination"]["has_more"] != true {
                break;
            }
            let cursor = body["pagination"]["cursor"].as_str().expect("cursor");
            body = get(
                &st,
                &format!("/v1/search?type=feed&q=common&sort=name&limit={limit}&cursor={cursor}"),
            )
            .await
            .1;
        }
        let want: Vec<String> = NAMES_IN_ORDER
            .iter()
            .map(|name| {
                let i = NAMES_SEEDED
                    .iter()
                    .position(|n| n == name)
                    .expect("seeded name");
                format!("s-{i}")
            })
            .collect();
        assert_eq!(
            ids, want,
            "ADR 0090 §3: sort=name at limit {limit} must give the # group first, then A to Z \
             with folded accents."
        );
    }
}

// ── §4: the fill in open_db ─────────────────────────────────────────────────

#[test]
fn open_db_fills_a_null_title_sort_with_the_order_key() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("adr0090-fill.db");
    let path_str = path.to_str().expect("utf-8 path");

    let titles = ["Øystein Berge", "Track 2", "✳ZBD✳Chill"];
    let conn = stophammer::db::open_db(path_str);
    for (i, title) in titles.iter().enumerate() {
        stophammer::db::upsert_feed(&conn, &music_feed(&format!("f-{i}"), title))
            .expect("upsert feed");
        stophammer::db::upsert_track(
            &conn,
            &sample_track(&format!("f-{i}"), &format!("t-{i}"), title),
        )
        .expect("upsert track");
    }
    conn.execute("UPDATE feeds SET title_sort = NULL", [])
        .expect("clear feeds");
    conn.execute("UPDATE tracks SET title_sort = NULL", [])
        .expect("clear tracks");
    drop(conn);

    let conn = stophammer::db::open_db(path_str);
    for (i, title) in titles.iter().enumerate() {
        let feed_sort: Option<String> = conn
            .query_row(
                "SELECT title_sort FROM feeds WHERE feed_guid = ?1",
                [format!("f-{i}")],
                |r| r.get(0),
            )
            .expect("feed row");
        assert_eq!(
            feed_sort,
            Some(order_key(title)),
            "ADR 0090 §4: open_db must fill a null feeds.title_sort with the order key."
        );
        let track_sort: Option<String> = conn
            .query_row(
                "SELECT title_sort FROM tracks WHERE track_guid = ?1",
                [format!("t-{i}")],
                |r| r.get(0),
            )
            .expect("track row");
        assert_eq!(
            track_sort,
            Some(order_key(title)),
            "ADR 0090 §4: open_db must fill a null tracks.title_sort with the order key."
        );
    }
}
