# ADR 0049 Review Checklist

Use this after each task in the
[phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md). Review the
diff against [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md),
the plan and the task packet.

## Mechanical Gates

Run these in the repository that the task changes.

- [ ] `cargo build` green
- [ ] `cargo test` green
- [ ] `cargo clippy --all-targets -- -D warnings` silent
- [ ] `cargo fmt -- --check` silent
- [ ] Each deleted test states a Wavlake rule that ADR 0049 supersedes, and the
      task report names it
- [ ] A task with a migration: `cargo test --test migration_tests` green
- [ ] A task with an API change: `cargo run --bin gen_openapi` gives valid JSON,
      and each new response type is in `components.schemas`
- [ ] A task in a crate repository: the commit message names ADR 0049

## Visual Gate

Keep this apart from the mechanical list. If the check cannot run, report the
gate as open, not as met.

- [ ] Task 013 only: `/api` on the node shows each new field and the new route
      with an example

## Drift

- [ ] The task changes only the files that its packet names, or the report
      gives the reason for each other file
- [ ] No `v1` field is renamed or removed
- [ ] No new field is required in an event payload. Each new field has
      `#[serde(default)]`
- [ ] No merged migration is changed
- [ ] No code decides a relationship from the host or the URL layout
- [ ] No code compares a listed GUID with a feed GUID outside
      `db::resolve_listed_feed` (task 005 and later)
- [ ] No stored resolution. The node resolves a link when it reads it
- [ ] No fetch in the node. Only the crawler fetches (ADR 0006)
- [ ] No opportunistic cleanup outside the task
- [ ] `#[expect(..., reason = ...)]`, not `#[allow]`

## Invariants From The ADR

- [ ] The node stores the declared `feedGuid`, `feedUrl` and `rel` unchanged
- [ ] A link that the node cannot resolve is `unresolved`
- [ ] A `rel` conflict gives `role` null and `role_source = "conflict"`
- [ ] `release_artist_source` names the source of `release_artist`
- [ ] `publisher_text` is the `itunes:owner` name
- [ ] A known GUID that arrives through a second URL keeps its stored `feed_url`
- [ ] The crawler follows a link one level only

## Merge Decision

- [ ] Pass or fail
- [ ] Required fixes
- [ ] Optional improvements
- [ ] Can this task merge
- [ ] Does the next task packet need a change
