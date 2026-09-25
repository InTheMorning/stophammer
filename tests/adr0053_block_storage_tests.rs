// ADR 0053 task 001: the feed_blocks table, the FeedBlockKind and FeedBlock
// types, the database functions, and the FeedBlocked/FeedUnblocked events and
// their idempotent apply arms.
//
// No route and no ingest check exists yet (later tasks). These tests cover
// only the storage layer named in the task's Acceptance Criteria.

mod common;

use stophammer::apply::{ApplyOutcome, apply_single_event};
use stophammer::db::{FeedBlock, FeedBlockKind};
use stophammer::event::{Event, EventPayload, EventType, FeedBlockedPayload, FeedUnblockedPayload};

// ---------------------------------------------------------------------------
// FeedBlockKind::normalize
// ---------------------------------------------------------------------------

#[test]
fn normalize_lower_cases_and_trims_a_guid_and_trims_only_a_url() {
    assert_eq!(
        FeedBlockKind::Guid.normalize("  ABC-Def "),
        "abc-def",
        "a guid must be lower-cased and trimmed"
    );
    assert_eq!(
        FeedBlockKind::Url.normalize("https://X.example/a "),
        "https://X.example/a",
        "a url must be trimmed only, keeping its case"
    );
}

// ---------------------------------------------------------------------------
// insert_feed_block
// ---------------------------------------------------------------------------

#[test]
fn insert_feed_block_returns_true_once_and_false_for_the_same_pair_under_a_new_block_id() {
    let conn = common::test_db();
    let now = common::now();

    let first = FeedBlock {
        block_id: "block-1".into(),
        kind: FeedBlockKind::Guid,
        value: "Some-Guid".into(),
        reason: "test".into(),
        blocked_at: now,
    };
    assert!(
        stophammer::db::insert_feed_block(&conn, &first).expect("insert first block"),
        "the first insert of a pair must write a row and return true"
    );

    let second = FeedBlock {
        block_id: "block-2".into(),
        kind: FeedBlockKind::Guid,
        // Same normalized value, different case, must still collide.
        value: "some-guid".into(),
        reason: "test again".into(),
        blocked_at: now + 100,
    };
    assert!(
        !stophammer::db::insert_feed_block(&conn, &second).expect("insert second block"),
        "the same kind and normalized value under a different block_id must write nothing"
    );

    let rows = stophammer::db::list_feed_blocks(&conn).expect("list blocks");
    assert_eq!(rows.len(), 1, "only the first row must exist");
    assert_eq!(rows[0].block_id, "block-1");
}

// ---------------------------------------------------------------------------
// find_feed_block
// ---------------------------------------------------------------------------

#[test]
fn find_feed_block_finds_a_guid_in_upper_case_and_a_url_by_its_position_and_none_with_no_match() {
    let conn = common::test_db();
    let now = common::now();

    stophammer::db::insert_feed_block(
        &conn,
        &FeedBlock {
            block_id: "block-guid".into(),
            kind: FeedBlockKind::Guid,
            value: "held-guid".into(),
            reason: "guid block".into(),
            blocked_at: now,
        },
    )
    .expect("insert guid block");

    let found = stophammer::db::find_feed_block(&conn, Some("HELD-GUID"), &[])
        .expect("query find_feed_block")
        .expect("an upper-case guid must still match the stored lower-case value");
    assert_eq!(found.block_id, "block-guid");

    stophammer::db::insert_feed_block(
        &conn,
        &FeedBlock {
            block_id: "block-url".into(),
            kind: FeedBlockKind::Url,
            value: "https://example.com/second.xml".into(),
            reason: "url block".into(),
            blocked_at: now,
        },
    )
    .expect("insert url block");

    let urls = [
        "https://example.com/first.xml",
        "https://example.com/second.xml",
    ];
    let found = stophammer::db::find_feed_block(&conn, None, &urls)
        .expect("query find_feed_block")
        .expect("the second url of the list must match the stored block");
    assert_eq!(found.block_id, "block-url");

    let none = stophammer::db::find_feed_block(
        &conn,
        Some("no-match-guid"),
        &["https://example.com/no-match.xml"],
    )
    .expect("query find_feed_block");
    assert!(none.is_none(), "no match must return None");

    // An empty url must not be checked.
    let empty_url_only =
        stophammer::db::find_feed_block(&conn, None, &[""]).expect("query find_feed_block");
    assert!(empty_url_only.is_none(), "an empty url must not be checked");
}

// ---------------------------------------------------------------------------
// delete_feed_block
// ---------------------------------------------------------------------------

#[test]
fn delete_feed_block_returns_true_once_and_false_after() {
    let conn = common::test_db();
    let now = common::now();

    stophammer::db::insert_feed_block(
        &conn,
        &FeedBlock {
            block_id: "block-delete".into(),
            kind: FeedBlockKind::Url,
            value: "https://example.com/delete.xml".into(),
            reason: "to remove".into(),
            blocked_at: now,
        },
    )
    .expect("insert block");

    assert!(
        stophammer::db::delete_feed_block(&conn, "block-delete").expect("delete block"),
        "deleting an existing block must return true"
    );
    assert!(
        !stophammer::db::delete_feed_block(&conn, "block-delete").expect("delete again"),
        "deleting an already-removed block must return false"
    );
}

// ---------------------------------------------------------------------------
// Event payload round trip
// ---------------------------------------------------------------------------

