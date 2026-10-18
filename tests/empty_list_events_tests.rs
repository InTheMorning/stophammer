// A feed that removes a whole list must give an event, so that a community
// node removes the same rows.
//
// `ingest_transaction` replaces the feed payment routes, the feed remote
// items and the track remote items at each ingest. Before this fix,
// `build_diff_events` gave an event for each of the three lists only when
// the new list was not empty. A change to an empty list thus deleted the
// rows on the primary and signed no event, and a community node kept the
// old rows. The musicL list values (ADR 0060 §4) already had the correct
// rule.
//
// Database A ingests `detox-album` with one item in each list, then the
// same feed with each list empty. Database B applies each event of A. Both
// databases must hold no row of the three lists.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use rusqlite::params;
use tower::ServiceExt;

const ALBUM_URL: &str = "https://example.com/empty-list-events/detox-album.xml";
const ALBUM_GUID: &str = "e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9";
const CRAWL_TOKEN: &str = "empty-list-events-token";

fn app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-empty-list-events-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let names: Vec<String> = stophammer::verify::ChainSpec::DEFAULT
        .split(',')
        .map(ToString::to_string)
        .collect();
    let spec = stophammer::verify::ChainSpec { names };
    let chain = stophammer::verify::build_chain(&spec, CRAWL_TOKEN.to_string());

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
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn ingest(app: axum::Router, content_hash: &str, feed_data: &serde_json::Value) {
    let payload = serde_json::json!({
        "canonical_url": ALBUM_URL,
        "source_url": ALBUM_URL,
        "crawl_token": CRAWL_TOKEN,
        "http_status": 200,
        "content_hash": content_hash,
        "feed_data": feed_data,
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
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
}

/// Gives the row counts of the feed payment routes, the feed remote items
/// and the track remote items of the album.
fn list_row_counts(db: &Arc<Mutex<rusqlite::Connection>>) -> (i64, i64, i64) {
    let conn = db.lock().expect("lock db");
    let count = |sql: &str| -> i64 {
        conn.query_row(sql, params![ALBUM_GUID], |row| row.get(0))
            .expect("count rows")
    };
    (
        count("SELECT COUNT(*) FROM feed_payment_routes WHERE feed_guid = ?1"),
        count("SELECT COUNT(*) FROM feed_remote_items_raw WHERE feed_guid = ?1"),
        count("SELECT COUNT(*) FROM track_remote_items_raw WHERE feed_guid = ?1"),
    )
}

#[tokio::test]
async fn a_feed_that_empties_its_lists_gives_events_that_empty_a_replica() {
    let db_a = common::test_db_arc();
    let app = stophammer::api::build_router(app_state(Arc::clone(&db_a)));

    let mut full = common::adr0049_feed_data("detox-album");
    full["tracks"][0]["remote_items"] = serde_json::json!([{
        "position": 0,
        "medium": "music",
        "remote_feed_guid": "empty-list-events-remote-feed",
        "remote_feed_url": "https://example.com/empty-list-events/remote.xml",
    }]);
    ingest(app.clone(), "empty-list-events-full", &full).await;
    assert_eq!(
        list_row_counts(&db_a),
        (1, 1, 1),
        "the first ingest must store one row of each list"
    );

    let mut empty = full.clone();
    empty["remote_items"] = serde_json::json!([]);
    empty["feed_payment_routes"] = serde_json::json!([]);
    empty["tracks"][0]["remote_items"] = serde_json::json!([]);
    let later_build = full["last_build_date"].as_i64().expect("last_build_date") + 60;
    empty["last_build_date"] = serde_json::json!(later_build);
    ingest(app, "empty-list-events-empty", &empty).await;
    assert_eq!(
        list_row_counts(&db_a),
        (0, 0, 0),
        "the second ingest must delete each list on the primary"
    );

    let events: Vec<stophammer::event::Event> = {
        let conn = db_a.lock().expect("lock db_a");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events from A")
    };
    let db_b = common::test_db_arc();
    let pool_b = common::wrap_pool(Arc::clone(&db_b));
    for ev in &events {
        let result = stophammer::apply::apply_single_event(&pool_b, ev);
        assert!(
            result.is_ok(),
            "applying event {:?} on the replica must succeed: {result:?}",
            ev.event_type
        );
    }

    assert_eq!(
        list_row_counts(&db_b),
        (0, 0, 0),
        "a replica that applies each event of the primary must also hold no \
         row of the emptied lists (feed routes, feed remote items, track \
         remote items)"
    );
}

#[tokio::test]
async fn a_feed_with_no_list_items_gives_no_list_event_on_reingest() {
    let db = common::test_db_arc();
    let app = stophammer::api::build_router(app_state(Arc::clone(&db)));

    let mut data = common::adr0049_feed_data("detox-album");
    data["remote_items"] = serde_json::json!([]);
    ingest(app.clone(), "empty-list-events-none-1", &data).await;
    let before: i64 = {
        let conn = db.lock().expect("lock db");
        conn.query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'feed_remote_items_replaced'",
            [],
            |row| row.get(0),
        )
        .expect("count events")
    };

    let later_build = data["last_build_date"].as_i64().expect("last_build_date") + 60;
    data["last_build_date"] = serde_json::json!(later_build);
    ingest(app, "empty-list-events-none-2", &data).await;
    let after: i64 = {
        let conn = db.lock().expect("lock db");
        conn.query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'feed_remote_items_replaced'",
            [],
            |row| row.get(0),
        )
        .expect("count events")
    };

    assert_eq!(
        before, after,
        "an empty list that stays empty must give no remote-item event"
    );
}
