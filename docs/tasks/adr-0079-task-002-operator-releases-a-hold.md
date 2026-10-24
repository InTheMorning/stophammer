# ADR 0079 Task 002: The Operator Releases A Hold

Owner: [ADR 0079](../adr/0079-a-retired-guid-returns-only-from-its-source.md)
§3, and the contract consequences of §2. The phase plan is
[the search index and retired source plan](../plans/search-index-and-retired-source-phase-plan.md).

Repository: `stophammer`. The operator commits. This task starts after task
001.

## Goal

`DELETE /v1/retired-source-holds/{guid}` deletes the hold of one GUID, with the
admin token. The contract gives the route and the ingest reason
`retired_source_conflict`.

## Files To Inspect

- `src/api.rs`: `build_router` (the `/v1/blocks/{block_id}` route near line
  2095) and `handle_delete_block`, the pattern to copy
- `src/openapi.rs`: the path `/v1/blocks/{block_id}` (near line 900), and the
  ingest response description that lists the reasons (near line 751)
- `docs/API.md`: `DELETE /v1/blocks/{block_id}` (near line 2456), and the list
  of ingest reasons
- `src/db.rs`: the hold functions of task 001
- `tests/adr0053_blocks_api_tests.rs`, `tests/api_docs_tests.rs`,
  `tests/adr0044_schema_refs_tests.rs`
- `tests/fixtures/v1-schema-fields.json`

## Files Likely To Change

- `src/api.rs`, `src/db.rs`, `src/openapi.rs`, `docs/API.md`
- `tests/fixtures/v1-schema-fields.json`, only when a guard asks for a new entry
- `tests/adr0079_retired_source_tests.rs`: new tests at the end of the file

## Constraints

- **The route.** `DELETE /v1/retired-source-holds/{guid}`. Copy the shape of
  `handle_delete_block`: `check_admin_token` first, the database work in
  `spawn_blocking`, then the response.
  - `204 No Content` when a hold was deleted.
  - `404` when no hold has the GUID.
  - `403` with no admin token or a wrong admin token.
- **The GUID.** Compare in lower case, as task 001 stores it.
- **The primary only.** A community node does not register the route, as for
  `/v1/blocks/{block_id}`.
- **No event.** The release signs no event, because the table is local to the
  primary (ADR 0079 §1).
- **The log.** One `tracing::info!` with `feed_guid` and the held URL, and a
  message that names ADR 0079 §3.
- **The contract.** Add the path to `spec_value()` in `src/openapi.rs` and a
  section to `docs/API.md`, with the shape of `DELETE /v1/blocks/{block_id}`.
  Add `retired_source_conflict` to the list of ingest reasons in both, with
  one sentence: the GUID is held for its last source URL (ADR 0079 §2).
- **Comments.** Each new item names ADR 0079 and its section.

## Do Not Touch

- The hold write, the rule in the `NewFeed` arm and the table. Task 001 owns
  them.
- The fill of earlier retires. Task 003 owns it.
- The routes of `/v1/blocks`
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add the delete function in `src/db.rs`. It returns `true` when it deleted a
   row.
2. Add the handler and the route.
3. Add the contract text in `src/openapi.rs` and `docs/API.md`.
4. Run `cargo run --bin gen_openapi`, then `cargo test`. Add a fixture entry
   only when a guard of ADR 0044 asks for one.
5. Write the tests.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0079_retired_source_tests.rs`.

- ADR 0079 check 6: after the release, a submission of the GUID from a
  different URL admits the feed as a new feed.
- The release gives `204` and deletes the row. A second release gives `404`.
- The route with no admin token, and with a wrong admin token, gives `403`.
  The row stays.
- The release signs no event.
- The OpenAPI document has the path `/v1/retired-source-holds/{guid}` with the
  method `delete`, and the ingest description contains
  `retired_source_conflict`.
- Each test of `tests/api_docs_tests.rs` and `tests/adr0044_schema_refs_tests.rs`
  passes.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo run --bin gen_openapi > /dev/null
cargo test --test adr0079_retired_source_tests
cargo test --test api_docs_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- `handle_delete_block` uses an authentication pattern that differs from
  `check_admin_token`.
- A guard asks for a change to `src/openapi.rs` outside the new path and the
  reason list.
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
- `~/build/stophammer/AGENTS.md` (the code style, the API endpoint steps and the pre-commit gate)
- `~/build/stophammer/docs/adr/0079-a-retired-guid-returns-only-from-its-source.md`, §2 and §3
- `~/build/stophammer/docs/tasks/adr-0079-task-002-operator-releases-a-hold.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- The admin route `DELETE /v1/retired-source-holds/{guid}` deletes one hold, and the contract gives the route and the reason `retired_source_conflict`.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Copy the shape of `handle_delete_block`. Check the admin token first.
- Sign no event. Register the route on the primary only.
- Stop at each escalation trigger of this task.

Do not touch:
- the hold write, the `NewFeed` rule, the table, the fill, the routes of `/v1/blocks`
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo run --bin gen_openapi > /dev/null`
- `cargo test --test adr0079_retired_source_tests`
- `cargo test --test api_docs_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
