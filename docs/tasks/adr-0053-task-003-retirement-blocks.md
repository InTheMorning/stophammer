# ADR 0053 Task 003: Retirement Blocks

Owner: [ADR 0053](../adr/0053-a-correction-stays-applied.md) section 1.
Plan: [phase plan](../plans/adr-0053-durable-corrections-phase-plan.md),
decision 7.

## Goal

`DELETE /v1/feeds/{guid}` blocks the GUID and the source URL in the same
transaction as the retirement. `?block=false` retires with no block, and only
the admin token can send it.

## Files To Inspect

- `src/db.rs`: `delete_feed_with_event`, `insert_event`, the block functions
  (task 001)
- `src/api.rs`: `handle_retire_feed`, `check_admin_or_bearer_with_conn`, and
  the `POST /v1/blocks` handler (task 002)
- Each caller of `delete_feed_with_event` in `src/` and `tests/`

## Files Likely To Change

- `src/db.rs`: `delete_feed_with_event`
- `src/api.rs`: `handle_retire_feed`
- Each caller of `delete_feed_with_event`
- `docs/API.md`: the `DELETE /v1/feeds/{guid}` entry
- `src/openapi.rs`: the `block` query parameter
- `tests/adr0053_retirement_tests.rs`, new

## Do Not Touch

- `handle_remove_track`. A track removal writes no block.
- `src/verify.rs`, `src/verifiers/`, `src/main.rs`, `src/query.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- `delete_feed_with_event` gets a parameter `blocks: &[db::FeedBlock]`. In
  its one transaction, after the delete and the `FeedRetired` event, it calls
  `insert_feed_block` for each block. For each block that it wrote, it signs
  one `FeedBlocked` event with `db::insert_event`. It returns each event with
  its seq and signature, so the handler can fan out all of them.
- `handle_retire_feed` reads `Query<RetireParams { block: Option<bool> }>`.
  The default is `true`.
- `block=false` with no `X-Admin-Token` header answers `403` with a message
  that names ADR 0053 section 1. The check is after the token check.
- With `block=true`, the handler builds two blocks: kind `guid` with the feed
  GUID, and kind `url` with the stored `feed_url`. Each has a new UUID v4
  `block_id`, `blocked_at` now, and the reason `retired`.
- A publisher bearer token retires and blocks, as ADR 0053 section 1 states.
- A short comment names ADR 0053 section 1.

## Implementation Steps

1. Change `delete_feed_with_event` and each caller.
2. Change `handle_retire_feed`.
3. Update `docs/API.md` and `src/openapi.rs`.
4. Add the tests below.
5. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0053_retirement_tests.rs`, through the router:

- An admin retire with no query: the feed is gone, two `feed_blocks` rows
  exist, and `events` has one `feed_retired` and two `feed_blocked` rows.
  An ingest of the same feed then answers `blocked`.
- An admin retire with `?block=false`: the feed is gone, no block row. The
  same ingest is then accepted.
- A retire with a publisher bearer token and `?block=false`: `403`, and the
  feed stays.
- A retire of a feed whose GUID is already blocked: the retire succeeds, and
  only the URL block is new.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0053_retirement_tests
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

- A test fails that expects a retirement to leave no other row.
- The bearer-token path of the retire handler cannot be tested with the
  existing proof test helpers.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0053-task-003-retirement-blocks.md`, all of it
- `docs/plans/adr-0053-durable-corrections-phase-plan.md`, decision 7
- The files in "Files To Inspect"

Goal:
- `DELETE /v1/feeds/{guid}` blocks the GUID and the stored URL in the same
  transaction as the retirement, with one signed `FeedBlocked` event for each
  new block. `?block=false` needs the admin token.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `handle_remove_track`, `src/verify.rs`, `src/verifiers/`, `src/main.rs`,
  `src/query.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0053_retirement_tests.rs`.
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
