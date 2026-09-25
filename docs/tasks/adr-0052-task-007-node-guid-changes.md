# ADR 0052 Task 007: Node GUID Changes

Owner: [ADR 0052](../adr/0052-a-source-moves-its-own-feed.md) sections 4 and
5. Plan: [phase plan 2](../plans/adr-0052-moves-and-guid-changes-phase-plan.md),
decisions 8 to 12.

Repository: `stophammer`. Needs task 006.

## Goal

A new GUID at the source URL is a pending row that each node shows. It applies
at once when it is the UUIDv5 of the source URL, or when the operator approves
it. A return to the old GUID deletes the row.

## Files To Inspect

- `src/api.rs`: `handle_ingest_feed`, and its `SubmissionClass::GuidChange`
  arms in the write phase and in the `NoChange` branch
- `src/api.rs`: `handle_retire_feed` and its block option
- `src/api.rs`: `handle_resolve_copy`, as the pattern of an admin route with
  events and fan-out
- `src/db.rs`: `classify_submission`, `delete_feed_with_event`,
  `list_feed_copies` as the pattern of a replicated table with a local column
- `src/model.rs`: `guid_origin_matches`
- `src/event.rs`, `src/apply.rs`: the ADR 0058 events as the pattern
- `src/query.rs`: `FeedResponse`, `handle_get_feed`, `GET /v1/copies` as the
  pattern of a paginated list
- `src/openapi.rs`, `docs/API.md`

## Files Likely To Change

- `migrations/0042_feed_guid_changes.sql`, new
- `src/schema.sql`, `src/db.rs`, `src/event.rs`, `src/apply.rs`, `src/api.rs`,
  `src/query.rs`, `src/openapi.rs`, `docs/API.md`
- `tests/migration_tests.rs`
- `tests/adr0052_guid_change_tests.rs`, new

## Do Not Touch

- `classify_submission`, `src/verify.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **The table.** `feed_guid_changes`, `STRICT`, primary key `source_url`.
  Columns: `old_guid`, `new_guid`, `first_seen`, `last_seen`, `decision`
  (`approve` or `reject`, nullable), `decision_reason`, `decided_at`.
  `last_seen` is written only by the primary ingest path.
- **The events.**
  - `FeedGuidChangeObserved { source_url, old_guid, new_guid, first_seen }`,
    signed when a row is new or its `new_guid` changes.
  - `FeedGuidChangeDecided { source_url, old_guid, new_guid, decision,
    reason, decided_at }`.
  - `FeedGuidSuperseded { old_guid, new_guid, source_url }`.
  - The apply step of each is idempotent. `FeedGuidSuperseded` writes a row in
    a new table `feed_guid_supersessions (old_guid PRIMARY KEY, new_guid,
    source_url, superseded_at)`.
  - Deleting a pending row replicates too. Use `FeedGuidChangeObserved` with
    `new_guid` equal to `old_guid` as the delete, or add a fourth event. Report
    the choice.
- **The ingest.** In each `GuidChange` arm, with the writer lock:
  1. When `guid_origin_matches(new_guid, source_url)`, run the transition of
     item 3 at once, and answer `accepted: true` with the warning
     `GUID changed from <old> to <new> (ADR 0052 UUIDv5)`.
  2. Otherwise, write or update the row, sign `FeedGuidChangeObserved` when
     the row is new or its `new_guid` changed, update `last_seen`, and answer
     `guid_change_pending` as today. When the row has a `reject` decision for
     the same `new_guid`, answer `guid_change_rejected`.
  3. **The transition**, in one transaction: retire the old record with
     reason `guid_superseded` and **no block**. Sign `FeedGuidSuperseded`.
     Admit the body as a new record at the source URL, as a new feed. Delete
     the pending row.
- **A return.** An ingest in the update case from a source URL that has a
  pending row deletes the row, and replicates the delete.
- **The operator route.** `POST /v1/feeds/{guid}/guid-change`, admin token,
  body `{ decision, reason }`. `{guid}` is the old GUID. `404` when no pending
  row names it. `400` for an empty reason or a wrong decision. Each decision
  signs `FeedGuidChangeDecided`. The node does not keep the body. Thus
  `approve` sets the decision only, and the next submission of the same new
  GUID from the source URL runs the transition. Say this in `docs/API.md`.
- **The public read.**
  - `FeedResponse` gets `pending_guid_change`: null, or `{ new_guid,
    first_seen, last_seen, decision }`.
  - `GET /v1/guid-changes`: each row with no `reject` decision, newest
    `first_seen` first, with the cursor pagination of `GET /v1/copies`.
  - `GET /v1/feeds/{guid}` for a retired GUID that has a supersession row
    answers `404` with `superseded_by`: the new GUID.
- OpenAPI entries and `docs/API.md` for each route and field.
- Comments name ADR 0052 sections 4 and 5.

## Implementation Steps

1. Add the migration, the tables, the events and the apply steps.
2. Add the ingest behavior and the return.
3. Add the operator route.
4. Add the public read.
5. Add the tests below.
6. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0052_guid_change_tests.rs`, through the router:

- The Doerfelverse shape: a record at A with GUID G1 and five items. A
  submission from A with GUID G2, not the UUIDv5 of A, and the same five item
  GUIDs: `guid_change_pending`, the record keeps G1 and its tracks, and
  `GET /v1/guid-changes` lists the row.
- A second submission with G2: no new `FeedGuidChangeObserved` event, and
  `last_seen` changes.
- A submission from A with G1 again deletes the row, and the record keeps its
  track identities.
- A submission from A whose GUID is the UUIDv5 of A: the transition runs at
  once. G1 is retired with reason `guid_superseded`, `GET /v1/blocks` is
  empty, and `GET /v1/feeds/G1` answers `404` with `superseded_by`.
- After the transition, a submission from A with the new GUID is an update.
- `reject` with the admin token: a later submission with the same G2 answers
  `guid_change_rejected`. A submission with G3 opens a new row.
- `approve`: the next submission with G2 runs the transition.
- The routes answer `403` with no admin token.
- A replica that applies the events has the same rows, supersession and
  answers, except `last_seen`.
- The gate is green, with `cargo test --test migration_tests`.

## Test Commands

```bash
cargo build
cargo test --test adr0052_guid_change_tests
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

- The retirement path cannot run with no block and no change to ADR 0053
  behavior for the admin route.
- The admission of the new record needs the NewFeed path with a
  classification that `classify_submission` does not give.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0052-task-007-node-guid-changes.md`, all of it
- `docs/adr/0052-a-source-moves-its-own-feed.md`, sections 4 and 5, and the
  amendment
- `docs/plans/adr-0052-moves-and-guid-changes-phase-plan.md`, decisions 8 to
  12
- The files in "Files To Inspect"

Goal:
- A GUID change at the source URL is pending and public. It applies for the
  UUIDv5 of the source URL, or after operator approval. The transition
  retires the old record with no block.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `classify_submission`, `src/verify.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0052_guid_change_tests.rs`.
- The gate is green.

Test commands:
- The commands of "Test Commands".

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
