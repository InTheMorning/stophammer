# ADR 0058 Task 001: Copy Storage, Events And Summary

Owner: [ADR 0058](../adr/0058-a-copy-of-a-feed-is-public.md) sections 1, 1a
and 1b. Plan: [phase plan](../plans/adr-0058-feed-copies-phase-plan.md),
decisions 1 to 6 and 8.

## Goal

The node keeps a summary row for each pair of GUID and URL. It signs and
applies the two new events. It deletes the rows of a blocked URL. No route and
no ingest path uses them in this task.

## Files To Inspect

- `migrations/0038_feed_blocks.sql`, `migrations/0039_feed_declared_self_url.sql`
- `src/db.rs`: the `MIGRATIONS` array, `insert_feed_block`,
  `delete_feed_sql` (the feed delete), `insert_event`
- `src/schema.sql`
- `src/model.rs`: `RouteRecipient`, `recipient_set`, `feed_recipient_set`
- `src/ingest.rs`: `IngestFeedData`, `IngestTrackData`, `IngestPaymentRoute`
- `src/event.rs`: `FeedBlockedPayload`, `EventType`, `EventPayload`
- `src/apply.rs`: the `FeedBlocked` arm
- `tests/adr0053_block_storage_tests.rs`, `tests/migration_tests.rs`

## Files Likely To Change

- `migrations/0040_feed_copies.sql`, new
- `src/schema.sql`, `src/db.rs`, `src/model.rs`, `src/event.rs`, `src/apply.rs`
- `tests/migration_tests.rs`: the watermark tests that count migrations
- `tests/adr0058_copy_storage_tests.rs`, new

## Do Not Touch

- `src/api.rs`, `src/query.rs`, `src/openapi.rs`, `src/verify.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **The migration.** `0040_feed_copies.sql` creates the two tables of plan
  decisions 1 and 2, with `CREATE TABLE IF NOT EXISTS` and `STRICT`. Add it as
  the next entry of `MIGRATIONS`, and add the same tables to `src/schema.sql`.
- **The summary type.** In `src/model.rs`: `pub struct CopySummary { title:
  String, item_guids: Vec<String>, feed_recipients: Vec<RouteRecipient>,
  track_recipients: BTreeMap<String, Vec<RouteRecipient>> }`, with serde and
  `utoipa::ToSchema`. Add `impl From<&IngestPaymentRoute> for RouteRecipient`
  if the ingest module can be imported without a cycle. If it cannot, put the
  conversion in `src/ingest.rs`.
- `pub fn copy_summary(feed: &IngestFeedData) -> CopySummary`. It keeps the
  item order of the feed.
- `pub fn summary_digest(summary: &CopySummary) -> String`: the SHA-256 hex of
  `serde_json::to_vec` of a value with `item_guids`, `feed_recipients` and
  `track_recipients` only. `BTreeMap` keeps the key order stable. The title is
  not part of the digest.
- `pub fn guid_origin_matches(feed_guid: &str, url: &str) -> bool`: true when
  `feed_guid`, lowercased, equals the UUIDv5 of `url` with the scheme and one
  trailing `/` removed, in the namespace
  `ead4c236-bf58-58c6-a2c6-a6b28d128cb6`. If the `uuid` crate lacks the `v5`
  feature, add it in `Cargo.toml` and name ADR 0058 in the report.
- **The events.** `FeedCopyObserved` and `FeedCopyResolved`, with the payloads
  of plan decision 4, `rename_all = "snake_case"` like the other events.
- **The apply step.** `FeedCopyObserved` upserts the row: it keeps an existing
  `first_seen`, and it writes the summary columns and `summary_digest`. It
  never writes `last_seen` or the counter.
- `FeedCopyResolved` writes the four
  resolution columns of an existing row. It does nothing when the row does not
  exist. Both are idempotent.
- **The db functions**, in `src/db.rs`, next to the block functions:
  - `pub const MAX_COPIES_PER_GUID: i64 = 20;`
  - `pub struct FeedCopyRow` with each column.
  - `get_feed_copy(conn, feed_guid, url) -> Result<Option<FeedCopyRow>, DbError>`
  - `list_feed_copies(conn, feed_guid) -> Result<Vec<FeedCopyRow>, DbError>`,
    ordered by `first_seen`.
  - `count_feed_copies(conn, feed_guid) -> Result<i64, DbError>`
  - `upsert_feed_copy_summary(conn, feed_guid, url, first_seen, &CopySummary,
    digest)`, used by the apply step and by task 002.
  - `touch_feed_copy_last_seen(conn, feed_guid, url, now)`
  - `increment_copy_overflow(conn, feed_guid)` and
    `get_copy_overflow(conn, feed_guid) -> Result<i64, DbError>`
  - `set_feed_copy_resolution(conn, feed_guid, url, decision, reason,
    resolved_at, resolved_digest)`
- **A URL block.** `insert_feed_block` deletes each `feed_copies` row with
  that URL when it inserts a row of kind `url`. It does this in the same
  statement group, on the same connection.
- **A feed delete.** The feed delete deletes the `feed_copies` rows and the
  `feed_copy_overflow` row of the GUID.
- A short comment names ADR 0058 at each new function group.

## Implementation Steps

1. Write the migration and the schema.
2. Add the types and functions of `src/model.rs`.
3. Add the events and the apply arms.
4. Add the db functions, and the two delete rules.
5. Update the migration watermark tests.
6. Add the tests below.
7. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0058_copy_storage_tests.rs`:

- `copy_summary` of a feed with two items keeps the item order, and each
  recipient keeps `custom_key` and `custom_value`.
- Two summaries that differ only in the title have the same digest. A change
  of one `custom_value` changes the digest.
- `guid_origin_matches("7192ec54-3aa2-5c61-987b-51bf75f68568",
  "https://wavlake.com/feed/music/a82acc2f-3440-491c-94c2-d27bebf6cfe6")` is
  true. The same GUID with the MSP URL
  `https://musicsideproject.com/api/hosted/7192ec54-3aa2-5c61-987b-51bf75f68568.xml`
  is false.
- Applying a `FeedCopyObserved` event two times gives one row, with the
  `first_seen` of the first event. `last_seen` stays null.
- Applying `FeedCopyResolved` writes the resolution. Applying it for a missing
  row does nothing and does not fail.
- Inserting a URL block deletes the row of that URL, and keeps a row of a
  different URL.
- The feed delete removes the rows and the counter of the GUID.
- `cargo test --test migration_tests` passes.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0058_copy_storage_tests
cargo test --test migration_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- `src/model.rs` cannot use `IngestPaymentRoute` without a module cycle, and
  the conversion in `src/ingest.rs` also makes a cycle.
- The feed delete is SQL in a trigger, not in Rust. Report where it is, and
  do not change the trigger.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0058-task-001-copy-storage-and-events.md`, all of it
- `docs/adr/0058-a-copy-of-a-feed-is-public.md`
- `docs/plans/adr-0058-feed-copies-phase-plan.md`, decisions 1 to 6 and 8
- The files in "Files To Inspect"

Goal:
- Add the `feed_copies` storage, the summary, the digest, the UUIDv5 check,
  the two events with their apply steps, and the delete rules. No route and no
  ingest path uses them yet.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `src/api.rs`, `src/query.rs`, `src/openapi.rs`, `src/verify.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0058_copy_storage_tests.rs`.
- The gate is green.

Test commands:
- `cargo build`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
