# ADR 0050 Review Checklist

Use this after each task in the
[phase plan](../plans/adr-0050-feed-revalidation-phase-plan.md). Review the
diff against [ADR 0050](../adr/0050-the-crawler-revalidates-a-feed.md), the
plan and the task packet.

## Mechanical Gates

Run these in the repository that the task changes.

- [ ] `cargo build` green
- [ ] `cargo test` green
- [ ] `cargo clippy --all-targets -- -D warnings` silent
- [ ] `cargo fmt -- --check` silent
- [ ] No test sends a request to an external host
- [ ] A task in `stophammer-crawler`: the commit message names ADR 0050

## Visual Gate

Keep this apart from the mechanical list. If the check cannot run, report the
gate as open, not as met.

- [ ] Task 005 only: `docker compose config` on the VPS shows the
      `--feed-cache` argument for `gossip`, `import` and `import-wavlake`, and
      `FEED_CACHE_DB` in the env of `stophammer-crawler`

## Drift

- [ ] The task changes only the files that its packet names, or the report
      gives the reason for each other file
- [ ] The node is unchanged (ADR 0050 decision 7)
- [ ] No lock on the cache is held across an HTTP await
- [ ] A `fetch_error` writes no row
- [ ] The `ndjson` mode does not use the cache
- [ ] No opportunistic cleanup outside the task

## Invariants From The ADR

- [ ] The row holds `ETag` and `Last-Modified` unchanged, with a weak prefix
      kept
- [ ] A `304` in a normal crawl sends no ingest, unless the row has no node
      answer or an ingest error
- [ ] A `304` in a `--force` pass submits the kept body with its kept hash
- [ ] A `304` with no kept body gives one unconditional GET
- [ ] `--no-revalidate` sends no conditional header and still writes rows
- [ ] The batch pass reports the `200`, `304` and `429` counts

## Merge Decision

- [ ] Pass or fail
- [ ] Required fixes
- [ ] Optional improvements
- [ ] Can this task merge
- [ ] Does the next task packet need a change
