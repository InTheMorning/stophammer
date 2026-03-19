mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use http::Request;
use http_body_util::BodyExt;
use stophammer::db;
use tower::ServiceExt;

fn seed_feed(conn: &rusqlite::Connection, feed_guid: &str) {
    let artist = db::resolve_artist(conn, "Resolver Artist", Some(feed_guid)).expect("artist");
    let credit = db::get_or_create_artist_credit(
        conn,
        &artist.name,
        &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
        Some(feed_guid),
    )
    .expect("artist credit");
    let now = db::unix_now();
    let feed = stophammer::model::Feed {
        feed_guid: feed_guid.to_string(),
        feed_url: format!("https://example.com/{feed_guid}.xml"),
        title: format!("Feed {feed_guid}"),
        title_lower: format!("feed {feed_guid}"),
        artist_credit_id: credit.id,
        description: Some("resolver test feed".into()),
        image_url: None,
        language: Some("en".into()),
        explicit: false,
        itunes_type: None,
        episode_count: 0,
        newest_item_at: None,
        oldest_item_at: None,
        created_at: now,
        updated_at: now,
        raw_medium: Some("music".into()),
    };
    db::upsert_feed(conn, &feed).expect("feed");
}

fn test_app_state(pool: stophammer::db_pool::DbPool) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-resolver-status-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    Arc::new(stophammer::api::AppState {
        db: pool,
        chain: Arc::new(stophammer::verify::VerifierChain::new(vec![])),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token: Some("test-sync-token".into()),
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

fn seed_split_artist_feeds(conn: &rusqlite::Connection) {
    let now = db::unix_now();

    let artist_a = db::resolve_artist(conn, "Resolver Split Artist", Some("feed-resolver-split-a"))
        .expect("artist a");
    let credit_a = db::get_or_create_artist_credit(
        conn,
        &artist_a.name,
        &[(
            artist_a.artist_id.clone(),
            artist_a.name.clone(),
            String::new(),
        )],
        Some("feed-resolver-split-a"),
    )
    .expect("credit a");
    let artist_b = db::resolve_artist(conn, "Resolver Split Artist", Some("feed-resolver-split-b"))
        .expect("artist b");
    let credit_b = db::get_or_create_artist_credit(
        conn,
        &artist_b.name,
        &[(
            artist_b.artist_id.clone(),
            artist_b.name.clone(),
            String::new(),
        )],
        Some("feed-resolver-split-b"),
    )
    .expect("credit b");

    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-resolver-split-a', 'https://wavlake.com/feed/music/resolver-a', 'Resolver A', 'resolver a', ?1, ?2, ?2)",
        rusqlite::params![credit_a.id, now],
    )
    .expect("feed a");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-resolver-split-b', 'https://feeds.fountain.fm/resolver-b', 'Resolver B', 'resolver b', ?1, ?2, ?2)",
        rusqlite::params![credit_b.id, now],
    )
    .expect("feed b");

    for feed_guid in ["feed-resolver-split-a", "feed-resolver-split-b"] {
        conn.execute(
            "INSERT INTO source_entity_links \
             (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
             VALUES (?1, 'feed', ?1, 0, 'website', 'https://wavlake.com/resolver-split-artist', 'rss_link', 'feed.link', ?2)",
            rusqlite::params![feed_guid, now],
        )
        .expect("website link");
    }
}

#[test]
fn mark_claim_complete_queue_entry() {
    let mut conn = common::test_db();
    seed_feed(&conn, "feed-resolver-queue");

    stophammer::resolver::queue::mark_feed_dirty_for_resolver(&conn, "feed-resolver-queue")
        .expect("mark dirty");

    let claimed = db::claim_dirty_feeds(&mut conn, "worker-a", 10, db::unix_now()).expect("claim");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].feed_guid, "feed-resolver-queue");
    assert_eq!(
        claimed[0].dirty_mask,
        stophammer::resolver::queue::DEFAULT_DIRTY_MASK
    );

    db::complete_dirty_feed(&conn, "feed-resolver-queue", "worker-a").expect("complete");
    let claimed_again =
        db::claim_dirty_feeds(&mut conn, "worker-a", 10, db::unix_now()).expect("claim again");
    assert!(claimed_again.is_empty());
}

#[test]
fn completion_preserves_re_marked_entry() {
    let mut conn = common::test_db();
    seed_feed(&conn, "feed-resolver-retry");

    stophammer::resolver::queue::mark_feed_dirty_for_resolver(&conn, "feed-resolver-retry")
        .expect("mark dirty");
    let claimed = db::claim_dirty_feeds(&mut conn, "worker-a", 10, 1_000).expect("claim");
    assert_eq!(claimed.len(), 1);

    db::mark_feed_dirty(
        &conn,
        "feed-resolver-retry",
        stophammer::resolver::queue::DIRTY_CANONICAL_SEARCH,
    )
    .expect("re-mark");
    db::complete_dirty_feed(&conn, "feed-resolver-retry", "worker-a").expect("complete");

    let claimed_again =
        db::claim_dirty_feeds(&mut conn, "worker-b", 10, 2_000).expect("claim again");
    assert_eq!(claimed_again.len(), 1);
}

