# ADR 0083 Task 002: The Node Stores The Left URLs

Owner: [ADR 0083](../adr/0083-a-feed-move-refuses-a-loop.md) §4.

Repository: `stophammer`. The operator commits. This task starts after the
operator accepts ADR 0083 in task 001.

## Goal

A new table holds the URLs that a record left by a move. `src/db.rs` gives the
functions to read, add and delete them. A relocation and a delete of a feed
remove the rows of that GUID. No ingest writes the table yet.

## Context

`feeds.declared_self_url` and `feeds.declared_new_feed_url` are local to the
primary, and no event carries them. The new table is the same. `relocate_feed`
in `src/api.rs` already clears the two columns for the same reason.
`delete_feed_sql` in `src/db.rs` deletes the rows of each dependent table,
`feed_copies` among them.

## Files To Inspect

- `src/db.rs`: `MIGRATIONS`, `set_declared_self_url`,
  `clear_declared_new_feed_url`, `delete_feed_sql`
- `src/api.rs`: `relocate_feed`
- `migrations/0058_source_link_label.sql`, for the form
- `tests/migration_tests.rs`
- `AGENTS.md`, "New Database Migration"

## Files Likely To Change

- New: `migrations/0059_feed_left_urls.sql`
- `src/schema.sql`
- `src/db.rs`
- `src/api.rs`: `relocate_feed` only
- `tests/fixtures/migration_sha256.txt`
- `tests/migration_tests.rs`, when it names the migration count
- New: `tests/adr0083_left_url_tests.rs`

## Constraints

- **The table.** `feed_left_urls` with `feed_guid TEXT NOT NULL`,
  `url TEXT NOT NULL` and `PRIMARY KEY (feed_guid, url)`, as `STRICT`. Use
  `CREATE TABLE IF NOT EXISTS`, with a comment that names ADR 0083 §4.
- **The functions.** `get_left_urls(conn, feed_guid) -> Result<Vec<String>, DbError>`,
  `add_left_url(conn, feed_guid, url)` with `INSERT OR IGNORE`,
  `remove_left_url(conn, feed_guid, url)` and
  `clear_left_urls(conn, feed_guid)`. Each has a doc comment that names ADR 0083.
- **The relocation.** `relocate_feed` calls `clear_left_urls` in its transaction,
  beside `clear_declared_new_feed_url`.
- **The delete.** `delete_feed_sql` deletes the rows of the GUID.
- **No event.** No event type and no payload changes.
- **Migration version.** ADR 0046 owns this. When `open_db` reports a skipped
  migration in a test, stop and report.
- **Git.** The agent runs no git write command.

## Do Not Touch

- `move_target` and `handle_ingest_feed` in `src/api.rs`
- `src/openapi.rs`, `docs/`
- Each committed migration

## Implementation Steps

1. Write the migration, and add it to `MIGRATIONS` and `src/schema.sql`.
2. Add the line of the migration to `tests/fixtures/migration_sha256.txt`.
3. Add the four functions.
4. Change `relocate_feed` and `delete_feed_sql`.
5. Write the tests, and run the gate.

## Acceptance Criteria

Mechanical, in `tests/adr0083_left_url_tests.rs`:

- A URL added two times gives one row.
- `remove_left_url` removes one row and keeps the others.
- `PATCH /v1/feeds/{guid}` with a new URL removes each row of that GUID and keeps
  the rows of a different GUID.
- `delete_feed` removes the rows of that GUID.

Mechanical, in the crate:

- `cargo test --test migration_tests` and
  `cargo test --test adr0023_migration_hash_tests` pass.
- `cargo test`, `cargo clippy --all-targets -- -D warnings` and
  `cargo fmt -- --check` pass.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test migration_tests
cargo test --test adr0023_migration_hash_tests
cargo test --test adr0083_left_url_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- A skipped migration appears in a test.
- A test asserts the exact list of tables, and a change to it needs more than the
  new name.

## Expected Final Report

1. files changed and created
2. tests run, with the result of the gate
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/docs/adr/0083-a-feed-move-refuses-a-loop.md`, §4
- `~/build/stophammer/docs/tasks/adr-0083-task-002-left-url-storage.md` (this task)
- `~/build/stophammer/AGENTS.md`, "New Database Migration"
- `~/build/stophammer/migrations/0058_source_link_label.sql`, for the form

Goal:
- The table `feed_left_urls`, four `src/db.rs` functions, and the removal of the rows on a relocation and on a feed delete.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Write each comment in Simplified Technical English, with the shared skill `~/.agents/skills/asd-ste100/SKILL.md`.
- Use the `rust-dev` skill.

Do not touch:
- `move_target` and `handle_ingest_feed`
- `src/openapi.rs`, `docs/`, each committed migration
- git: run no git write command

Acceptance criteria:
- Each mechanical item of the section "Acceptance Criteria" of this task passes.

Test commands:
- the block of the section "Test Commands" of this task

At the end, report:
1. files changed and created
2. tests run
3. behavior changed
4. deviations from this task
5. unresolved concerns
