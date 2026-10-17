// ADR 0070 task 001: the sort key and the release list.
//
// docs/tasks/adr-0070-task-001-sort-key-and-release-list.md
//
// Checks:
// - `model::sort_key` trims the white space at the start and at the end,
//   collapses an internal run of white space, and lowercases by the
//   Unicode rule (ADR 0070 §1, ADR 0049 §7).
// - `GET /v1/feeds/by-name` orders by the stored sort key, then feed_guid,
//   with a working cursor and `from`.
// - The default medium excludes a publisher feed.
// - `title_sort` is never part of the signed `FeedUpserted` payload: the
//   node derives it from `title` at each write, in `src/db.rs`, so a
//   historical event cannot carry a stale or missing key.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state(crawl_token: &str) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0070-release-list-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: vec!["content_hash".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());

    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(common::test_db_arc()),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0070-release-list-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
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

async fn get(app: axum::Router, uri: &str) -> (http::StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = resp.status();
    (status, body_json(resp).await)
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
    let status = resp.status();
    let body = body_json(resp).await;
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {status} {body:?}"
    );
}

fn ingest_payload(
    feed_guid: &str,
    title: &str,
    raw_medium: &str,
    crawl_token: &str,
) -> serde_json::Value {
    let url = format!("https://example.com/{feed_guid}.xml");
    serde_json::json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": format!("hash-{feed_guid}"),
        "feed_data": {
            "feed_guid": feed_guid,
            "title": title,
            "raw_medium": raw_medium,
            "explicit": false,
        },
    })
}

fn by_name_titles(body: &serde_json::Value) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("data must be an array")
        .iter()
        .map(|f| {
            f["title"]
                .as_str()
                .expect("each row must give title")
                .to_string()
        })
        .collect()
}

fn by_name_guids(body: &serde_json::Value) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("data must be an array")
        .iter()
        .map(|f| {
            f["feed_guid"]
                .as_str()
                .expect("each row must give feed_guid")
                .to_string()
        })
        .collect()
}

// ── model::sort_key (ADR 0070 §1, ADR 0049 §7) ──────────────────────────────

#[test]
fn sort_key_trims_collapses_white_space_and_lowercases() {
    assert_eq!(
        stophammer::model::sort_key("the  best\tsong "),
        "the best song"
    );
}

#[test]
fn sort_key_lowercases_by_unicode_rule() {
    assert_eq!(stophammer::model::sort_key("Élan"), "élan");
}

// ── title_sort is derived, never signed ─────────────────────────────────────
// `FeedUpsertedPayload` carries the whole `Feed`, and the node signs it
// (src/event.rs). ADR 0070 does not decide the signed event format, so
// `title_sort` must never be a field of `Feed`: `src/db.rs` derives it from
// `title` at each write instead.

fn sample_feed(feed_guid: &str, title: &str, now: i64) -> stophammer::model::Feed {
    stophammer::model::Feed {
        feed_guid: feed_guid.into(),
        feed_url: format!("https://example.com/{feed_guid}.xml"),
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
        created_at: now,
        updated_at: now,
        raw_medium: None,
        last_build_date: None,
        release_artist_source: None,
    }
}

#[test]
fn serialized_feed_upserted_payload_has_no_title_sort_key() {
    use stophammer::event::FeedUpsertedPayload;

    let feed = sample_feed(
        "title-sort-wire-feed",
        "  The  Caf\u{e9} Session ",
        1_000_000,
    );
    let payload = FeedUpsertedPayload { feed, reason: None };
    let json = serde_json::to_value(&payload).expect("serialize FeedUpsertedPayload");

    assert!(
        json["feed"].is_object(),
        "FeedUpsertedPayload must serialize feed as an object: {json:?}"
    );
    assert!(
        json["feed"].get("title_sort").is_none(),
        "ADR 0070 §1: the signed event never carries title_sort: {json:?}"
    );
}

