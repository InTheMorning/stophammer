# ADR 0073 Task 001: The Node Stores Both Forms Of A Split

Owner: [ADR 0073](../adr/0073-a-value-time-split-keeps-both-forms.md) §2 and
§3. The phase plan is
[the compression, splits and GUID plan](../plans/compression-splits-guid-phase-plan.md).

Repository: `stophammer`. The operator commits.

## Goal

The node accepts, stores, signs and applies each value time split with the
stated values of ADR 0073 §2. This applies to the remote form and the local form. It accepts
the ingest of an old crawler, and applies an old event.

## Files To Inspect

- `src/schema.sql`: `value_time_splits`, `payment_routes`
- `migrations/0052_copy_item_facts.sql`, and the `MIGRATIONS` array in
  `src/db.rs`
- `src/model.rs`: `ValueTimeSplit`, `PaymentRoute`, `RouteType`
- `src/ingest.rs`: `IngestValueTimeSplit`, `IngestPaymentRoute`
- `src/event.rs`: `TrackUpsertedPayload`
- `src/api.rs`: the mapping from `track_data.value_time_splits` to
  `model::ValueTimeSplit`, and the track PATCH handler that reads
  `get_value_time_splits_for_feed_track`
- `src/apply.rs`: the `TrackUpserted` arm
- `src/db.rs`: `get_value_time_splits_for_track`,
  `get_value_time_splits_for_feed_track`, `replace_value_time_splits`,
  `replace_value_time_splits_for_feed_track`, the split insert in the ingest
  transaction, and each `DELETE FROM value_time_splits`
- `src/quality.rs`: the `has_vts` query
- `docs/adr/0046-migration-versions-are-array-positions.md`

## Files Likely To Change

- `migrations/0053_value_time_split_forms.sql`, new. Use the next free number
  if 0053 is taken.
- `src/schema.sql`, `src/db.rs`, `src/model.rs`, `src/ingest.rs`, `src/api.rs`
- `src/query.rs`: only the mapping to `VtsResponse`, so that it compiles. Task
  002 changes the response.
- `tests/migration_tests.rs`: each recorded version moves by one
- `tests/adr0073_split_store_tests.rs`, new

## Constraints

- **The table.** The migration rebuilds `value_time_splits` with these columns.
  It copies each old row with `form` `remote`, `position` 0 and a null
  `remote_percentage`. It drops the old `split` column.

  ```sql
  id                     INTEGER PRIMARY KEY,
  source_feed_guid       TEXT,
  source_track_guid      TEXT NOT NULL,
  position               INTEGER NOT NULL DEFAULT 0,
  form                   TEXT NOT NULL CHECK(form IN ('remote','local')),
  start_time_secs        REAL NOT NULL,
  duration_secs          REAL,
  remote_feed_guid       TEXT,
  remote_item_guid       TEXT,
  remote_start_time_secs REAL,
  remote_percentage      REAL,
  created_at             INTEGER NOT NULL,
  CHECK(form = 'local' OR remote_feed_guid IS NOT NULL),
  FOREIGN KEY (source_feed_guid, source_track_guid) REFERENCES tracks(feed_guid, track_guid)
  ```

  Keep the two indexes. The table stays `STRICT`.
- **The child table.** `value_time_split_recipients`, `STRICT`, with `id`,
  `split_id INTEGER NOT NULL REFERENCES value_time_splits(id) ON DELETE
  CASCADE`, `position INTEGER NOT NULL`, and the recipient columns of
  `payment_routes` with the same types and checks: `recipient_name`,
  `route_type`, `address`, `custom_key`, `custom_value`, `split`, `fee`. Add an
  index on `split_id`.
- **Deletes.** Each SQL that deletes from `value_time_splits` first deletes
  the child rows of those splits. Do not depend on the cascade only. A test
  database can run with foreign keys off.
- **The model.** `model::ValueTimeSplit` has these fields. Each new field has
  `#[serde(default)]`, so an old event applies:
  - `position: i64`
  - `form: ValueTimeSplitForm`, a new enum `Remote` and `Local`, with
    `#[serde(rename_all = "snake_case")]` and the default `Remote`
  - `start_time_secs: f64`, `duration_secs: Option<f64>`
  - `remote_feed_guid: Option<String>`, `remote_item_guid: Option<String>`
  - `remote_start_time_secs: Option<f64>`, `remote_percentage: Option<f64>`,
    each with `skip_serializing_if = "Option::is_none"`
  - `recipients: Vec<ValueTimeSplitRecipient>`, with
    `skip_serializing_if = "Vec::is_empty"`. The new struct has
    `recipient_name`, `route_type`, `address`, `custom_key`, `custom_value`,
    `split` and `fee`, with the types of `PaymentRoute`.
  - Remove `split`. An old event that holds `split` still applies, because the
    model does not deny unknown fields.
