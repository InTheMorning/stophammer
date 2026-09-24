# ADR 0050 Task 003: The Flags And The Wiring

Owner: [ADR 0050](../adr/0050-the-crawler-revalidates-a-feed.md) §1 and §5.
Plan: [phase plan](../plans/adr-0050-feed-revalidation-phase-plan.md),
decisions 6 and 7.
Needs: task 002.

**This task changes a different repository.** The work is in
`stophammer-crawler`. Read `stophammer-crawler/AGENTS.md` first. The commit goes
in that repository and names ADR 0050.

## Goal

The `feed`, `refresh`, `gossip` and `import` modes open the cache and pass it
to each fetch. Two flags control it. This is the task that changes behavior.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `src/main.rs`: the `Cli` struct with the global `--force`, and each mode's
  `--skip-db` flag
- `src/modes/batch.rs`: `run_urls`, the `step` closure, `crawl_feed_with_retries`
- `src/modes/gossip.rs`: `run`, and the three `crawl_feed_report` calls
- `src/modes/import.rs`: `run`, and its `crawl_feed_report` call
- `src/modes/refresh.rs`: how it calls `batch::run_urls`
- `README.md`: the option tables of each mode

## Files Likely To Change

- `src/main.rs`, `src/modes/batch.rs`, `src/modes/gossip.rs`,
  `src/modes/import.rs`, `src/modes/refresh.rs`
- `README.md`

## Do Not Touch

- `src/crawl.rs` and `src/feed_cache.rs`, other than a `pub(crate)` change
- `src/modes/ndjson.rs`. It does not fetch and does not use the cache
- the follow logic of ADR 0049
- `stophammer`, `stophammer-parser`

## Constraints

- `--feed-cache <path>`, `env = "FEED_CACHE_DB"`, default `./feed_cache.db`,
  on the `feed`, `refresh`, `gossip` and `import` modes. The `feed` and
  `refresh` modes have no `--skip-db`, and they get only this flag.
- `--no-revalidate`, global like `--force`. It calls
  `CrawlConfig::with_revalidate(false)` in each mode.
- Each mode opens the cache once, as `Arc<std::sync::Mutex<FeedCacheDb>>`,
  and passes `Some(&cache)` to each `crawl_feed_report` call. `batch::run_urls`
  keeps its signature and gains the cache through the same route as its
  config: read `FEED_CACHE_DB` from the environment inside `run_urls`, or add
  a parameter to `run` and `run_urls`. Choose the smaller change, and report
  it. The `refresh` mode must pass the same path.
- The `import` mode's dry run opens no cache and passes `None`.
- `README.md`: add `--feed-cache` and `FEED_CACHE_DB` to the option table of
  each of the four modes, and `--no-revalidate` beside `--force`. One
  paragraph says what the cache file holds and when a `304` skips the ingest.
- Lints: `pedantic = "deny"`. Tests inline.

## Implementation Steps

1. Add the two flags to `src/main.rs`, and pass them into each mode.
2. Open the cache in each mode and thread it to the fetch calls.
3. Update `README.md`.
4. Add the tests below.
5. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- Tests with `clap` introspection, as task 012 of ADR 0049 did. Each of the
  four modes has `--feed-cache` with env `FEED_CACHE_DB` and default
  `./feed_cache.db`. `--no-revalidate` is global.
- A test proves that `batch::run_urls`, through its stub step seam, passes a
  cache to the step when one is configured. If the seam hides the cache, prove
  it one level down, and say where.
- The existing tests of the four modes pass with no edit, other than a new
  argument at a call site.
- `README.md` names the flags in each table.

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
6. how the cache reaches `batch::run_urls`

## Escalation Triggers

Stop and report when:

- `batch::run_urls` needs a signature change that `refresh.rs` cannot follow
- a mode holds the cache lock across an await
- an existing test fails for a reason other than a new argument

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-crawler` repository. Read its `AGENTS.md` first.

Read:
- `docs/tasks/adr-0050-task-003-flags-and-wiring.md` in the `stophammer`
  repository, the section "Constraints"
- `src/main.rs`, `src/modes/batch.rs`, `src/modes/gossip.rs`,
  `src/modes/import.rs`, `src/modes/refresh.rs`
- `README.md`, the option tables

Goal:
- Add `--feed-cache` and `--no-revalidate`, and pass the cache to each fetch in
  the `feed`, `refresh`, `gossip` and `import` modes.

Constraints:
- `--feed-cache <path>`, env `FEED_CACHE_DB`, default `./feed_cache.db`, on
  the four modes. `--no-revalidate` global.
- One `Arc<Mutex<FeedCacheDb>>` per mode, `Some(&cache)` at each
  `crawl_feed_report` call. The import dry run passes `None`.
- Keep the signature of `batch::run_urls` if possible. Report how the cache
  reaches it.
- Update the README tables and add one paragraph on the cache.

Do not touch:
- `src/crawl.rs`, `src/feed_cache.rs` (except `pub(crate)`),
  `src/modes/ndjson.rs`, the ADR 0049 follow logic
- `stophammer`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- `clap` tests prove the flags, their env and defaults on each mode, and that
  `--no-revalidate` is global.
- A test proves that the batch step gets the cache when configured.
- The existing mode tests pass, other than a new argument at a call site.

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
