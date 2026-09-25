# ADR 0053 Task 007: Crawler Reasons

Owner: [ADR 0053](../adr/0053-a-correction-stays-applied.md) section 1.
Plan: [phase plan](../plans/adr-0053-durable-corrections-phase-plan.md),
decision 12.

**This task changes a different repository.** The work is in
`stophammer-crawler`. The commit goes in that repository and names ADR 0053.

## Goal

The crawler does not store `blocked` or `stale_submission` as the node
answer. Thus an unblock takes effect on the next crawl.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `src/crawl.rs`: `NODE_CONFLICT_REASONS`, `is_node_conflict`, each caller,
  and their tests

## Files Likely To Change

- `src/crawl.rs`

## Do Not Touch

- `src/feed_cache.rs`, `plan_after_response`, `src/modes/`
- `stophammer`, `stophammer-parser`

## Constraints

- Rename `NODE_CONFLICT_REASONS` to `UNCACHED_NODE_REASONS`, and
  `is_node_conflict` to `is_uncached_node_answer`. Update each caller and
  each test name that names the old function.
- The list has five strings: `source_conflict`, `record_conflict`,
  `guid_change_pending`, `blocked` and `stale_submission`.
- The doc comment names stophammer ADR 0051 section 5 and ADR 0053 section 1.
- No other behavior changes.

## Acceptance Criteria

Mechanical:

- The test of the true cases covers the five strings.
- The test of the false cases still covers `[medium_music] absent`,
  `source_conflict_extra` and an `Accepted` outcome. Add `blocked_extra`.
- A stub test: a `200` and a `blocked` answer leave the cache row with a null
  node answer.
- The gate is green.

## Test Commands

```bash
cd stophammer-crawler
cargo build
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

- A caller of `is_node_conflict` exists outside `src/crawl.rs`.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-crawler` repository. Read its `AGENTS.md` first.

Read:
- `docs/tasks/adr-0053-task-007-crawler-uncached-reasons.md` in the
  `stophammer` repository, all of it
- `src/crawl.rs`: `NODE_CONFLICT_REASONS`, `is_node_conflict`, the callers and
  the tests

Goal:
- Rename the list and the function, and add `blocked` and
  `stale_submission`, so the crawler does not store those answers.

Constraints:
- Follow "Constraints" of the task file exactly.
- Lints: `pedantic = "deny"`. Tests inline.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `src/feed_cache.rs`, `plan_after_response`, `src/modes/`
- `stophammer`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" of the task file.
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
