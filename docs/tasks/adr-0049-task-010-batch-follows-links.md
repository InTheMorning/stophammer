# ADR 0049 Task 010: The Batch Path Follows Publisher Links

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §2.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md),
decision 10.
Needs: task 004 deployed on the node. Without it, the follow step makes fetches
that record no observation.

**This task changes a different repository.** The work is in
`stophammer-crawler`. Read `stophammer-crawler/AGENTS.md` first. The commit goes
in that repository and names ADR 0049.

## Goal

After the batch path crawls its URL list, it crawls one more wave: the feeds
that the accepted feeds name through a publisher link. The `feed` mode and the
`refresh` mode both use the batch path.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `src/modes/batch.rs`: `run_urls`, `crawl_feed_with_retries`, `run_pool`,
  `interleave_by_host`, `HostThrottle`
- `src/crawl.rs`: `crawl_feed`, `crawl_feed_report`, `CrawlReport`,
  `CrawlOutcome`
- `src/modes/refresh.rs`: how it calls `batch::run_urls`
- `stophammer-parser/src/types.rs`: `IngestFeedData.raw_medium`,
  `IngestRemoteFeedRef`

## Files Likely To Change

- `src/follow.rs`, new
- `src/lib.rs` or `src/main.rs`: the module declaration, as the crate declares
  modules
- `src/modes/batch.rs`

## Do Not Touch

- the signature of `batch::run` and `batch::run_urls`
- `src/crawl.rs` fetch and submit behavior
- `src/modes/gossip.rs`. Task 012 changes it
- `src/modes/import.rs`, `src/modes/ndjson.rs`
- `src/modes/refresh.rs`. Task 011 changes it
- the `stophammer` and `stophammer-parser` repositories

## Constraints

`follow::follow_urls(feed: &IngestFeedData) -> Vec<String>`:

- For a feed with `raw_medium` equal to `music`, ignoring ASCII case: give the
  `remote_feed_url` of each channel-level remote item with `medium` equal to
  `publisher`.
- For a feed with `raw_medium` equal to `publisher`: give the
  `remote_feed_url` of each channel-level remote item with `medium` equal to
  `music`.
- For any other medium: give nothing.
- Skip an item with no URL, or with a URL whose scheme is not `http` or
  `https`.
- Do not change a URL. Do not make one from a GUID. Do not follow an
  item-level remote item.
- Remove duplicates and keep the first position.

The waves in `run_urls`:

- Wave 1 is the current behavior over the given list.
- Collect the follow URLs of each wave-1 report whose outcome is `Accepted` or
  `NoChange` and that has a parsed feed.
- Wave 2 is the collected URLs, less each URL of wave 1 by exact string, with
  no duplicates, in host interleave order.
- Wave 2 uses the same client, config, throttle and concurrency as wave 1.
- Do not collect follow URLs from wave 2. The walk stops at one level.
- A retryable failure in wave 2 goes to the same failed-feeds output as a
  failure in wave 1.
- Print the size of wave 2 before its first fetch. Print nothing more when it is
  empty.

`crawl_feed_with_retries` must give the parsed feed as well as the outcome. Use
`crawl_feed_report`. Keep its retry behavior.

Put the two-wave logic in a function that takes the crawl step as a closure.
`run_urls` gives it the real step. A test gives it a stub step and needs no
network.

Lints are `[lints.clippy] pedantic = "deny"`. Tests are inline `#[cfg(test)]`
modules.

## Implementation Steps

1. Add `src/follow.rs` with `follow_urls` and its tests.
2. Change `crawl_feed_with_retries` to give the report. Keep the retry loop.
3. Collect the follow URLs in wave 1.
4. Run wave 2.
5. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- Unit tests for `follow_urls`:
  - A music feed gives its publisher URL.
  - A publisher feed gives its music URLs.
  - A `musicL` feed gives nothing.
  - An item with no URL is skipped.
  - An `ftp:` URL is skipped.
  - Duplicates give one URL.
  - An item-level remote item is not followed.
- A test proves the wave-2 set: the follow URLs, less the wave-1 URLs, with no
  duplicates.
- A test with a stub fetch proves that wave 2 is not followed. A wave-2 feed
  that names a publisher does not cause a third wave.
- A test proves that a `Rejected` report gives no follow URL.
- The existing batch and refresh tests pass with no edit.

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

- `run_urls` needs a signature change
- the retry loop cannot give the report without a change to `src/crawl.rs`
- an existing batch or refresh test fails

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-crawler` repository. Read its `AGENTS.md` first.

Read:
- `stophammer-crawler/AGENTS.md`
- `docs/tasks/adr-0049-task-010-batch-follows-links.md` in the `stophammer`
  repository, the section "Constraints"
- `src/modes/batch.rs`: `run_urls`, `crawl_feed_with_retries`, `run_pool`,
  `interleave_by_host`
- `src/crawl.rs`: `crawl_feed_report`, `CrawlReport`, `CrawlOutcome`

Goal:
- After wave 1, the batch path crawls a second wave of the feeds that accepted
  feeds name through a publisher link, and stops there.

Constraints:
- `follow::follow_urls` gives the channel-level `remote_feed_url` values:
  - For a `music` feed, the `publisher` items.
  - For a `publisher` feed, the `music` items.
  - For a different medium, nothing.
- `follow_urls` gives only `http` and `https` URLs. It changes no URL, makes no
  URL from a GUID, follows no item-level item and gives no duplicate.
- Collect from `Accepted` and `NoChange` reports with a parsed feed.
- Wave 2 is those URLs less the wave-1 URLs by exact string, deduplicated and
  host-interleaved, with the same client, config, throttle and concurrency.
- Never collect from wave 2.
- Wave-2 retryable failures go to the same failed-feeds output.
- `crawl_feed_with_retries` uses `crawl_feed_report` and keeps its retries.
- The two-wave logic takes the crawl step as a closure, so a test can give a
  stub step and needs no network.
- Lints: `pedantic = "deny"`. Tests are inline.

Do not touch:
- the signatures of `batch::run` and `batch::run_urls`
- `src/crawl.rs` fetch and submit behavior
- `gossip.rs`, `import.rs`, `ndjson.rs`, `refresh.rs`
- `stophammer`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- `follow_urls` tests cover music, publisher, `musicL`, no URL, `ftp:`,
  duplicates and item-level items.
- Tests prove the wave-2 set, that wave 2 is not followed, and that a
  `Rejected` report gives nothing.
- The existing batch and refresh tests pass unedited.

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
