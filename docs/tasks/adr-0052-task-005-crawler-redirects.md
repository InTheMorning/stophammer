# ADR 0052 Task 005: Crawler Redirect Hops And New-Feed Follow

Owner: [ADR 0052](../adr/0052-a-source-moves-its-own-feed.md) section 2.
Plan: [phase plan 2](../plans/adr-0052-moves-and-guid-changes-phase-plan.md),
decisions 2 and 3.

Repository: `stophammer-crawler`. Read `stophammer-crawler/AGENTS.md` first.
Needs task 004 in `stophammer-parser`.

## Goal

The ingest request tells the node each redirect hop and its status. The
crawler follows a declared `itunes:new-feed-url`.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `stophammer-crawler/src/crawl.rs`: the two fetch functions that call
  `client.get(url)` (about lines 687 and 845), `refetch_unconditional`,
  `build_crawl_report`, `post_ingest_payload`, `CrawlReport`
- `stophammer-crawler/src/modes/gossip.rs`: `create_async_client`
- `stophammer-crawler/src/modes/batch.rs`, `src/modes/refresh.rs`,
  `src/modes/import.rs`: where each mode builds its fetch client
- `stophammer-crawler/src/follow.rs`: `follow_urls`,
  `follow_urls_at_level`
- `stophammer-crawler/src/feed_cache.rs`: what the cache keeps for a `304`

## Files Likely To Change

- `stophammer-crawler/src/crawl.rs`, `src/follow.rs`, the modes that build a
  fetch client
- Tests in `stophammer-crawler/tests/` or in the modules

## Do Not Touch

- `stophammer`, `stophammer-parser`
- The behavior of the fetch cache, apart from the hops

## Constraints

- **Manual redirects.** Each feed fetch client uses
  `reqwest::redirect::Policy::none()`. One helper sends a request and follows
  each `301`, `302`, `303`, `307` and `308` answer to its `Location`, at most
  10 hops. A relative `Location` resolves against the current URL. It returns
  the last response and the hops, as `Vec<RedirectHop { url, status }>` in
  order. `url` is the URL that answered with the redirect.
- The conditional headers of ADR 0050 go on each request of the chain. The
  helper needs a way to build the request for each hop, for example a closure.
- More than 10 hops is a fetch error, as the default policy made it.
- **The final URL** stays the URL of the last response, as today.
- **The payload.** `post_ingest_payload` adds `"redirects": [{ "url": ...,
  "status": ... }]`. The list is empty when there was no redirect. After a
  `304`, the list holds the hops of the conditional request that answered
  `304`.
- **The replay modes.** `ndjson` has no hops. It sends an empty list.
- **The follow.** `follow_urls` also returns the `new_feed_url` of the feed
  when it is present and not the URL that the crawler fetched. It is a
  follow of the first level, as a publisher link. The throttle and the
  deduplication of each mode stay as they are.
- Each change names stophammer ADR 0052 in a short comment.

## Implementation Steps

1. Add the redirect helper, and use it in each feed fetch.
2. Set `Policy::none()` on each feed fetch client.
3. Add `redirects` to the payload.
4. Add the `new_feed_url` follow.
5. Add the tests below.
6. Run the gate.

## Acceptance Criteria

Mechanical. Tests with a local mock server, as the existing fetch tests do:

- A `301` to a second URL that answers `200`: the report has one hop with
  status `301` and the first URL, and the final URL is the second URL.
- A `302` then a `301`: two hops, in that order.
- A relative `Location` resolves against the current URL.
- 11 redirects give a fetch error.
- A conditional request that answers `304` after one `301`: the payload has
  that hop.
- A fetch with no redirect sends `"redirects": []`.
- `follow_urls` returns `new_feed_url` for a feed that declares it, and does
  not return it when it equals the fetched URL.
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

- A mode fetches feeds with a client that it shares with a request that must
  follow redirects by itself, for example the gossip SSE stream. List it, and
  give that request its own client only if the change is small.
- The fetch cache keeps the final URL in a way that the hops change.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0052-task-005-crawler-redirects.md`, all of it
- `stophammer-crawler/AGENTS.md`
- The files in "Files To Inspect"

Goal:
- Record each redirect hop of a feed fetch and send it to the node as
  `redirects`. Follow a declared `itunes:new-feed-url`.

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
