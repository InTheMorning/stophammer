# ADR 0072 Task 002: A Stored ETag Is Sent With Its Base Tag

Owner: [ADR 0072](../adr/0072-a-feed-fetch-accepts-compression.md) §5, which
amends ADR 0050 §2. The phase plan is
[the compression, splits and GUID plan](../plans/compression-splits-guid-phase-plan.md).

Repository: `stophammer-crawler`. The operator commits. Task 001 is complete.

## Goal

When a stored `ETag` ends with a compression suffix, `If-None-Match` carries
the stored tag and the tag without the suffix. The cache keeps the stored
value.

## Files To Inspect

- `src/crawl.rs`: `build_hop_request`, the stub tests that read
  `feed_request.header("if-none-match")`
- `src/feed_cache.rs`: `CachedFeed`, `FetchedFeed`, `put`
- `docs/adr/0050-the-crawler-revalidates-a-feed.md` §1 and §2

## Files Likely To Change

- `src/crawl.rs`

## Constraints

- **The function.** Add `fn if_none_match_value(etag: &str) -> String` in
  `src/crawl.rs`. `build_hop_request` sends its result in `IF_NONE_MATCH`.
- **The suffixes.** A constant `ETAG_COMPRESSION_SUFFIXES: &[&str] =
  &["-gzip", "-br"]`.
- **The rule.** Trim the value. When it ends with `<suffix>"` for one suffix,
  give `<stored>, <base>`. `<base>` is the stored value with the suffix removed
  before the last `"`. A weak prefix `W/` stays on both tags. In each other
  case, give the stored value unchanged.

  | Stored | Sent |
  |---|---|
  | `"1288-63c94b87e7da9-gzip"` | `"1288-63c94b87e7da9-gzip", "1288-63c94b87e7da9"` |
  | `W/"abc-br"` | `W/"abc-br", W/"abc"` |
  | `"abc"` | `"abc"` |
  | `"abc-gzip-x"` | `"abc-gzip-x"` |
  | `"-gzip"` | `"-gzip"` (the base tag must not be empty) |
  | `abc-gzip` (no quotes) | `abc-gzip` |

- **The cache.** Do not change the value that `FeedCache` stores or reads.
- **Comments.** The function names ADR 0072 §5 and the Apache `mod_deflate`
  behavior in one sentence.

## Do Not Touch

- `If-Modified-Since`, the redirect chain, `plan_after_response`
- `src/feed_cache.rs`, `FetchCounts`
- `../stophammer-parser/`, and the `stophammer` repository
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add the constant and `if_none_match_value`, with a unit test for each row of
   the table.
2. Use it in `build_hop_request`.
3. Add the stub test of the acceptance criteria.

## Acceptance Criteria

Mechanical. Each is a test in the `tests` module of `src/crawl.rs`.

- Each row of the table of "Constraints" gives the value in the column "Sent".
- A crawl with a cached row whose `ETag` is `"v1-gzip"` sends
  `If-None-Match: "v1-gzip", "v1"` to the stub. After a `304`, the cached row
  still holds `"v1-gzip"`.
- Each existing test that reads `if-none-match` passes with no change.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-crawler
cargo build
cargo test if_none_match
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A case is not in the table, and the rule does not give one answer.
- An existing test expects a different `If-None-Match` value.
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
- `~/build/stophammer/docs/adr/0072-a-feed-fetch-accepts-compression.md`, §5
- `~/build/stophammer/docs/tasks/adr-0072-task-002-etag-base-tag.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- A stored `ETag` that ends with `-gzip"` or `-br"` is sent as a list of the stored tag and the base tag. The cache keeps the stored value.

Constraints:
- Follow the table and the rule of the section "Constraints" of this task.
- Change only the request. Do not change the cache.
- Stop at each escalation trigger of this task.

Do not touch:
- `If-Modified-Since`, the redirect chain, `plan_after_response`, `src/feed_cache.rs`, `FetchCounts`
- `../stophammer-parser/`, the `stophammer` repository
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test if_none_match`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
