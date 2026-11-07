// ADR 0085 §1 to §3, task 001: `GET /v1/feeds/by-name` takes `stated_rel` and
// `two_way_links`. Checks that:
// - `stated_rel=label` keeps only the rows that state it, in name order, over
//   all pages at each page size, in each direction, and with `from`
//   (criterion 1),
// - `two_way_links=none` keeps only the rows with `two_way_link_count` 0
//   (criterion 2),
// - each error of ADR 0068 §4 has its condition on this route, and the
//   conflicts with `artist_key` and `named_feed_guid` stay errors
//   (criterion 3),
// - a filtered request stops at the scan limit with `has_more` true and a
//   cursor after the last examined row, and a walk over the cursor finds each
//   matching row beyond the limit in both directions (criterion 4),
// - a request with no filter keeps its rows, and a filtered row has the link
//   facts of the same row on `GET /v1/feeds/recent`.
// The publishers hold real links, so `stated_rels` and `two_way_link_count`
// are real values.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::body::Body;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use stophammer::model::Feed;

const TOKEN: &str = "adr0085-crawl-token";

fn state(db: Arc<std::sync::Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("adr0085-signer"));
    let chain = stophammer::verify::build_chain(
        &stophammer::verify::ChainSpec { names: vec![] },
        TOKEN.to_string(),
    );
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        node_pubkey_hex: signer.pubkey_hex().to_string(),
        signer,
        admin_token: "adr0085-admin-token".into(),
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

async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    title: &str,
    medium: &str,
    remote_items: Value,
) {
    let url = format!("https://p.example/{guid}.xml");
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": title,
            "raw_medium": medium,
            "explicit": false,
            "author_name": "Artist",
            "remote_items": remote_items,
        }
    });
    let resp = stophammer::api::build_router(Arc::clone(st))
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
        "ingest of {guid} must succeed, got {}",
        resp.status()
    );
}

/// Stores a publisher with real links. `links` holds `(rel, two_way)` for each
/// album. A two-way album names the publisher back, and a one-way album does
/// not.
async fn store_publisher(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    title: &str,
    links: &[(&str, bool)],
) {
    let back = json!([{
        "position": 0,
        "medium": "publisher",
        "remote_feed_guid": guid,
        "remote_feed_url": format!("https://p.example/{guid}.xml"),
        "publisher_reference": true,
    }]);
    let mut entries = Vec::new();
    for (n, (rel, two_way)) in links.iter().enumerate() {
        let album = format!("alb-{guid}-{n}");
        let remote = if *two_way { back.clone() } else { json!([]) };
        ingest(st, &album, &format!("Album {guid} {n}"), "music", remote).await;
        entries.push(json!({
            "position": n,
            "medium": "music",
            "remote_feed_guid": album,
            "remote_feed_url": format!("https://p.example/{album}.xml"),
            "rel": rel,
        }));
    }
    ingest(st, guid, title, "publisher", Value::Array(entries)).await;
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn reversed(items: &[&str]) -> Vec<String> {
    items.iter().rev().map(|s| (*s).to_string()).collect()
}

/// Reads each page of `uri` with `limit`. Gives the `feed_guid` of each row in
/// sequence, and the count of pages.
async fn walk(
    st: &Arc<stophammer::api::AppState>,
    uri: &str,
    limit: usize,
) -> (Vec<String>, usize) {
    let mut rows = Vec::new();
    let mut cursor: Option<String> = None;
    for page in 1..=500 {
        let mut full = format!("{uri}&limit={limit}");
        if let Some(c) = &cursor {
            full = format!("{full}&cursor={c}");
        }
        let (status, body) = get(st, &full).await;
        assert_eq!(status, StatusCode::OK, "GET {full} gave {body}");
        for row in body["data"].as_array().expect("data array") {
            rows.push(row["feed_guid"].as_str().expect("feed_guid").to_string());
        }
        if body["pagination"]["has_more"] == true {
            cursor = Some(
                body["pagination"]["cursor"]
                    .as_str()
                    .expect(
                        "ADR 0085 §2: a page with has_more true must give a cursor. \
                         Return the position of the last examined row from filtered_scan \
                         in src/query.rs.",
                    )
                    .to_string(),
            );
        } else {
            return (rows, page);
        }
    }
    panic!("the walk of {uri} did not end");
}

// ── The sequence, the filters and the directions ────────────────────────────

/// Eight publishers in name order. The link of each one is real.
async fn named_state() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    store_publisher(&st, "p-alpha", "Alpha", &[("label", true)]).await;
    store_publisher(&st, "p-bravo", "Bravo", &[("artist", true)]).await;
    store_publisher(&st, "p-charlie", "Charlie", &[("label", false)]).await;
    store_publisher(&st, "p-delta", "Delta", &[]).await;
    store_publisher(&st, "p-echo", "Echo", &[("label", true), ("artist", true)]).await;
    store_publisher(&st, "p-foxtrot", "Foxtrot", &[("artist", false)]).await;
    store_publisher(&st, "p-golf", "Golf", &[("label", true)]).await;
    store_publisher(&st, "p-hotel", "Hotel", &[]).await;
    st
}

