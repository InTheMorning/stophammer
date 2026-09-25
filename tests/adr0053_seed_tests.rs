// ADR 0053 task 004: the environment seed.
//
// `feed_blocklist` was a verifier. ADR 0053 section 2 replaces it with a
// startup seed: `blocks::seed_blocks` reads `BLOCKED_FEED_GUIDS` and
// `BLOCKED_FEED_URLS` through `blocks::blocks_from_env` and writes a durable
// `feed_blocks` row and a signed `FeedBlocked` event for each value with no
// row yet. The four tests that lived in
// `src/verifiers/feed_blocklist.rs` (`blocks_exact_feed_guid`,
// `blocks_exact_canonical_url`, `blocks_exact_source_url`,
// `passes_when_not_listed`) checked that a block rejects a matching ingest.
// That behavior is unchanged and is covered by
// `tests/adr0053_blocks_api_tests.rs` (task 002's ingest check), so none of
// the four moved here unchanged. This file instead covers the two behaviors
// task 002 does not: reading the seed's env vars (`blocks_from_env`) and
// writing the seed's rows and events (`seed_blocks`), plus `build_chain`
// skipping the retired name.

mod common;

use std::sync::{Mutex, MutexGuard};

use stophammer::db::{self, FeedBlock, FeedBlockKind};
use stophammer::event::EventType;
use stophammer::verify::{ChainSpec, build_chain};

// ---------------------------------------------------------------------------
// blocks_from_env
// ---------------------------------------------------------------------------

// `BLOCKED_FEED_GUIDS` and `BLOCKED_FEED_URLS` are process-wide state, so
// serialize the tests that set them.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().expect("env lock")
}

fn restore_env(var: &str, old: Option<String>) {
    match old {
        // SAFETY: serialized by ENV_LOCK in this test module.
        Some(value) => unsafe { std::env::set_var(var, value) },
        // SAFETY: serialized by ENV_LOCK in this test module.
        None => unsafe { std::env::remove_var(var) },
    }
}

#[test]
fn blocks_from_env_trims_each_value_drops_empty_ones_and_tags_the_kind() {
    let _guard = lock_env();
    let old_guids = std::env::var("BLOCKED_FEED_GUIDS").ok();
    let old_urls = std::env::var("BLOCKED_FEED_URLS").ok();

    // SAFETY: serialized by ENV_LOCK in this test module.
    unsafe {
        std::env::set_var("BLOCKED_FEED_GUIDS", " GuidOne , ,GuidTwo ");
        std::env::set_var("BLOCKED_FEED_URLS", " https://a.example/feed.xml ,");
    }

    let entries = stophammer::blocks::blocks_from_env();

    restore_env("BLOCKED_FEED_GUIDS", old_guids);
    restore_env("BLOCKED_FEED_URLS", old_urls);

    assert_eq!(
        entries,
        vec![
            (FeedBlockKind::Guid, "GuidOne".to_string()),
            (FeedBlockKind::Guid, "GuidTwo".to_string()),
            (FeedBlockKind::Url, "https://a.example/feed.xml".to_string()),
        ],
        "each value must be trimmed, an empty value dropped, and each entry tagged with its kind: {entries:?}"
    );
}

#[test]
fn blocks_from_env_gives_no_entries_when_both_variables_are_absent() {
    let _guard = lock_env();
    let old_guids = std::env::var("BLOCKED_FEED_GUIDS").ok();
    let old_urls = std::env::var("BLOCKED_FEED_URLS").ok();

    // SAFETY: serialized by ENV_LOCK in this test module.
    unsafe {
        std::env::remove_var("BLOCKED_FEED_GUIDS");
        std::env::remove_var("BLOCKED_FEED_URLS");
    }

    let entries = stophammer::blocks::blocks_from_env();

    restore_env("BLOCKED_FEED_GUIDS", old_guids);
    restore_env("BLOCKED_FEED_URLS", old_urls);

    assert!(
        entries.is_empty(),
        "no BLOCKED_FEED_GUIDS or BLOCKED_FEED_URLS must give no entries: {entries:?}"
    );
}

// ---------------------------------------------------------------------------
// seed_blocks
// ---------------------------------------------------------------------------

fn feed_blocked_event_count(conn: &rusqlite::Connection) -> usize {
    db::get_events_since(conn, 0, 1000)
        .expect("read events")
        .into_iter()
        .filter(|event| event.event_type == EventType::FeedBlocked)
        .count()
}

