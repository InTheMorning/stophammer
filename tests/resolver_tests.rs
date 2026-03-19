mod common;

use stophammer::db;

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

#[test]
fn mark_claim_complete_queue_entry() {
    let mut conn = common::test_db();
    seed_feed(&conn, "feed-resolver-queue");

    stophammer::resolver::queue::mark_feed_phase1_dirty(&conn, "feed-resolver-queue")
        .expect("mark dirty");

    let claimed = db::claim_dirty_feeds(&mut conn, "worker-a", 10, db::unix_now()).expect("claim");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].feed_guid, "feed-resolver-queue");
    assert_eq!(
        claimed[0].dirty_mask,
        stophammer::resolver::queue::PHASE2_DIRTY_MASK
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

    stophammer::resolver::queue::mark_feed_phase1_dirty(&conn, "feed-resolver-retry")
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
        stophammer::resolver::queue::mark_feed_phase1_dirty(&conn, "feed-resolver-pause")
            .expect("mark dirty");
        db::set_resolver_import_active(&conn, true).expect("set import state");
    }

    let summary =
        stophammer::resolver::worker::run_batch(&pool, "worker-a", 10).expect("run batch");
    assert!(summary.skipped_import_active);
    assert_eq!(summary.claimed, 0);
}

#[test]
fn resolver_batch_drains_phase1_work() {
    let (pool, _dir) = common::test_db_pool();
    {
        let conn = pool.writer().lock().expect("writer");
        seed_feed(&conn, "feed-resolver-run");
        stophammer::resolver::queue::mark_feed_phase1_dirty(&conn, "feed-resolver-run")
            .expect("mark dirty");
    }

    let summary =
        stophammer::resolver::worker::run_batch(&pool, "worker-a", 10).expect("run batch");
    assert_eq!(summary.claimed, 1);
    assert_eq!(summary.resolved, 1);
    assert_eq!(summary.failed, 0);

    let mut conn = pool.writer().lock().expect("writer");
    let claimed = db::claim_dirty_feeds(&mut conn, "worker-b", 10, db::unix_now()).expect("claim");
    assert!(claimed.is_empty());
}

#[test]
fn resolver_queue_counts_reflect_ready_locked_and_failed_rows() {
    let mut conn = common::test_db();
    seed_feed(&conn, "feed-resolver-counts");

    stophammer::resolver::queue::mark_feed_phase1_dirty(&conn, "feed-resolver-counts")
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
