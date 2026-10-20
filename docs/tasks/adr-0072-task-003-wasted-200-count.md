# ADR 0072 Task 003: The Report Counts A Wasted 200

Owner: [ADR 0072](../adr/0072-a-feed-fetch-accepts-compression.md) §6, which
amends ADR 0050 §6. The phase plan is
[the compression, splits and GUID plan](../plans/compression-splits-guid-phase-plan.md).

Repository: `stophammer-crawler`. The operator commits. Task 002 is complete.

## Goal

A crawl report marks a wasted `200`: a `200` to a conditional request, with the
same body hash as the cached row. A batch pass reports the wasted answers for
each host. The `gossip` mode prints one line for each.

## Files To Inspect

- `src/crawl.rs`: `CrawlReport`, `build_crawl_report`, the fetch function that
  reads `cached` and `conditional_row` before `build_hop_request`, and
  `plan_after_response`
- `src/modes/batch.rs`: `FetchCounts`, `note`, its `Display`, and the place
  that notes each report
- `src/modes/gossip.rs`: `record_follow_outcome`, and the place that handles
  the report of a podping crawl

## Files Likely To Change

- `src/crawl.rs`
- `src/modes/batch.rs`
- `src/modes/gossip.rs`

## Constraints

- **The mark.** Add `pub wasted_200: bool` to `CrawlReport`, `false` by default.
  The fetch function sets it to `true` only when all of these are true:
  - a conditional header was sent (`conditional_row` is `Some`),
  - the final status is `200`,
  - the SHA-256 of the new body is equal to `content_sha256` of that row.
- **The host.** Take the host from the URL that the crawl requested, with
  `reqwest::Url::parse`. A URL with no host counts under `"(none)"`.
- **The batch report.** `FetchCounts` gets
  `wasted_by_host: std::collections::BTreeMap<String, u32>`. Change `note` to
  take the `&CrawlReport` and the URL, or add a second method. Keep the current
  buckets. The `Display` line adds `wasted_200=<total>`. When the total is more
  than 0, the line after it gives each host with its count, the highest count
  first, at most 20 hosts.
- **The gossip line.** For a report with `wasted_200`, print
  `  wasted_200: <host> <url>` with `eprintln!`, also when `quiet` is set.
- **Comments.** Each new item names ADR 0072 §6.

## Do Not Touch

- The node answer, the ingest decision and the cache write
- `if_none_match_value` and `build_hop_request`
- `src/feed_cache.rs`, `src/feed_skip.rs`
- `../stophammer-parser/`, and the `stophammer` repository
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add the field, set it in the fetch function, and keep `false` in each other
   constructor.
2. Extend `FetchCounts` and its `Display`.
3. Add the gossip line.
4. Write the tests.

## Acceptance Criteria

Mechanical. Each is a test in the `tests` module of the file that it names.

- `src/crawl.rs`: a cached row with hash H, a conditional request, and a stub
  `200` with a body of hash H give `wasted_200 == true`.
- `src/crawl.rs`: the same with a different body give `wasted_200 == false`.
- `src/crawl.rs`: a `200` with no cached row gives `wasted_200 == false`.
- `src/crawl.rs`: a `304` gives `wasted_200 == false`.
- `src/modes/batch.rs`: three wasted reports for host A and one for host B give
  `wasted_200=4`, then A before B in the host lines.
- `src/modes/batch.rs`: with no wasted report, the line is the line of today
  plus `wasted_200=0`, and no host line follows.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-crawler
cargo build
cargo test wasted
cargo test fetch_counts
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- The fetch function does not have the new hash and the cached hash at one
  place.
- A test parses the `Display` line of `FetchCounts` and fails.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

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
- `~/build/stophammer/docs/adr/0072-a-feed-fetch-accepts-compression.md`, §6
- `~/build/stophammer/docs/tasks/adr-0072-task-003-wasted-200-count.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- A report marks a `200` to a conditional request with an unchanged body hash. A batch pass counts these for each host, and `gossip` prints a line for each.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Do not change the ingest decision or the cache write.
- Stop at each escalation trigger of this task.

Do not touch:
- `if_none_match_value`, `build_hop_request`, `src/feed_cache.rs`, `src/feed_skip.rs`
- `../stophammer-parser/`, the `stophammer` repository
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test wasted`
- `cargo test fetch_counts`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
