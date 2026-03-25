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

fn seed_feed_with_track(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    track_guid: &str,
    track_title: &str,
) {
    seed_feed(conn, feed_guid);
    let feed = db::get_feed(conn, feed_guid)
        .expect("get feed")
        .expect("feed exists");
    let now = db::unix_now();
    let track = stophammer::model::Track {
        track_guid: track_guid.to_string(),
        feed_guid: feed_guid.to_string(),
        artist_credit_id: feed.artist_credit_id,
        title: track_title.to_string(),
        title_lower: track_title.to_lowercase(),
        pub_date: Some(now),
        duration_secs: Some(180),
        enclosure_url: Some(format!("https://cdn.example.com/{track_guid}.mp3")),
        enclosure_type: Some("audio/mpeg".into()),
        enclosure_bytes: Some(1024),
        track_number: Some(1),
        season: None,
        explicit: false,
        description: Some("resolver snapshot test track".into()),
        created_at: now,
        updated_at: now,
    };
    db::upsert_track(conn, &track).expect("track");
}

fn seed_feed_promotions_source_claims(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    track_guid: &str,
) {
    let now = db::unix_now();
    db::replace_source_entity_ids_for_feed(
        conn,
        feed_guid,
        &[stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: feed_guid.to_string(),
            entity_type: "feed".into(),
            entity_id: feed_guid.to_string(),
            position: 0,
            scheme: "nostr_npub".into(),
            value: "npub1resolverpromotions".into(),
            source: "rss_guid".into(),
            extraction_path: "feed.podcast:valueRecipient".into(),
            observed_at: now,
        }],
    )
    .expect("source entity ids");
    db::replace_source_entity_links_for_feed(
        conn,
        feed_guid,
        &[
            stophammer::model::SourceEntityLink {
                id: None,
                feed_guid: feed_guid.to_string(),
                entity_type: "feed".into(),
                entity_id: feed_guid.to_string(),
                position: 0,
                link_type: "website".into(),
                url: "https://wavlake.com/resolver-promotions".into(),
                source: "rss_link".into(),
                extraction_path: "feed.link".into(),
                observed_at: now,
            },
            stophammer::model::SourceEntityLink {
                id: None,
                feed_guid: feed_guid.to_string(),
                entity_type: "track".into(),
                entity_id: track_guid.to_string(),
                position: 0,
                link_type: "web_page".into(),
                url: "https://wavlake.com/resolver-promotions/track".into(),
                source: "item.link".into(),
                extraction_path: "item.link".into(),
                observed_at: now,
            },
        ],
    )
    .expect("source entity links");
    db::replace_source_item_enclosures_for_feed(
        conn,
        feed_guid,
        &[stophammer::model::SourceItemEnclosure {
            id: None,
            feed_guid: feed_guid.to_string(),
            entity_type: "track".into(),
            entity_id: track_guid.to_string(),
            position: 0,
            url: format!("https://cdn.example.com/{track_guid}.mp3"),
            mime_type: Some("audio/mpeg".into()),
            bytes: Some(1024),
            rel: None,
            title: None,
            is_primary: true,
            source: "rss_enclosure".into(),
            extraction_path: "item.enclosure".into(),
            observed_at: now,
        }],
    )
    .expect("source item enclosures");
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
    assert_eq!(summary.artist_merge_events_emitted, 0);

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
    assert_eq!(summary.artist_candidate_groups, 1);
    assert_eq!(summary.artist_groups_processed, 1);
    assert_eq!(summary.artist_merges_applied, 1);
    assert_eq!(summary.artist_merge_events_emitted, 0);

    let conn = pool.writer().lock().expect("writer");
    let artist_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'resolver split artist'",
            [],
            |row| row.get(0),
        )
        .expect("artist count");
    assert_eq!(artist_count, 1);

    let reviews = db::list_artist_identity_reviews_for_feed(&conn, "feed-resolver-split-b")
        .expect("reviews for feed");
    assert!(
        reviews
            .iter()
            .any(|review| review.status == "merged" && review.source == "normalized_website"),
        "resolver should persist a merged review item for the feed-scoped candidate"
    );
}

