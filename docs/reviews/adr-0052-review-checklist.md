# ADR 0052 Review Checklist

Use this after each task of an
[ADR 0052](../adr/0052-a-source-moves-its-own-feed.md) phase plan. Review the
diff against the ADR, the plan and the task packet.

## Mechanical Gates

- [ ] `cargo build` green
- [ ] `cargo test` green
- [ ] `cargo clippy --all-targets -- -D warnings` silent
- [ ] `cargo fmt -- --check` silent
- [ ] `cargo test --test migration_tests` green, for a task with a migration

## Manual Gate

Keep this apart from the mechanical list. If the check cannot run, report the
gate as open, not as met.

- [ ] A deploy task: the ADR 0051 repair ran before the deploy
- [ ] A deploy task: the operator read the logged moves of one hour

## Drift

- [ ] The task changes only the files that its packet names, or the report
      gives the reason for each other file
- [ ] No move reads `source_entity_links`
- [ ] No event type is added for the self-link phase
- [ ] No URL normalization
- [ ] No opportunistic cleanup outside the task

## Invariants From The ADR

- [ ] Only an update or new-feed ingest writes `feeds.declared_self_url`
- [ ] A move happens only when the new URL equals the recorded value exactly
- [ ] A conflict case of ADR 0051 never moves
- [ ] A move revokes the proof tokens of the record
- [ ] The source URL changes only by a trigger of ADR 0052 section 1

## Merge Decision

- [ ] Pass or fail
- [ ] Required fixes
- [ ] Optional improvements
- [ ] Can this task merge
- [ ] Does the next task packet need a change
