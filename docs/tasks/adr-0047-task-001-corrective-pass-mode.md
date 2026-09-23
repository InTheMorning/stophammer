# ADR 0047 Task 001: A Corrective Pass Mode For The Crawler

Owner: [ADR 0047](../adr/0047-a-corrective-pass-reads-the-index.md).

**This task changes a different repository.** The work is in
`stophammer-crawler`, which has its own GitHub upstream, its own gate and its
own lint set. Read `stophammer-crawler/AGENTS.md` first. The commit goes in
that repository and names ADR 0047.

## Goal

A `refresh` mode that reads the node's feed list, then runs the existing crawl
pipeline over it, so a corrective pass covers the feeds the index holds and
needs no external URL list.

The mode name `refresh` is this packet's choice. Keep it unless the operator
says otherwise.

## Files To Inspect

- `src/main.rs` — the `Mode` enum, the `main` match, how `feed` declares flags
- `src/modes/batch.rs` — `run` and `run_urls`. `run_urls` already exists and is
  the entry point this mode needs
- `src/modes/mod.rs` — the module list
- `src/crawl.rs` — `CrawlConfig::from_env_with_force` reads `INGEST_URL`, and
  `build_ingest_client` shows the client style this crate uses
- `src/modes/gossip.rs` — `create_async_client` is the other client builder

## Files Likely To Change

- `src/main.rs`
- `src/modes/mod.rs`
- `src/modes/refresh.rs` — new

## Do Not Touch

- `src/modes/batch.rs`. `run_urls` is already the seam. Call it
- `src/modes/gossip.rs`, `src/modes/import.rs`, `src/modes/ndjson.rs`
- `src/crawl.rs` fetch or submit behavior
- `src/feed_skip.rs` and the `import_feed_memory` schema
- the `stophammer` and `stophammer-parser` repositories

## Constraints

- **The corpus is the node's feed list, nothing else.** Do not read
  `feed_skip.db` or `import_feed_memory`. They record what the crawler did, not
  what the index holds.
- **Derive the query origin from `INGEST_URL`.** It is a full URL that ends in
  the ingest path, default `http://localhost:8008/ingest/feed`. Strip that path
  to get the origin. Add no required environment variable. An optional override
  is acceptable, but the mode must work with `INGEST_URL` alone.
- **Page to the end.** `GET /v1/feeds/recent` takes `limit` and `cursor` and
  returns `{"data": [...], "pagination": {"cursor": ..., "has_more": ...}}`.
  Each item carries `feed_url`. Use `limit=100`, which is the route's cap.
  Follow the cursor while `has_more` is true.
- **A failed page stops the pass.** Never continue with a short corpus. A pass
  that silently covers part of the index is the defect this mode exists to
  prevent.
- Report the corpus size once paging is complete, before the first fetch.
- The mode does not imply `--force`. The operator passes both.
- Lints are `[lints.clippy] pedantic = "deny"` and nothing more.
- Tests are inline `#[cfg(test)]` modules. There is no `tests/` directory.

## Implementation Steps

1. Add `src/modes/refresh.rs` and register it in `src/modes/mod.rs`.
2. Derive the query origin from `INGEST_URL`. Keep that derivation in its own
   small function so it can be tested without a network.
3. Page `GET /v1/feeds/recent`, collecting `feed_url`. Stop on the first page
   that fails, and return the failure rather than a partial list.
4. Report the corpus size.
5. Call `batch::run_urls` with the collected list.
6. Add the mode to `src/main.rs` with the same flag and environment variable
   conventions `feed` mode uses for concurrency, host delay and the failed
   feeds output.
7. Add the guards listed under acceptance.
8. Run the gate.

## Acceptance Criteria

Mechanical, each proved by a test or a command:

- `cargo build` succeeds.
- `cargo test` is green, and the new tests are present.
- `cargo clippy --all-targets -- -D warnings` reports nothing.
- `cargo fmt -- --check` reports nothing.
- The origin derivation turns `http://host:8008/ingest/feed` into the origin
  `http://host:8008`, and it is covered by a test that needs no network.
- Paging collects every page of a multi-page response. Prove this against a
  stub or fixture, not the live node.
- A page that fails stops the pass and reports an error. The pass does not call
  `batch::run_urls` with a partial list.
- An empty feed list reports zero and performs no fetch.
- `batch::run` and `batch::run_urls` are unchanged, and the existing batch
  tests pass with no edit.

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

Stop and report rather than deciding, when:

- `run_urls` appears to need a signature or behavior change
- the route does not paginate as described
- the origin cannot be derived from `INGEST_URL` for a real deployment shape
- an existing batch, gossip, import or ndjson test fails

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-crawler` repository. It has its own gate and
its own lint set. Read its `AGENTS.md` first.

Read:
- `stophammer-crawler/AGENTS.md`
- `src/modes/batch.rs`, especially `run_urls`
- `src/main.rs`, especially how `Mode::Feed` declares its flags
- `src/crawl.rs`, especially `CrawlConfig::from_env_with_force` and
  `build_ingest_client`
- `src/modes/gossip.rs`, especially `create_async_client`

Goal:
- Add a `refresh` mode that reads the node's feed list, then runs the existing
  crawl pipeline over it through `batch::run_urls`.

Constraints:
- The corpus is the node's feed list and nothing else. Do not read
  `feed_skip.db` or `import_feed_memory`: they record what the crawler did, not
  what the index holds.
- Derive the query origin from `INGEST_URL`, a full URL ending in the ingest
  path (default `http://localhost:8008/ingest/feed`). Strip that path. Add no
  required environment variable. Keep the derivation in its own small function
  so a test can cover it without a network.
- `GET /v1/feeds/recent` takes `limit` and `cursor` and returns
  `{"data": [...], "pagination": {"cursor": ..., "has_more": ...}}`. Each item
  has `feed_url`. Use `limit=100`. Follow the cursor while `has_more` is true.
- A page that fails stops the pass. Never call `batch::run_urls` with a partial
  list. A pass that silently covers part of the index is the defect this mode
  exists to prevent.
- Report the corpus size once paging completes, before the first fetch.
- The mode does not imply `--force`.
- Lints are `[lints.clippy] pedantic = "deny"`. Tests are inline
  `#[cfg(test)]` modules; there is no `tests/` directory.

Do not touch:
- `src/modes/batch.rs`. `run_urls` is already the seam; call it
- `src/modes/gossip.rs`, `src/modes/import.rs`, `src/modes/ndjson.rs`
- `src/crawl.rs` fetch or submit behavior
- `src/feed_skip.rs`, the `import_feed_memory` schema
- the `stophammer` and `stophammer-parser` repositories

Acceptance criteria:
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` and
  `cargo fmt -- --check` are all green.
- A test proves `http://host:8008/ingest/feed` yields origin
  `http://host:8008`, with no network.
- A test proves paging collects every page of a multi-page response, against a
  stub or fixture rather than the live node.
- A test proves a failed page stops the pass and that `run_urls` is not called
  with a partial list.
- An empty list reports zero and performs no fetch.
- `batch::run` and `batch::run_urls` are unchanged and their tests pass
  unedited.

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