#[test]
fn do_not_merge_override_blocks_targeted_artist_identity_merge() {
    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_split_artist_feeds(&conn);
        let plan = db::explain_artist_identity_for_feed(&conn, "feed-resolver-split-b")
            .expect("feed plan");
        let now = db::unix_now();
        for group in &plan.candidate_groups {
            conn.execute(
                "INSERT INTO artist_identity_override \
                 (source, name_key, evidence_key, override_type, note, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, 'do_not_merge', 'operator decision', ?4, ?4)",
                rusqlite::params![group.source, group.name_key, group.evidence_key, now],
            )
            .expect("insert override");
        }
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
    assert_eq!(summary.artist_groups_processed, 0);
    assert_eq!(summary.artist_merges_applied, 0);
    assert_eq!(summary.artist_merge_events_emitted, 0);

    let conn = pool.writer().lock().expect("writer");
    let artist_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'resolver split artist'",
            [],
            |row| row.get(0),
        )
        .expect("artist count");
    assert_eq!(
        artist_count, 2,
        "do_not_merge override should preserve both split artists"
    );
    let reviews = db::list_artist_identity_reviews_for_feed(&conn, "feed-resolver-split-b")
        .expect("reviews for feed");
    assert!(
        reviews.iter().any(|review| {
            review.status == "blocked"
                && review.override_type.as_deref() == Some("do_not_merge")
                && review.note.as_deref() == Some("operator decision")
        }),
        "resolver should persist a blocked review item when do_not_merge is set"
    );
}

#[test]
fn resolver_batch_emits_artist_merged_events_when_signer_present() {
    let signer = common::temp_signer("resolver-artist-merge-events");
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
        stophammer::resolver::worker::run_batch_with_signer(&pool, "worker-a", 10, Some(&signer))
            .expect("run batch with signer");
    assert_eq!(summary.claimed, 1);
    assert_eq!(summary.resolved, 1);
    assert_eq!(summary.artist_groups_processed, 1);
    assert_eq!(summary.artist_merges_applied, 1);
    assert_eq!(summary.artist_merge_events_emitted, 1);
    assert_eq!(summary.artist_identity_events_emitted, 1);

    let conn = pool.writer().lock().expect("writer");
    let event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'artist_merged'",
            [],
            |row| row.get(0),
        )
        .expect("artist merged event count");
    assert_eq!(event_count, 1);
    let resolved_event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'artist_identity_feed_resolved'",
            [],
            |row| row.get(0),
        )
        .expect("artist identity resolved event count");
    assert_eq!(resolved_event_count, 1);
}

#[test]
fn resolver_batch_emits_source_and_canonical_snapshot_events_when_signer_present() {
    let signer = common::temp_signer("resolver-source-canonical-events");
    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_feed_with_track(
            &conn,
            "feed-resolver-signed-snapshots",
            "track-resolver-signed-snapshots",
            "Signed Snapshot Track",
        );
        stophammer::resolver::queue::mark_feed_dirty_for_resolver(
            &conn,
            "feed-resolver-signed-snapshots",
        )
        .expect("mark dirty");
    }

    let summary =
        stophammer::resolver::worker::run_batch_with_signer(&pool, "worker-a", 10, Some(&signer))
            .expect("run batch with signer");
    assert_eq!(summary.claimed, 1);
    assert_eq!(summary.resolved, 1);
    assert_eq!(summary.source_read_model_events_emitted, 1);
    assert_eq!(summary.canonical_state_events_emitted, 1);
    assert_eq!(summary.artist_identity_events_emitted, 1);

    let conn = pool.writer().lock().expect("writer");
    let source_event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'source_feed_read_models_resolved'",
            [],
            |row| row.get(0),
        )
        .expect("source event count");
    let canonical_event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'canonical_feed_state_replaced'",
            [],
            |row| row.get(0),
        )
        .expect("canonical state event count");
    assert_eq!(source_event_count, 1);
    assert_eq!(canonical_event_count, 1);
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
fn resolver_queue_counts_are_zero_when_queue_is_empty() {
    let conn = common::test_db();
    let counts = db::get_resolver_queue_counts(&conn).expect("counts");
    assert_eq!(counts.total, 0);
    assert_eq!(counts.ready, 0);
    assert_eq!(counts.locked, 0);
    assert_eq!(counts.failed, 0);
}

