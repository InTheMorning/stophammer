# ADR 0056 Task 002: Drop The Proof Tables

Owner: [ADR 0056](../adr/0056-the-public-proof-flow-is-offline.md) §3. Plan:
[ADR 0056 phase plan](../plans/adr-0056-proof-flow-removal-phase-plan.md),
step 4.

Repository: `stophammer`. The build can start at any time. The deploy comes
on 2026-10-02 or after, one week after the ADR 0056 deploy of 2026-09-25. On
2026-09-28 the operator decided to build it before that date, so that release
0.3.0 is ready on that date.

## Goal

The tables `proof_challenges` and `proof_tokens` are gone, and the delete
trigger of a feed does not name them.

## Files To Inspect

- `migrations/0043_list_feed_items.sql`: the last form of
  `trg_feeds_cleanup_before_delete`
- `src/schema.sql`: the two tables and their comment
- `src/db.rs`: `MIGRATIONS`, and each reference to the two tables
- `tests/migration_tests.rs`
- `docs/schema-reference.md`

## Files Likely To Change

- `migrations/0046_drop_proof_tables.sql`, new
- `src/schema.sql`, `src/db.rs`, `docs/schema-reference.md`
- `tests/migration_tests.rs`

## Constraints

- **Before the deploy.** The production copy of 2026-09-26 has no row in
  either table. Before the deploy, the operator confirms on the VPS that each
  table still has no row:

  ```bash
  docker run --rm -v stophammer_primary-data:/node alpine:3.20 sh -c \
    'apk add -q sqlite && sqlite3 -readonly /node/stophammer.db "SELECT COUNT(*) FROM proof_challenges; SELECT COUNT(*) FROM proof_tokens;"'
  ```

  When a count is not 0, stop. The task needs a decision.
- **The migration.** Create the trigger again from its form in migration
  0043, with the two `DELETE` statements of the proof tables removed. Then
  drop the two tables with `DROP TABLE IF EXISTS`. Append the file to
  `MIGRATIONS` as the next position.
- Remove the two tables and their comment from `src/schema.sql`. Remove each
  reference from the code and from `docs/schema-reference.md`.
- Migration 0045 (ADR 0067) comes before this one. This migration is file
  0046, and array position 40 in `MIGRATIONS`. The trigger still has its last
  form from migration 0043.
- ADR 0046: when a test shows that a database can record version 40 with no
  change, add an `ensure_` repair. Else, no repair is needed.

## Acceptance Criteria

Mechanical:

- A migration test opens a database at the version of migration 0045 and
  runs the migrations. The two tables are then gone, and the trigger does not name
  them.
- A test deletes a feed after the migration, and the delete succeeds.
- `cargo test --test migration_tests` passes.
- The gate is green: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0056-task-002-drop-proof-tables.md`, all of it
- `AGENTS.md`, section "New Database Migration"
- The files in "Files To Inspect"

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.
- Write each test with real assertions. Paste the real output of each gate
  command.

At the end, report:
1. files changed
2. the real output of each gate command
3. behavior changed
4. deviations from task
5. unresolved concerns
