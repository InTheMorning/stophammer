# ADR 0052 Task 003: A Self-Link Move Is Never No Change

Owner: [ADR 0052](../adr/0052-a-source-moves-its-own-feed.md) section 2.

## Goal

A submission from the declared self link of a record moves the record. This
is also true when the body did not change since the last submission from that
URL.

## The Defect

The move of task 001 runs only in the write phase of `handle_ingest_feed`.
The read phase runs the verifier chain first. The content-hash verifier
answers `no_change` when the node crawl cache holds the same hash for the
URL. The `ReadPhaseOutcome::NoChange` branch then records an observation and
a copy, and returns. It does not move the record.

The node crawl cache holds a music-form Wavlake URL when the node accepted it
before ADR 0051. On 2026-09-25 the operator moved about 1,400 records with
`--force` to avoid this path.

## Files To Inspect

- `src/api.rs`: `handle_ingest_feed`, the read phase, the
  `ReadPhaseOutcome::NoChange` branch, and the `SubmissionClass::Mirror` arm
  with `self_link_move`
- `src/verifiers/content_hash.rs`: the use of `force_reingest`
- `src/db.rs`: `classify_submission`, `get_declared_self_url`,
  `upsert_feed_crawl_cache`
- `tests/adr0052_self_link_tests.rs`

## Files Likely To Change

- `src/api.rs`
- `tests/adr0052_self_link_tests.rs`

## Do Not Touch

- `src/verify.rs`, `src/verifiers/`, `classify_submission`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- In the read phase, after the block check and before the chain runs, find
  whether the submission is a self-link move. Use the same test as the
  `Mirror` arm: `classify_submission` gives `Mirror`, and
  `get_declared_self_url` equals `req.source_url` or `req.canonical_url`.
  Use the reader connection.
- When it is a move, set `force_reingest` to true on the request before the
  chain runs. Bind the request as `mut` for this. The other verifiers still
  run. A comment names ADR 0052 section 2 and this task.
- Put the move test in one private function. Call it from the read phase and
  from the `Mirror` arm, so the two places cannot disagree.
- The write phase classifies again under the writer lock, as it does today.
  A record that changed between the two phases takes the result of the write
  phase.
- No other behavior changes. A mirror that is not a move still reaches
  `no_change` when its hash matches.

## Implementation Steps

1. Add the private function for the move test.
2. Use it in the read phase and in the `Mirror` arm.
3. Add the tests below.
4. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0052_self_link_tests.rs`, through the router:

- A record at URL A declares the self link B. The node crawl cache holds B
  with hash H, seeded with `db::upsert_feed_crawl_cache`. A submission from B
  with hash H and `force_reingest: false` moves the record to B, and the
  response is `accepted: true` with the move warning.
- A mirror submission from C, not the self link, with a cached hash: the
  response is `no_change` as before, and the record does not move.
- The existing self-link tests pass unchanged.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0052_self_link_tests
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

- `force_reingest` changes a later step of the write phase in a way that a
  move must not take. List the step.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0052-task-003-self-link-no-change.md`, all of it
- `docs/adr/0052-a-source-moves-its-own-feed.md`, section 2
- The files in "Files To Inspect"

Goal:
- A submission from the declared self link of a record moves the record,
  also when the node would answer `no_change`.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `src/verify.rs`, `src/verifiers/`, `classify_submission`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in `tests/adr0052_self_link_tests.rs`.
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