#[test]
fn seed_blocks_writes_three_rows_and_events_then_nothing_on_a_repeat_call() {
    let mut conn = common::test_db();
    let signer = common::temp_signer("adr0053-seed-a");
    let now = common::now();

    let entries = vec![
        (
            FeedBlockKind::Guid,
            "27293ad7-c199-5047-8135-a864fb546492".to_string(),
        ),
        (
            FeedBlockKind::Guid,
            "27293ad7-c199-5047-8135-a864fb546491".to_string(),
        ),
        (
            FeedBlockKind::Url,
            "https://feeds.podcastindex.org/100retro.xml".to_string(),
        ),
    ];

    let written = stophammer::blocks::seed_blocks(&mut conn, &entries, &signer, now)
        .expect("seed the first call");
    assert_eq!(written, 3, "the first call must write all three entries");

    let rows = db::list_feed_blocks(&conn).expect("list blocks");
    assert_eq!(rows.len(), 3, "three feed_blocks rows must exist");
    for row in &rows {
        assert_eq!(
            row.reason, "seeded from environment",
            "a seeded row must carry the fixed seed reason: {row:?}"
        );
    }
    assert_eq!(
        feed_blocked_event_count(&conn),
        3,
        "one feed_blocked event must exist per new row"
    );

    let written_again = stophammer::blocks::seed_blocks(&mut conn, &entries, &signer, now)
        .expect("seed the second call");
    assert_eq!(
        written_again, 0,
        "a second call with the same entries must write nothing"
    );
    assert_eq!(
        db::list_feed_blocks(&conn).expect("list blocks").len(),
        3,
        "the row count must not change on the repeat call"
    );
    assert_eq!(
        feed_blocked_event_count(&conn),
        3,
        "no new feed_blocked event must be signed on the repeat call"
    );
}

#[test]
fn seed_blocks_writes_nothing_for_a_pair_already_blocked_under_a_different_reason() {
    let mut conn = common::test_db();
    let signer = common::temp_signer("adr0053-seed-b");
    let now = common::now();

    let existing = FeedBlock {
        block_id: "operator-block-1".to_string(),
        kind: FeedBlockKind::Guid,
        value: "preexisting-guid".to_string(),
        reason: "operator note: known spam feed".to_string(),
        blocked_at: now,
    };
    assert!(
        db::insert_feed_block(&conn, &existing).expect("insert the operator's block"),
        "the setup insert must write a row"
    );

    // Different case, same normalized value: the pair already has a row.
    let entries = vec![(FeedBlockKind::Guid, "PREEXISTING-GUID".to_string())];
    let written = stophammer::blocks::seed_blocks(&mut conn, &entries, &signer, now)
        .expect("seed over an existing pair");
    assert_eq!(
        written, 0,
        "an entry whose pair already has a row under a different reason must write nothing"
    );

    let row = db::get_feed_block_by_pair(&conn, FeedBlockKind::Guid, "preexisting-guid")
        .expect("read the block")
        .expect("the row must still exist");
    assert_eq!(
        row.reason, "operator note: known spam feed",
        "the original reason must be unchanged"
    );
    assert_eq!(
        feed_blocked_event_count(&conn),
        0,
        "no feed_blocked event must be signed for an entry the seed skips"
    );
}

#[test]
fn a_seeded_block_stays_after_a_later_call_with_an_empty_entry_list() {
    let mut conn = common::test_db();
    let signer = common::temp_signer("adr0053-seed-c");
    let now = common::now();

    let entries = vec![(FeedBlockKind::Guid, "stays-blocked-guid".to_string())];
    let written = stophammer::blocks::seed_blocks(&mut conn, &entries, &signer, now)
        .expect("seed the first entry");
    assert_eq!(written, 1, "the first call must write the one entry");

    let written_empty = stophammer::blocks::seed_blocks(&mut conn, &[], &signer, now)
        .expect("seed with no entries");
    assert_eq!(
        written_empty, 0,
        "a call with no entries must write nothing"
    );

    let row = db::get_feed_block_by_pair(&conn, FeedBlockKind::Guid, "stays-blocked-guid")
        .expect("read the block")
        .expect("the row seeded earlier must still exist");
    assert_eq!(row.reason, "seeded from environment");
}

// ---------------------------------------------------------------------------
// build_chain / ChainSpec::DEFAULT
// ---------------------------------------------------------------------------

#[test]
fn build_chain_skips_feed_blocklist_and_does_not_panic() {
    let spec = ChainSpec {
        names: vec![
            "content_hash".to_string(),
            "feed_blocklist".to_string(),
            "medium_music".to_string(),
        ],
    };

    let chain = build_chain(&spec, "test-token".to_string());
    let debug = format!("{chain:?}");

    assert!(
        debug.contains("content_hash"),
        "content_hash must still run: {debug}"
    );
    assert!(
        debug.contains("medium_music"),
        "medium_music must still run: {debug}"
    );
    assert!(
        !debug.contains("feed_blocklist"),
        "feed_blocklist must not appear in the built chain: {debug}"
    );
}

#[test]
fn chain_spec_default_has_no_feed_blocklist() {
    assert!(
        !ChainSpec::DEFAULT
            .split(',')
            .any(|name| name == "feed_blocklist"),
        "ChainSpec::DEFAULT must not list feed_blocklist: {}",
        ChainSpec::DEFAULT
    );
}
