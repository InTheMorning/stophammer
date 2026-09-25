# ADR 0053 Task 005: Stale Submission

Owner: [ADR 0053](../adr/0053-a-correction-stays-applied.md) section 3.
Plan: [phase plan](../plans/adr-0053-durable-corrections-phase-plan.md),
decision 9.

Status: released on 2026-09-25. The operator kept ADR 0053 section 3 as
written, after [the research record](../reviews/last-build-date-behavior-research.md).
The deploy conditions are steps 1 and 2 of
[task 008](adr-0053-task-008-deploy.md).

## Goal

A submission whose `lastBuildDate` is earlier than the stored value does not
change the record.

## Files To Inspect

- `src/api.rs`: `handle_ingest_feed`, the write phase from the ADR 0051
  classification to step 3b
- `src/db.rs`: `get_feed`, the `last_build_date` field of `Feed`
- `src/ingest.rs`: `IngestFeedData::last_build_date`
- `tests/adr0051_source_url_tests.rs`: the helpers

## Files Likely To Change

- `src/api.rs`: `handle_ingest_feed`
- `docs/API.md`: the ingest reasons
- `tests/adr0053_stale_tests.rs`, new

## Do Not Touch

- `classify_submission`, `src/verify.rs`, `src/main.rs`, `src/query.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- After the classification gives `Update`, and before step 3b, read the
  stored `last_build_date` of the record. When the stored value and
  `feed_data.last_build_date` are both present and the submitted value is
  smaller, answer `accepted: false`, `no_change: false`,
  `reason: Some("stale_submission")`, no events, no warnings,
  `source_url: None`. Write nothing.
- `force_reingest` does not skip the rule. An equal value passes.
- Log it with `tracing::info!` and the fields `feed_guid`, `canonical_url`,
  `stored_last_build_date` and `submitted_last_build_date`.
- A short comment names ADR 0053 section 3.
- `docs/API.md` adds `stale_submission` to the ingest reasons.

## Implementation Steps

1. Add the check.
2. Update `docs/API.md`.
3. Add the tests below.
4. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0053_stale_tests.rs`, through the router:

- A feed accepted with `last_build_date` `T`. Then a submission from the
  source URL with `T - 60`, a new title and `force_reingest: true`: the
  response is `stale_submission`, and the title does not change.
- The same with `T`: accepted.
- The same with `T + 60`: accepted, and the stored value becomes `T + 60`.
- A submission with no `last_build_date`: accepted.
- A record with no stored `last_build_date`, then a submission with a value:
  accepted.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0053_stale_tests
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

- An existing test fails because it sends an older `last_build_date` to the
  same feed. List each one, and do not change it.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0053-task-005-stale-submission.md`, all of it
- The files in "Files To Inspect"

Goal:
- In the ingest write phase, after the ADR 0051 classification gives
  `Update`, a submitted `last_build_date` earlier than the stored value
  answers `stale_submission` and writes nothing. `force_reingest` does not
  skip the rule.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `classify_submission`, `src/verify.rs`, `src/main.rs`, `src/query.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0053_stale_tests.rs`.
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
