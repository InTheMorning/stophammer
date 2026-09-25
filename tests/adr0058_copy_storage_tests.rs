// ADR 0058 task 001: the feed_copies and feed_copy_overflow tables, the
// CopySummary type, the digest, the UUIDv5 origin check, the
// FeedCopyObserved/FeedCopyResolved events and their idempotent apply arms,
// and the two delete rules.
//
// No route and no ingest path uses any of this yet (later tasks). These
// tests cover only the storage layer named in the task's Acceptance
// Criteria.

mod common;

use stophammer::apply::apply_single_event;
use stophammer::db::{self, FeedBlock, FeedBlockKind};
use stophammer::event::{
    Event, EventPayload, EventType, FeedCopyObservedPayload, FeedCopyResolvedPayload,
};
use stophammer::ingest::IngestFeedData;
use stophammer::model::{self, CopySummary, RouteRecipient};

// ---------------------------------------------------------------------------
// copy_summary
// ---------------------------------------------------------------------------

/// A feed with two tracks, each with one payment route, and one feed-level
/// payment route. Each route carries a distinct `custom_key`/`custom_value`
/// so a test can tell recipients apart.
fn sample_feed_data() -> IngestFeedData {
    let json = serde_json::json!({
        "feed_guid": "feed-guid-1",
        "title": "Sample Feed",
        "explicit": false,
        "feed_payment_routes": [
            {
                "recipient_name": "Feed Recipient",
                "route_type": "lnaddress",
                "address": "feed@getalby.com",
                "custom_key": null,
                "custom_value": null,
                "split": 100,
                "fee": false
            }
        ],
        "tracks": [
            {
                "track_guid": "track-a",
                "title": "Track A",
                "explicit": false,
                "payment_routes": [
                    {
                        "recipient_name": "Track A Recipient",
                        "route_type": "keysend",
                        "address": "03aaaa",
                        "custom_key": "696969",
                        "custom_value": "aaa111",
                        "split": 100,
                        "fee": false
                    }
                ]
            },
            {
                "track_guid": "track-b",
                "title": "Track B",
                "explicit": false,
                "payment_routes": [
                    {
                        "recipient_name": "Track B Recipient",
                        "route_type": "keysend",
                        "address": "03bbbb",
                        "custom_key": "696969",
                        "custom_value": "bbb222",
                        "split": 100,
                        "fee": false
                    }
                ]
            }
        ]
    });
    serde_json::from_value(json).expect("parse sample IngestFeedData fixture")
}

#[test]
fn copy_summary_keeps_item_order_and_each_recipients_custom_fields() {
    let feed = sample_feed_data();
    let summary = model::copy_summary(&feed);

    assert_eq!(
        summary.item_guids,
        vec!["track-a".to_string(), "track-b".to_string()],
        "item_guids must keep the feed's own track order"
    );

    let track_a = summary
        .track_recipients
        .get("track-a")
        .expect("track-a must have a recipient set");
    assert_eq!(track_a.len(), 1, "track-a must have one recipient");
    assert_eq!(track_a[0].custom_key.as_deref(), Some("696969"));
    assert_eq!(track_a[0].custom_value.as_deref(), Some("aaa111"));

    let track_b = summary
        .track_recipients
        .get("track-b")
        .expect("track-b must have a recipient set");
    assert_eq!(track_b[0].custom_key.as_deref(), Some("696969"));
    assert_eq!(track_b[0].custom_value.as_deref(), Some("bbb222"));

    assert_eq!(summary.feed_recipients.len(), 1, "one feed-level recipient");
    assert_eq!(summary.feed_recipients[0].address, "feed@getalby.com");
}

// ---------------------------------------------------------------------------
// summary_digest
// ---------------------------------------------------------------------------

fn recipient(custom_value: &str) -> RouteRecipient {
    RouteRecipient {
        address: "addr".into(),
        custom_key: Some("key".into()),
        custom_value: Some(custom_value.into()),
        split: 100,
    }
}

#[test]
fn summary_digest_ignores_the_title_but_changes_with_a_recipient() {
    let base = CopySummary {
        title: "Title One".into(),
        item_guids: vec!["item-1".into()],
        feed_recipients: vec![recipient("aaa")],
        track_recipients: std::collections::BTreeMap::new(),
    };
    let renamed = CopySummary {
        title: "A Completely Different Title".into(),
        ..base.clone()
    };
    assert_eq!(
        model::summary_digest(&base),
        model::summary_digest(&renamed),
        "a title-only change must not change the digest"
    );

    let mut changed = base.clone();
    changed.feed_recipients = vec![recipient("bbb")];
    assert_ne!(
        model::summary_digest(&base),
        model::summary_digest(&changed),
        "a changed custom_value must change the digest"
    );
}

// ---------------------------------------------------------------------------
// guid_origin_matches
// ---------------------------------------------------------------------------