#[test]
fn feed_blocked_and_feed_unblocked_payloads_round_trip_through_event_payload() {
    let now = common::now();

    let blocked = EventPayload::FeedBlocked(FeedBlockedPayload {
        block_id: "block-rt".into(),
        kind: FeedBlockKind::Guid,
        value: "rt-guid".into(),
        reason: "round trip".into(),
        blocked_at: now,
    });
    let blocked_json = serde_json::to_string(&blocked).expect("serialize FeedBlocked");
    assert!(
        blocked_json.contains(r#""type":"feed_blocked""#),
        "the serialized payload must carry the feed_blocked tag: {blocked_json}"
    );
    let blocked_back: EventPayload =
        serde_json::from_str(&blocked_json).expect("parse FeedBlocked back");
    match blocked_back {
        EventPayload::FeedBlocked(p) => {
            assert_eq!(p.block_id, "block-rt");
            assert_eq!(p.kind, FeedBlockKind::Guid);
            assert_eq!(p.value, "rt-guid");
            assert_eq!(p.reason, "round trip");
            assert_eq!(p.blocked_at, now);
        }
        other => panic!("expected FeedBlocked, got {other:?}"),
    }

    let unblocked = EventPayload::FeedUnblocked(FeedUnblockedPayload {
        block_id: "block-rt".into(),
    });
    let unblocked_json = serde_json::to_string(&unblocked).expect("serialize FeedUnblocked");
    assert!(
        unblocked_json.contains(r#""type":"feed_unblocked""#),
        "the serialized payload must carry the feed_unblocked tag: {unblocked_json}"
    );
    let unblocked_back: EventPayload =
        serde_json::from_str(&unblocked_json).expect("parse FeedUnblocked back");
    match unblocked_back {
        EventPayload::FeedUnblocked(p) => assert_eq!(p.block_id, "block-rt"),
        other => panic!("expected FeedUnblocked, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Apply on a second database
// ---------------------------------------------------------------------------

fn make_feed_blocked_event(event_id: &str, block_id: &str, seq: i64, now: i64) -> Event {
    let inner = FeedBlockedPayload {
        block_id: block_id.into(),
        kind: FeedBlockKind::Guid,
        value: "apply-guid".into(),
        reason: "apply test".into(),
        blocked_at: now,
    };
    let payload_json = serde_json::to_string(&inner).expect("serialize");

    Event {
        event_id: event_id.into(),
        event_type: EventType::FeedBlocked,
        payload: EventPayload::FeedBlocked(inner),
        subject_guid: block_id.into(),
        signed_by: "deadbeef".into(),
        signature: "cafebabe".into(),
        seq,
        created_at: now,
        warnings: vec![],
        payload_json,
    }
}

fn make_feed_unblocked_event(event_id: &str, block_id: &str, seq: i64, now: i64) -> Event {
    let inner = FeedUnblockedPayload {
        block_id: block_id.into(),
    };
    let payload_json = serde_json::to_string(&inner).expect("serialize");

    Event {
        event_id: event_id.into(),
        event_type: EventType::FeedUnblocked,
        payload: EventPayload::FeedUnblocked(inner),
        subject_guid: block_id.into(),
        signed_by: "deadbeef".into(),
        signature: "cafebabe".into(),
        seq,
        created_at: now,
        warnings: vec![],
        payload_json,
    }
}

#[test]
fn apply_feed_blocked_twice_gives_one_row_then_unblocked_removes_it_and_a_repeat_unblock_is_ok() {
    let db = common::test_db_arc();
    let pool = common::wrap_pool(std::sync::Arc::clone(&db));
    let now = common::now();

    let first = make_feed_blocked_event("evt-block-1", "block-apply", 1, now);
    let outcome = apply_single_event(&pool, &first).expect("first apply must succeed");
    assert!(
        matches!(outcome, ApplyOutcome::Applied(_)),
        "the first FeedBlocked apply must be Applied"
    );

    // A second, distinct event carrying the same block_id must still leave
    // exactly one row: insert_feed_block is INSERT OR IGNORE on block_id.
    let second = make_feed_blocked_event("evt-block-2", "block-apply", 2, now);
    apply_single_event(&pool, &second).expect("second apply must succeed");

    let row_count: i64 = {
        let conn = db.lock().expect("lock db");
        conn.query_row("SELECT COUNT(*) FROM feed_blocks", [], |r| r.get(0))
            .expect("count feed_blocks")
    };
    assert_eq!(
        row_count, 1,
        "applying FeedBlocked twice for the same block_id must give exactly one row"
    );

    let unblock = make_feed_unblocked_event("evt-unblock-1", "block-apply", 3, now);
    apply_single_event(&pool, &unblock).expect("unblock apply must succeed");

    let row_count_after_unblock: i64 = {
        let conn = db.lock().expect("lock db");
        conn.query_row("SELECT COUNT(*) FROM feed_blocks", [], |r| r.get(0))
            .expect("count feed_blocks")
    };
    assert_eq!(
        row_count_after_unblock, 0,
        "FeedUnblocked must remove the row"
    );

    // A second FeedUnblocked (a distinct event, same block_id) must not fail
    // even though no row remains.
    let unblock_again = make_feed_unblocked_event("evt-unblock-2", "block-apply", 4, now);
    let result = apply_single_event(&pool, &unblock_again);
    assert!(
        result.is_ok(),
        "applying FeedUnblocked with no matching row must not fail: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// A fresh database has the table
// ---------------------------------------------------------------------------

#[test]
fn a_fresh_database_has_the_feed_blocks_table() {
    let conn = common::test_db();
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='feed_blocks'",
            [],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    assert!(exists, "a fresh database must have the feed_blocks table");
}
