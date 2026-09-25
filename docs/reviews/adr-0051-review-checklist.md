# ADR 0051 Review Checklist

Use this after each task in the
[phase plan](../plans/adr-0051-source-url-phase-plan.md). Review the diff
against [ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md),
the plan and the task packet.

## Mechanical Gates

Run these in the repository that the task changes.

- [ ] `cargo build` green
- [ ] `cargo test` green
- [ ] `cargo clippy --all-targets -- -D warnings` silent
- [ ] `cargo fmt -- --check` silent
- [ ] No test sends a request to an external host
- [ ] A task in `stophammer-crawler`: the commit message names ADR 0051

## Manual Gate

Keep this apart from the mechanical list. If the check cannot run, report the
gate as open, not as met.

- [ ] Task 005 only: the operator examined `VERIFIER_CHAIN` on the VPS before
      the deploy
- [ ] Task 005 only: the operator read the list of changed records

## Drift

- [ ] The task changes only the files that its packet names, or the report
      gives the reason for each other file
- [ ] No new event type, no migration and no new table
- [ ] No URL normalization
- [ ] The token check is outside the configurable verifier list
- [ ] The classification happens after the writer lock and before the first
      write
- [ ] Each changed existing test expected content from a URL that is not the
      source URL, and the report names it
- [ ] No opportunistic cleanup outside the task

## Invariants From The ADR

- [ ] Content that changes a held record came through its source URL
- [ ] A rejection writes no row, except the URL observation of the mirror case
- [ ] The mirror case does not write the crawl cache
- [ ] No configuration lets an unauthenticated submission read or write the
      database
- [ ] The rejection envelope is HTTP `200` with `accepted: false`
- [ ] `source_url` is in the OpenAPI document

## Merge Decision

- [ ] Pass or fail
- [ ] Required fixes
- [ ] Optional improvements
- [ ] Can this task merge
- [ ] Does the next task packet need a change
