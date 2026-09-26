# ADR 0054 Task 002: Crawler Body And Follow Limits

Owner: [ADR 0054](../adr/0054-a-fetch-reaches-only-public-feed-hosts.md)
sections 2 and 3. Plan: [phase plan](../plans/adr-0054-fetch-rule-phase-plan.md),
decisions 5 and 6.

Repository: `stophammer-crawler`. Needs task 001.

## Goal

One fetch reads at most 16 MiB. One source feed adds at most 200 follow URLs
to one wave, and one pass queues at most 50,000 follow URLs.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `stophammer-crawler/src/crawl.rs`: the two `resp.bytes()` calls, and
  `refetch_unconditional`
- `stophammer-crawler/src/follow.rs`: `follow_urls`, `follow_urls_at_level`
- `stophammer-crawler/src/modes/batch.rs`: the waves, `report_follow_urls`
- `stophammer-crawler/src/modes/gossip.rs`: the two follow levels
- `stophammer-crawler/src/url_queue.rs`

## Files Likely To Change

- `stophammer-crawler/src/crawl.rs`, `src/follow.rs`, `src/modes/batch.rs`,
  `src/modes/gossip.rs`

## Do Not Touch

- `stophammer`, `stophammer-parser`
- The per-host delay and the podping cooldowns

## Constraints

- **The body.** `pub const MAX_FEED_BODY_BYTES: usize = 16 * 1024 * 1024;`
  in `src/crawl.rs`. One helper reads a response body with `chunk()` into a
  buffer and stops when the buffer passes the limit. A `Content-Length` over
  the limit fails before the first chunk. Each place that now calls
  `resp.bytes()` for a feed body uses the helper.
- An oversized body is a `FetchError` with `retryable: false` and the reason
  `body_too_large`. The crawler writes no cache row for it.
- **The follow limit of one feed.** `follow_urls` returns at most 200 URLs,
  in the order it finds them today. When it drops URLs, the caller logs the
  source URL and the number dropped, once.
- **The queue limit of one pass.** A batch pass and a gossip run hold at most
  50,000 follow URLs in total. Past the limit, a new follow URL is dropped,
  and the pass logs the total dropped at its end.
- Constants for the two limits, adjacent to the code that uses them, each with a
  comment that names ADR 0054 section 3.
- The crawler uses `eprintln!` for its log today. Use the same.

## Implementation Steps

1. Add the body helper, and use it for each feed body.
2. Add the follow limits.
3. Add the tests below.
4. Run the gate.

## Acceptance Criteria

Mechanical. Tests with a local stub server, and the test switch of task 001:

- A body of 16 MiB and one byte with no `Content-Length`: `body_too_large`,
  and no cache row.
- A `Content-Length` of 17 MiB: `body_too_large`, and the stub sends no
  more than the headers and one chunk before the crawler stops.
- A body of 1 MiB: accepted as before.
- A publisher feed that lists 201 music feeds: `follow_urls` returns 200.
- A pass whose follow queue reaches 50,000 drops the next URL. A unit test
  with a small limit value is enough, if the limit is a parameter of the
  queue.
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

- A publisher feed in the index lists more than 200 music feeds. Report the
  feed. The limit would then drop real albums.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0054-task-002-crawler-limits.md`, all of it
- `docs/adr/0054-a-fetch-reaches-only-public-feed-hosts.md`, sections 2 and 3
- `docs/plans/adr-0054-fetch-rule-phase-plan.md`, decisions 5 and 6
- The files in "Files To Inspect"

Goal:
- One fetch reads at most 16 MiB. Limit the follow URLs of one feed and of
  one pass.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `stophammer`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria".
- The gate is green.

Test commands:
- The commands of "Test Commands".

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
