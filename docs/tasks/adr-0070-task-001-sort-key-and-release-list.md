# ADR 0070 Task 001: The Sort Key And The Release List

Owner: [ADR 0070](../adr/0070-a-name-list-gives-each-identity-one-row.md)
sections 1, 2 and 6. v4vmm request 6.

Repository: `stophammer`. The operator commits.

## Goal

The node stores the sort key of each feed title. `GET /v1/feeds/by-name` gives
the feeds of one medium in the order of that key, with `cursor` and `from`.

## Files To Inspect

- `src/query.rs`: `recent_feed_rows`, `handle_get_recent_feeds`,
  `ListQuery`, `parse_recent_cursor`, `encode_cursor`, `FeedResponse`,
  `handle_capabilities`, the router
- `src/api.rs`: the place where the ingest sets `title_lower`
- `src/db.rs`: the feed upsert, `MIGRATIONS`, `open_db`
- `src/schema.sql`
- `src/openapi.rs`: the path of `/v1/feeds/recent`, `envelope_schema`
- `tests/migration_tests.rs`, `tests/adr0044_*`

## Files Likely To Change

- `migrations/0051_feed_title_sort.sql`, new
- `src/schema.sql`, `src/db.rs`, `src/model.rs`, `src/api.rs`
- `src/query.rs`, `src/openapi.rs`
- `tests/adr0070_release_list_tests.rs`, new
- `docs/API.md`, `docs/schema-reference.md`

## Constraints

- **Sort key.** One public function gives the key of ADR 0049 §7. It trims
  the white space at the start and at the end. It replaces each internal run
  of white space with one space. Then it changes the text to Unicode lowercase.
- **No other change.** Task 002 uses the same function. Do not remove an
  article, punctuation or an accent.
- **Storage.** Migration 0051 adds `feeds.title_sort TEXT` and the index
  `(title_sort, feed_guid)`. The ingest writes the key at each feed upsert.
  SQLite cannot give the Unicode lowercase of Rust. Thus `open_db` fills each
  null `title_sort` from Rust after the migration, in one transaction. Do not
  change `title_lower`.
- **Order.** `ORDER BY title_sort, feed_guid` with the `BINARY` collation.
  This is the code point order.
- **Medium.** `medium` as in `/v1/feeds/recent`. The default is `music`.
- **Cursor.** The cursor encodes `(title_sort, feed_guid)` of the last row. A
  page with `has_more` true always has a cursor.
- **`from`.** The route applies the sort key function to `from`, and starts
  at the first row with `title_sort` equal to or after it. A request with
  `cursor` ignores `from`.
- **Row.** The `FeedResponse` row of `/v1/feeds/recent`, and `sort_name`.
  `sort_name` is absent from the other routes. No `include` name is necessary.
- **Contract.** The route is in `spec_value()` with an envelope from
  `envelope_schema`, and `/v1/node/capabilities` lists it.
- **Cost.** Measure a page of 50 rows at the start, and a page after
  `from=m`, on a copy of the backup of 2026-09-26. Report both times.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- Each existing migration file. Never rewrite a merged migration.
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- Each existing route, response field and column, except where a step below
  names it
- `backups/`. Read a backup only with `?immutable=1`, or from a copy in the
  scratch directory.
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add `pub fn sort_key(text: &str) -> String` in `src/model.rs`, with a doc
   comment that names ADR 0070 §1 and ADR 0049 §7, and unit tests.
2. Add `migrations/0051_feed_title_sort.sql`, add it at the end of
   `MIGRATIONS` in `src/db.rs`, and add the column and the index to
   `src/schema.sql`.
3. Do not add `title_sort` to `model::Feed`, because the node signs that
   struct in `FeedUpsertedPayload`. The feed upsert and `ingest_transaction`
   in `src/db.rs` write `sort_key(&feed.title)` into the column.
4. Add the fill at open, modeled on `ensure_search_index_track_identity`, and
   call it from `open_db`.
5. Add the handler and the route `/v1/feeds/by-name` in `src/query.rs`. Reuse
   `ListQuery`, `FeedResponse` and the response mapping of
   `handle_get_recent_feeds`. Move a shared part into a function, and do not
   copy it.
6. Add `sort_name` to `FeedResponse` with
   `#[serde(default, skip_serializing_if = "Option::is_none")]`.
7. Add the path to `spec_value()` in `src/openapi.rs`, and the route to
   `handle_capabilities`.
8. Write the tests, then `docs/API.md` and `docs/schema-reference.md`.
9. Measure the fill and the two pages on a copy of the backup.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0070_release_list_tests.rs`, or in
the test file that the item names.

- The sort key function gives `"the  best\tsong "` as `"the best song"`, and
  `"Élan"` as `"élan"`.
- Three feeds with the titles `"b"`, `" A"` and `"Ä"` come in the order `a`,
  `b`, `ä`.
- Two feeds with the same title come in `feed_guid` order.
- Pages of size 2 over 5 feeds give each feed one time, and the last page has
  `has_more` false.
- `from=M` starts at the first title that starts with `m`.
- A feed of the medium `publisher` is absent with the default medium.
- `tests/migration_tests.rs`: migration 0051 on a database with feeds fills
  each `title_sort`.
- The ADR 0044 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0070_release_list_tests
cargo test --test migration_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A step needs a change to an ADR, or to a file of "Do Not Touch".
- A constraint of this task disagrees with the code.
- An existing test fails after the change, and the cause is not clear.
- The fill at open takes more than 10 seconds on the backup.
- `EXPLAIN QUERY PLAN` shows that the release list does not use the index.

## Expected Final Report

1. files changed
2. tests run and their results, and the two page times
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `AGENTS.md` (the code style and the pre-commit gate)
- `docs/adr/0070-a-name-list-gives-each-identity-one-row.md`
- `docs/tasks/adr-0070-task-001-sort-key-and-release-list.md` (this task: the constraints, the steps and the triggers)
- the files of "Files To Inspect" in this task

Goal:
- The node stores the sort key of each feed title, and `GET /v1/feeds/by-name` gives the feeds of one medium in the order of that key, with `cursor` and `from`.

Constraints:
- Follow the section "Constraints" and the section "Implementation Steps" of this task.
- One Rust function makes each sort key. No SQL `lower()` makes a key.
- `title_lower` and `/v1/feeds/recent` do not change.
- Stop at each escalation trigger of this task.

Do not touch:
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`
- each existing migration file
- each existing route, response field and column, except as this task names
- `backups/` except for read-only measurement
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0070_release_list_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cargo test --test migration_tests`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
