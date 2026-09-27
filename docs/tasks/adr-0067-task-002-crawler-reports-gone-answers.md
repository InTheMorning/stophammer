# ADR 0067 Task 002: The Crawler Reports A Gone Answer

Owner: [ADR 0067](../adr/0067-a-gone-source-retires-its-feed.md) §1. Plan:
[the phase plan](../plans/adr-0067-gone-source-phase-plan.md).

Repository: `stophammer-crawler`. The operator commits. The commit names ADR
0067.

## Goal

The `feed`, `refresh` and `gossip` modes send the node a report for each gone
answer. A gone answer is `404` or `410` from the requested URL, with no
redirect before it.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `src/crawl.rs`: `CrawlConfig`, `with_revalidate` (the builder pattern),
  the branch `status != 200` in the fetch function, `post_ingest_payload`,
  and how it builds the ingest JSON
- `src/modes/batch.rs`, `src/modes/gossip.rs`, `src/modes/import.rs`,
  `src/modes/ndjson.rs`: where each builds its `CrawlConfig`
- The existing tests that use a local HTTP stub (search `127.0.0.1`)

## Files Likely To Change

- `src/crawl.rs`, `src/modes/batch.rs`, `src/modes/gossip.rs`
- `README.md` of the crawler, only when it lists what a mode sends
- Tests in `src/crawl.rs` or `tests/`

## Do Not Touch

- The outcome of a gone answer: it stays `CrawlOutcome::FetchError`, and
  the batch counts do not change.
- The skip stores, the feed cache, the retry rules.
- `src/modes/import.rs`, `src/modes/ndjson.rs`, except when the compiler needs
  the new field.
- The `stophammer` and `stophammer-parser` repositories.

## Constraints

- `CrawlConfig.report_gone: bool`, default false, with a builder
  `with_report_gone(bool)`. `batch.rs` and `gossip.rs` set it to true.
- In the branch `status != 200`: when `report_gone` is true, the status is
  `404` or `410`, and `redirects` is empty, post one ingest request to the
  node before the function returns. The request has:
  - `canonical_url` and `source_url`: the requested URL,
  - `http_status`: the status,
  - `content_hash`: an empty string,
  - no `feed_data` (JSON `null`),
  - `force_reingest`: false, and `redirects`: empty.
- Use the same HTTP client, token, timeout and ingest URL as
  `post_ingest_payload`. Build the JSON in one helper adjacent to it.
- A failed post is logged with `tracing::warn!` and changes nothing else. The
  node answer is logged with its reason at `debug`, and at `info` when the
  reason is `source_gone`.
- No report for a `404` that came after a redirect, for another status, or
  when `report_gone` is false.

## Acceptance

Mechanical, each a test with a local stub on `127.0.0.1` for the feed host and
for the node:

- A feed URL that answers `404`, with `report_gone` true, makes one post to
  the node. Its JSON has the URL twice, `http_status` 404, and
  `feed_data` null.
- The same with `410`.
- A `404` after a `301` redirect makes no post.
- A `500` makes no post.
- A `404` with `report_gone` false makes no post.
- The report of each case is still a `FetchError` with the status.
- The gate of the crawler is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer/stophammer-crawler
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report without a workaround when:

- The fetch function has no access to the config or the client at the
  branch `status != 200`.
- The ingest JSON needs a field that a gone report cannot fill.
- A mode other than `feed`, `refresh` and `gossip` uses the batch or gossip
  config.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0067-task-002-crawler-reports-gone-answers.md
- /home/citizen/build/stophammer/docs/adr/0067-a-gone-source-retires-its-feed.md (section 1)
- /home/citizen/build/stophammer/stophammer-crawler/AGENTS.md
- Only the parts of stophammer-crawler/src/crawl.rs named in the task file. Use grep.

Goal:
- The feed, refresh and gossip modes post a report to the node for each 404 or 410 from the requested URL with no redirect.

Constraints:
- The rules under "Constraints" in the task file.
- Write any scratch file under target/, not /tmp.

Do not touch:
- The outcome of a gone answer, the skip stores, the feed cache, the retry rules, the import and ndjson modes, the stophammer and stophammer-parser repositories.
- Git: run no git command that writes (no add, commit, stash, checkout, reset, push).

Acceptance criteria:
- The criteria under "Acceptance" in the task file.

Test commands:
- The commands under "Test Commands" in the task file.

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