#[tokio::test]
async fn canonical_feed_state_snapshot_applies_without_local_resolver() {
    let signer = common::temp_signer("resolver-canonical-snapshot");
    let primary = common::test_db();
    seed_feed_with_track(
        &primary,
        "feed-resolver-snapshot",
        "track-resolver-snapshot",
        "Snapshot Track",
    );
    db::sync_canonical_state_for_feed(&primary, "feed-resolver-snapshot")
        .expect("sync canonical state");
    let payload = db::build_canonical_feed_state_snapshot(&primary, "feed-resolver-snapshot")
        .expect("build snapshot");
    assert_eq!(payload.release_maps.len(), 1);
    assert_eq!(payload.recording_maps.len(), 1);

    let payload_json = serde_json::to_string(&payload).expect("serialize payload");
    let created_at = db::unix_now();
    let (signed_by, signature) = signer.sign_event(
        "event-canonical-feed-state",
        &stophammer::event::EventType::CanonicalFeedStateReplaced,
        &payload_json,
        "feed-resolver-snapshot",
        created_at,
        1,
    );
    let event = stophammer::event::Event {
        event_id: "event-canonical-feed-state".into(),
        event_type: stophammer::event::EventType::CanonicalFeedStateReplaced,
        payload: stophammer::event::EventPayload::CanonicalFeedStateReplaced(payload.clone()),
        subject_guid: "feed-resolver-snapshot".into(),
        signed_by,
        signature,
        seq: 1,
        created_at,
        warnings: Vec::new(),
        payload_json,
    };

    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_feed_with_track(
            &conn,
            "feed-resolver-snapshot",
            "track-resolver-snapshot",
            "Snapshot Track",
        );
        db::mark_feed_dirty(
            &conn,
            "feed-resolver-snapshot",
            stophammer::resolver::queue::DIRTY_CANONICAL_STATE,
        )
        .expect("mark canonical dirty");
    }

    let summary = stophammer::apply::apply_events(pool.clone(), vec![event], None).await;
    assert_eq!(summary.applied, 1);
    assert_eq!(summary.rejected, 0);

    let conn = pool.writer().lock().expect("writer");
    let release_maps = db::get_source_feed_release_maps_for_feed(&conn, "feed-resolver-snapshot")
        .expect("release maps");
    let recording_maps =
        db::get_source_item_recording_maps_for_feed(&conn, "feed-resolver-snapshot")
            .expect("recording maps");
    assert_eq!(release_maps.len(), 1);
    assert_eq!(recording_maps.len(), 1);

    let release = db::get_release(&conn, &release_maps[0].release_id)
        .expect("get release")
        .expect("release exists");
    let recording = db::get_recording(&conn, &recording_maps[0].recording_id)
        .expect("get recording")
        .expect("recording exists");
    assert_eq!(release.title, "Feed feed-resolver-snapshot");
    assert_eq!(recording.title, "Snapshot Track");
    let release_search: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'release' AND entity_id = ?1",
            rusqlite::params![release.release_id],
            |row| row.get(0),
        )
        .expect("release search row");
    let recording_search: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'recording' AND entity_id = ?1",
            rusqlite::params![recording.recording_id],
            |row| row.get(0),
        )
        .expect("recording search row");
    assert_eq!(release_search, 1);
    assert_eq!(recording_search, 1);

    let counts = db::get_resolver_queue_counts(&conn).expect("queue counts");
    assert_eq!(
        counts.total, 0,
        "canonical snapshot should clear canonical dirty work"
    );
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "end-to-end source read-model snapshot application is clearest as one flow"
)]
async fn source_read_models_resolved_event_applies_without_local_resolver() {
    let signer = common::temp_signer("resolver-source-read-models");
    let primary = common::test_db();
    seed_feed_with_track(
        &primary,
        "feed-resolver-source-models",
        "track-resolver-source-models",
        "Source Models Track",
    );
    db::sync_source_read_models_for_feed(&primary, "feed-resolver-source-models")
        .expect("sync source read models");
    let payload =
        db::build_source_feed_read_models_resolved_payload(&primary, "feed-resolver-source-models")
            .expect("build source payload")
            .expect("source payload exists");
    assert_eq!(payload.feed_rows, 1);
    assert_eq!(payload.track_rows, 1);
    assert_eq!(payload.artist_rows, 1);

    let payload_json = serde_json::to_string(&payload).expect("serialize payload");
    let created_at = db::unix_now();
    let (signed_by, signature) = signer.sign_event(
        "event-source-feed-read-models",
        &stophammer::event::EventType::SourceFeedReadModelsResolved,
        &payload_json,
        "feed-resolver-source-models",
        created_at,
        1,
    );
    let event = stophammer::event::Event {
        event_id: "event-source-feed-read-models".into(),
        event_type: stophammer::event::EventType::SourceFeedReadModelsResolved,
        payload: stophammer::event::EventPayload::SourceFeedReadModelsResolved(payload),
        subject_guid: "feed-resolver-source-models".into(),
        signed_by,
        signature,
        seq: 1,
        created_at,
        warnings: Vec::new(),
        payload_json,
    };

    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_feed_with_track(
            &conn,
            "feed-resolver-source-models",
            "track-resolver-source-models",
            "Source Models Track",
        );
        db::mark_feed_dirty(
            &conn,
            "feed-resolver-source-models",
            stophammer::resolver::queue::DIRTY_SOURCE_READ_MODELS,
        )
        .expect("mark source dirty");
    }

    {
        let conn = pool.writer().lock().expect("writer");
        let search_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'feed' AND entity_id = 'feed-resolver-source-models'",
                [],
                |row| row.get(0),
            )
            .expect("search before apply");
        assert_eq!(search_count, 0);
    }

    let summary = stophammer::apply::apply_events(pool.clone(), vec![event], None).await;
    assert_eq!(summary.applied, 1);
    assert_eq!(summary.rejected, 0);

    let conn = pool.writer().lock().expect("writer");
    let feed_search: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'feed' AND entity_id = 'feed-resolver-source-models'",
            [],
            |row| row.get(0),
        )
        .expect("feed search row");
    let track_search: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'track' AND entity_id = 'track-resolver-source-models'",
            [],
            |row| row.get(0),
        )
        .expect("track search row");
    let feed_quality: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entity_quality WHERE entity_type = 'feed' AND entity_id = 'feed-resolver-source-models'",
            [],
            |row| row.get(0),
        )
        .expect("feed quality row");
    let track_quality: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entity_quality WHERE entity_type = 'track' AND entity_id = 'track-resolver-source-models'",
            [],
            |row| row.get(0),
        )
        .expect("track quality row");
    assert_eq!(feed_search, 1);
    assert_eq!(track_search, 1);
    assert_eq!(feed_quality, 1);
    assert_eq!(track_quality, 1);

    let counts = db::get_resolver_queue_counts(&conn).expect("queue counts");
    assert_eq!(
        counts.total, 0,
        "source read-model completion event should clear source dirty work"
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "end-to-end resolved snapshot application is clearer as one test flow"
)]
#[tokio::test]
async fn canonical_feed_promotions_snapshot_applies_without_local_derivation() {
    let signer = common::temp_signer("resolver-promotions-snapshot");
    let primary = common::test_db();
    seed_feed_with_track(
        &primary,
        "feed-resolver-promotions",
        "track-resolver-promotions",
        "Resolver Promotions Track",
    );
    seed_feed_promotions_source_claims(
        &primary,
        "feed-resolver-promotions",
        "track-resolver-promotions",
    );
    db::sync_canonical_state_for_feed(&primary, "feed-resolver-promotions")
        .expect("sync canonical state");
    db::sync_canonical_promotions_for_feed(&primary, "feed-resolver-promotions")
        .expect("sync canonical promotions");

    let state_payload =
        db::build_canonical_feed_state_snapshot(&primary, "feed-resolver-promotions")
            .expect("build canonical state snapshot");
    let promotions_payload =
        db::build_canonical_feed_promotions_snapshot(&primary, "feed-resolver-promotions")
            .expect("build promotions snapshot");
    assert_eq!(promotions_payload.external_ids.len(), 1);
    assert_eq!(promotions_payload.entity_sources.len(), 4);

    let primary_feed = db::get_feed(&primary, "feed-resolver-promotions")
        .expect("get primary feed")
        .expect("primary feed exists");
    let primary_artist_id = primary
        .query_row(
            "SELECT artist_id FROM artist_credit_name WHERE artist_credit_id = ?1 ORDER BY position LIMIT 1",
            rusqlite::params![primary_feed.artist_credit_id],
            |row| row.get::<_, String>(0),
        )
        .expect("primary artist id");
    let primary_artist = db::get_artist_by_id(&primary, &primary_artist_id)
        .expect("get primary artist")
        .expect("primary artist exists");
    let release_id = state_payload.release_maps[0].release_id.clone();
    let recording_id = state_payload.recording_maps[0].recording_id.clone();

    let state_payload_json =
        serde_json::to_string(&state_payload).expect("serialize state payload");
    let promotions_payload_json =
        serde_json::to_string(&promotions_payload).expect("serialize promotions payload");
    let created_at = db::unix_now();
    let (state_signed_by, state_signature) = signer.sign_event(
        "event-canonical-feed-state-promotions",
        &stophammer::event::EventType::CanonicalFeedStateReplaced,
        &state_payload_json,
        "feed-resolver-promotions",
        created_at,
        1,
    );
    let state_event = stophammer::event::Event {
        event_id: "event-canonical-feed-state-promotions".into(),
        event_type: stophammer::event::EventType::CanonicalFeedStateReplaced,
        payload: stophammer::event::EventPayload::CanonicalFeedStateReplaced(state_payload),
        subject_guid: "feed-resolver-promotions".into(),
        signed_by: state_signed_by,
        signature: state_signature,
        seq: 1,
        created_at,
        warnings: Vec::new(),
        payload_json: state_payload_json,
    };
    let (promotions_signed_by, promotions_signature) = signer.sign_event(
        "event-canonical-feed-promotions",
        &stophammer::event::EventType::CanonicalFeedPromotionsReplaced,
        &promotions_payload_json,
        "feed-resolver-promotions",
        created_at + 1,
        2,
    );
    let promotions_event = stophammer::event::Event {
        event_id: "event-canonical-feed-promotions".into(),
        event_type: stophammer::event::EventType::CanonicalFeedPromotionsReplaced,
        payload: stophammer::event::EventPayload::CanonicalFeedPromotionsReplaced(
            promotions_payload.clone(),
        ),
        subject_guid: "feed-resolver-promotions".into(),
        signed_by: promotions_signed_by,
        signature: promotions_signature,
        seq: 2,
        created_at: created_at + 1,
        warnings: Vec::new(),
        payload_json: promotions_payload_json,
    };

    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        db::upsert_artist_if_absent(&conn, &primary_artist).expect("seed primary artist");
        let replica_credit = db::create_single_artist_credit(
            &conn,
            &primary_artist,
            Some("feed-resolver-promotions"),
        )
        .expect("create replica artist credit");
        assert_eq!(replica_credit.id, primary_feed.artist_credit_id);

        let now = db::unix_now();
        let feed = stophammer::model::Feed {
            feed_guid: "feed-resolver-promotions".into(),
            feed_url: "https://example.com/feed-resolver-promotions.xml".into(),
            title: "Feed feed-resolver-promotions".into(),
            title_lower: "feed feed-resolver-promotions".into(),
            artist_credit_id: replica_credit.id,
            description: Some("resolver promotions replica feed".into()),
            image_url: None,
            language: Some("en".into()),
            explicit: false,
            itunes_type: None,
            episode_count: 1,
            newest_item_at: Some(now),
            oldest_item_at: Some(now),
            created_at: now,
            updated_at: now,
            raw_medium: Some("music".into()),
        };
        db::upsert_feed(&conn, &feed).expect("upsert replica feed");
        let track = stophammer::model::Track {
            track_guid: "track-resolver-promotions".into(),
            feed_guid: "feed-resolver-promotions".into(),
            artist_credit_id: replica_credit.id,
            title: "Resolver Promotions Track".into(),
            title_lower: "resolver promotions track".into(),
            pub_date: Some(now),
            duration_secs: Some(180),
            enclosure_url: Some("https://cdn.example.com/track-resolver-promotions.mp3".into()),
            enclosure_type: Some("audio/mpeg".into()),
            enclosure_bytes: Some(1024),
            track_number: Some(1),
            season: None,
            explicit: false,
            description: Some("replica promotions track".into()),
            created_at: now,
            updated_at: now,
        };
        db::upsert_track(&conn, &track).expect("upsert replica track");
        db::mark_feed_dirty(
            &conn,
            "feed-resolver-promotions",
            stophammer::resolver::queue::DIRTY_CANONICAL_STATE
                | stophammer::resolver::queue::DIRTY_CANONICAL_PROMOTIONS,
        )
        .expect("mark dirty");
    }

    let summary =
        stophammer::apply::apply_events(pool.clone(), vec![state_event, promotions_event], None)
            .await;
    assert_eq!(summary.applied, 2);
    assert_eq!(summary.rejected, 0);

    let conn = pool.writer().lock().expect("writer");
    let external_ids =
        db::get_external_ids(&conn, "artist", &primary_artist_id).expect("get artist external ids");
    assert_eq!(external_ids.len(), 1);
    assert_eq!(external_ids[0].scheme, "nostr_npub");
    assert_eq!(external_ids[0].value, "npub1resolverpromotions");

    let release_sources =
        db::get_entity_sources(&conn, "release", &release_id).expect("get release sources");
    assert_eq!(release_sources.len(), 2);
    assert!(
        release_sources
            .iter()
            .any(|row| row.source_type == "source_feed"
                && row.source_url.as_deref()
                    == Some("https://example.com/feed-resolver-promotions.xml"))
    );
    assert!(
        release_sources
            .iter()
            .any(|row| row.source_type == "source_release_page"
                && row.source_url.as_deref() == Some("https://wavlake.com/resolver-promotions"))
    );

    let recording_sources =
        db::get_entity_sources(&conn, "recording", &recording_id).expect("get recording sources");
    assert_eq!(recording_sources.len(), 2);
    assert!(
        recording_sources
            .iter()
            .any(|row| row.source_type == "source_primary_enclosure"
                && row.source_url.as_deref()
                    == Some("https://cdn.example.com/track-resolver-promotions.mp3"))
    );
    assert!(
        recording_sources
            .iter()
            .any(|row| row.source_type == "source_recording_page"
                && row.source_url.as_deref()
                    == Some("https://wavlake.com/resolver-promotions/track"))
    );

    let resolved_external_ids =
        db::get_resolved_external_ids_for_feed(&conn, "feed-resolver-promotions")
            .expect("resolved external ids");
    let resolved_sources =
        db::get_resolved_entity_sources_for_feed(&conn, "feed-resolver-promotions")
            .expect("resolved entity sources");
    assert_eq!(resolved_external_ids.len(), 1);
    assert_eq!(resolved_sources.len(), 4);

    let counts = db::get_resolver_queue_counts(&conn).expect("queue counts");
    assert_eq!(
        counts.total, 0,
        "promotions snapshot should clear promotions dirty work"
    );
}