// A role on a one-way link is not a stated role (ADR 0068 §1), so Charlie and
// Foxtrot state none.
const LABEL: [&str; 3] = ["p-alpha", "p-echo", "p-golf"];
const ARTIST: [&str; 2] = ["p-bravo", "p-echo"];
const NO_TWO_WAY: [&str; 4] = ["p-charlie", "p-delta", "p-foxtrot", "p-hotel"];
const BASE: &str = "/v1/feeds/by-name?medium=publisher";

#[tokio::test]
async fn stated_rel_keeps_the_rows_that_state_it_at_each_page_size_and_direction() {
    let st = named_state().await;
    for (filter, expected) in [
        ("stated_rel=label", &LABEL[..]),
        ("stated_rel=artist", &ARTIST[..]),
        ("two_way_links=none", &NO_TWO_WAY[..]),
        ("stated_rel=label&two_way_links=none", &[][..]),
    ] {
        for size in 1..=9 {
            let (rows, _) = walk(&st, &format!("{BASE}&{filter}"), size).await;
            assert_eq!(
                rows,
                strs(expected),
                "ADR 0085 §1: {filter} at limit {size} must keep only the matching rows in \
                 name order. Read the page through filtered_by_name_rows in src/query.rs."
            );
            let (rows, _) = walk(&st, &format!("{BASE}&{filter}&reverse=true"), size).await;
            assert_eq!(
                rows,
                reversed(expected),
                "ADR 0085 §1: {filter} with reverse at limit {size} must give Z to A. Pass \
                 reverse to by_name_feed_rows."
            );
        }
    }
}

#[tokio::test]
async fn from_applies_to_the_first_batch_in_each_direction() {
    let st = named_state().await;
    for size in 1..=5 {
        let (rows, _) = walk(&st, &format!("{BASE}&stated_rel=label&from=c"), size).await;
        assert_eq!(
            rows,
            strs(&["p-echo", "p-golf"]),
            "ADR 0085 §1: from=c at limit {size} must start at the first title from c. \
             Apply from_floor to the first batch only."
        );
        let uri = format!("{BASE}&stated_rel=label&from=e&reverse=true");
        let (rows, _) = walk(&st, &uri, size).await;
        assert_eq!(
            rows,
            strs(&["p-echo", "p-alpha"]),
            "ADR 0085 §1: from=e with reverse at limit {size} must start at the last title \
             that starts with e. Use reverse_from_bound for the first batch."
        );
    }
}

