# ADR 0053 Review Checklist

Use this after each task in the
[phase plan](../plans/adr-0053-durable-corrections-phase-plan.md). Review the
diff against [ADR 0053](../adr/0053-a-correction-stays-applied.md), the plan
and the task packet.

## Mechanical Gates

Run these in the repository that the task changes.

- [ ] `cargo build` green
- [ ] `cargo test` green
- [ ] `cargo clippy --all-targets -- -D warnings` silent
- [ ] `cargo fmt -- --check` silent
- [ ] `cargo test --test migration_tests` green, for task 001
- [ ] A task in `stophammer-crawler`: the commit message names ADR 0053

## Manual Gate

Keep this apart from the mechanical list. If the check cannot run, report the
gate as open, not as met.

- [ ] Task 008 only: each community node runs the new version before the
      primary
- [ ] Task 008 only: the seed count equals the count of the environment
      values

## Drift

- [ ] The task changes only the files that its packet names, or the report
      gives the reason for each other file
- [ ] Each write of a block row and its signed event is in one transaction
- [ ] The token check of ADR 0051 stays first on ingest
- [ ] No hold or delay of a payment change
- [ ] No opportunistic cleanup outside the task

## Invariants From The ADR

- [ ] A block removes nothing by itself. A retirement removes rows
- [ ] Only the admin token removes a block or sends `block=false`
- [ ] A submission never replaces content that has a later `last_build_date`
- [ ] Apply of each new event is idempotent on a community node
- [ ] The route history comes from signed events only

## Merge Decision

- [ ] Pass or fail
- [ ] Required fixes
- [ ] Optional improvements
- [ ] Can this task merge
- [ ] Does the next task packet need a change