- **The ingest DTO.** `ingest::IngestValueTimeSplit` has the same new fields,
  with `#[serde(default)]` on `form`, `remote_start_time_secs`,
  `remote_percentage` and `recipients`. `recipients` uses
  `IngestPaymentRoute`. An ingest from an old crawler, with an integer
  `start_time_secs`, a `split` and no `form`, gives a remote-form split.
- **The ingest check.** The mapping in `src/api.rs` drops a split that breaks
  its form: a remote split with no non-empty `remote_feed_guid`, or a local
  split with no recipient. It logs one `tracing::warn!` with `feed_guid` and
  `track_guid`, and the message names ADR 0073 §2. The other splits of the
  track stay. `position` is the index of the split in the ingest list.
- **No defaults in the store.** Store `remote_percentage` and
  `remote_start_time_secs` as stated, also when the value is outside 0 to 100.
- **The migration watermark.** If `tests/migration_tests.rs` shows that a
  database can record a later version without these columns, add an
  `ensure_value_time_split_forms_schema` repair in `src/db.rs` (ADR 0046).
- **Comments.** Each new item names ADR 0073 and its section.

## Do Not Touch

- `VtsResponse` fields and `TRACK_INCLUDES`. Task 002 changes them.
- The route history and `recipient_set` (ADR 0053)
- `src/quality.rs`: the `has_vts` query keeps its meaning. Change it only if
  it does not compile.
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`, each existing migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the migration, the `MIGRATIONS` entry and the `src/schema.sql` change.
2. Change the model, the ingest DTO and the event payload.
3. Change the read, write and delete functions in `src/db.rs`. A read gives the
   splits in `position` order, with their recipients in `position` order.
4. Change the mapping and the check in `src/api.rs`.
5. Keep `src/query.rs` compiling with the smallest change.
6. Write the tests.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0073_split_store_tests.rs`, or in the
test file that the item names.

- An ingest with the two remote splits of the specification example stores two
  rows with `remote_percentage` 95, and the second row has
  `remote_start_time_secs` 174.
- An ingest with the two local splits of the specification example stores two
  local rows with two recipients each, in source order.
- An ingest with `start_time_secs` 12.5 stores 12.5.
- An ingest with no `remote_percentage` stores null.
- An ingest with `remote_percentage` 150 stores 150.
- A remote split with an empty `remote_feed_guid` is dropped, and the other
  split of the track stays.
- An ingest in the old shape, with `"start_time_secs": 60`, `"split": 50` and
  no `form`, stores a remote row.
- The signed `TrackUpserted` event carries both forms. A second node that
  applies it gives the same rows and recipients.
- An event in the old shape, with `split` and integer times, applies.
- A second ingest that removes a local split deletes its recipient rows.
- The removal of a feed leaves no row in `value_time_split_recipients`.
- `tests/migration_tests.rs`: the migration rebuilds the table and keeps an
  old row as a remote row.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0073_split_store_tests
cargo test --test migration_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A trigger in a migration deletes from `value_time_splits`, and the child
  rows need a new trigger.
- An existing test asserts the `split` field of a value time split.
- The model denies unknown fields, so an old event does not apply.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/AGENTS.md` (the code style, the migration steps and the pre-commit gate)
- `~/build/stophammer/docs/adr/0073-a-value-time-split-keeps-both-forms.md`, §2 and §3
- `~/build/stophammer/docs/adr/0046-migration-versions-are-array-positions.md`
- `~/build/stophammer/docs/tasks/adr-0073-task-001-node-stores-both-forms.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- The node stores, signs and applies both forms of a value time split with the stated values, and accepts an old ingest and an old event.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task, with the exact columns and fields.
- Store stated values. Apply no default or limit in the store.
- Delete child rows explicitly before each delete of a split.
- Stop at each escalation trigger of this task.

Do not touch:
- `VtsResponse` fields, `TRACK_INCLUDES`, the route history, `recipient_set`
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`, each existing migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0073_split_store_tests`
- `cargo test --test migration_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