#[tokio::test]
async fn a_page_break_inside_removed_rows_loses_and_repeats_no_row() {
    let st = named_state().await;
    // The filter removes Charlie and Delta between Bravo and Echo, so the page
    // break falls inside removed rows.
    let (status, body) = get(&st, &format!("{BASE}&stated_rel=artist&limit=1")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"][0]["feed_guid"], "p-bravo", "{body}");
    assert_eq!(body["pagination"]["has_more"], true, "{body}");
    let cursor = body["pagination"]["cursor"].as_str().expect("cursor");
    let (_, body) = get(
        &st,
        &format!("{BASE}&stated_rel=artist&limit=1&cursor={cursor}"),
    )
    .await;
    assert_eq!(
        body["data"][0]["feed_guid"], "p-echo",
        "ADR 0085 §2: the next page must skip the removed rows and give Echo: {body}"
    );
}

#[tokio::test]
async fn a_filter_with_no_match_gives_an_empty_list() {
    let st = named_state().await;
    for reverse in ["", "&reverse=true"] {
        let uri = format!("{BASE}&stated_rel=nomatch&limit=3{reverse}");
        let (status, body) = get(&st, &uri).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"].as_array().map(Vec::len), Some(0), "{uri}");
        assert_eq!(
            body["pagination"]["has_more"], false,
            "ADR 0085 §2: a scan that reaches the end of a short list must give has_more \
             false ({uri})"
        );
        assert!(body["pagination"]["cursor"].is_null(), "{uri}");
    }
}

#[tokio::test]
async fn two_way_links_none_keeps_only_rows_with_count_zero() {
    let st = named_state().await;
    let (status, body) = get(&st, &format!("{BASE}&two_way_links=none")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = body["data"].as_array().expect("data");
    assert_eq!(rows.len(), NO_TWO_WAY.len(), "{body}");
    for row in rows {
        assert_eq!(
            row["two_way_link_count"], 0,
            "ADR 0085 criterion 2: each kept row must have two_way_link_count 0: {row}"
        );
    }
}

#[tokio::test]
async fn a_filtered_row_has_the_link_facts_and_the_sort_name() {
    let st = named_state().await;
    let (_, by_name) = get(&st, &format!("{BASE}&stated_rel=label")).await;
    let (_, recent) = get(
        &st,
        "/v1/feeds/recent?medium=publisher&stated_rel=label&limit=50",
    )
    .await;
    let recent_rows = recent["data"].as_array().expect("recent data");
    for row in by_name["data"].as_array().expect("by-name data") {
        assert!(
            row["sort_name"].is_string(),
            "ADR 0070 §2: a filtered row keeps sort_name: {row}"
        );
        let twin = recent_rows
            .iter()
            .find(|r| r["feed_guid"] == row["feed_guid"])
            .expect("the same row on GET /v1/feeds/recent");
        for field in ["stated_rels", "two_way_link_count", "agreed_roles"] {
            assert!(
                !row[field].is_null(),
                "ADR 0085 §1: a filter gives the link facts, so {field} must be set: {row}"
            );
            assert_eq!(
                row[field], twin[field],
                "ADR 0085 Invariants: {field} must equal the value on GET /v1/feeds/recent"
            );
        }
    }
    let (_, plain) = get(&st, &format!("{BASE}&limit=50")).await;
    assert_eq!(
        plain["data"].as_array().map(Vec::len),
        Some(8),
        "a request with no filter keeps all rows: {plain}"
    );
    assert!(
        plain["data"][0]["stated_rels"].is_null(),
        "a request with no filter and no include gives no link facts: {plain}"
    );
}

// ── Errors ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn each_error_of_adr_0068_section_4_has_its_condition_on_this_route() {
    let st = named_state().await;
    for uri in [
        "/v1/feeds/by-name?stated_rel=label",
        "/v1/feeds/by-name?medium=music&two_way_links=none",
        "/v1/feeds/by-name?medium=all&stated_rel=label",
        "/v1/feeds/by-name?medium=publisher&stated_rel=",
        "/v1/feeds/by-name?medium=publisher&stated_rel=label%20artist",
        "/v1/feeds/by-name?medium=publisher&two_way_links=some",
        "/v1/feeds/by-name?medium=publisher&two_way_links=",
        // The conflicts with the music filters stay errors.
        "/v1/feeds/by-name?medium=publisher&stated_rel=label&artist_key=x",
        "/v1/feeds/by-name?medium=music&stated_rel=label&artist_key=x",
        "/v1/feeds/by-name?medium=publisher&stated_rel=label&named_feed_guid=x",
        "/v1/feeds/by-name?medium=music&stated_rel=label&named_feed_guid=x",
    ] {
        let (status, body) = get(&st, uri).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "ADR 0085 criterion 3: {uri} must give 400, got {body}. Call \
             LinkFactFilter::from_params in handle_get_feeds_by_name."
        );
    }
    let (status, body) = get(
        &st,
        "/v1/feeds/by-name?medium=publisher&stated_rel=label&named_feed_guid=x",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string()
            .contains("named_feed_guid needs medium=music"),
        "the named_feed_guid error comes before the filter error: {body}"
    );
    let (status, body) = get(
        &st,
        "/v1/feeds/by-name?medium=publisher&stated_rel=label&artist_key=x",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string().contains("artist_key needs medium=music"),
        "artist_key with medium=publisher keeps its own message: {body}"
    );
    let (status, body) = get(
        &st,
        "/v1/feeds/by-name?medium=music&stated_rel=label&artist_key=x",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string().contains("need medium=publisher"),
        "the filter error names the medium: {body}"
    );
    // No filter: the old rules stay.
    let (status, _) = get(&st, "/v1/feeds/by-name?medium=publisher&artist_key=x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ── The scan limit ──────────────────────────────────────────────────────────

fn filler(guid: &str, title: &str) -> Feed {
    Feed {
        feed_guid: guid.into(),
        feed_url: format!("https://f.example/{guid}.xml"),
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
        newest_item_at: None,
        oldest_item_at: None,
        created_at: 1_000_000,
        updated_at: 1_000_000,
        raw_medium: Some("publisher".into()),
        last_build_date: None,
        release_artist_source: None,
    }
}

/// Publishers that state no role fill the middle of the name order, with more
/// rows than the scan limit. Real label publishers come before the fillers
/// (`head`) and after them (`tail`).
async fn scan_state(head: &[&str], tail: &[&str]) -> Arc<stophammer::api::AppState> {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("db lock");
        for n in 0..stophammer::query::LINK_FACT_FILTER_SCAN_MAX + 50 {
            let guid = format!("fill-{n:05}");
            stophammer::db::upsert_feed(&conn, &filler(&guid, &format!("m {n:05}")))
                .expect("upsert filler");
        }
    }
    let st = state(db);
    for guid in head {
        store_publisher(&st, guid, &format!("a {guid}"), &[("label", true)]).await;
    }
    for guid in tail {
        store_publisher(&st, guid, &format!("z {guid}"), &[("label", true)]).await;
    }
    st
}

#[tokio::test]
async fn a_filtered_request_stops_at_the_scan_limit_and_the_walk_finds_the_rest() {
    let st = scan_state(&["h-1", "h-2"], &["t-1", "t-2"]).await;
    let all = ["h-1", "h-2", "t-1", "t-2"];
    for reverse in ["", "&reverse=true"] {
        let uri = format!("{BASE}&stated_rel=label&limit=10{reverse}");
        let (status, body) = get(&st, &uri).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["pagination"]["has_more"], true,
            "ADR 0085 §2: the first request must stop at the scan limit with has_more true \
             ({uri}). Share LINK_FACT_FILTER_SCAN_MAX through filtered_scan."
        );
        assert!(
            body["data"].as_array().expect("data").len() < 10,
            "the page must be short ({uri}): {body}"
        );
        assert!(body["pagination"]["cursor"].is_string(), "{uri}");
        let (rows, pages) = walk(&st, &uri.replace("&limit=10", ""), 10).await;
        let expected = if reverse.is_empty() {
            strs(&all)
        } else {
            reversed(&all)
        };
        assert_eq!(
            rows, expected,
            "ADR 0085 §2: the walk must find each matching row beyond the scan limit, with \
             no repeat ({uri}). The cursor must name the last examined row."
        );
        assert!(pages >= 2, "the walk needs more than one page ({uri})");
    }
}

#[tokio::test]
async fn a_scan_limit_page_can_be_empty_with_a_cursor() {
    // Only tail rows match, so the ascending first page is empty. Only head
    // rows match, so the descending first page is empty.
    for (head, tail, reverse) in [
        (&[][..], &["t-1", "t-2"][..], ""),
        (&["h-1", "h-2"][..], &[][..], "&reverse=true"),
    ] {
        let st = scan_state(head, tail).await;
        let uri = format!("{BASE}&stated_rel=label&limit=5{reverse}");
        let (status, body) = get(&st, &uri).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"].as_array().map(Vec::len), Some(0), "{uri}");
        assert_eq!(
            body["pagination"]["has_more"], true,
            "ADR 0085 §2: an empty page at the scan limit must give has_more true ({uri})"
        );
        let cursor = body["pagination"]["cursor"].as_str().expect("cursor");
        let (_, next) = get(&st, &format!("{uri}&cursor={cursor}")).await;
        assert_eq!(
            next["data"].as_array().map(Vec::len),
            Some(2),
            "the cursor of the stopped scan must continue after the last examined row \
             ({uri}): {next}"
        );
        assert_eq!(next["pagination"]["has_more"], false, "{next}");
    }
}