#[test]
fn resolver_batch_skips_when_import_is_active() {
    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_feed(&conn, "feed-resolver-pause");
        stophammer::resolver::queue::mark_feed_dirty_for_resolver(&conn, "feed-resolver-pause")
            .expect("mark dirty");
        db::set_resolver_import_active(&conn, true).expect("set import state");
    }

    let summary =
        stophammer::resolver::worker::run_batch(&pool, "worker-a", 10).expect("run batch");
    assert!(summary.skipped_import_active);
    assert_eq!(summary.claimed, 0);
}

#[test]
fn resolver_batch_ignores_stale_import_active_heartbeat() {
    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_feed(&conn, "feed-resolver-stale-import");
        stophammer::resolver::queue::mark_feed_dirty_for_resolver(
            &conn,
            "feed-resolver-stale-import",
        )
        .expect("mark dirty");
        db::set_resolver_import_active_with_now(&conn, true, db::unix_now() - (11 * 60))
            .expect("set stale import state");
    }

    let summary =
        stophammer::resolver::worker::run_batch(&pool, "worker-a", 10).expect("run batch");
    assert!(!summary.skipped_import_active);
    assert!(summary.stale_import_active_ignored);
    assert_eq!(summary.claimed, 1);
    assert_eq!(summary.resolved, 1);
}

#[test]
fn resolver_batch_drains_phase1_work() {
    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_feed(&conn, "feed-resolver-run");
        stophammer::resolver::queue::mark_feed_dirty_for_resolver(&conn, "feed-resolver-run")
            .expect("mark dirty");
    }

    let summary =
        stophammer::resolver::worker::run_batch(&pool, "worker-a", 10).expect("run batch");
    assert_eq!(summary.claimed, 1);
    assert_eq!(summary.resolved, 1);
    assert_eq!(summary.failed, 0);
    assert!(!summary.stale_import_active_ignored);
    assert_eq!(summary.artist_seed_artists, 1);
    assert_eq!(summary.artist_candidate_groups, 0);
    assert_eq!(summary.artist_groups_processed, 0);
    assert_eq!(summary.artist_merges_applied, 0);

    let mut conn = pool.writer().lock().expect("writer");
    let claimed = db::claim_dirty_feeds(&mut conn, "worker-b", 10, db::unix_now()).expect("claim");
    assert!(claimed.is_empty());
}

#[test]
fn resolver_batch_runs_targeted_artist_identity_work() {
    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_split_artist_feeds(&conn);
        db::mark_feed_dirty(
            &conn,
            "feed-resolver-split-b",
            stophammer::resolver::queue::DIRTY_ARTIST_IDENTITY,
        )
        .expect("mark dirty");
    }

    let summary =
        stophammer::resolver::worker::run_batch(&pool, "worker-a", 10).expect("run batch");
    assert_eq!(summary.claimed, 1);
    assert_eq!(summary.resolved, 1);
    assert_eq!(summary.failed, 0);
    assert!(!summary.stale_import_active_ignored);
    assert_eq!(summary.artist_seed_artists, 1);
    assert_eq!(summary.artist_candidate_groups, 2);
    assert_eq!(summary.artist_groups_processed, 1);
    assert_eq!(summary.artist_merges_applied, 1);

    let conn = pool.writer().lock().expect("writer");
    let artist_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'resolver split artist'",
            [],
            |row| row.get(0),
        )
        .expect("artist count");
    assert_eq!(artist_count, 1);
}

#[test]
fn resolver_queue_counts_reflect_ready_locked_and_failed_rows() {
    let mut conn = common::test_db();
    seed_feed(&conn, "feed-resolver-counts");

    stophammer::resolver::queue::mark_feed_dirty_for_resolver(&conn, "feed-resolver-counts")
        .expect("mark dirty");
    let counts = db::get_resolver_queue_counts(&conn).expect("counts");
    assert_eq!(counts.total, 1);
    assert_eq!(counts.ready, 1);
    assert_eq!(counts.locked, 0);
    assert_eq!(counts.failed, 0);

    let claimed = db::claim_dirty_feeds(&mut conn, "worker-a", 10, db::unix_now()).expect("claim");
    assert_eq!(claimed.len(), 1);
    let counts = db::get_resolver_queue_counts(&conn).expect("counts after claim");
    assert_eq!(counts.total, 1);
    assert_eq!(counts.ready, 0);
    assert_eq!(counts.locked, 1);
    assert_eq!(counts.failed, 0);

    db::fail_dirty_feed(&conn, "feed-resolver-counts", "worker-a", "boom").expect("fail");
    let counts = db::get_resolver_queue_counts(&conn).expect("counts after fail");
    assert_eq!(counts.total, 1);
    assert_eq!(counts.ready, 1);
    assert_eq!(counts.locked, 0);
    assert_eq!(counts.failed, 1);
}

