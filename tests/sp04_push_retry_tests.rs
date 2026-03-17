// SP-04 push retry — 2026-03-13

mod common;

use rusqlite::params;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ── Helpers ────────────────────────────────────────────────────────────────

fn seed_peer(
    db: &Arc<Mutex<rusqlite::Connection>>,
    pubkey: &str,
    push_url: &str,
) {
    let conn = db.lock().unwrap();
    let now = stophammer::db::unix_now();
    conn.execute(
        "INSERT OR REPLACE INTO peer_nodes (node_pubkey, node_url, discovered_at, consecutive_failures, last_push_at)
         VALUES (?1, ?2, ?3, 0, ?3)",
        params![pubkey, push_url, now],
    )
    .unwrap();
}

fn get_failures(db: &Arc<Mutex<rusqlite::Connection>>, pubkey: &str) -> i64 {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT consecutive_failures FROM peer_nodes WHERE node_pubkey = ?1",
        params![pubkey],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

fn make_test_event() -> stophammer::event::Event {
    stophammer::event::Event {
        event_id:     "evt-retry-test-001".into(),
        event_type:   stophammer::event::EventType::FeedRetired,
        payload:      stophammer::event::EventPayload::FeedRetired(
            stophammer::event::FeedRetiredPayload {
                feed_guid: "feed-retry-guid".into(),
                reason:    None,
            },
        ),
        subject_guid: "feed-retry-guid".into(),
        signed_by:    "test-node-pubkey".into(),
        signature:    "deadbeef".into(),
        seq:          1,
        created_at:   stophammer::db::unix_now(),
        warnings:     vec![],
        payload_json: "{}".into(),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

/// A push target that returns 503 once then 200 should NOT be counted as a failure.
/// The retry logic should recover on the second attempt.
#[tokio::test]
async fn push_retry_recovers_on_second_attempt() {
    let mock_server = MockServer::start().await;

    // First request: 503, second request: 200
    Mock::given(method("POST"))
        .and(path("/sync/push"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/sync/push"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let push_url = format!("{}/sync/push", mock_server.uri());

    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let pubkey = "peer-retry-ok";
    seed_peer(&db, pubkey, &push_url);

    let subscribers: Arc<RwLock<HashMap<String, String>>> =
        Arc::new(RwLock::new(HashMap::from([(pubkey.to_string(), push_url)])));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let events = vec![make_test_event()];

    stophammer::api::fan_out_push_public(
        pool.clone(),
        client,
        Arc::clone(&subscribers),
        events,
    )
    .await;

    // Give spawned tasks time to complete
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;

    let failures = get_failures(&db, pubkey);
    assert_eq!(failures, 0, "peer should have 0 failures after retry success, got {failures}");

    // Peer should still be in subscribers
    assert!(subscribers.read().unwrap().contains_key(pubkey), "peer should not be evicted");
}

/// A push target that ALWAYS fails should eventually be evicted — but only
/// after the new threshold of 10 consecutive failures.
#[tokio::test]
async fn push_eviction_after_threshold() {
    let mock_server = MockServer::start().await;

    // Always return 503
    Mock::given(method("POST"))
        .and(path("/sync/push"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&mock_server)
        .await;

    let push_url = format!("{}/sync/push", mock_server.uri());

    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let pubkey = "peer-always-fail";
    seed_peer(&db, pubkey, &push_url);

    let subscribers: Arc<RwLock<HashMap<String, String>>> =
        Arc::new(RwLock::new(HashMap::from([(pubkey.to_string(), push_url)])));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    // Push 10 times — each fails all 3 attempts, so each call increments failures by 1.
    for _ in 0..10 {
        let events = vec![make_test_event()];
        stophammer::api::fan_out_push_public(
            pool.clone(),
            client.clone(),
            Arc::clone(&subscribers),
            events,
        )
        .await;

        // Wait for spawned tasks to finish
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    }

    let failures = get_failures(&db, pubkey);
    assert!(
        failures >= 10,
        "peer should have >= 10 failures, got {failures}"
    );

    // Peer should be evicted from in-memory cache
    assert!(
        !subscribers.read().unwrap().contains_key(pubkey),
        "peer should be evicted after {failures} failures"
    );
}

/// With the old threshold of 5, a peer at 5 failures would be evicted.
/// With the new threshold of 10, it should NOT be evicted at 5.
#[tokio::test]
async fn push_not_evicted_at_old_threshold() {
    let mock_server = MockServer::start().await;

    // Always return 503
    Mock::given(method("POST"))
        .and(path("/sync/push"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&mock_server)
        .await;

    let push_url = format!("{}/sync/push", mock_server.uri());

    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let pubkey = "peer-not-evicted-yet";
    seed_peer(&db, pubkey, &push_url);

    let subscribers: Arc<RwLock<HashMap<String, String>>> =
        Arc::new(RwLock::new(HashMap::from([(pubkey.to_string(), push_url)])));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    // Push 5 times — under old threshold this would evict, under new threshold it should not.
    for _ in 0..5 {
        let events = vec![make_test_event()];
        stophammer::api::fan_out_push_public(
            pool.clone(),
            client.clone(),
            Arc::clone(&subscribers),
            events,
        )
        .await;

        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    }

    let failures = get_failures(&db, pubkey);
    assert!(
        failures >= 5,
        "peer should have >= 5 failures, got {failures}"
    );

    // Peer should NOT be evicted yet (new threshold is 10)
    assert!(
        subscribers.read().unwrap().contains_key(pubkey),
        "peer should NOT be evicted at only {failures} failures (threshold is 10)"
    );
}