#[test]
fn applying_a_feed_upserted_event_derives_title_sort_from_title() {
    use stophammer::apply::{ApplyOutcome, apply_single_event};
    use stophammer::event::{Event, EventPayload, EventType, FeedUpsertedPayload};

    let db = common::test_db_arc();
    let pool = common::wrap_pool(Arc::clone(&db));
    let now = common::now();

    let feed = sample_feed("title-sort-apply-feed", "  The  Caf\u{e9} Session ", now);
    let inner = FeedUpsertedPayload { feed, reason: None };
    let payload_json = serde_json::to_string(&inner).expect("serialize");
    let ev = Event {
        event_id: "title-sort-apply-event".into(),
        event_type: EventType::FeedUpserted,
        payload: EventPayload::FeedUpserted(inner),
        subject_guid: "title-sort-apply-feed".into(),
        signed_by: "deadbeef".into(),
        signature: "cafebabe".into(),
        seq: 1,
        created_at: now,
        warnings: vec![],
        payload_json,
    };

    let outcome = apply_single_event(&pool, &ev).expect("apply the event");
    assert!(
        matches!(outcome, ApplyOutcome::Applied(_)),
        "the event must apply: {outcome:?}"
    );

    let title_sort: Option<String> = db
        .lock()
        .expect("lock")
        .query_row(
            "SELECT title_sort FROM feeds WHERE feed_guid = 'title-sort-apply-feed'",
            [],
            |r| r.get(0),
        )
        .expect("read the derived title_sort");

    assert_eq!(
        title_sort,
        Some(stophammer::model::sort_key("  The  Caf\u{e9} Session ")),
        "ADR 0070 §1: the apply path derives title_sort from title, in src/db.rs"
    );
}

// ── GET /v1/feeds/by-name: order, ties, from, pagination, medium ───────────

#[tokio::test]
async fn orders_by_sort_key_in_code_point_order() {
    let crawl_token = "adr0070-order-token";
    let state = test_app_state(crawl_token);
    let app = stophammer::api::build_router(state);

    ingest(
        app.clone(),
        &ingest_payload("order-b", "b", "music", crawl_token),
    )
    .await;
    ingest(
        app.clone(),
        &ingest_payload("order-a", " A", "music", crawl_token),
    )
    .await;
    ingest(
        app.clone(),
        &ingest_payload("order-umlaut", "Ä", "music", crawl_token),
    )
    .await;

    let (status, body) = get(app.clone(), "/v1/feeds/by-name").await;
    assert_eq!(status, http::StatusCode::OK, "body: {body:?}");

    assert_eq!(
        by_name_titles(&body),
        vec![" A", "b", "Ä"],
        "ADR 0070 §1: the sort key of \" A\", \"b\" and \"Ä\" orders them a, b, ä"
    );
}

#[tokio::test]
async fn ties_on_the_sort_key_come_in_feed_guid_order() {
    let crawl_token = "adr0070-tie-token";
    let state = test_app_state(crawl_token);
    let app = stophammer::api::build_router(state);

    ingest(
        app.clone(),
        &ingest_payload("tie-z", "Same Title", "music", crawl_token),
    )
    .await;
    ingest(
        app.clone(),
        &ingest_payload("tie-a", "Same Title", "music", crawl_token),
    )
    .await;

    let (status, body) = get(app.clone(), "/v1/feeds/by-name").await;
    assert_eq!(status, http::StatusCode::OK, "body: {body:?}");

    assert_eq!(
        by_name_guids(&body),
        vec!["tie-a", "tie-z"],
        "ADR 0070 §2: a tie on the sort key breaks by feed_guid"
    );
}

