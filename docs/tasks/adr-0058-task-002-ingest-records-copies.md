# ADR 0058 Task 002: Ingest Records Copies

Owner: [ADR 0058](../adr/0058-a-copy-of-a-feed-is-public.md) sections 1 and
1a. Plan: [phase plan](../plans/adr-0058-feed-copies-phase-plan.md),
decisions 3 to 5.

## Goal

Each mirror submission on the primary records a summary row, with the limit of
20 rows for each GUID. A new or changed summary signs one `FeedCopyObserved`
event.

## Files To Inspect

- `src/api.rs`: `handle_ingest_feed`. Find the two mirror paths: the
  `ReadPhaseOutcome::NoChange` branch, and the `SubmissionClass::Mirror` arm
  of the write phase when `self_link_move` stays `None`.
- `src/api.rs`: `record_feed_url_observations_for_ingest`, `sign_event_row`,
  `signed_row_to_event`
- The functions of task 001 in `src/db.rs` and `src/model.rs`
- `tests/adr0051_source_url_tests.rs`, `tests/adr0052_self_link_tests.rs`: the
  test helpers

## Files Likely To Change

- `src/api.rs`: `handle_ingest_feed`, and one new helper
- `tests/adr0058_ingest_copy_tests.rs`, new

## Do Not Touch

- `classify_submission`, `src/verify.rs`, `src/query.rs`, `src/openapi.rs`
- The self-link move path. A move is not a copy.
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **One helper**, `record_feed_copy_for_ingest(conn, feed_guid, url,
  &IngestFeedData, now, signer) -> Result<Vec<SignedEventRow>, ApiError>`.
  It uses one transaction:
  1. Build the summary and the digest with the task 001 functions.
  2. When a row exists for the pair: update `last_seen`. When the digest
     changed, write the summary and sign one `FeedCopyObserved` event.
  3. When no row exists and the GUID has fewer than `MAX_COPIES_PER_GUID`
     rows: insert the row with `first_seen = now` and `last_seen = now`, and
     sign one `FeedCopyObserved` event.
  4. When no row exists and the GUID has the maximum: write no row and no
     event, and call `increment_copy_overflow`.
- The event and the apply step of task 001 write the same columns, so a
  community node gets the same row, with `last_seen` null.
- **The URL of the row** is `req.canonical_url`. When `req.source_url` is
  different and is not the stored source URL, record a second row for it with
  the same summary.
- **Where to call it.** In each of the two mirror paths, after the URL
  observation, in the same writer lock. Add the event rows to the events that
  the response lists and fans out. The response keeps its current reason.
- The `NoChange` branch has `req.feed_data` only when the crawler sent a body.
  With no body, do nothing.
- A block check of ADR 0053 runs before classification, so a blocked URL never
  reaches this helper. Do not add a second check.
- A short comment names ADR 0058 sections 1 and 1a.

## Implementation Steps

1. Add the helper.
2. Call it in the two mirror paths.
3. Add the tests below.
4. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0058_ingest_copy_tests.rs`, through the router:

- A feed accepted at URL A. A submission with the same GUID from URL B with a
  different recipient: the response reason is `source_conflict`, one
  `FeedCopyObserved` event is in `events_emitted`, and the row of B has the
  recipient.
- The same body from B again, with a different `content_hash` and a different
  `last_build_date`: no new `FeedCopyObserved` event, and `last_seen` changes.
- The body from B with a changed `custom_value`: one new event, and a new
  digest.
- The `NoChange` path. The node answers `no_change` only when its crawl cache
  holds the URL with the same hash. A mirror answer does not write that
  cache, but an accept before ADR 0051 did. So the test seeds the cache for B
  with `db::upsert_feed_crawl_cache`, then submits B with that hash, and then
  examines the row. The row must be as in the first case.
- 20 different URLs with the same GUID make 20 rows. The 21st makes no row and
  no event, and the overflow count is 1.
- A self-link move of ADR 0052 makes no row.
- The events of this test file, applied to a second database with
  `apply::apply_single_event`, give the same rows, with `last_seen` null.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0058_ingest_copy_tests
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

- An existing test counts the events of a mirror response, and fails because
  of the new event. List each one, and do not change it.
- The `NoChange` branch cannot get the writer lock in the way the observation
  does.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0058-task-002-ingest-records-copies.md`, all of it
- `docs/adr/0058-a-copy-of-a-feed-is-public.md`, sections 1 and 1a
- `docs/plans/adr-0058-feed-copies-phase-plan.md`, decisions 3 to 5
- The files in "Files To Inspect"

Goal:
- Each mirror submission records a summary row, with the limit of 20 rows for
  each GUID, and signs `FeedCopyObserved` only for a new or changed summary.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `classify_submission`, `src/verify.rs`, `src/query.rs`, `src/openapi.rs`
- The self-link move path
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0058_ingest_copy_tests.rs`.
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
