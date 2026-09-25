# ADR 0053 Task 001: Block Storage And Events

Owner: [ADR 0053](../adr/0053-a-correction-stays-applied.md) section 1.
Plan: [phase plan](../plans/adr-0053-durable-corrections-phase-plan.md),
decisions 1 to 4.

## Goal

The node has a `feed_blocks` table, two signed event types that change it,
and the database functions that later tasks call. No route and no ingest
check uses them in this task.

## Files To Inspect

- `migrations/0036_feed_url_observations.sql`, and the `MIGRATIONS` array in
  `src/db.rs`, as the pattern for a new table
- `src/db.rs`: `insert_event`, `record_feed_url_observation`,
  `apply_feed_url_observation` (near line 4439), `FeedUrlObservation`
- `src/event.rs`: `EventType`, `EventPayload`, `FeedUrlObservedPayload`
- `src/apply.rs`: the `FeedUrlObserved` arm near line 182
- `tests/migration_tests.rs`, `tests/adr0049_url_observation_tests.rs`
  (`feed_url_observations_replicate_identically_from_the_event_log`)

## Files Likely To Change

- `migrations/0038_feed_blocks.sql`, new
- `src/schema.sql`
- `src/db.rs`: the `MIGRATIONS` array, `FeedBlockKind`, `FeedBlock`, and the
  functions of plan decision 4
- `src/event.rs`: two event types and two payloads
- `src/apply.rs`: two match arms
- Each other `match` on `EventType` or `EventPayload` that the compiler names
- `tests/adr0053_block_storage_tests.rs`, new

## Do Not Touch

- `src/api.rs`, `src/verify.rs`, `src/verifiers/`, `src/main.rs`,
  `src/query.rs`, `src/openapi.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- The table of plan decision 1, in the migration and in `src/schema.sql`.
- `FeedBlockKind { Guid, Url }` derives `Serialize`, `Deserialize`,
  `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq` with
  `#[serde(rename_all = "snake_case")]`. It has `as_str()` (`"guid"`,
  `"url"`) and `normalize(&self, value: &str) -> String`: lower case and
  trimmed for `Guid`, trimmed only for `Url`.
- `FeedBlock { block_id, kind, value, reason, blocked_at }` derives
  `Serialize`, `Deserialize`, `Debug`, `Clone`, `PartialEq`, `Eq` and
  `utoipa::ToSchema`.
- The functions of plan decision 4, with these rules:
  - `insert_feed_block` normalizes the value and uses `INSERT OR IGNORE`. It
    returns `true` only when it wrote a row.
  - `find_feed_block` normalizes each input. It returns the first matching
    row, GUID first, then each URL in order. An empty URL is not checked.
- `EventType::FeedBlocked` and `EventType::FeedUnblocked`, in the style of
  the existing variants. `FeedBlockedPayload { block_id, kind, value, reason,
  blocked_at }` and `FeedUnblockedPayload { block_id }`.
- `apply.rs`: `FeedBlocked` calls `insert_feed_block` with the payload
  values. `FeedUnblocked` calls `delete_feed_block`. Both are idempotent.
- Doc comments name ADR 0053 section 1.
- No function in this task signs an event. Later tasks sign with
  `db::insert_event` in the same transaction as the row.

## Implementation Steps

1. Add the migration, the array entry and the schema table.
2. Add the kind, the row type and the functions.
3. Add the two event types, the payloads and the apply arms.
4. Add the tests below.
5. Run the gate, and `cargo test --test migration_tests`.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0053_block_storage_tests.rs`:

- `normalize` gives `"abc-def"` for a GUID of `"  ABC-Def "`, and
  `"https://X.example/a "` trimmed to `"https://X.example/a"` for a URL.
- `insert_feed_block` returns `true` once, and `false` for the same kind and
  normalized value with a different `block_id`.
- `find_feed_block` finds a GUID block with a GUID in upper case, and finds
  a URL block by the second URL of the list. It returns `None` with no match.
- `delete_feed_block` returns `true` once, then `false`.
- An event round trip: a `FeedBlocked` and a `FeedUnblocked` payload
  serialize and parse back through `EventPayload` with the tag of the event
  type.
- Apply on a second database: apply `FeedBlocked` twice gives one row. Apply
  `FeedUnblocked` gives no row. Apply `FeedUnblocked` again does not fail.
- `cargo test --test migration_tests` passes, and a fresh database has the
  table.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0053_block_storage_tests
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

- The next free migration position is not the one after `0037`.
- A migration test expects a fixed number of migrations and the change needs
  more than an update of that number.
- `apply.rs` applies an event through a path that this task cannot reach
  without an edit to `src/api.rs`.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0053-task-001-block-storage-and-events.md`, all of it
- `docs/plans/adr-0053-durable-corrections-phase-plan.md`, decisions 1 to 4
- The files in "Files To Inspect"

Goal:
- Add the `feed_blocks` table, `FeedBlockKind`, `FeedBlock`, the database
  functions, the `FeedBlocked` and `FeedUnblocked` events and their apply
  arms. Nothing calls them yet.

Constraints:
- Follow "Constraints" of the task file exactly.
- Migration `0038_feed_blocks.sql` and the same table in `src/schema.sql`.
- Apply is idempotent. No function signs an event.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `src/api.rs`, `src/verify.rs`, `src/verifiers/`, `src/main.rs`,
  `src/query.rs`, `src/openapi.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0053_block_storage_tests.rs`.
- `cargo test --test migration_tests` passes.
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
