# ADR 0064 Task 002: The Node Stores And Replicates The Relay Link

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) sections 3 and 4
(the compare). Plan: [ADR 0064 phase plan](../plans/adr-0064-live-items-phase-plan.md).

Repository: `stophammer`. The operator commits.

## Goal

`live_events` holds `live_value_uri` and `live_value_protocol`. The ingest DTO,
`LiveEvent`, the event payload and the apply path carry them. The ingest emits
`LiveEventsReplaced` only when the deduplicated, sorted set of rows changes.

This task changes no ingest rule of ADR 0064 §4 other than the compare. Task
002b does the rest.

## Files To Inspect

- `src/ingest.rs`: `IngestLiveItemData`
- `src/model.rs`: `LiveEvent`
- `src/event.rs`: `LiveEventsReplacedPayload`
- `src/api.rs`: where `live_events: Vec<model::LiveEvent>` is built (search
  `let live_events: Vec<model::LiveEvent>`)
- `src/db.rs`: `MIGRATIONS`, `get_live_events_for_feed`,
  `dedupe_live_events`, `replace_live_events_for_feed`,
  `live_events_changed`, `build_live_events_event`, and the call site of
  `live_events_changed`
- `src/apply.rs`: the arms for `LiveEventsReplaced`
- `src/schema.sql`: `live_events`
- `migrations/0034_*.sql`: the form of a column migration
- `docs/adr/0046-migration-versions-are-array-positions.md`
- `tests/migration_tests.rs`, `tests/common/mod.rs`

## Files Likely To Change

- `migrations/0044_live_item_relay_link.sql`, new
- `src/schema.sql`, `src/db.rs`, `src/ingest.rs`, `src/model.rs`, `src/api.rs`
- `docs/schema-reference.md`
- `tests/adr0064_live_storage_tests.rs`, new

## Do Not Touch

- The promotion of an `ended` item to a track, and the filter to `pending` and
  `live`. Task 002b changes them.
- `src/query.rs`, `src/openapi.rs`. Task 003 changes them.
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`.

## Constraints

- The migration adds two nullable `TEXT` columns with `ALTER TABLE … ADD
  COLUMN`, as migration 0034 does. Append it to `MIGRATIONS`. Update
  `src/schema.sql` to match.
- In `IngestLiveItemData` and `LiveEvent`, add `live_value_uri:
  Option<String>` and `live_value_protocol: Option<String>` with
  `#[serde(default)]`. An old payload with no such field must deserialize.
- Do not add `deny_unknown_fields`.
- In the ingest, copy the two fields from the DTO into each `LiveEvent`.
- Before `live_events_changed`, deduplicate the new rows by
  `live_item_guid` (keep the first) and sort both lists by `live_item_guid`.
  The compare also covers the two new fields. The payload of
  `LiveEventsReplaced` holds the deduplicated, sorted rows.
- `get_live_events_for_feed` and `replace_live_events_for_feed` read and write
  the two columns.

## Steps

1. Write the migration and update `schema.sql` and `MIGRATIONS`.
2. Add the fields to the DTO and to `LiveEvent`. Correct each constructor.
3. Update the read and the write of `live_events`.
4. Deduplicate and sort before the compare and the payload.
5. Write the tests.

## Acceptance

Mechanical, each an integration test:

- An ingest with a relay link stores both values, and a second read gives
  them.
- A second ingest of the same live items in a different RSS order emits no new
  `LiveEventsReplaced`.
- An ingest with one `live_item_guid` two times emits one event, and a second
  ingest of the same feed emits none.
- A change of only `live_value_uri` emits a new event.
- `apply` of a `LiveEventsReplaced` payload with no relay fields stores `None`
  for both. `apply` of a payload with them stores them.
- `cargo test --test migration_tests` passes.
- The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --test migration_tests
cargo test --test adr0064_live_storage_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report without a workaround when:

- The migration runner reports that a database already records version 44.
  ADR 0046 then needs an `ensure_` repair, and the planner decides it.
- A test outside this task fails after the change.
- The compare change needs a change of the event format beyond the two
  optional fields.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0064-task-002-node-storage.md
- /home/citizen/build/stophammer/AGENTS.md (Code Style, Tests, New Database Migration)
- The functions named under "Files To Inspect". Use grep and read only those parts. The files are long.

Goal:
- Store, replicate and apply `live_value_uri` and `live_value_protocol` on each live row, and emit `LiveEventsReplaced` only when the deduplicated, sorted set of rows changes.

Constraints:
- Migration 0044 adds two nullable TEXT columns. Keep `schema.sql` consistent.
- New fields are `Option<String>` with `#[serde(default)]`. No `deny_unknown_fields`.
- Deduplicate by `live_item_guid` (keep the first) and sort by `live_item_guid` before the compare and in the payload.

Do not touch:
- The promotion of ended items to tracks and the pending/live filter in `src/api.rs`.
- `src/query.rs`, `src/openapi.rs`, `docs/adr/`, `stophammer-crawler/`, `stophammer-parser/`.
- Git: run no git command that writes (no add, commit, stash, checkout, reset).

Acceptance criteria:
- The seven criteria under "Acceptance" in the task file.

Test commands:
- cargo build
- cargo test --test migration_tests
- cargo test --test adr0064_live_storage_tests
- cargo test
- cargo clippy --all-targets -- -D warnings
- cargo fmt -- --check

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