#[tokio::test]
async fn pages_of_two_give_each_of_five_feeds_one_time() {
    let crawl_token = "adr0070-page-token";
    let state = test_app_state(crawl_token);
    let app = stophammer::api::build_router(state);

    for i in 1..=5_u32 {
        let guid = format!("page-{i:02}");
        let title = format!("Page Feed {i:02}");
        ingest(
            app.clone(),
            &ingest_payload(&guid, &title, "music", crawl_token),
        )
        .await;
    }

    let mut seen: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        pages += 1;
        assert!(pages <= 10, "pagination must terminate within 10 pages");
        let uri = match &cursor {
            Some(c) => format!("/v1/feeds/by-name?limit=2&cursor={c}"),
            None => "/v1/feeds/by-name?limit=2".to_string(),
        };
        let (status, body) = get(app.clone(), &uri).await;
        assert_eq!(status, http::StatusCode::OK, "body: {body:?}");

        seen.extend(by_name_guids(&body));

        let has_more = body["pagination"]["has_more"]
            .as_bool()
            .expect("has_more must be a bool");
        if !has_more {
            assert!(
                body["pagination"]["cursor"].is_null(),
                "ADR 0070 task 001: has_more false must pair with no cursor"
            );
            break;
        }
        cursor = Some(
            body["pagination"]["cursor"]
                .as_str()
                .expect("a page with has_more true must give a cursor")
                .to_string(),
        );
    }

    seen.sort();
    let mut unique = seen.clone();
    unique.dedup();
    assert_eq!(
        seen.len(),
        5,
        "each of the 5 feeds must appear exactly one time: {seen:?}"
    );
    assert_eq!(unique.len(), 5, "no feed repeats across pages: {seen:?}");
}

#[tokio::test]
async fn from_starts_at_the_first_title_at_or_after_its_sort_key() {
    let crawl_token = "adr0070-from-token";
    let state = test_app_state(crawl_token);
    let app = stophammer::api::build_router(state);

    ingest(
        app.clone(),
        &ingest_payload("from-apple", "Apple", "music", crawl_token),
    )
    .await;
    ingest(
        app.clone(),
        &ingest_payload("from-mango", "Mango", "music", crawl_token),
    )
    .await;
    ingest(
        app.clone(),
        &ingest_payload("from-zebra", "Zebra", "music", crawl_token),
    )
    .await;

    let (status, body) = get(app.clone(), "/v1/feeds/by-name?from=M").await;
    assert_eq!(status, http::StatusCode::OK, "body: {body:?}");

    assert_eq!(
        by_name_titles(&body),
        vec!["Mango", "Zebra"],
        "ADR 0070 §2: from=M starts at the first title whose sort key is at or after \"m\""
    );
}

#[tokio::test]
async fn default_medium_excludes_a_publisher_feed() {
    let crawl_token = "adr0070-medium-token";
    let state = test_app_state(crawl_token);
    let app = stophammer::api::build_router(state);

    ingest(
        app.clone(),
        &ingest_payload("medium-music", "Music Feed", "music", crawl_token),
    )
    .await;
    ingest(
        app.clone(),
        &ingest_payload(
            "medium-publisher",
            "Publisher Feed",
            "publisher",
            crawl_token,
        ),
    )
    .await;

    let (status, body) = get(app.clone(), "/v1/feeds/by-name").await;
    assert_eq!(status, http::StatusCode::OK, "body: {body:?}");

    let guids = by_name_guids(&body);
    assert!(
        guids.contains(&"medium-music".to_string()),
        "the music feed must be in the default list: {guids:?}"
    );
    assert!(
        !guids.contains(&"medium-publisher".to_string()),
        "ADR 0070 §2: the default medium is music, so a publisher feed is absent: {guids:?}"
    );
}

#[tokio::test]
async fn each_row_gives_sort_name_and_the_recent_route_does_not() {
    let crawl_token = "adr0070-sort-name-token";
    let state = test_app_state(crawl_token);
    let app = stophammer::api::build_router(state);

    ingest(
        app.clone(),
        &ingest_payload("sort-name-feed", "Sort Name Feed", "music", crawl_token),
    )
    .await;

    let (status, by_name_body) = get(app.clone(), "/v1/feeds/by-name").await;
    assert_eq!(status, http::StatusCode::OK, "body: {by_name_body:?}");
    let row = &by_name_body["data"][0];
    assert_eq!(
        row["sort_name"].as_str(),
        Some("sort name feed"),
        "GET /v1/feeds/by-name must give sort_name: {row:?}"
    );

    let (status, recent_body) = get(app.clone(), "/v1/feeds/recent").await;
    assert_eq!(status, http::StatusCode::OK, "body: {recent_body:?}");
    let recent_row = &recent_body["data"][0];
    assert!(
        recent_row.get("sort_name").is_none(),
        "ADR 0070 §2: sort_name is absent from GET /v1/feeds/recent: {recent_row:?}"
    );
}
