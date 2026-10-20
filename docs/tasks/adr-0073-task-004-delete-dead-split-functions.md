# ADR 0073 Task 004: Delete The Dead Split Functions

Owner: [ADR 0073](../adr/0073-a-value-time-split-keeps-both-forms.md) §2. The
rule "Delete Dead Things" of the operator governance applies. This task adds no
rule.

Repository: `stophammer`. The operator commits.

## The Defect

Two functions in `src/db.rs` have no caller in `src/` and none in `tests/`:

- `get_value_time_splits_for_track`
- `replace_value_time_splits`

Each one reads or writes splits by `source_track_guid` only. The live code uses
`get_value_time_splits_for_feed_track` and
`replace_value_time_splits_for_feed_track`, which match the feed-scoped track
identity of ADR 0040. ADR 0073 task 001 updated the two dead functions, but did
not delete them.

## Goal

The two functions, and the banner comments above them, are deleted. No
behavior changes.

## Files To Inspect

- `src/db.rs`: the two functions, the banner comment
  `// ── get_value_time_splits_for_track ──…` and the banner comment
  `// ── replace_value_time_splits ──…`
- `src/lib.rs`: the exports of `db`

## Files Likely To Change

- `src/db.rs`

## Constraints

- **Check first.** Run `grep -rnw get_value_time_splits_for_track src tests` and
  `grep -rnw replace_value_time_splits src tests`. Each must find only the
  definition and its banner. Stop if a caller exists.
- **Delete.** Delete each function, its doc comment and its banner comment.
- **Helpers.** Delete a helper only when two conditions are true. The compiler
  or clippy reports it as unused after the deletion. No other code calls it.
- **Keep.** `get_value_time_splits_for_feed_track`,
  `replace_value_time_splits_for_feed_track` and the shared helpers of ADR 0073
  task 001 stay.

## Do Not Touch

- Each other function of `src/db.rs`
- `migrations/`, `src/schema.sql`
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/`, `AGENTS.md`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Run the two searches.
2. Delete the two functions with their comments.
3. Run the gate.

## Acceptance Criteria

Mechanical.

- The two searches of "Constraints" find nothing after the change.
- `cargo build` and `cargo clippy --all-targets -- -D warnings` report no
  unused item.
- `cargo test` passes with the same count of tests as before the change.

## Test Commands

```bash
cd ~/build/stophammer
grep -rnw get_value_time_splits_for_track src tests
grep -rnw replace_value_time_splits src tests
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A search finds a caller.
- The deletion makes a helper unused that other code also calls.
- A step needs a change to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results, with the test count before and after
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/AGENTS.md` (the code style and the pre-commit gate)
- `~/build/stophammer/docs/tasks/adr-0073-task-004-delete-dead-split-functions.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- Delete `get_value_time_splits_for_track` and `replace_value_time_splits` from `src/db.rs`, with their comments. No behavior changes.

Constraints:
- Run the two searches first, and stop if a caller exists.
- Delete a helper only if it becomes unused and no other code calls it.
- Stop at each escalation trigger of this task.

Do not touch:
- each other function of `src/db.rs`, `migrations/`, `src/schema.sql`
- `stophammer-crawler/`, `stophammer-parser/`, `docs/`, `AGENTS.md`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is met.

Test commands:
- `grep -rnw get_value_time_splits_for_track src tests`
- `grep -rnw replace_value_time_splits src tests`
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
