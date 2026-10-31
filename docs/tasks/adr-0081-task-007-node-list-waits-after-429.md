# ADR 0081 Task 007: The Node List Read Waits After A `429`

Owner: [ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md)
§4, and [ADR 0047](../adr/0047-a-corrective-pass-reads-the-index.md) for the
`refresh` mode. The phase plan is
[the music list plan](../plans/podcastindex-music-list-phase-plan.md).

Repository: `stophammer-crawler`. The operator commits. This task starts
after task 004. It can run at the same time as tasks 005 and 006.

## Goal

A `429` from `GET /v1/feeds/recent` makes the reader wait and send the page
again, at most 5 times. Then a pass does not stop because of the rate limit
of the node. That limit is 50 requests a second with a burst of 100, and the
list read is faster.

## Context

On 2026-10-31, a `--dry-run` pass of the `podcastindex` mode against a local
node read 106 pages in less than one second. A second pass 2 seconds later
got `429 Too Many Requests` on a page and stopped. `refresh` uses the same
reader and has the same risk.

## Files To Inspect

- `src/modes/refresh.rs`: `fetch_feeds_recent_page` and `collect_feeds`
- `src/crawl.rs`: `CrawlOutcome::retry_delay` (near line 205), for the form
  of a wait rule, and the `TcpListener` stub pattern in its tests
- `AGENTS.md` of the crawler

## Files Likely To Change

- `src/modes/refresh.rs`

## Constraints

- **The wait.** `pub(crate) fn wait_after_429(retry_after: Option<&str>,
  attempt: u32) -> Option<Duration>`. It gives `None` when `attempt` is 5 or
  more. Else it gives the `Retry-After` seconds when the header is an
  integer, with a maximum of 30 seconds, or 1 second with no usable header.
  A pure function.
- **The retry.** `fetch_feeds_recent_page` keeps its signature. On a `429`,
  it calls `wait_after_429` with the `Retry-After` header and the attempt
  number, starting at 1. With `Some(wait)`, it prints
  `feeds/recent: 429, waiting {n} s before try {attempt + 1}` to standard
  error, sleeps with `tokio::time::sleep`, and sends the page again. With
  `None`, it gives the error that it gives now.
- **Only `429`.** Each other status keeps its behavior. No wait between
  pages that give `200`.
- **Both modes.** `refresh` and `podcastindex` call the same function, so
  no change in `src/modes/podcastindex.rs` is necessary.
- **Comments.** The doc comment of the wait names ADR 0081 §4 and the
  measurement of the Context.

## Do Not Touch

- `src/modes/podcastindex.rs`, `src/modes/batch.rs`, `src/modes/import.rs`,
  `src/main.rs`, `src/podcastindex_api.rs`, `src/crawl.rs`
- `README.md`, `AGENTS.md`
- `stophammer/`, `stophammer-parser/`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write `wait_after_429` and its tests.
2. Change `fetch_feeds_recent_page`, and write one stub-server test.

## Acceptance Criteria

Mechanical. Each is an inline test in `src/modes/refresh.rs`.

- `wait_after_429(Some("3"), 1)` gives 3 seconds.
  `wait_after_429(Some("120"), 1)` gives 30 seconds.
  `wait_after_429(Some("Wed, 21 Oct 2026 07:28:00 GMT"), 1)` and
  `wait_after_429(None, 1)` give 1 second. `wait_after_429(Some("1"), 5)`
  gives `None`.
- A `TcpListener` stub answers `429` with `Retry-After: 1` one time, then
  `200` with a one-feed page. One call of `fetch_feeds_recent_page` gives
  that page, and the stub saw two requests.
- A stub that answers `429` six times gives an error that holds `429`.
- A stub that answers `500` one time gives an error at once, and the stub saw
  one request.
- The existing `refresh` tests pass with no change to their assertions.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-crawler
cargo build
cargo test refresh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- `fetch_feeds_recent_page` cannot keep its signature.
- The stub test needs a change to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/stophammer-crawler/AGENTS.md`
- `~/build/stophammer/docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md`, §4
- `~/build/stophammer/docs/tasks/adr-0081-task-007-node-list-waits-after-429.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- A `429` from `GET /v1/feeds/recent` makes `fetch_feeds_recent_page` wait and try again, at most 5 times, with the `Retry-After` seconds or 1 second.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- `fetch_feeds_recent_page` keeps its signature. Only a `429` waits.
- Stop at each escalation trigger of this task.

Do not touch:
- `src/modes/podcastindex.rs`, `src/modes/batch.rs`, `src/modes/import.rs`, `src/main.rs`, `src/podcastindex_api.rs`, `src/crawl.rs`, `README.md`, `AGENTS.md`
- `stophammer/`, `stophammer-parser/`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test refresh`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