#[tokio::test]
async fn artist_identity_feed_resolved_event_clears_dirty_bit_without_local_resolution() {
    let signer = common::temp_signer("resolver-artist-identity-resolved");
    let payload = stophammer::event::ArtistIdentityFeedResolvedPayload {
        feed_guid: "feed-resolver-identity-complete".into(),
        seed_artists: 1,
        candidate_groups: 0,
        groups_processed: 0,
        merges_applied: 0,
        pending_reviews: 0,
        blocked_reviews: 0,
    };
    let payload_json = serde_json::to_string(&payload).expect("serialize payload");
    let created_at = db::unix_now();
    let (signed_by, signature) = signer.sign_event(
        "event-artist-identity-feed-resolved",
        &stophammer::event::EventType::ArtistIdentityFeedResolved,
        &payload_json,
        "feed-resolver-identity-complete",
        created_at,
        1,
    );
    let event = stophammer::event::Event {
        event_id: "event-artist-identity-feed-resolved".into(),
        event_type: stophammer::event::EventType::ArtistIdentityFeedResolved,
        payload: stophammer::event::EventPayload::ArtistIdentityFeedResolved(payload),
        subject_guid: "feed-resolver-identity-complete".into(),
        signed_by,
        signature,
        seq: 1,
        created_at,
        warnings: Vec::new(),
        payload_json,
    };

    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_feed(&conn, "feed-resolver-identity-complete");
        db::mark_feed_dirty(
            &conn,
            "feed-resolver-identity-complete",
            stophammer::resolver::queue::DIRTY_ARTIST_IDENTITY,
        )
        .expect("mark dirty");
    }

    let summary = stophammer::apply::apply_events(pool.clone(), vec![event], None).await;
    assert_eq!(summary.applied, 1);
    assert_eq!(summary.rejected, 0);

    let conn = pool.writer().lock().expect("writer");
    let claimed = db::get_resolver_queue_counts(&conn).expect("queue counts");
    assert_eq!(claimed.total, 0);
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
        body["source_layer"]["immediate_endpoints"]
            .as_array()
            .expect("immediate endpoints")
            .iter()
            .all(|v| v != "/v1/search?type=feed")
    );
    assert!(
        body["resolver"]["resolver_backed_endpoints"]
            .as_array()
            .expect("resolver-backed endpoints")
            .iter()
            .any(|v| v == "/v1/releases/{id}")
    );
    assert!(
        body["resolver"]["resolver_backed_endpoints"]
            .as_array()
            .expect("resolver-backed endpoints")
            .iter()
            .any(|v| v == "/v1/search?type=feed")
    );
}

