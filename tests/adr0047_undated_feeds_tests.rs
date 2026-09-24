// ADR 0047 task 002: `GET /v1/feeds/recent` must list every feed the filter
// matches, once, for every `medium`. A publisher feed carries no item, and a
// music feed can carry only undated items, so `newest_item_at` is null for
// each of them. Before this change, the cursor condition and the `ORDER BY`
// both used the raw `newest_item_at` column, so a page never reached a null
// row, and a page that ended on one gave a null cursor while `has_more` was
// `true`. These tests prove the sort key is
// `COALESCE(newest_item_at, -1)` everywhere the route uses it, so the
// undated feeds come after the dated ones and paging still reaches them.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0047-undated-signer"));
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

/// Ingests one feed. `track_dates` gives one track per entry, each carrying
/// the given `pub_date` (`None` for an undated track). An empty slice
/// ingests the feed with no track at all, so `newest_item_at` is null either
/// way.
async fn ingest_feed(
    app: axum::Router,
    token: &str,
    guid: &str,
    medium: &str,
    track_dates: &[Option<i64>],
) {
    let tracks: Vec<serde_json::Value> = track_dates
        .iter()
        .enumerate()
        .map(|(i, pub_date)| {
            serde_json::json!({
                "track_guid": format!("{guid}-track-{i}"),
                "title": format!("Track {i} of {guid}"),
                "pub_date": pub_date,
                "duration_secs": null,
                "image_url": null,
                "language": null,
                "enclosure_url": format!("https://example.com/{guid}/{i}.mp3"),
                "enclosure_type": "audio/mpeg",
                "enclosure_bytes": null,
                "track_number": null,
                "season": null,
                "explicit": false,
                "description": null,
                "author_name": null
            })
        })
        .collect();
    let payload = serde_json::json!({
        "canonical_url": format!("https://example.com/{guid}.xml"),
        "source_url": format!("https://example.com/{guid}.xml"),
        "crawl_token": token,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": format!("Feed {guid}"),
            "raw_medium": medium,
            "explicit": false,
            "tracks": tracks
        }
    });
    let resp = app
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
        "seeding {medium} feed {guid} must succeed, got {}",
        resp.status()
    );
}

/// One page of `GET /v1/feeds/recent`.
struct Page {
    guids: Vec<String>,
    cursor: Option<String>,
    has_more: bool,
}

async fn recent_page(app: axum::Router, query: &str) -> Page {
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/feeds/recent{query}"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert!(
        resp.status().is_success(),
        "recent feeds must succeed, got {}",
        resp.status()
    );
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    let guids = body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|f| f["feed_guid"].as_str().expect("feed_guid").to_string())
        .collect();
    let cursor = body["pagination"]["cursor"].as_str().map(str::to_string);
    let has_more = body["pagination"]["has_more"]
        .as_bool()
        .expect("has_more bool");
    Page {
        guids,
        cursor,
        has_more,
    }
}

/// Pages through the whole route from the start, asserting on every page
/// that `has_more: true` never pairs with a null cursor. Returns each page
/// in order.
async fn page_through(st: &Arc<stophammer::api::AppState>, query_prefix: &str) -> Vec<Page> {
    let mut pages = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let query = cursor.as_ref().map_or_else(
            || query_prefix.to_string(),
            |c| format!("{query_prefix}&cursor={c}"),
        );
        let page = recent_page(stophammer::api::build_router(Arc::clone(st)), &query).await;
        assert!(
            !page.has_more || page.cursor.is_some(),
            "has_more: true must carry a non-null cursor, page query was {query}"
        );
        let has_more = page.has_more;
        let next_cursor = page.cursor.clone();
        pages.push(page);
        if !has_more {
            break;
        }
        cursor = next_cursor;
    }
    pages
}

/// Builds a cursor the way the route encoded one before this task: the raw
/// value of `newest_item_at`, never `COALESCE(newest_item_at, -1)`. For a
/// dated row the two values are the same, so this reproduces a cursor a
/// client already holds from before the change.
fn old_style_cursor(newest_item_at: i64, feed_guid: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{newest_item_at}\0{feed_guid}").as_bytes())
}

