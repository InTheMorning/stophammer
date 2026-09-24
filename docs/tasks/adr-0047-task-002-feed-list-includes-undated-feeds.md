# ADR 0047 Task 002: The Feed List Includes Feeds With No Dated Item

Owner: [ADR 0047](../adr/0047-a-corrective-pass-reads-the-index.md), decision 2.
This task corrects a defect. It changes no decision.

## Why This Task Exists

ADR 0047 decision 2 makes the corrective pass read `GET /v1/feeds/recent` with
`medium=all`, so that the pass covers the publisher and `musicL` feeds too.
The route does not list them.

On 2026-09-24 the production node gave these results:

| Filter | Pages | Feeds | Feeds with `newest_item_at` null | End |
|---|---:|---:|---:|---|
| `all` | 84 | 8,334 | 0 | `has_more: false` |
| `music` | 84 | 8,333 | 0 | `has_more: false` |
| `publisher` | 1 | 100 | 99 | `has_more: true` and `cursor: null` |

A publisher feed has no item, so its `newest_item_at` is null. The handler in
`src/query.rs` (`handle_get_recent_feeds`) has three faults:

1. The cursor condition `(newest_item_at, feed_guid) < (?2, ?3)` is never true
   for a null `newest_item_at`. So a page after the first one never holds a
   feed with a null value.
2. `ORDER BY newest_item_at DESC` puts the null rows last. With `medium=all`,
   the pages reach the end of the dated rows and stop there.
3. The next cursor comes from `r.newest_item_at.map(...)`. When the last row of
   a page has a null value, the cursor is `null` while `has_more` is `true`.

So each corrective pass since ADR 0047 left out three groups of feeds:

- each publisher feed,
- each `musicL` feed with no dated item,
- each music feed with no dated item. `stophammer-crawler` stops with an error when it gets `has_more: true`
with a null cursor, so fault 3 did not give a short corpus without a message.
Faults 1 and 2 did.

## Goal

`GET /v1/feeds/recent` lists each feed that matches the filter, exactly once,
for each `medium` value. The feeds with no dated item come after the dated
feeds.

## Files To Inspect

- `src/query.rs`: `handle_get_recent_feeds`, `encode_cursor`, `decode_cursor`,
  `RecentFeedsParams`
- `src/schema.sql`: `feeds.newest_item_at`, and `idx_feeds_newest`
- `tests/adr0047_medium_all_tests.rs`: the pattern for this route
- `docs/API.md`: the section for `GET /v1/feeds/recent`

## Files Likely To Change

- `src/query.rs`
- `tests/adr0047_undated_feeds_tests.rs`, new
- `docs/API.md`

## Do Not Touch

- the response shape. `newest_item_at` stays `null` in the response for a feed
  with no dated item
- the cursor encoding, other than the value of its first part
- migrations. The task adds no index. See "Escalation Triggers"
- the other routes
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- The sort key is `COALESCE(newest_item_at, -1)`. A real timestamp is never
  negative, so -1 puts each undated feed after each dated feed.
- Both SQL statements of the handler use the same key:
  - `ORDER BY COALESCE(newest_item_at, -1) DESC, feed_guid DESC`
  - the cursor condition
    `(COALESCE(newest_item_at, -1), feed_guid) < (?2, ?3)`
- The next cursor encodes the same key: `-1` for an undated last row. When
  `has_more` is true, the cursor is never null.
- A cursor that a client got before this change has a real timestamp as its
  first part. It must still work, because the key of a dated row does not
  change.
- `decode_cursor` already parses the first part as `i64`, so `-1` needs no new
  parsing. Confirm this with a test.

## Implementation Steps

1. Change the two SQL statements and the cursor construction.
2. Add the tests below.
3. Measure one page request against a copy of the local database, as task 009
   of ADR 0049 did. Never open `stophammer.db` itself for writing.
4. Add one sentence to `docs/API.md`: feeds with no dated item come last.
5. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- Test, all mediums: a database holds 3 dated music feeds, 2 publisher feeds
  with no item, and 1 music feed with no dated item. `medium=all` with
  `limit=2` pages to the end and gives each of the 6 feeds exactly once, the
  dated feeds first.
- Test, one medium: `medium=publisher` with `limit=1` pages to the end and
  gives both publisher feeds.
- Test, boundary: a page that ends on an undated row has `has_more: true` and a
  cursor that is not null. The next page continues from it.
- Test, rule: for each page in the tests above, `has_more: true` implies a
  cursor that is not null.
- Test, old cursor: a cursor made from a dated row before this change gives the
  same next page as before.
- The existing tests of this route pass with no edit.

## Test Commands

```bash
cargo build
cargo test --test adr0047_undated_feeds_tests
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
6. the time of one page request against the database copy, and the number of
   feeds that `medium=all` gives on that copy before and after the change

## Escalation Triggers

Stop and report when:

- one page request takes more than 100 ms against the database copy. An
  expression index then needs a migration, and that is a separate decision
- an existing test asserts that a feed with no dated item is absent from this
  route
- a client in this repository reads `newest_item_at` from a cursor

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0047-task-002-feed-list-includes-undated-feeds.md`, in full
- `src/query.rs`: `handle_get_recent_feeds`, `encode_cursor`, `decode_cursor`
- `tests/adr0047_medium_all_tests.rs`

Goal:
- `GET /v1/feeds/recent` lists each matching feed exactly once, for each
  `medium`, with the undated feeds after the dated feeds.

Constraints:
- The sort key is `COALESCE(newest_item_at, -1)` in the `ORDER BY` and in the
  cursor condition of both SQL statements.
- The next cursor encodes that key, so it is never null when `has_more` is
  true.
- The response still gives `newest_item_at: null` for an undated feed.
- A cursor from a dated row keeps working.
- No migration and no index.

Do not touch:
- the response shape, migrations, the other routes
- `stophammer-crawler`, `stophammer-parser`
- `stophammer.db`. Measure only on a copy

Acceptance criteria:
- The gate is green.
- Tests prove:
  - `medium=all` with `limit=2` gives 3 dated and 3 undated feeds exactly once,
    dated first.
  - `medium=publisher` with `limit=1` gives both publisher feeds.
  - A page that ends on an undated row has a cursor that is not null.
  - `has_more: true` always has a cursor that is not null.
  - A cursor from a dated row gives the same next page as before.
- The existing tests of this route pass unedited.

Test commands:
- `cargo build`
- `cargo test --test adr0047_undated_feeds_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