#[tokio::test]
async fn source_feed_search_appears_only_after_resolver_batch() {
    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_feed(&conn, "feed-resolver-search");
        stophammer::resolver::queue::mark_feed_dirty_for_resolver(&conn, "feed-resolver-search")
            .expect("mark dirty");
    }

    let app = stophammer::api::build_readonly_router(test_app_state(pool.clone()));

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/search?q=resolver&type=feed")
                .body(axum::body::Body::empty())
                .expect("request"),
        )
        .await
        .expect("response before resolver");
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json body");
    assert_eq!(
        body["data"].as_array().expect("data before resolver").len(),
        0,
        "feed search should stay empty until resolver writes source read models"
    );

    let summary =
        stophammer::resolver::worker::run_batch(&pool, "worker-a", 10).expect("run batch");
    assert_eq!(summary.claimed, 1);
    assert_eq!(summary.resolved, 1);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/search?q=resolver&type=feed")
                .body(axum::body::Body::empty())
                .expect("request"),
        )
        .await
        .expect("response after resolver");
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json body");
    assert!(
        body["data"]
            .as_array()
            .expect("data after resolver")
            .iter()
            .any(|row| row["entity_type"] == "feed" && row["entity_id"] == "feed-resolver-search"),
        "feed search should appear after resolver writes source read models"
    );
}