#[test]
fn resolver_batch_preserves_source_feed_track_and_claim_rows() {
    let (pool, _dir) = common::test_db_pool();
    let now = db::unix_now();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_feed(&conn, "feed-resolver-preserve");
        let feed = stophammer::db::get_feed(&conn, "feed-resolver-preserve")
            .expect("get feed")
            .expect("feed exists");
        let track = stophammer::model::Track {
            track_guid: "track-resolver-preserve".into(),
            feed_guid: "feed-resolver-preserve".into(),
            artist_credit_id: feed.artist_credit_id,
            title: "Resolver Preserve Track".into(),
            title_lower: "resolver preserve track".into(),
            pub_date: Some(now),
            duration_secs: Some(180),
            enclosure_url: Some("https://cdn.example.com/preserve.mp3".into()),
            enclosure_type: Some("audio/mpeg".into()),
            enclosure_bytes: Some(1234),
            track_number: Some(1),
            season: None,
            explicit: false,
            description: Some("preserve me".into()),
            created_at: now,
            updated_at: now,
        };
        db::upsert_track(&conn, &track).expect("upsert track");
        db::replace_source_entity_ids_for_feed(
            &conn,
            "feed-resolver-preserve",
            &[stophammer::model::SourceEntityIdClaim {
                id: None,
                feed_guid: "feed-resolver-preserve".into(),
                entity_type: "feed".into(),
                entity_id: "feed-resolver-preserve".into(),
                position: 0,
                scheme: "nostr_npub".into(),
                value: "npub1resolverpreserve".into(),
                source: "podcast_txt".into(),
                extraction_path: "feed.podcast:txt".into(),
                observed_at: now,
            }],
        )
        .expect("replace source ids");
        db::replace_source_entity_links_for_feed(
            &conn,
            "feed-resolver-preserve",
            &[stophammer::model::SourceEntityLink {
                id: None,
                feed_guid: "feed-resolver-preserve".into(),
                entity_type: "feed".into(),
                entity_id: "feed-resolver-preserve".into(),
                position: 0,
                link_type: "website".into(),
                url: "https://artist.example.com/preserve".into(),
                source: "rss_link".into(),
                extraction_path: "feed.link".into(),
                observed_at: now,
            }],
        )
        .expect("replace source links");
        stophammer::resolver::queue::mark_feed_dirty_for_resolver(&conn, "feed-resolver-preserve")
            .expect("mark dirty");
    }

    let summary =
        stophammer::resolver::worker::run_batch(&pool, "worker-a", 10).expect("run batch");
    assert_eq!(summary.claimed, 1);
    assert_eq!(summary.resolved, 1);

    let conn = pool.writer().lock().expect("writer");
    let feed = stophammer::db::get_feed(&conn, "feed-resolver-preserve")
        .expect("get feed after resolver")
        .expect("feed still exists");
    assert_eq!(feed.title, "Feed feed-resolver-preserve");
    assert_eq!(
        feed.feed_url,
        "https://example.com/feed-resolver-preserve.xml"
    );

    let track = stophammer::db::get_track(&conn, "track-resolver-preserve")
        .expect("get track after resolver")
        .expect("track still exists");
    assert_eq!(track.title, "Resolver Preserve Track");
    assert_eq!(track.description.as_deref(), Some("preserve me"));

    let ids =
        stophammer::db::get_source_entity_ids_for_entity(&conn, "feed", "feed-resolver-preserve")
            .expect("source ids after resolver");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].scheme, "nostr_npub");
    assert_eq!(ids[0].value, "npub1resolverpreserve");

    let links =
        stophammer::db::get_source_entity_links_for_entity(&conn, "feed", "feed-resolver-preserve")
            .expect("source links after resolver");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url, "https://artist.example.com/preserve");
}

#[tokio::test]
async fn resolver_status_reports_queue_counts_and_boundary_contract() {
    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_feed(&conn, "feed-resolver-status");
        stophammer::resolver::queue::mark_feed_dirty_for_resolver(&conn, "feed-resolver-status")
            .expect("mark dirty");
        db::set_resolver_import_active(&conn, true).expect("set import state");
    }

    let app = stophammer::api::build_readonly_router(test_app_state(pool));
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/resolver/status")
                .body(axum::body::Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json body");

    assert_eq!(body["api_version"], "v1");
    assert_eq!(body["source_layer"]["authoritative"], true);
    assert_eq!(body["source_layer"]["preserved"], true);
    assert_eq!(body["resolver"]["import_active"], true);
    assert_eq!(body["resolver"]["caught_up"], false);
    assert_eq!(body["resolver"]["queue"]["total"], 1);
    assert_eq!(body["resolver"]["queue"]["ready"], 1);
    assert!(
        body["source_layer"]["immediate_endpoints"]
            .as_array()
            .expect("immediate endpoints")
            .iter()
            .any(|v| v == "/v1/feeds/{guid}")
    );
    assert!(
        body["resolver"]["resolver_backed_endpoints"]
            .as_array()
            .expect("resolver-backed endpoints")
            .iter()
            .any(|v| v == "/v1/releases/{id}")
    );
}
