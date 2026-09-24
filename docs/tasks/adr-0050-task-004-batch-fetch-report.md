# ADR 0050 Task 004: The Batch Pass Reports Its Fetch Counts

Owner: [ADR 0050](../adr/0050-the-crawler-revalidates-a-feed.md) §6.
Plan: [phase plan](../plans/adr-0050-feed-revalidation-phase-plan.md),
decisions 9 and 10.
Needs: task 003.

**This task changes a different repository.** The work is in
`stophammer-crawler`. Read `stophammer-crawler/AGENTS.md` first. The commit goes
in that repository and names ADR 0050.

## Goal

At the end of a batch pass, the crawler prints how many fetches gave `200`,
`304`, `429` and any other result. The second pass after the deploy answers
the open question of ADR 0050 with this line.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `src/modes/batch.rs`: `run_waves`, `run_wave`, and how a wave collects its
  follow URLs from each report without keeping the report
- `src/crawl.rs`: `CrawlReport.fetch_http_status`
- `src/modes/refresh.rs`: the `refresh: ` prefix of its output lines

## Files Likely To Change

- `src/modes/batch.rs`

## Do Not Touch

- `src/crawl.rs`, `src/feed_cache.rs`
- the follow logic and the wave rules of ADR 0049
- `src/modes/gossip.rs`, `src/modes/import.rs`, `src/modes/refresh.rs`
- `stophammer`, `stophammer-parser`

## Constraints

- A counter type, for example `FetchCounts { ok, not_modified, rate_limited, other }`,
  with a pure method `fn note(&mut self, status: Option<u16>)`: `Some(200)`
  gives `ok`, `Some(304)` gives `not_modified`, `Some(429)` gives
  `rate_limited`, anything else gives `other`.
- Each wave task notes the status of its report before it drops the report,
  under the same rule that keeps only the follow URLs.
- The counts add up over all waves.
- After the last wave, print one line:
  `fetch: ok=N not_modified=N rate_limited=N other=N`.
  Print it always, also when each count is zero.
- The `refresh` mode calls `batch::run_urls`, so it gets the same line. Do not
  change `refresh.rs`.
- Lints: `pedantic = "deny"`. Tests inline.

## Implementation Steps

1. Add the counter type and `note`, with a unit test for each status class.
2. Note each report in `run_wave`, and pass the counts through `run_waves`.
3. Print the line at the end of `run_urls`.
4. Add the stub test below.
5. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- Unit tests prove `note` for `200`, `304`, `429`, `500` and `None`.
- A stub-step test over two waves proves the total counts, for example three
  `200`, two `304` and one `429` across both waves.
- The line is printed after the last wave and not between waves. Prove it
  through the same seam that proves the wave-2 and wave-3 lines, or report
  that the seam does not reach the output.
- The existing batch tests pass with no edit.

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

- the counts cannot pass through `run_waves` without a change to the follow
  rules
- an existing batch test fails

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-crawler` repository. Read its `AGENTS.md` first.

Read:
- `docs/tasks/adr-0050-task-004-batch-fetch-report.md` in the `stophammer`
  repository
- `src/modes/batch.rs`: `run_urls`, `run_waves`, `run_wave`
- `src/crawl.rs`: `CrawlReport.fetch_http_status`

Goal:
- At the end of a batch pass, print
  `fetch: ok=N not_modified=N rate_limited=N other=N`, counted over all waves.

Constraints:
- A counter type with a pure `note(status: Option<u16>)`: 200, 304, 429,
  other.
- Each wave task notes its report's status before it drops the report.
- Print once, after the last wave, always.
- Do not change `refresh.rs`. It gets the line through `run_urls`.

Do not touch:
- `src/crawl.rs`, `src/feed_cache.rs`, the follow logic, the other modes
- `stophammer`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- Unit tests for `note` on 200, 304, 429, 500 and `None`.
- A stub-step test over two waves proves the totals.
- The existing batch tests pass unedited.

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
