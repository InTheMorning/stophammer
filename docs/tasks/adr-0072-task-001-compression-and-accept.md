# ADR 0072 Task 001: A Feed Fetch Accepts Compression

Owner: [ADR 0072](../adr/0072-a-feed-fetch-accepts-compression.md) §1 to §4.
The phase plan is
[the compression, splits and GUID plan](../plans/compression-splits-guid-phase-plan.md).

Repository: `stophammer-crawler`. The operator commits.

## Goal

Each feed fetch asks for `gzip` and `br`, decodes the body, and sends the
`Accept` header of ADR 0072 §2. The snapshot download and the gossip-listener
stream do not ask for compression.

## Files To Inspect

- `Cargo.toml`: the `reqwest` features
- `src/crawl.rs`: `build_hop_request`, `read_capped_body`, the test helpers
  `spawn_stub`, `stub_response`, `test_config` and `KEPT_BODY`
- `src/modes/batch.rs`: `build_feed_fetch_client`
- `src/modes/gossip.rs`: `create_async_client`, `create_sse_client`
- `src/modes/import.rs`: `download_snapshot_archive`
- `~/build/musicindex-research/sources/captures/2026-10-20-stophammer-claim/tests/crawler-claim-tests.diff`:
  the two tests of the research request
- `~/build/musicindex-research/sources/captures/2026-10-20-stophammer-claim/tests/crawler-minimal-fix.diff`:
  the minimal fix. This task puts the header in a different place.

## Files Likely To Change

- `Cargo.toml`, `Cargo.lock`
- `src/crawl.rs`
- `src/modes/gossip.rs`
- `src/modes/import.rs`

## Constraints

- **Features.** Add `gzip` and `brotli` to the `reqwest` features. Add no other
  feature.
- **The `Accept` header.** Add a constant `FEED_ACCEPT` in `src/crawl.rs` with
  this exact value:
  `application/rss+xml, application/atom+xml, application/xml;q=0.9, text/xml;q=0.9, */*;q=0.8`.
  `build_hop_request` sets it with `reqwest::header::ACCEPT`, adjacent to
  `User-Agent`. Do not set it on a client with `default_headers`.
- **Opt-out.** The blocking client in `download_snapshot_archive` calls
  `.no_gzip()` and `.no_brotli()`. `create_sse_client` calls the same two
  methods, so the stream from the gossip listener does not change.
- **The ingest client.** `build_ingest_client` does not change.
- **The hash.** The hash stays the SHA-256 of the decoded body. Do not change
  the hash code.
- **Comments.** Each new item names ADR 0072 and its section, in the form of
  the comments in `src/crawl.rs`: "(`stophammer` ADR 0072 §2)".

## Do Not Touch

- The conditional headers of `build_hop_request`. Task 002 changes them.
- `FetchCounts` in `src/modes/batch.rs`. Task 003 changes it.
- `src/feed_cache.rs`, `src/feed_skip.rs`, `src/fetch_guard.rs`
- `../stophammer-parser/`, and the `stophammer` repository
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Change the `reqwest` features, and run `cargo build` to update
   `Cargo.lock`.
2. Add `FEED_ACCEPT`, and set the header in `build_hop_request`.
3. Add the two opt-outs.
4. Add the two tests of the research request to the `tests` module of
   `src/crawl.rs`. Keep their names. Change the comment on each test to name
   ADR 0072, not "Claim (research)".
5. Add a test for the snapshot client. See the acceptance criteria.

## Acceptance Criteria

Mechanical. Each is a test in the `tests` module of the file that it names.

- `src/crawl.rs`: `claim_feed_request_advertises_compression_and_feed_types`
  passes. The feed request has `gzip` in `accept-encoding`, and
  `application/rss+xml` in `accept`.
- `src/crawl.rs`: `claim_gzip_encoded_feed_is_decoded_and_accepted` passes. A
  body with `content-encoding: gzip` gives `CrawlOutcome::Accepted`.
- `src/crawl.rs`: a gzip body and the same plain body give the same
  `content_sha256` in the report.
- `src/crawl.rs`: a request from the client of
  `crate::modes::gossip::create_async_client` also sends `FEED_ACCEPT`. Make the
  function `pub(crate)` if a test needs it.
- `src/modes/import.rs`: `download_snapshot_archive` sends no
  `accept-encoding` header. Use a `std::net::TcpListener` on `127.0.0.1` in a
  thread. It reads the request head, keeps it, and answers `304`. Call the
  function with `if_modified_since` set, so a `304` is a correct answer.
- Before the change, the first two tests fail. Check this one time, and give
  the result in the report.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-crawler
cargo build
cargo test claim_
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- `reqwest` 0.12 has no `no_gzip` or `no_brotli` on the blocking client.
- A test of an existing feature fails after the feature change, and the cause
  is not clear.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results, with the two tests before the change
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/stophammer-crawler/AGENTS.md`
- `~/build/stophammer/docs/adr/0072-a-feed-fetch-accepts-compression.md`, §1 to §4
- `~/build/stophammer/docs/tasks/adr-0072-task-001-compression-and-accept.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- Each feed fetch asks for and decodes `gzip` and `br`, and sends `FEED_ACCEPT`. The snapshot download and the SSE stream do not ask for compression.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Set `Accept` in `build_hop_request`, not with `default_headers`.
- Do not change the hash code or the conditional headers.
- Stop at each escalation trigger of this task.

Do not touch:
- `src/feed_cache.rs`, `src/feed_skip.rs`, `src/fetch_guard.rs`, `FetchCounts`
- `../stophammer-parser/`, the `stophammer` repository
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test claim_`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
