# ADR 0053 Task 004: Environment Seed

Owner: [ADR 0053](../adr/0053-a-correction-stays-applied.md) section 2.
Plan: [phase plan](../plans/adr-0053-durable-corrections-phase-plan.md),
decision 8.

## Goal

The environment blocklist becomes block rows with signed events at primary
startup. The `feed_blocklist` verifier is deleted.

## Files To Inspect

- `src/verifiers/feed_blocklist.rs`: `read_csv_env` and the GUID
  normalization
- `src/verifiers/mod.rs`, `src/verify.rs`: `build_chain`, `ChainSpec::DEFAULT`
- `src/main.rs`: `run_primary`, where the database and the signer are ready
- `src/lib.rs`
- `src/db.rs`: `insert_feed_block`, `get_feed_block_by_pair`, `insert_event`
- `AGENTS.md`: the module table
- `docs/operations.md`, `docs/verifier-guide.md`: `BLOCKED_FEED_GUIDS`,
  `BLOCKED_FEED_URLS`, `VERIFIER_CHAIN`
- Each test that names `feed_blocklist` or `FeedBlocklistVerifier`

## Files Likely To Change

- `src/blocks.rs`, new
- `src/lib.rs`: `pub mod blocks;`
- `src/main.rs`
- `src/verify.rs`, `src/verifiers/mod.rs`
- `src/verifiers/feed_blocklist.rs`, deleted
- `AGENTS.md`: one row in the module table
- `docs/operations.md`, `docs/verifier-guide.md`
- Each test that names the verifier
- `tests/adr0053_seed_tests.rs`, new

## Do Not Touch

- `src/api.rs`, `src/query.rs`, `src/openapi.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- `src/blocks.rs` has a `//!` module doc that names ADR 0053 section 2. It
  holds:
  - `pub fn blocks_from_env() -> Vec<(db::FeedBlockKind, String)>`, with the
    parser of `read_csv_env`.
  - `pub fn seed_blocks(conn: &mut Connection, entries: &[(FeedBlockKind,
    String)], signer: &NodeSigner, now: i64) -> Result<usize, DbError>`. It
    uses one transaction. For each entry with no row, it inserts a block with
    a new UUID v4 `block_id` and the reason `seeded from environment`. It
    signs one `FeedBlocked` event for each new block. It returns the number of new rows. It never
    deletes a row.
- `main.rs`, primary only, after the database opens and before the router
  starts: call `seed_blocks` with `blocks_from_env()`. Log the count with
  `tracing::info!`. A community node does not seed.
- Delete `src/verifiers/feed_blocklist.rs` and its `mod` line.
- `build_chain` skips the name `feed_blocklist`, with a `tracing::warn!`
  that names ADR 0053 section 2. `ChainSpec::DEFAULT` becomes
  `content_hash,medium_music,feed_guid,v4v_payment,enclosure_type`.
- The panic text for an unknown verifier no longer lists `feed_blocklist`.
- A test of the deleted verifier moves to `tests/adr0053_seed_tests.rs` as a
  test of `blocks_from_env` or of the ingest check. Name each moved test in
  the report.
- `AGENTS.md` module table gets `blocks`: "Durable feed blocks: environment
  seed (ADR 0053)".
- `docs/operations.md` and `docs/verifier-guide.md` state three facts:
  - The environment lists seed block rows at startup.
  - A value removed from the environment removes no block.
  - `feed_blocklist` is not a chain name.

## Implementation Steps

1. Add `src/blocks.rs` and register it.
2. Call the seed in `main.rs`.
3. Delete the verifier and change `build_chain` and `ChainSpec::DEFAULT`.
4. Move or rewrite each test that used the verifier.
5. Update `AGENTS.md` and the two documents.
6. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0053_seed_tests.rs`:

- `seed_blocks` with two GUIDs and one URL writes three rows and three
  `feed_blocked` events. A second call with the same entries writes nothing
  and returns `0`.
- `seed_blocks` with an entry whose pair a row holds under a different reason
  writes nothing for it.
- A block that the seed wrote stays after a second call with an empty list.
- `build_chain` with `feed_blocklist` in the names returns a chain without
  that name. It does not panic.
- `ChainSpec::DEFAULT` has no `feed_blocklist`.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0053_seed_tests
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

Add a list of each moved or removed test.

## Escalation Triggers

Stop and report when:

- A test of the deleted verifier checks a behavior that the ingest check of
  task 002 does not give.
- `main.rs` has no place where the signer and the writer are both ready
  before the router starts.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0053-task-004-environment-seed.md`, all of it
- `docs/plans/adr-0053-durable-corrections-phase-plan.md`, decision 8
- The files in "Files To Inspect"

Goal:
- A new module `src/blocks.rs` seeds block rows with signed events from
  `BLOCKED_FEED_GUIDS` and `BLOCKED_FEED_URLS` at primary startup. The
  `feed_blocklist` verifier is deleted, and `build_chain` skips that name
  with a warning.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `src/api.rs`, `src/query.rs`, `src/openapi.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0053_seed_tests.rs`.
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