/// Canonical promotions must reflect post-merge artist ownership.
///
/// When a feed is dirty for both `DIRTY_ARTIST_IDENTITY` and
/// `DIRTY_CANONICAL_PROMOTIONS`, the resolver processes identity before
/// promotions. The `resolved_external_ids_by_feed` entry must reference the
/// surviving (merge-target) artist, not a pre-merge artist that was
/// subsequently redirected.
/// Seed two feeds (A and B) sharing a website, with feed B also having
/// a `nostr_npub`. Mark feed B dirty for identity + promotions.
fn seed_promo_order_test(pool: &stophammer::db_pool::DbPool) {
    let conn = pool.writer().lock().expect("writer");
    let now = db::unix_now();

    let artist_x =
        db::resolve_artist(&conn, "Promo Order Artist", Some("feed-po-a")).expect("artist x");
    let credit_x = db::get_or_create_artist_credit(
        &conn,
        &artist_x.name,
        &[(
            artist_x.artist_id.clone(),
            artist_x.name.clone(),
            String::new(),
        )],
        Some("feed-po-a"),
    )
    .expect("credit x");
    conn.execute(
        "INSERT INTO feeds \
         (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-po-a', 'https://example.com/po-a.xml', 'PO A', 'po a', ?1, ?2, ?2)",
        rusqlite::params![credit_x.id, now],
    )
    .expect("feed a");
    conn.execute(
        "INSERT INTO source_entity_links \
         (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
         VALUES ('feed-po-a', 'feed', 'feed-po-a', 0, 'website', 'https://shared-promo.example.com', 'rss_link', 'feed.link', ?1)",
        rusqlite::params![now],
    )
    .expect("website a");

    let artist_y =
        db::resolve_artist(&conn, "Promo Order Artist", Some("feed-po-b")).expect("artist y");
    let credit_y = db::get_or_create_artist_credit(
        &conn,
        &artist_y.name,
        &[(
            artist_y.artist_id.clone(),
            artist_y.name.clone(),
            String::new(),
        )],
        Some("feed-po-b"),
    )
    .expect("credit y");
    conn.execute(
        "INSERT INTO feeds \
         (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES ('feed-po-b', 'https://example.com/po-b.xml', 'PO B', 'po b', ?1, ?2, ?2)",
        rusqlite::params![credit_y.id, now],
    )
    .expect("feed b");
    conn.execute(
        "INSERT INTO source_entity_links \
         (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
         VALUES ('feed-po-b', 'feed', 'feed-po-b', 0, 'website', 'https://shared-promo.example.com', 'rss_link', 'feed.link', ?1)",
        rusqlite::params![now],
    )
    .expect("website b");
    db::replace_source_entity_ids_for_feed(
        &conn,
        "feed-po-b",
        &[stophammer::model::SourceEntityIdClaim {
            id: None,
            feed_guid: "feed-po-b".into(),
            entity_type: "feed".into(),
            entity_id: "feed-po-b".into(),
            position: 0,
            scheme: "nostr_npub".into(),
            value: "npub1promoordertest".into(),
            source: "podcast_txt".into(),
            extraction_path: "feed.podcast:txt".into(),
            observed_at: now,
        }],
    )
    .expect("source entity ids");

    db::mark_feed_dirty(
        &conn,
        "feed-po-b",
        stophammer::resolver::queue::DIRTY_ARTIST_IDENTITY
            | stophammer::resolver::queue::DIRTY_CANONICAL_PROMOTIONS,
    )
    .expect("mark dirty");
}

