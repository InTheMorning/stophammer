# ADR 0060 Task 002: The Node Keeps List Items

Owner: [ADR 0060](../adr/0060-a-list-feed-keeps-its-items.md) sections 2, 3
and 4. Plan: [ADR 0060 phase plan](../plans/adr-0060-list-feeds-phase-plan.md).

Repository: `stophammer`.

## Goal

The node stores `itemGuid` and `title` of each channel remote item. A read
gives them with the indexed track. The node also keeps the value block of a
`musicL` feed as source data, and no route gives it.

## Files To Inspect

- `src/ingest.rs`: `IngestRemoteFeedRef`
- `src/model.rs`: `FeedRemoteItemRaw`, `FeedPaymentRoute`
- `src/event.rs`: `EventPayload`, `FeedRemoteItemsReplacedPayload`,
  `FeedRoutesReplacedPayload`
- `src/apply.rs`: the arms for `FeedRemoteItemsReplaced` and
  `FeedRoutesReplaced`
- `src/api.rs`: the ingest path near `is_musicl`, where `feed_routes` is
  built, and where the events of one ingest are signed
- `src/db.rs`: `MIGRATIONS`, the write and read of `feed_remote_items_raw`,
  `get_feed_remote_items_for_feed`
- `src/schema.sql`: `feed_remote_items_raw`, `feed_payment_routes`
- `src/query.rs`: `FeedRemoteItemResponse`, `feed_remote_item_response`,
  `remote_item_summary`
- `tests/adr0059_entry_summary_tests.rs`: helpers to copy
- `tests/migration_tests.rs`
- `docs/adr/0046-migration-versions-are-array-positions.md`

## Files Likely To Change

- `migrations/0043_list_feed_items.sql`, new
- `src/schema.sql`, `src/db.rs`, `src/ingest.rs`, `src/model.rs`,
  `src/event.rs`, `src/apply.rs`, `src/api.rs`, `src/query.rs`
- `docs/API.md`, `docs/schema-reference.md`
- `tests/adr0060_list_items_tests.rs`, new

## Do Not Touch

- `stophammer-parser`, `stophammer-crawler`
- The ADR 0048 exemption of `musicL`, and the medium check
- Item-level remote items (`track_remote_items_raw`)
- The ADR 0059 fields

## Constraints

- **Migration 0043.** Add `remote_item_guid TEXT` and `remote_item_title TEXT`
  to `feed_remote_items_raw`. Create `feed_list_value_raw` with the columns
  of `feed_payment_routes`: `feed_guid` with a reference to `feeds`,
  `recipient_name`, `route_type`, `address`, `custom_key`, `custom_value`,
  `split`, `fee`, plus `position`. Add an index on `feed_guid`.

  Add the same to `src/schema.sql`. Append the file to `MIGRATIONS` as the next position.
  Examine the delete of a feed. A trigger or a cleanup function can delete
  the rows of `feed_payment_routes` for a feed. Each one must also delete the
  rows of `feed_list_value_raw`. `trg_feeds_cleanup_before_delete` is one of
  them.
- **Ingest.** `IngestRemoteFeedRef` gets `item_guid` and `item_title` as
  `Option<String>` with `#[serde(default)]`. A crawler that does not send
  them stays valid.
- **Model and event.** `FeedRemoteItemRaw` gets `remote_item_guid` and
  `remote_item_title`, with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`. An old event
  still parses. The node stores them only for a channel element.
- **The value block.** For a `musicL` feed, the node keeps
  `feed_payment_routes` empty, as today. It writes the channel routes of the
  submission to `feed_list_value_raw`, and replaces the earlier rows of that
  feed.

  Add one event type, `FeedListValueReplaced`, with `feed_guid` and the
  rows. Sign it with the other events of the ingest. `apply.rs` applies it,
  so a community node stores the same rows. Do not add the rows to the route
  history of ADR 0053.
- **The read.** `FeedRemoteItemResponse` gets three keys, always present:
  - `remote_item_guid`, the stored value or null
  - `remote_item_title`, the stored value or null
  - `remote_track_guid`: when `remote_item_guid` is not null and the entry
    resolves to a feed (the resolution of `remote_item_summary`), the
    `track_guid` of the row in `tracks` with that `feed_guid` and
    `track_guid = remote_item_guid`. Otherwise null.

  Reuse the resolution that the summary makes. Do not resolve twice. The
  track check is one point read on the primary key of `tracks`.
- No route gives a row of `feed_list_value_raw`.
- Document the three read fields in `docs/API.md`, and the table and columns
  in `docs/schema-reference.md`. Write in ASD-STE100 Simplified Technical
  English, and run
  `python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --check --no-heuristics`
  on each changed document.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0060_list_items_tests.rs`. Each failure
message names ADR 0060 and its guard:

- A `musicL` feed with an entry that gives `item_guid` and `item_title`, for
  a track that is indexed, gives `remote_item_guid`, `remote_item_title` and
  that `remote_track_guid`.
- An entry for a track that is not indexed gives null in `remote_track_guid`.
- An entry with no `item_guid` gives null in each of the three fields, and
  each key is present.
- An entry whose feed resolves by URL gives the track of the feed at that
  URL.
- A `musicL` feed with channel routes: `include=payment_routes` gives an
  empty list, `feed_list_value_raw` holds the rows, and a second ingest with
  different routes replaces them.
- The `FeedListValueReplaced` event applies on a second database through
  `apply.rs`, and that database holds the same rows.
- A delete of the `musicL` feed removes its rows of `feed_list_value_raw`.
- `cargo test --test migration_tests` passes, and a migration test shows
  that 0043 adds the columns and the table.
- The gate is green: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`.

## Escalation Triggers

Stop and report when:

- The events of one ingest are not signed in one place, and the new event
  needs a second signing path.
- A trigger or function that deletes feed rows cannot be changed without a
  change to an earlier migration.

## Expected Final Report

1. files changed
2. tests run and their real results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0060-task-002-node-list-items.md`, all of it
- `docs/adr/0060-a-list-feed-keeps-its-items.md`
- `AGENTS.md`, section "New Database Migration"
- The files in "Files To Inspect"

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.
- Run `cargo clippy --all-targets -- -D warnings` exactly, and report its
  real output. Write each test that the criteria name, with real assertions.

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
