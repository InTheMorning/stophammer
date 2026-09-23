# ADR 0049 Task 003: The Node Stores `rel`

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §6.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md).
Needs: task 001. Merge before task 004, because of the migration number.

## Goal

The node accepts `rel` on each remote item, stores it unchanged, replicates it
in the remote-item events, and reports it in the `remote_items` include.

## Files To Inspect

- `src/ingest.rs`: `IngestRemoteFeedRef`
- `src/model.rs`: `FeedRemoteItemRaw`, `TrackRemoteItemRaw`
- `src/db.rs`: the `MIGRATIONS` array, `replace_feed_remote_items_raw`,
  `replace_track_remote_items_raw`,
  `replace_track_remote_items_raw_for_feed_track`,
  `get_feed_remote_items_for_feed`, and each other reader of these tables
- `src/schema.sql`: `feed_remote_items_raw`, `track_remote_items_raw`
- `src/api.rs`: where ingest turns an `IngestRemoteFeedRef` into a raw row
- `src/apply.rs`: `FeedRemoteItemsReplaced`, `TrackRemoteItemsReplaced`
- `src/query.rs`: `FeedRemoteItemResponse`, `TrackRemoteItemResponse`,
  `load_track_remote_items`
- `src/openapi.rs`: the examples for `include=remote_items`
- `migrations/0034_feed_last_build_date.sql` and its test, as a pattern
- `tests/migration_tests.rs`, `tests/event_tests.rs`, `tests/apply_tests.rs`

## Files Likely To Change

- `migrations/0035_remote_item_rel.sql`, new
- `src/schema.sql`, `src/db.rs`, `src/model.rs`, `src/ingest.rs`, `src/api.rs`,
  `src/query.rs`, `src/openapi.rs`
- `tests/migration_tests.rs`, `tests/adr0049_rel_tests.rs` (new), and the tests
  that construct the raw structs with a struct literal

## Do Not Touch

- `load_publisher` and `load_track_publisher`. Task 005 and task 006 change them
- any Wavlake rule in `src/api.rs`. Task 007 deletes them
- `stophammer-parser`, `stophammer-crawler`

## Constraints

- The column is `rel TEXT`, nullable, on both tables. The migration only adds
  the columns. It does not fill them.
- Each new struct field is `Option<String>` with `#[serde(default)]`, so an
  event that an earlier node signed still decodes.
- The node stores the value that the crawler sent, unchanged.
- The response field is `rel`. Its doc comment, and so its schema description,
  says: "Raw `rel` attribute. The Podcast Namespace does not define `rel` on
  `podcast:remoteItem`, so this value is non-standard." Serialize it as `null`
  when absent. Do not skip it.
- ADR 0046: if `open_db` on an existing database reports version 35 as already
  applied, add an `ensure_remote_item_rel_schema` repair and call it from
  `open_db`.

## Implementation Steps

1. Add the migration, add it to `MIGRATIONS`, and add the columns to
   `src/schema.sql`.
2. Add the field to the ingest type and the two model types.
3. Write and read `rel` in each function of `src/db.rs` that writes or reads
   these tables.
4. Carry `rel` from ingest into the raw rows in `src/api.rs`.
5. Add `rel` to the two response types and to the OpenAPI examples.
6. Add the tests below.
7. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- A migration test proves that both tables have a `rel` column after
  migration, and that a row from before the migration reads `rel` as null.
- A test ingests `sirlibre-label` from the task 002 fixtures and proves that
  `GET /v1/feeds/{guid}?include=remote_items` gives `rel == "label"` on at least
  one item.
- A test proves that a remote item with no `rel` gives `"rel": null` in the
  response.
- A test proves that a `FeedRemoteItemsReplaced` payload with no `rel` key
  decodes and applies.
- A test proves that a `FeedRemoteItemsReplaced` event with `rel` applies on a
  second database and gives the same stored value.

## Test Commands

```bash
cargo build
cargo test --test migration_tests
cargo test --test adr0049_rel_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- migration number 0035 is already taken
- a reader of these tables is outside `src/db.rs`, and a change to it is
  necessary
- the task 002 fixtures are not merged. Use an inline JSON payload for the
  tests then, and say so in the report

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `src/ingest.rs` `IngestRemoteFeedRef`
- `src/model.rs` `FeedRemoteItemRaw` and `TrackRemoteItemRaw`
- `src/db.rs`: `MIGRATIONS`, and each function that reads or writes
  `feed_remote_items_raw` or `track_remote_items_raw`
- `src/schema.sql`, `src/apply.rs`, and `src/query.rs`
  `FeedRemoteItemResponse` and `TrackRemoteItemResponse`
- `migrations/0034_feed_last_build_date.sql` and its migration test

Goal:
- The node accepts, stores, replicates and reports the raw `rel` of each remote
  item.

Constraints:
- `migrations/0035_remote_item_rel.sql` adds nullable `rel TEXT` to both tables
  and nothing more. Update `src/schema.sql` and `MIGRATIONS`.
- Each new struct field is `Option<String>` with `#[serde(default)]`.
- Store the value unchanged.
- The response field `rel` is serialized as `null` when absent. Its doc comment
  says: "Raw `rel` attribute. The Podcast Namespace does not define `rel` on
  `podcast:remoteItem`, so this value is non-standard."
- ADR 0046: if an existing database already records version 35, add
  `ensure_remote_item_rel_schema` and call it from `open_db`.

Do not touch:
- `load_publisher`, `load_track_publisher`
- any Wavlake rule in `src/api.rs`
- `stophammer-parser`, `stophammer-crawler`

Acceptance criteria:
- The gate is green.
- Tests prove:
  - Both columns exist after migration, and an earlier row reads null.
  - The `sirlibre-label` fixture gives `rel == "label"` through `include=remote_items`.
  - A missing `rel` gives `"rel": null`.
  - A `FeedRemoteItemsReplaced` payload with no `rel` key decodes and applies.
  - An event with `rel` applies on a second database with the same value.

Test commands:
- `cargo build`
- `cargo test --test migration_tests`
- `cargo test --test adr0049_rel_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cargo run --bin gen_openapi > /dev/null`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
