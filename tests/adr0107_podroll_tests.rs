//! ADR 0107 node tests for stored podroll data and feed responses.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0107-podroll-token";

fn state(db: Arc<std::sync::Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("adr0107-podroll-signer"));
    let chain = stophammer::verify::build_chain(
        &stophammer::verify::ChainSpec { names: vec![] },
        TOKEN.to_string(),
    );
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        node_pubkey_hex: signer.pubkey_hex().to_string(),
        signer,
        admin_token: "adr0107-admin".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn post_feed(st: &Arc<stophammer::api::AppState>, guid: &str, podroll: Value, n: u8) {
    let url = format!("https://example.com/{guid}.xml");
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("{guid}-{n}"),
        "feed_data": {
            "feed_guid": guid,
            "title": format!("Feed {guid}"),
            "raw_medium": "publisher",
            "explicit": false,
            "tracks": [],
            "podroll": podroll,
            "remote_items": []
        }
    });
    let response = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::post("/ingest/feed")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.status().is_success());
}

async fn get_feed(st: &Arc<stophammer::api::AppState>, guid: &str, include: &str) -> Value {
    let response = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::get(format!("/v1/feeds/{guid}?include={include}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// ADR 0107: An ingest stores podroll rows and returns named feed summaries.
#[tokio::test]
async fn ingest_keeps_the_podroll() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    post_feed(&st, "held-feed", json!([]), 1).await;
    post_feed(&st, "podroll-owner", json!([
        {"position": 0, "medium": "music", "remote_feed_guid": "held-feed", "remote_feed_url": "https://example.com/held.xml"},
        {"position": 1, "medium": "music", "remote_feed_guid": "missing-feed", "remote_feed_url": null}
    ]), 1).await;
    let owner = get_feed(&st, "podroll-owner", "podroll").await;
    let rows = owner["data"]["podroll"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["position"], 0);
    assert_eq!(rows[0]["remote_feed_guid"], "held-feed");
    assert_eq!(rows[0]["remote_feed_title"], "Feed held-feed");
    assert_eq!(rows[1]["position"], 1);
    assert_eq!(rows[1]["remote_feed_title"], Value::Null);
    let conn = db.lock().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM feed_podroll_items WHERE feed_guid = 'podroll-owner'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

/// ADR 0107: A podroll-only change emits the existing remote-items event.
#[tokio::test]
async fn changed_podroll_gives_the_event() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    post_feed(
        &st,
        "podroll-event",
        json!([
            {"position": 0, "medium": "music", "remote_feed_guid": "first", "remote_feed_url": null}
        ]),
        1,
    )
    .await;
    post_feed(&st, "podroll-event", json!([
        {"position": 0, "medium": "music", "remote_feed_guid": "second", "remote_feed_url": null}
    ]), 2).await;
    let conn = db.lock().unwrap();
    let payload: String = conn.query_row(
        "SELECT payload_json FROM events WHERE event_type = 'feed_remote_items_replaced' ORDER BY seq DESC LIMIT 1",
        [], |row| row.get(0),
    ).unwrap();
    let value: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(value["podroll"][0]["remote_feed_guid"], "second");
}

/// ADR 0107: A feed retire removes the stored podroll rows.
#[tokio::test]
async fn retire_deletes_the_podroll() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    post_feed(
        &st,
        "podroll-retire",
        json!([{"position": 0, "remote_feed_guid": "listed", "remote_feed_url": null}]),
        1,
    )
    .await;
    let mut conn = db.lock().unwrap();
    let signer = common::temp_signer("adr0107-retire-signer");
    let payload = json!({"feed_guid": "podroll-retire", "reason": "test"}).to_string();
    stophammer::db::delete_feed_with_event(
        &mut conn,
        "podroll-retire",
        "adr0107-retire-event",
        &payload,
        "podroll-retire",
        &signer,
        stophammer::db::unix_now(),
        &[],
        &[],
    )
    .unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM feed_podroll_items WHERE feed_guid = 'podroll-retire'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

/// ADR 0107 §2: a podroll entry with `medium="music"` in a publisher feed is
/// not a remote item, and the publisher view gives no album for it.
#[tokio::test]
async fn podroll_entry_is_not_a_remote_item() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    post_feed(&st, "held-album", json!([]), 1).await;
    post_feed(
        &st,
        "podroll-publisher",
        json!([{"position": 0, "medium": "music", "remote_feed_guid": "held-album", "remote_feed_url": null}]),
        1,
    )
    .await;
    let owner = get_feed(&st, "podroll-publisher", "remote_items,publisher,podroll").await;
    assert_eq!(owner["data"]["remote_items"], json!([]));
    assert_eq!(owner["data"]["publisher"], json!([]));
    assert_eq!(owner["data"]["podroll"].as_array().unwrap().len(), 1);
}

fn count_remote_items_events(db: &Arc<std::sync::Mutex<rusqlite::Connection>>, guid: &str) -> i64 {
    db.lock()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'feed_remote_items_replaced' \
             AND subject_guid = ?1",
            [guid],
            |row| row.get(0),
        )
        .unwrap()
}

