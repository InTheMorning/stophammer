# ADR 0050 Task 002: The Conditional Fetch

Owner: [ADR 0050](../adr/0050-the-crawler-revalidates-a-feed.md) §2, §3, §4
and §5.
Plan: [phase plan](../plans/adr-0050-feed-revalidation-phase-plan.md),
decisions 3, 4, 5, 6 and 8.
Needs: task 001.

**This task changes a different repository.** The work is in
`stophammer-crawler`. Read `stophammer-crawler/AGENTS.md` first. The commit goes
in that repository and names ADR 0050.

## Goal

`crawl_feed_report` reads the cache, sends a conditional GET, and handles a
`304` as ADR 0050 §3 says. No mode passes a cache yet, so no behavior changes
in this task.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `src/crawl.rs`: `CrawlConfig`, `crawl_feed_report`,
  `ingest_cached_feed_report`, `build_crawl_report`, `CrawlReport`,
  `CrawlOutcome`
- `src/feed_cache.rs` from task 001

## Files Likely To Change

- `src/crawl.rs`

## Do Not Touch

- the call sites of `crawl_feed_report` in the modes. Task 003 changes them
- `ingest_cached_feed_report` and `post_ingest_payload`
- `src/feed_cache.rs`, other than a read-only helper if one is missing
- `stophammer`, `stophammer-parser`

## Constraints

- `CrawlConfig` gains `pub revalidate: bool`. `from_env_with_force` and
  `dry_run` set it to `true`. A method `with_revalidate(self, bool) -> Self`
  changes it.
- `crawl_feed_report` gains a parameter `cache: Option<&FeedCache>`, where
  `pub type FeedCache = Arc<std::sync::Mutex<FeedCacheDb>>`. With `None`, the
  function behaves as today.
- The decision is a pure function, for example
  `fn plan_after_response(status: u16, cached: Option<&CachedFeed>, force: bool) -> FetchAction`,
  with `FetchAction` as an enum: `IngestFresh`, `IngestKept`, `SkipIngest`,
  `RefetchUnconditional`, `FetchError`. Its rules, from ADR 0050 §3:
  - `200`: `IngestFresh`.
  - `304`, a kept body, and `force`: `IngestKept`.
  - `304`, a kept body, no `force`, and the row's `node_answer` is
    `accepted`, `no_change`, `rejected` or `parse_error`: `SkipIngest`.
  - `304`, a kept body, no `force`, and the row's `node_answer` is null or
    `ingest_error`: `IngestKept`.
  - `304` and no kept body: `RefetchUnconditional`.
  - any other status: `FetchError`, as today.
- The request. Three conditions make it conditional: `cache` is `Some`,
  `config.revalidate` is true, and a row exists. Then send `If-None-Match`
  with the stored `etag`, and `If-Modified-Since` with the stored
  `last_modified`. Send each header only when its value is stored. Read the
  row with the lock, then release the lock before the request.
- `IngestFresh`: as today, then `put` the row with the response `ETag`,
  `Last-Modified`, the final URL, the hash, the body and the time. Then
  `record_node_answer` with `report.outcome.label()` and `reason()`.
- `IngestKept`: call `ingest_cached_feed_report` with the kept body, the kept
  hash, `http_status` 200, and the row's `final_url` as the canonical URL. Then
  `record_node_answer`. Set `fetch_http_status` in the report to `Some(304)`.
- `SkipIngest`: parse the kept body with `parse_feed_xml`, so that the report
  carries `parsed_feed`, `raw_medium` and `parsed_feed_guid`. The outcome is
  `NoChange`, `fetch_http_status` is `Some(304)`, `content_sha256` is the kept
  hash. Do not call `record_node_answer`.
- `RefetchUnconditional`: send one GET with no conditional header, then
  continue as for `200`. Never loop.
- `FetchError` (a `429` or any other status): as today. Do not write the row.
- Each lock is held for one `get`, one `put` or one `record_node_answer`, and
  never across an await.
- Lints: `pedantic = "deny"`. Tests inline. A test may start an HTTP stub on
  `127.0.0.1` with `tokio::net::TcpListener` and answer with a fixed status and
  headers. No test sends a request to an external host.

## Implementation Steps

1. Add `revalidate` to `CrawlConfig`.
2. Add `FetchAction` and `plan_after_response`, with a unit test for each rule.
3. Add the `cache` parameter and the request headers.
4. Add the handling for each action.
5. Add the stub tests below.
6. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- Unit tests prove each rule of `plan_after_response`, including
  `node_answer` null and `ingest_error` for `IngestKept`.
- Stub test, `200`: the stub answers `200` with `ETag: "v1"` and a body. After
  the call, the cache holds the row with `etag = "v1"`, the hash of the body,
  and a node answer. The ingest POST goes to a second stub, or the test uses a
  config whose ingest URL points at the stub and asserts the POST arrived.
- Stub test, `304` normal: the cache holds a row with `node_answer =
  accepted`. The stub asserts that the request carried `If-None-Match: "v1"`
  and answers `304`. The report has `NoChange`, `fetch_http_status ==
  Some(304)`, and `parsed_feed.is_some()`. The ingest stub received no POST.
- Stub test, `304` force: the same row, `force_reingest = true`. The ingest
  stub received one POST with the kept body's hash, and the row's answer is
  updated.
- Stub test, `304` with `node_answer` null: the ingest stub received one POST.
- Stub test, `429`: the report is a retryable `FetchError`, and the row is
  unchanged.
- Stub test, `revalidate = false`: the request carries no `If-None-Match`.
- With `cache = None`, the existing tests of `crawl.rs` pass with no edit.

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

- `ingest_cached_feed_report` needs a signature change
- the stub on `127.0.0.1` cannot run inside the test runtime
- `parse_feed_xml` is not reachable from the `SkipIngest` path without a
  change to its visibility that the crate's lints refuse

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-crawler` repository. Read its `AGENTS.md` first.

Read:
- `docs/tasks/adr-0050-task-002-conditional-fetch.md` in the `stophammer`
  repository, the section "Constraints", in full
- `src/crawl.rs`: `CrawlConfig`, `crawl_feed_report`,
  `ingest_cached_feed_report`, `build_crawl_report`
- `src/feed_cache.rs`

Goal:
- `crawl_feed_report` takes `cache: Option<&FeedCache>`, sends a conditional
  GET when a row exists, and handles `304` through the pure function
  `plan_after_response`.

Constraints:
- `CrawlConfig.revalidate`, default true, with `with_revalidate`.
- The action rules of the task file, exactly. `node_answer` null or
  `ingest_error` on a `304` gives `IngestKept`.
- `SkipIngest` parses the kept body and gives `NoChange` with `parsed_feed`.
- `IngestKept` submits the kept body with `http_status` 200 and the row's
  `final_url`.
- A `429` or other status writes no row.
- No lock across an await. Tests may use a stub on `127.0.0.1`, and no external
  host.

Do not touch:
- the call sites in the modes, `ingest_cached_feed_report`,
  `post_ingest_payload`
- `stophammer`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- Unit tests cover each `plan_after_response` rule.
- Stub tests cover `200`, `304` normal, `304` force, `304` with null answer,
  `429`, and `revalidate = false`, as the task file states.
- The existing `crawl.rs` tests pass with no edit.

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
