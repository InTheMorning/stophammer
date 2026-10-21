//! ADR 0016: the peer rules of the primary.
//!
//! - A new `POST /sync/register` resets the failure count of the peer and
//!   puts the peer in the push cache again.
//! - `GET /sync/peers` gives only the peers with fewer than
//!   `MAX_PEER_FAILURES` consecutive failures.
//! - The push handler of a community node records the time of the last push,
//!   so the fallback poll stays quiet.

mod common;

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SYNC_TOKEN: &str = "adr0016-sync-token";

fn test_app_state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("adr0016-primary"));
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(
            "test-token".into(),
            vec![],
        )),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "adr0016-admin-token".into(),
        sync_token: Some(SYNC_TOKEN.into()),
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

fn failures(db: &Arc<Mutex<rusqlite::Connection>>, pubkey: &str) -> i64 {
    db.lock()
        .expect("lock db")
        .query_row(
            "SELECT consecutive_failures FROM peer_nodes WHERE node_pubkey = ?1",
            [pubkey],
            |row| row.get(0),
        )
        .expect("read the failure count")
}

fn seed_peer(db: &Arc<Mutex<rusqlite::Connection>>, pubkey: &str, url: &str, failures: i64) {
    db.lock()
        .expect("lock db")
        .execute(
            "INSERT INTO peer_nodes (node_pubkey, node_url, discovered_at, consecutive_failures) \
             VALUES (?1, ?2, 0, ?3)",
            rusqlite::params![pubkey, url, failures],
        )
        .expect("seed a peer");
}

#[tokio::test]
async fn a_new_registration_resets_the_failure_count_and_the_cache() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let peer_signer =
        stophammer::signing::NodeSigner::load_or_create(temp_dir.path().join("peer.key"))
            .expect("create the peer key");
    let peer_pubkey = peer_signer.pubkey_hex().to_string();
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/node/info"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "node_pubkey": peer_pubkey })),
        )
        .mount(&mock_server)
        .await;
    let push_url = format!("{}/sync/push", mock_server.uri());

    let db = common::test_db_arc();
    seed_peer(
        &db,
        &peer_pubkey,
        &push_url,
        stophammer::db::MAX_PEER_FAILURES,
    );
    let state = test_app_state(Arc::clone(&db));
    let app = stophammer::api::build_router(Arc::clone(&state));

    let body = common::signed_sync_register_body(&peer_signer, &push_url);
    let req = Request::builder()
        .method("POST")
        .uri("/sync/register")
        .header("Content-Type", "application/json")
        .header("X-Sync-Token", SYNC_TOKEN)
        .body(axum::body::Body::from(
            serde_json::to_vec(&body).expect("serialize"),
        ))
        .expect("build request");
    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 200, "the registration must succeed");

    assert_eq!(
        failures(&db, &peer_pubkey),
        0,
        "ADR 0016: a new registration resets the failure count"
    );
    assert!(
        state
            .push_subscribers
            .read()
            .expect("read the cache")
            .contains_key(&peer_pubkey),
        "ADR 0016: a new registration puts the peer in the push cache again"
    );
}

#[tokio::test]
async fn the_peer_list_gives_only_peers_under_the_failure_threshold() {
    let db = common::test_db_arc();
    let threshold = stophammer::db::MAX_PEER_FAILURES;
    seed_peer(
        &db,
        "adr0016-active",
        "https://active.example/sync/push",
        threshold - 1,
    );
    seed_peer(
        &db,
        "adr0016-evicted",
        "https://evicted.example/sync/push",
        threshold,
    );
    let app = stophammer::api::build_router(test_app_state(Arc::clone(&db)));

    let req = Request::builder()
        .method("GET")
        .uri("/sync/peers")
        .header("X-Sync-Token", SYNC_TOKEN)
        .body(axum::body::Body::empty())
        .expect("build request");
    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 200);
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let text = String::from_utf8_lossy(&bytes);

    assert!(
        text.contains("adr0016-active"),
        "ADR 0016: a peer under the threshold is in the list: {text}"
    );
    assert!(
        !text.contains("adr0016-evicted"),
        "ADR 0016: a peer at the threshold is not in the list: {text}"
    );
}