/// ADR 0107 §3: the same podroll in a new body gives no event. A stored row
/// has an `id` and a new row has none, so the comparison must not use `id`.
#[tokio::test]
async fn same_podroll_gives_no_event() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let podroll = json!([{"position": 0, "medium": "music", "remote_feed_guid": "same", "remote_feed_url": null}]);
    post_feed(&st, "podroll-same", podroll.clone(), 1).await;
    let after_first = count_remote_items_events(&db, "podroll-same");
    post_feed(&st, "podroll-same", podroll, 2).await;
    assert_eq!(
        count_remote_items_events(&db, "podroll-same"),
        after_first,
        "an unchanged podroll must not give a new event"
    );
}

fn podroll_event(
    seq: i64,
    podroll: Option<Vec<stophammer::model::FeedPodrollItem>>,
) -> stophammer::event::Event {
    use stophammer::event::{Event, EventPayload, EventType, FeedRemoteItemsReplacedPayload};
    let payload = FeedRemoteItemsReplacedPayload {
        feed_guid: "podroll-apply".into(),
        remote_items: vec![],
        podroll,
    };
    Event {
        event_id: format!("adr0107-apply-{seq}"),
        event_type: EventType::FeedRemoteItemsReplaced,
        payload_json: serde_json::to_string(&payload).unwrap(),
        payload: EventPayload::FeedRemoteItemsReplaced(payload),
        subject_guid: "podroll-apply".into(),
        signed_by: "deadbeef".into(),
        signature: "cafebabe".into(),
        seq,
        created_at: stophammer::db::unix_now(),
        warnings: vec![],
    }
}

fn stored_podroll(db: &Arc<std::sync::Mutex<rusqlite::Connection>>) -> Vec<String> {
    stophammer::db::get_feed_podroll_for_feed(&db.lock().unwrap(), "podroll-apply")
        .unwrap()
        .into_iter()
        .map(|item| item.remote_feed_guid)
        .collect()
}

/// ADR 0107 §3: an applied event with `podroll` replaces the rows of the
/// node. An event with no `podroll` field leaves them.
#[tokio::test]
async fn apply_replaces_the_podroll_only_with_the_field() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    post_feed(&st, "podroll-apply", json!([]), 1).await;
    let pool = common::wrap_pool(Arc::clone(&db));

    let entry = stophammer::model::FeedPodrollItem {
        id: None,
        feed_guid: "podroll-apply".into(),
        position: 0,
        medium: Some("music".into()),
        remote_feed_guid: "applied".into(),
        remote_feed_url: None,
    };
    stophammer::apply::apply_single_event(&pool, &podroll_event(1001, Some(vec![entry]))).unwrap();
    assert_eq!(stored_podroll(&db), vec!["applied".to_string()]);

    let old_event = podroll_event(1002, None);
    assert!(
        !old_event.payload_json.contains("\"podroll\":"),
        "an event with None has no podroll field, as an event from before ADR 0107"
    );
    stophammer::apply::apply_single_event(&pool, &old_event).unwrap();
    assert_eq!(
        stored_podroll(&db),
        vec!["applied".to_string()],
        "an event with no podroll field must leave the rows"
    );
}