#[test]
fn guid_origin_matches_the_url_that_derives_the_guid_and_not_another_url() {
    let feed_guid = "7192ec54-3aa2-5c61-987b-51bf75f68568";
    assert!(
        model::guid_origin_matches(
            feed_guid,
            "https://wavlake.com/feed/music/a82acc2f-3440-491c-94c2-d27bebf6cfe6"
        ),
        "the guid must match the UUIDv5 of the Wavlake URL that made it"
    );
    assert!(
        !model::guid_origin_matches(
            feed_guid,
            "https://musicsideproject.com/api/hosted/7192ec54-3aa2-5c61-987b-51bf75f68568.xml"
        ),
        "the same guid must not match a URL that did not derive it"
    );
}

// ---------------------------------------------------------------------------
// Apply: FeedCopyObserved and FeedCopyResolved
// ---------------------------------------------------------------------------

fn make_feed_copy_observed_event(
    event_id: &str,
    feed_guid: &str,
    url: &str,
    first_seen: i64,
    digest: &str,
    seq: i64,
    now: i64,
) -> Event {
    let inner = FeedCopyObservedPayload {
        feed_guid: feed_guid.into(),
        url: url.into(),
        first_seen,
        title: "A Copy".into(),
        item_guids: vec!["item-1".into()],
        feed_recipients: vec![recipient("aaa")],
        track_recipients: std::collections::BTreeMap::new(),
        summary_digest: digest.into(),
    };
    let payload_json = serde_json::to_string(&inner).expect("serialize");

    Event {
        event_id: event_id.into(),
        event_type: EventType::FeedCopyObserved,
        payload: EventPayload::FeedCopyObserved(inner),
        subject_guid: feed_guid.into(),
        signed_by: "deadbeef".into(),
        signature: "cafebabe".into(),
        seq,
        created_at: now,
        warnings: vec![],
        payload_json,
    }
}

fn make_feed_copy_resolved_event(
    event_id: &str,
    feed_guid: &str,
    url: &str,
    resolved_digest: &str,
    seq: i64,
    now: i64,
) -> Event {
    let inner = FeedCopyResolvedPayload {
        feed_guid: feed_guid.into(),
        url: url.into(),
        decision: "keep_source".into(),
        reason: "confirmed by artist".into(),
        resolved_at: now,
        resolved_digest: resolved_digest.into(),
    };
    let payload_json = serde_json::to_string(&inner).expect("serialize");

    Event {
        event_id: event_id.into(),
        event_type: EventType::FeedCopyResolved,
        payload: EventPayload::FeedCopyResolved(inner),
        subject_guid: feed_guid.into(),
        signed_by: "deadbeef".into(),
        signature: "cafebabe".into(),
        seq,
        created_at: now,
        warnings: vec![],
        payload_json,
    }
}

#[test]
fn apply_feed_copy_observed_twice_keeps_one_row_and_the_first_first_seen() {
    let db_arc = common::test_db_arc();
    let pool = common::wrap_pool(std::sync::Arc::clone(&db_arc));
    let now = common::now();
    let feed_guid = "feed-apply-1";
    let url = "https://mirror.example/feed.xml";

    let first =
        make_feed_copy_observed_event("evt-copy-1", feed_guid, url, now, "digest-one", 1, now);
    apply_single_event(&pool, &first).expect("first apply must succeed");

    // A distinct event for the same pair, carrying a later first_seen, must
    // not move the row's first_seen (ADR 0058 Section 1).
    let second = make_feed_copy_observed_event(
        "evt-copy-2",
        feed_guid,
        url,
        now + 1000,
        "digest-two",
        2,
        now + 1000,
    );
    apply_single_event(&pool, &second).expect("second apply must succeed");

    let conn = db_arc.lock().expect("lock db");
    let row_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM feed_copies", [], |r| r.get(0))
        .expect("count feed_copies");
    assert_eq!(
        row_count, 1,
        "applying FeedCopyObserved twice for the same pair must give one row"
    );

    let row = db::get_feed_copy(&conn, feed_guid, url)
        .expect("query feed_copies")
        .expect("the row must exist");
    assert_eq!(
        row.first_seen, now,
        "first_seen must stay the value of the first event"
    );
    assert_eq!(
        row.last_seen, None,
        "an applied FeedCopyObserved must never write last_seen"
    );
    assert_eq!(
        row.summary_digest, "digest-two",
        "the summary columns must still update to the latest observation"
    );
}