#[tokio::test]
async fn medium_all_lists_dated_and_undated_feeds_once_each_dated_first() {
    let token = "adr0047-undated-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);

    for (guid, medium, dates) in [
        ("dated-a", "music", &[Some(3000)][..]),
        ("dated-b", "music", &[Some(2000)][..]),
        ("dated-c", "music", &[Some(1000)][..]),
        ("pub-a", "publisher", &[][..]),
        ("pub-b", "publisher", &[][..]),
        ("music-undated", "music", &[None][..]),
    ] {
        ingest_feed(
            stophammer::api::build_router(Arc::clone(&st)),
            token,
            guid,
            medium,
            dates,
        )
        .await;
    }

    let pages = page_through(&st, "?limit=2&medium=all").await;
    let all_guids: Vec<String> = pages.iter().flat_map(|p| p.guids.clone()).collect();

    let expected: Vec<String> = [
        "dated-a",
        "dated-b",
        "dated-c",
        "pub-a",
        "pub-b",
        "music-undated",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(
        all_guids.len(),
        expected.len(),
        "each of the 6 feeds must appear exactly once, got {all_guids:?}"
    );
    for guid in &expected {
        let count = all_guids.iter().filter(|g| *g == guid).count();
        assert_eq!(
            count, 1,
            "{guid} must appear exactly once, got {all_guids:?}"
        );
    }

    let dated = ["dated-a", "dated-b", "dated-c"];
    let undated = ["pub-a", "pub-b", "music-undated"];
    let last_dated_index = all_guids
        .iter()
        .enumerate()
        .filter(|(_, g)| dated.contains(&g.as_str()))
        .map(|(i, _)| i)
        .max()
        .expect("a dated feed is present");
    let first_undated_index = all_guids
        .iter()
        .enumerate()
        .filter(|(_, g)| undated.contains(&g.as_str()))
        .map(|(i, _)| i)
        .min()
        .expect("an undated feed is present");
    assert!(
        last_dated_index < first_undated_index,
        "every dated feed must come before every undated feed, got {all_guids:?}"
    );

    // The last page of a 6-feed corpus at limit=2 ends with no more rows, so
    // `has_more` there must be false.
    assert!(
        !pages.last().expect("at least one page").has_more,
        "the final page must report has_more: false"
    );
}

#[tokio::test]
async fn medium_publisher_pages_to_the_end_and_gives_both_publisher_feeds() {
    let token = "adr0047-undated-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);

    for (guid, medium, dates) in [
        ("dated-only-music", "music", &[Some(5000)][..]),
        ("pub-only-a", "publisher", &[][..]),
        ("pub-only-b", "publisher", &[][..]),
    ] {
        ingest_feed(
            stophammer::api::build_router(Arc::clone(&st)),
            token,
            guid,
            medium,
            dates,
        )
        .await;
    }

    let pages = page_through(&st, "?limit=1&medium=publisher").await;
    let all_guids: Vec<String> = pages.iter().flat_map(|p| p.guids.clone()).collect();

    assert_eq!(
        all_guids.len(),
        2,
        "medium=publisher must page to exactly the 2 publisher feeds, got {all_guids:?}"
    );
    for guid in ["pub-only-a", "pub-only-b"] {
        assert!(
            all_guids.contains(&guid.to_string()),
            "{guid} must be present, got {all_guids:?}"
        );
    }
    assert!(
        !all_guids.contains(&"dated-only-music".to_string()),
        "medium=publisher must not return the music feed, got {all_guids:?}"
    );
}

#[tokio::test]
async fn a_page_ending_on_an_undated_row_has_a_working_non_null_cursor() {
    let token = "adr0047-undated-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);

    for (guid, medium, dates) in [
        ("boundary-dated", "music", &[Some(9000)][..]),
        ("boundary-pub-a", "publisher", &[][..]),
        ("boundary-pub-b", "publisher", &[][..]),
    ] {
        ingest_feed(
            stophammer::api::build_router(Arc::clone(&st)),
            token,
            guid,
            medium,
            dates,
        )
        .await;
    }

    // limit=2 with 3 feeds (1 dated, 2 undated): the first page ends on an
    // undated row (dated feed first, then the higher-feed_guid publisher).
    let page1 = recent_page(
        stophammer::api::build_router(Arc::clone(&st)),
        "?limit=2&medium=all",
    )
    .await;
    assert_eq!(page1.guids.len(), 2, "first page must hold 2 feeds");
    assert!(page1.has_more, "a third feed remains");
    let cursor = page1
        .cursor
        .clone()
        .expect("has_more: true must carry a non-null cursor");

    let page2 = recent_page(
        stophammer::api::build_router(Arc::clone(&st)),
        &format!("?limit=2&medium=all&cursor={cursor}"),
    )
    .await;
    assert_eq!(
        page2.guids.len(),
        1,
        "the second page must hold the one remaining feed"
    );
    assert!(!page2.has_more, "no feed remains after the second page");

    let mut all_guids = page1.guids.clone();
    all_guids.extend(page2.guids.clone());
    for guid in ["boundary-dated", "boundary-pub-a", "boundary-pub-b"] {
        assert!(
            all_guids.contains(&guid.to_string()),
            "{guid} must be present across both pages, got {all_guids:?}"
        );
    }
}

#[tokio::test]
async fn an_old_style_cursor_from_a_dated_row_gives_the_same_next_page_as_before() {
    let token = "adr0047-undated-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);

    for (guid, medium, dates) in [
        ("old-cursor-a", "music", &[Some(7000)][..]),
        ("old-cursor-b", "music", &[Some(6000)][..]),
        ("old-cursor-c", "music", &[Some(5000)][..]),
    ] {
        ingest_feed(
            stophammer::api::build_router(Arc::clone(&st)),
            token,
            guid,
            medium,
            dates,
        )
        .await;
    }

    // The new route's own cursor for the last row of the first page.
    let page1 = recent_page(
        stophammer::api::build_router(Arc::clone(&st)),
        "?limit=1&medium=all",
    )
    .await;
    assert_eq!(page1.guids, vec!["old-cursor-a".to_string()]);
    let new_cursor = page1.cursor.expect("has_more: true must carry a cursor");

    // A cursor built the way the route encoded one before this task, from
    // the same row (`old-cursor-a`, `newest_item_at` = 7000).
    let manual_cursor = old_style_cursor(7000, "old-cursor-a");
    assert_eq!(
        new_cursor, manual_cursor,
        "for a dated row the new cursor must match the pre-change encoding"
    );

    let page_via_new = recent_page(
        stophammer::api::build_router(Arc::clone(&st)),
        &format!("?limit=1&medium=all&cursor={new_cursor}"),
    )
    .await;
    let page_via_old = recent_page(
        stophammer::api::build_router(Arc::clone(&st)),
        &format!("?limit=1&medium=all&cursor={manual_cursor}"),
    )
    .await;
    assert_eq!(
        page_via_new.guids, page_via_old.guids,
        "an old-style cursor from a dated row must give the same next page"
    );
    assert_eq!(page_via_new.guids, vec!["old-cursor-b".to_string()]);
}

#[tokio::test]
async fn a_cursor_whose_first_part_is_negative_one_decodes_and_works() {
    let token = "adr0047-undated-token";
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db), token);

    for (guid, medium, dates) in [
        ("neg-one-a", "publisher", &[][..]),
        ("neg-one-b", "publisher", &[][..]),
    ] {
        ingest_feed(
            stophammer::api::build_router(Arc::clone(&st)),
            token,
            guid,
            medium,
            dates,
        )
        .await;
    }

    // Both feeds are undated, so their sort key is -1 and the tie breaks on
    // feed_guid DESC: "neg-one-b" before "neg-one-a".
    let cursor = URL_SAFE_NO_PAD.encode(b"-1\0neg-one-b");
    let page = recent_page(
        stophammer::api::build_router(Arc::clone(&st)),
        &format!("?limit=5&medium=publisher&cursor={cursor}"),
    )
    .await;
    assert_eq!(
        page.guids,
        vec!["neg-one-a".to_string()],
        "a cursor with a -1 first part must decode and continue the page"
    );
    assert!(!page.has_more, "no feed remains after neg-one-a");
}