#[test]
fn resolver_batch_canonical_promotions_use_post_merge_artist() {
    let (pool, _dir) = common::test_db_pool();
    seed_promo_order_test(&pool);

    stophammer::resolver::worker::run_batch(&pool, "worker-po", 10).expect("run batch");

    let conn = pool.writer().lock().expect("writer");

    let artist_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artists WHERE LOWER(name) = 'promo order artist'",
            [],
            |row| row.get(0),
        )
        .expect("artist count");
    assert_eq!(
        artist_count, 1,
        "merge should leave exactly one live artist row"
    );

    let promotions: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT entity_id, value FROM resolved_external_ids_by_feed \
                 WHERE feed_guid = 'feed-po-b' AND entity_type = 'artist' AND scheme = 'nostr_npub'",
            )
            .expect("prepare promotions");
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query promotions")
            .collect::<Result<_, _>>()
            .expect("collect promotions")
    };
    assert_eq!(
        promotions.len(),
        1,
        "feed-po-b must have exactly one npub promotion"
    );
    let (promoted_artist_id, promoted_value) = &promotions[0];
    assert_eq!(promoted_value, "npub1promoordertest");

    let has_outbound_redirect: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM artist_id_redirect WHERE old_artist_id = ?1",
            rusqlite::params![promoted_artist_id],
            |row| row.get::<_, i64>(0),
        )
        .expect("redirect check")
        > 0;
    assert!(
        !has_outbound_redirect,
        "promoted artist_id must be the merge target (no outbound redirect); \
         if this fails, canonical promotions ran before artist identity"
    );
}