#[test]
fn apply_feed_copy_resolved_writes_the_resolution_and_a_missing_row_is_a_no_op() {
    let db_arc = common::test_db_arc();
    let pool = common::wrap_pool(std::sync::Arc::clone(&db_arc));
    let now = common::now();
    let feed_guid = "feed-apply-2";
    let url = "https://mirror.example/resolved-feed.xml";

    let observed =
        make_feed_copy_observed_event("evt-obs-1", feed_guid, url, now, "digest-r", 1, now);
    apply_single_event(&pool, &observed).expect("observed apply must succeed");

    let resolved =
        make_feed_copy_resolved_event("evt-res-1", feed_guid, url, "digest-r", 2, now + 10);
    apply_single_event(&pool, &resolved).expect("resolved apply must succeed");

    {
        let conn = db_arc.lock().expect("lock db");
        let row = db::get_feed_copy(&conn, feed_guid, url)
            .expect("query feed_copies")
            .expect("the row must exist");
        assert_eq!(row.resolution.as_deref(), Some("keep_source"));
        assert_eq!(
            row.resolution_reason.as_deref(),
            Some("confirmed by artist")
        );
        assert_eq!(row.resolved_at, Some(now + 10));
        assert_eq!(row.resolved_digest.as_deref(), Some("digest-r"));
    }

    // A FeedCopyResolved for a pair with no row must not fail and must not
    // create one.
    let missing_url = "https://mirror.example/never-observed.xml";
    let resolved_missing = make_feed_copy_resolved_event(
        "evt-res-2",
        feed_guid,
        missing_url,
        "digest-missing",
        3,
        now + 20,
    );
    let result = apply_single_event(&pool, &resolved_missing);
    assert!(
        result.is_ok(),
        "resolving a missing row must not fail: {result:?}"
    );

    let conn = db_arc.lock().expect("lock db");
    let missing_row = db::get_feed_copy(&conn, feed_guid, missing_url).expect("query feed_copies");
    assert!(missing_row.is_none(), "a missing row must stay absent");
}

// ---------------------------------------------------------------------------
// A URL block deletes the feed_copies row of that URL
// ---------------------------------------------------------------------------

#[test]
fn inserting_a_url_block_deletes_the_row_of_that_url_and_keeps_another() {
    let conn = common::test_db();
    let now = common::now();
    let feed_guid = "feed-block-1";
    let blocked_url = "https://blocked.example/feed.xml";
    let kept_url = "https://kept.example/feed.xml";

    let summary = CopySummary {
        title: "A Copy".into(),
        item_guids: vec!["item-1".into()],
        feed_recipients: vec![recipient("aaa")],
        track_recipients: std::collections::BTreeMap::new(),
    };
    db::upsert_feed_copy_summary(
        &conn,
        feed_guid,
        blocked_url,
        now,
        &summary,
        "digest-blocked",
    )
    .expect("upsert blocked url row");
    db::upsert_feed_copy_summary(&conn, feed_guid, kept_url, now, &summary, "digest-kept")
        .expect("upsert kept url row");

    db::insert_feed_block(
        &conn,
        &FeedBlock {
            block_id: "block-url-1".into(),
            kind: FeedBlockKind::Url,
            value: blocked_url.into(),
            reason: "impersonation".into(),
            blocked_at: now,
        },
    )
    .expect("insert url block");

    assert!(
        db::get_feed_copy(&conn, feed_guid, blocked_url)
            .expect("query blocked url")
            .is_none(),
        "the row of the blocked url must be deleted"
    );
    assert!(
        db::get_feed_copy(&conn, feed_guid, kept_url)
            .expect("query kept url")
            .is_some(),
        "the row of a different url must be kept"
    );
}

// ---------------------------------------------------------------------------
// A feed delete removes the feed_copies rows and the overflow counter
// ---------------------------------------------------------------------------

#[test]
fn deleting_a_feed_removes_its_copy_rows_and_its_overflow_counter() {
    let mut conn = common::test_db();
    let now = common::now();
    let feed_guid = "feed-delete-1";

    let summary = CopySummary {
        title: "A Copy".into(),
        item_guids: vec!["item-1".into()],
        feed_recipients: vec![recipient("aaa")],
        track_recipients: std::collections::BTreeMap::new(),
    };
    db::upsert_feed_copy_summary(
        &conn,
        feed_guid,
        "https://mirror.example/delete-me.xml",
        now,
        &summary,
        "digest-delete",
    )
    .expect("upsert row");
    db::increment_copy_overflow(&conn, feed_guid).expect("increment overflow");
    assert_eq!(
        db::get_copy_overflow(&conn, feed_guid).expect("read overflow before delete"),
        1,
        "the overflow counter must be set before the delete"
    );

    db::delete_feed(&mut conn, feed_guid).expect("delete feed");

    assert_eq!(
        db::count_feed_copies(&conn, feed_guid).expect("count after delete"),
        0,
        "a feed delete must remove its feed_copies rows"
    );
    assert_eq!(
        db::get_copy_overflow(&conn, feed_guid).expect("read overflow after delete"),
        0,
        "a feed delete must remove its overflow counter"
    );
}

// ---------------------------------------------------------------------------
// A fresh database has the new tables
// ---------------------------------------------------------------------------

#[test]
fn a_fresh_database_has_the_feed_copies_and_overflow_tables() {
    let conn = common::test_db();
    for table in ["feed_copies", "feed_copy_overflow"] {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("query sqlite_master");
        assert!(exists, "a fresh database must have the {table} table");
    }
}
