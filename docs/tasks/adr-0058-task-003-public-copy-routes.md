# ADR 0058 Task 003: Public Copy Routes

Owner: [ADR 0058](../adr/0058-a-copy-of-a-feed-is-public.md) sections 2, 1b
and 3. Plan: [phase plan](../plans/adr-0058-feed-copies-phase-plan.md),
decisions 7 to 9 and 11.

## Goal

A client reads each copy of a feed, and the list of records with an open copy,
from any node.

## Files To Inspect

- `src/query.rs`: `query_routes`, `handle_get_feed`, `FeedResponse`,
  `handle_get_route_history` (as the pattern for a feed sub-route),
  `QueryResponse`, the pagination of `handle_get_recent_feeds`,
  `response_schemas`
- `src/openapi.rs`: the entry of `/v1/feeds/{guid}/route-history`
- The functions of task 001: `list_feed_copies`, `get_copy_overflow`,
  `guid_origin_matches`, `CopySummary`, `RouteRecipient`
- `src/db.rs`: the reads of the stored item GUIDs and the recipient sets of a
  record (`get_feed_payment_routes`, `get_payment_routes_for_track`, or the
  names in use)
- `docs/API.md`: the route-history section

## Files Likely To Change

- `src/query.rs`, `src/db.rs`, `src/openapi.rs`, `docs/API.md`
- `api.html`, only if it lists the feed routes by hand
- `tests/adr0058_copy_routes_tests.rs`, new

## Do Not Touch

- `src/api.rs`, `src/apply.rs`, `src/event.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **The comparison**, one function used by each route:
  `copy_differences(conn, feed_guid, &FeedCopyRow) -> Result<(bool, bool),
  DbError>`. It gives `differs_tracks` and `differs_recipients` of plan
  decision 7, against the current record.
- **Open.** Plan decision 9. One function, `is_open_copy`, used by each route.
- **`guid_origin`.** `guid_origin_matches(feed_guid, row.url) &&
  !guid_origin_matches(feed_guid, record.feed_url)`.
- **`GET /v1/feeds/{guid}/copies`.** `404` when the record does not exist.
  Otherwise each row in `first_seen` order, with: `url`, `first_seen`,
  `last_seen` (null on a community node), `title`, `differs_tracks`,
  `differs_recipients`, `guid_origin`, `open`, `feed_recipients`,
  `track_recipients`, and `resolution` (null, or `decision`, `reason`,
  `resolved_at`, and `current`, which is true when the resolution holds). The
  envelope also gives `copies_over_limit`, from the counter. The counter is 0
  on a community node.
- **`copy_count`** in `FeedResponse`: the number of open copies. Compute it
  with the same functions.
- **`GET /v1/copies`.** Each record with one or more open copies, or with a
  counter above zero. Each item gives `feed_guid`, `feed_url`, `title`,
  `copy_count`, `copies_over_limit` and the newest `first_seen` of its open
  copies. The order is that time, newest first, with the cursor pagination of
  `handle_get_recent_feeds` and a `limit` of at most 100.
- The routes are in `query_routes`, with no authentication.
- Each new response type derives `utoipa::ToSchema` and is in
  `response_schemas`. `src/openapi.rs` gets the two paths, as for route
  history.
- `docs/API.md` documents both routes and `copy_count`. It states the meaning
  of an alias, a copy, `open` and `guid_origin`, and that `last_seen` and
  `copies_over_limit` come from the primary only.

## Implementation Steps

1. Add the comparison and the open check.
2. Add the two routes and `copy_count`.
3. Update the OpenAPI document and `docs/API.md`.
4. Add the tests below.
5. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0058_copy_routes_tests.rs`. They make rows
through the ingest route of task 002, or with the db functions of task 001:

- A copy with a different recipient: `/copies` gives `differs_recipients`
  true, `open` true. `copy_count` is 1. `/v1/copies` lists the record.
- An alias with the same item GUIDs and recipients: both differences false,
  `open` false. `copy_count` is 0. `/v1/copies` does not list the record.
- A copy that differs only in a keysend `custom_value` is open.
- The record GUID `7192ec54-3aa2-5c61-987b-51bf75f68568` with source URL
  `https://musicsideproject.com/api/hosted/7192ec54-3aa2-5c61-987b-51bf75f68568.xml`
  and a row at
  `https://wavlake.com/feed/music/a82acc2f-3440-491c-94c2-d27bebf6cfe6`:
  `guid_origin` is true for the row.
- A `FeedCopyResolved` event with the current digest closes the copy. A new
  summary of the copy opens it again, and `resolution.current` is false.
- A record whose counter is above zero is in `/v1/copies`.
- `/v1/feeds/{unknown}/copies` answers `404`.
- A replica database with the same events gives the same answer from each
  route, except `last_seen` and `copies_over_limit`.
- `cargo run --bin gen_openapi` lists both paths.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0058_copy_routes_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi | grep -c "copies"
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- The query of `/v1/copies` needs a full scan of `feed_copies` for each
  request, and the table can hold more than 50,000 rows. Report the plan of
  `EXPLAIN QUERY PLAN`, and propose an index.
- An existing test compares the full JSON of the feed response and fails on
  `copy_count`. List each one.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0058-task-003-public-copy-routes.md`, all of it
- `docs/adr/0058-a-copy-of-a-feed-is-public.md`, sections 1b, 2 and 3
- `docs/plans/adr-0058-feed-copies-phase-plan.md`, decisions 7 to 9 and 11
- The files in "Files To Inspect"

Goal:
- Add `GET /v1/feeds/{guid}/copies`, `GET /v1/copies` and `copy_count`, with
  the differences, `guid_origin` and the open check.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `src/api.rs`, `src/apply.rs`, `src/event.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0058_copy_routes_tests.rs`.
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