#[tokio::test]
async fn an_accepted_push_records_the_time_of_the_last_push() {
    let db = common::test_db_arc();
    let pool = common::wrap_pool(Arc::clone(&db));
    let signer = common::temp_signer("adr0016-push-signer");
    let now = stophammer::db::unix_now();
    let payload = serde_json::json!({
        "feed": {
            "feed_guid": "adr0016-push-feed",
            "feed_url": "https://example.com/adr0016-push-feed.xml",
            "title": "ADR 0016 Push Feed",
            "title_lower": "adr 0016 push feed",
            "description": null, "image_url": null, "publisher": null, "language": null,
            "explicit": false, "itunes_type": null, "release_artist": null,
            "release_artist_sort": null, "release_date": null, "release_kind": null,
            "episode_count": 0, "newest_item_at": null, "oldest_item_at": null,
            "created_at": now, "updated_at": now, "raw_medium": null
        }
    });
    let payload_json = serde_json::to_string(&payload).expect("serialize payload");
    let event_id = uuid::Uuid::new_v4().to_string();
    let (signed_by, signature) = signer.sign_event(
        &event_id,
        &stophammer::event::EventType::FeedUpserted,
        &payload_json,
        "adr0016-push-feed",
        now,
        1,
    );
    let push_body = serde_json::json!({
        "events": [{
            "event_id": event_id,
            "event_type": "feed_upserted",
            "payload": { "type": "feed_upserted", "data": payload },
            "subject_guid": "adr0016-push-feed",
            "signed_by": signed_by,
            "signature": signature,
            "seq": 1,
            "created_at": now,
            "warnings": [],
            "payload_json": payload_json
        }]
    });

    let last_push_at = Arc::new(AtomicI64::new(0));
    let state = Arc::new(stophammer::community::CommunityState {
        db: pool,
        primary_pubkey_hex: signer.pubkey_hex().to_string(),
        last_push_at: Arc::clone(&last_push_at),
        sse_registry: None,
    });
    let app = stophammer::community::build_community_push_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/sync/push")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&push_body).expect("serialize"),
        ))
        .expect("build request");
    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 200, "the push must be accepted");

    assert!(
        last_push_at.load(Ordering::Relaxed) >= now,
        "ADR 0016: an accepted push records its time, so the fallback poll stays quiet"
    );
}

#[tokio::test]
async fn the_fallback_poll_stays_quiet_while_pushes_arrive() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sync/register"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/sync/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "events": [], "has_more": false, "next_seq": 0
        })))
        .expect(0)
        .mount(&mock_server)
        .await;

    let last_push_at = Arc::new(AtomicI64::new(stophammer::db::unix_now()));
    let config = stophammer::community::CommunityConfig {
        primary_url: mock_server.uri(),
        node_address: "http://localhost:9999".into(),
        poll_interval_secs: 1,
        push_timeout_secs: 90,
    };
    let pool = common::wrap_pool(common::test_db_arc());
    let signer = Arc::new(common::temp_signer("adr0016-quiet-poll"));
    let lp = Arc::clone(&last_push_at);
    let handle = tokio::spawn(async move {
        stophammer::community::run_community_sync(config, pool, signer, lp, None).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    handle.abort();

    // `expect(0)` fails at drop when the loop polled.
    mock_server.verify().await;
}

#[tokio::test]
async fn an_accepted_ingest_pushes_its_events_to_each_registered_peer() {
    let peer = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sync/push"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "applied": 1, "rejected": 0, "duplicate": 0
        })))
        .mount(&peer)
        .await;

    let crawl_token = "adr0016-crawl-token";
    let db = common::test_db_arc();
    let signer = Arc::new(common::temp_signer("adr0016-fanout"));
    let pubkey = signer.pubkey_hex().to_string();
    let chain = stophammer::verify::build_chain(
        &stophammer::verify::ChainSpec {
            names: vec!["content_hash".to_string()],
        },
        crawl_token.to_string(),
    );
    let mut subscribers = HashMap::new();
    subscribers.insert(
        "adr0016-peer".to_string(),
        format!("{}/sync/push", peer.uri()),
    );
    seed_peer(&db, "adr0016-peer", &format!("{}/sync/push", peer.uri()), 0);
    let state = Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(Arc::clone(&db)),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "adr0016-admin-token".into(),
        sync_token: Some(SYNC_TOKEN.into()),
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(subscribers)),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    });
    let app = stophammer::api::build_router(state);

    let url = "https://example.com/adr0016-fanout.xml";
    let ingest = serde_json::json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": "adr0016-fanout-hash",
        "feed_data": {
            "feed_guid": "adr0016-fanout-feed",
            "title": "ADR 0016 Fan-out Feed",
            "raw_medium": "episodic",
            "explicit": false,
            "tracks": [{
                "track_guid": "adr0016-fanout-feed-track-01",
                "title": "Track One",
                "explicit": false
            }]
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri("/ingest/feed")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&ingest).expect("serialize"),
        ))
        .expect("build request");
    let resp = app.oneshot(req).await.expect("call handler");
    assert_eq!(resp.status(), 200, "the ingest must be answered");

    let mut pushes = 0;
    for _ in 0..50 {
        pushes = peer.received_requests().await.map_or(0, |r| r.len());
        if pushes > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        pushes > 0,
        "ADR 0016: an accepted ingest pushes its events to each registered peer"
    );
}
