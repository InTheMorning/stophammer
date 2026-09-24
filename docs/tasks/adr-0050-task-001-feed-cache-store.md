# ADR 0050 Task 001: The Fetch Cache Store

Owner: [ADR 0050](../adr/0050-the-crawler-revalidates-a-feed.md) §1.
Plan: [phase plan](../plans/adr-0050-feed-revalidation-phase-plan.md),
decisions 1, 2 and 5.

**This task changes a different repository.** The work is in
`stophammer-crawler`. Read `stophammer-crawler/AGENTS.md` first. The commit goes
in that repository and names ADR 0050.

## Goal

A store with one row for each requested URL. A row keeps the validators, the
compressed body, the hash, the final URL, the fetch time and the last node
answer. No code calls the store in this task.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `src/feed_skip.rs`: `FeedSkipDb::open`, the WAL pragma, the busy timeout,
  `unix_now`, and how `record_outcome` writes with `INSERT ... ON CONFLICT`
- `src/crawl.rs`: `CrawlOutcome::label` and `CrawlOutcome::reason`
- `Cargo.toml`: `flate2` and `rusqlite` are dependencies

## Files Likely To Change

- `src/feed_cache.rs`, new
- `src/main.rs`: the `mod feed_cache;` line only

## Do Not Touch

- `src/crawl.rs`, `src/feed_skip.rs`, the modes
- `Cargo.toml`. No new dependency
- `stophammer`, `stophammer-parser`

## Constraints

The table, in the file that `FeedCacheDb::open(path)` opens:

```sql
CREATE TABLE IF NOT EXISTS feed_cache (
    url            TEXT PRIMARY KEY,
    final_url      TEXT NOT NULL,
    etag           TEXT,
    last_modified  TEXT,
    content_sha256 TEXT NOT NULL,
    body_gzip      BLOB NOT NULL,
    fetched_at     INTEGER NOT NULL,
    node_answer    TEXT,
    node_reason    TEXT,
    answered_at    INTEGER
) STRICT;
```

- `open` creates the parent directory, sets WAL mode and a 5 second busy
  timeout, as `FeedSkipDb::open` does.
- The public type is `FeedCacheDb`, with these methods:
  - `get(&self, url: &str) -> Option<CachedFeed>`. `CachedFeed` holds each
    column, with the body decompressed to a `String`.
  - `put(&self, url: &str, entry: &FetchedFeed)`. `FetchedFeed` holds
    `final_url`, `etag`, `last_modified`, `content_sha256`, `body: &str` and
    `fetched_at`. `put` compresses the body with gzip, and it replaces the
    whole row. It sets `node_answer`, `node_reason` and `answered_at` to null.
  - `record_node_answer(&self, url: &str, label: &str, reason: Option<&str>, at: i64)`.
    It updates the three answer columns of an existing row. It does nothing
    when no row exists.
- `etag` and `last_modified` are stored unchanged. A weak `ETag` keeps its
  `W/` prefix.
- A write is one statement. `put` and `record_node_answer` hold no transaction
  open across a call that the caller makes.
- A store error is logged with `eprintln!`, as `feed_skip.rs` does, and the
  method returns as if nothing happened. The crawler must not stop because of
  its cache.
- Lints: `[lints.clippy] pedantic = "deny"`. Tests are inline.

## Implementation Steps

1. Add `src/feed_cache.rs` with the table, the types and the three methods.
2. Add `mod feed_cache;` to `src/main.rs`.
3. Add the tests below, with `tempfile::tempdir()`.
4. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- `get` on a missing URL gives `None`.
- `put` then `get` gives back each field, and the body is equal byte for byte
  after the gzip round trip.
- A second `put` for the same URL replaces the row and clears the node answer.
- `record_node_answer` then `get` gives the label, the reason and the time.
- `record_node_answer` for a missing URL does nothing and does not panic.
- Two `FeedCacheDb` values on one file: a `put` through the first is visible
  through `get` on the second.
- A weak `ETag` `W/"abc"` comes back unchanged.

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

- `flate2` lacks a gzip encoder or decoder in the enabled features
- `rusqlite` `STRICT` tables are not supported by the bundled version

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-crawler` repository. Read its `AGENTS.md` first.

Read:
- `docs/tasks/adr-0050-task-001-feed-cache-store.md` in the `stophammer`
  repository, the section "Constraints"
- `src/feed_skip.rs`, as the pattern
- `src/crawl.rs`: `CrawlOutcome::label` and `reason`

Goal:
- Add `src/feed_cache.rs` with `FeedCacheDb`: `open`, `get`, `put`,
  `record_node_answer`, on the `feed_cache` table of the task file.

Constraints:
- WAL mode, 5 second busy timeout, parent directory created, as
  `FeedSkipDb::open`.
- `put` compresses the body with gzip and replaces the whole row, with the
  node answer cleared. `record_node_answer` updates an existing row only.
- `etag` and `last_modified` unchanged, weak prefix kept.
- A store error is logged and swallowed. The crawler never stops for its cache.
- No new dependency. Lints: `pedantic = "deny"`. Tests inline.

Do not touch:
- `src/crawl.rs`, `src/feed_skip.rs`, the modes, `Cargo.toml`
- `stophammer`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- Tests prove: missing URL gives `None`; put then get round-trips each field
  and the body; a second put replaces the row and clears the answer;
  `record_node_answer` then get gives the answer; `record_node_answer` on a
  missing URL does nothing; two handles on one file see each other's writes;
  a weak `ETag` is unchanged.

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
