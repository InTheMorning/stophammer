# ADR 0051 Task 004: The Crawler Keeps A Conflict Open

Owner: [ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md)
section 5. Plan: [phase plan](../plans/adr-0051-source-url-phase-plan.md),
decision 9.

**This task changes a different repository.** The work is in
`stophammer-crawler`. Read `stophammer-crawler/AGENTS.md` first. The commit goes
in that repository and names ADR 0051.

## Goal

After the node answers `source_conflict`, `record_conflict` or
`guid_change_pending`, the crawler stores no node answer for the URL. The next
`304` then sends the kept body again, so an operator decision takes effect
without a changed body.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `src/crawl.rs`: `CrawlOutcome`, `CrawlOutcome::label`, `CrawlOutcome::reason`,
  the function near line 590 that calls `record_node_answer`, the call near
  line 773, and `plan_after_response` near line 540
- `src/feed_cache.rs`: `record_node_answer`

## Files Likely To Change

- `src/crawl.rs`

## Do Not Touch

- `src/feed_cache.rs`, except the new method `clear_node_answer`
- `plan_after_response`. Its handling of a null answer already gives the
  needed behavior.
- The modes in `src/modes/`
- `stophammer`, `stophammer-parser`

## Constraints

- A constant list names the three reasons:
  `const NODE_CONFLICT_REASONS: [&str; 3] = ["source_conflict", "record_conflict", "guid_change_pending"];`
  with a doc comment that names stophammer ADR 0051 section 5.
- A function `fn is_node_conflict(outcome: &CrawlOutcome) -> bool` returns
  `true` only for `CrawlOutcome::Rejected` whose reason is exactly one of the
  three strings.
- Each place that calls `record_node_answer` after an ingest skips the call
  when `is_node_conflict` is `true`. When a row already holds an answer from
  an earlier ingest, the crawler clears it with
  `FeedCacheDb::clear_node_answer`. A kept answer of `accepted` would make the
  next normal `304` skip the ingest after an operator decision.

  Note: `put` clears the answer when the crawler stores a new body. Thus after
  a `200` with a conflict answer, the row has a null answer.
- The crawler still reports the outcome as `rejected` with its reason in the
  run summary. Only the cache write changes.
- Lints: `[lints.clippy] pedantic = "deny"`. Tests are inline.

## Implementation Steps

1. Add the constant and `is_node_conflict`.
2. Skip `record_node_answer` for a conflict at each call site after an ingest.
3. Add the tests below.
4. Run the gate.

## Acceptance Criteria

Mechanical. Inline tests in `src/crawl.rs`:

- `is_node_conflict` is `true` for each of the three reasons, and `false` for
  `[medium_music] absent`, for `source_conflict_extra`, and for an
  `Accepted` outcome.
- After a `200` and a `source_conflict` answer, the cache row has a null node
  answer. Use the existing test pattern for `record_node_answer` near line
  1433.
- With a row that has a null answer, `plan_after_response` for a `304` in a
  normal crawl gives `FetchAction::IngestKept`. This proves the next crawl
  submits the kept body.
- With a row that has the answer `rejected` and the reason
  `[medium_music] absent`, the behavior does not change: a `304` gives
  `FetchAction::SkipIngest`.
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

- The rejection reason reaches the call site in a form other than the exact
  node string, for example with a prefix.
- More than two call sites write a node answer after an ingest.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-crawler` repository. Read its `AGENTS.md` first.

Read:
- `docs/tasks/adr-0051-task-004-crawler-keeps-conflicts-open.md` in the
  `stophammer` repository, the sections "Constraints" and
  "Acceptance Criteria"
- `src/crawl.rs`: `CrawlOutcome`, the calls to `record_node_answer`,
  `plan_after_response`, and the tests near line 1433
- `src/feed_cache.rs`: `record_node_answer`, `put`

Goal:
- When the node answers `source_conflict`, `record_conflict` or
  `guid_change_pending`, the crawler does not store that answer in the fetch
  cache. A `304` then submits the kept body again.

Constraints:
- A constant `NODE_CONFLICT_REASONS` with the three exact strings, and
  `is_node_conflict(&CrawlOutcome) -> bool` for `Rejected` with an exact match.
- Skip `record_node_answer` for a conflict. Do not clear an earlier answer.
- The run summary still reports the rejection.
- Lints: `pedantic = "deny"`. Tests inline.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `src/feed_cache.rs`, `plan_after_response`, `src/modes/`
- `stophammer`, `stophammer-parser`

Acceptance criteria:
- Tests:
  - `is_node_conflict` for the three reasons and for three non-matches.
  - A conflict answer leaves a null node answer.
  - A null answer plus a `304` gives `IngestKept`.
  - A `rejected` answer with a different reason plus a `304` gives
    `SkipIngest`.
- The gate is green.

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
