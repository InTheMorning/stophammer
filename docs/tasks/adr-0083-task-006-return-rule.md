# ADR 0083 Task 006: A Return Needs A Tag At Each End

Owner: [ADR 0083](../adr/0083-a-feed-move-refuses-a-loop.md), each section.

Repository: `stophammer`. The operator commits. This task replaces the earlier
rule that tasks 002 to 004 built. Candidate `v0.18.1-rc.1` holds that earlier
rule and migration 0059, so migration 0059 does not change.

## Goal

`move_target` applies ADR 0083 §2, §5 and §6. A move stores its history in the
ingest transaction. A relocation keeps the left URLs. The documents give the new
rule.

## Context

`move_target` in `src/api.rs` returns `MoveDecision`. Its mirror branch finds the
trigger, then applies the earlier rules 1 to 4. Replace those rules. Its update
branch gives the permanent redirect.

Step 11a of `handle_ingest_feed` calls `db::add_left_url` and
`db::remove_left_url` after `ingest_transaction` commits. A crash between the
two loses the row. These writes move into the transaction (§4).

`relocate_feed` calls `db::clear_left_urls`. That changes (§4). `delete_feed_sql`
keeps its delete and also deletes the new table.

## Files To Inspect

- `src/api.rs`: `MoveDecision`, `move_target`, its three callers, step 11a, and
  `relocate_feed`. Use a search. Do not read the whole file.
- `src/db.rs`: `ingest_transaction`, `delete_feed_sql`, the `feed_left_urls`
  functions, `MIGRATIONS`
- `tests/adr0083_move_refusal_tests.rs`, `tests/adr0083_left_url_tests.rs`
- `docs/API.md`, `docs/operations.md`, `docs/schema-reference.md`: each
  `ADR 0083` text
- `AGENTS.md`, "New Database Migration"

## Files Likely To Change

- New: `migrations/0060_feed_move_state.sql`
- `src/schema.sql`, `src/db.rs`, `src/api.rs`
- `tests/fixtures/migration_sha256.txt`, `tests/migration_tests.rs`
- `tests/adr0083_move_refusal_tests.rs`, `tests/adr0083_left_url_tests.rs`
- `docs/API.md`, `docs/operations.md`, `docs/schema-reference.md`

## Constraints

- **The table.** `feed_move_state` with `feed_guid TEXT PRIMARY KEY`,
  `arrival_tag TEXT`, `return_count INTEGER NOT NULL DEFAULT 0` and
  `last_return_at INTEGER`, as `STRICT`. A record with no row has no arrival tag
  and a count of zero.
- **The return test.** In the mirror branch, after the trigger:
  - If no left URL is an own URL, the decision is a move.
  - Else the decision is a move only when the trigger is `new-feed-url`, the tag
    of the body is an own URL, the declared tag is not the arrival tag, and no
    cooldown is active. Otherwise it is a refusal.
- **The cooldown.** One pure function takes the count, the last return time and
  the time now, and gives the end of the wait or nothing. A count of zero gives
  nothing. With a count `c` of 1 or more, the wait is `3600 * 2^(c - 1)` seconds,
  with a limit of 604800. When 2592000 seconds or more passed since the last
  return, the count counts as zero. The function must not overflow on a large
  count. Give it unit tests. Use `db::unix_now()` for the time now.
- **A return adds to the count.** A return sets the count to the effective count
  plus one, and sets `last_return_at`. A move to a URL that is not a left URL
  changes neither value.
- **The redirect.** In the update branch, a redirect whose target is not a left
  URL moves as today. A target that is a left URL with an active cooldown gives a
  new decision value. The caller applies the body as an update, keeps the source
  URL and pushes the warning. A target that is a left URL with no cooldown moves,
  and it counts as a return. A redirect uses no tag test (§6).
- **The warning.** The text for each refusal and each waiting redirect is
  `move to <URL> refused: the record left that URL (ADR 0083)`. `Refused` no longer
  holds `held_url`. The `tracing::warn!` gives the GUID and the URL as structured
  fields.
- **The transaction.** Add one parameter to `ingest_transaction`, an
  `Option` of a small struct with the old URL, the new URL, the arrival tag, and
  the return values. The function writes the left URL rows and the state row
  before its commit. Remove the two calls from step 11a.
- **The arrival tag.** A move stores the tag of the body at the new URL, or null.
  An update or a new-feed ingest whose tag differs from the arrival tag clears
  the arrival tag, at step 11a.
- **The relocation.** `relocate_feed` reads the stored URL before its update. Then
  it adds that URL, removes the new URL, clears the arrival tag and sets the count
  to zero. Keep the name-free behavior for a relocation to the same URL: that URL
  is not a left URL after the call. `clear_left_urls` stays for `delete_feed_sql`
  only if it is used, otherwise delete it.
- **The migration.** Use the next sequence number. Migration 0059 does not change.
- **No event.** No event type or payload changes. `IngestResponse`,
  `src/openapi.rs` and `tests/fixtures/v1-schema-fields.json` do not change.
- **Tests.** Change the expected results of the existing ADR 0083 tests to the
  table in ADR 0083 "Guards". Add each row that has no test. A test moves the
  cooldown forward with a direct update of `last_return_at`. Do not change a test
  of a different ADR.
- **Documents.** Each text names ADR 0083, and no text creates a rule. Change the
  sequence in `docs/API.md`, the warning, the two ways to end a refusal in
  `docs/operations.md`, and the tables in `docs/schema-reference.md`. State that a
  return needs a tag at each end, and that a return can wait.
- **Git.** The agent runs no git write command.

## Do Not Touch

- `migrations/0059_feed_left_urls.sql` and each other committed migration
- `docs/adr/`, `AGENTS.md`
- `stophammer-crawler/`, `stophammer-parser/`

## Implementation Steps

1. Write the migration, the schema, and the state functions in `src/db.rs`.
2. Change `ingest_transaction`, `relocate_feed` and `delete_feed_sql`.
3. Change `move_target`, its callers and step 11a.
4. Change and add the tests.
5. Change the three documents.
6. Run the gate.

## Acceptance Criteria

Mechanical:

- Each row of ADR 0083 "Guards" has a test that asserts the move count and the
  final URL, and it passes.
- `cargo test`, `cargo clippy --all-targets -- -D warnings` and
  `cargo fmt -- --check` pass.
- No assertion of a test outside the ADR 0083 test files and
  `tests/migration_tests.rs` changes.
- `grep -n 'the body names' src docs/API.md docs/operations.md -r` gives no result.

This task has no visual criterion.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0083_move_refusal_tests
cargo test --test adr0083_left_url_tests
cargo test --test migration_tests
cargo test --test adr0023_migration_hash_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

Keep the build under `target/`. Do not set `CARGO_TARGET_DIR` to a folder in
`/tmp`, because `/tmp` is a memory volume of 32 GB.

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- A result differs from the table in ADR 0083 "Guards".
- A test of a different ADR fails, and the fix needs a change to its assertion.
- The rule needs a new event, a new response field or a crawler change.

## Expected Final Report

1. files changed and created
2. tests run, with the result of the gate
3. behavior changed
4. deviations from this task
5. unresolved concerns
