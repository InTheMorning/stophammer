# ADR 0049 Task 002: Real-Feed Fixtures And The Ingest Probe

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §9.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md).
Needs: task 001 merged in `stophammer-parser`.

## Goal

1. The `stophammer` tests hold real publisher and album feeds, with the ingest
   JSON that the parser makes from each feed.
2. A test proves that the verifier chain accepts a Wavlake artist feed. No
   Wavlake publisher feed is in the index today, so this is not known.

## The Feeds

Fetch each feed once. Use a `User-Agent` that names Stophammer. Wait at least
2 seconds between two requests to the same host.

| Fixture name | URL | Why |
|---|---|---|
| `detox-artist` | `https://wavlake.com/feed/artist/137aaa9c-75ff-4916-9f23-e02968b2d15e` | Wavlake publisher feed. Its `feedGuid` values are not album GUIDs |
| `detox-album` | `https://wavlake.com/feed/music/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a` | The album that the artist feed lists. Its `podcast:guid` is `e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9` |
| `rssblue-publisher` | `https://publishers.rssblue.com/acoldtripnowhere` | A publisher feed that an artist operates |
| `rssblue-album` | the first `medium="music"` `feedUrl` in `rssblue-publisher` | The album of that publisher |
| `sirlibre-label` | `https://sirlibre.com/publisher/sir-libre-records-publisher-rss.xml` | A label feed with `rel="label"` |
| `sirlibre-album` | the first `feedUrl` in `sirlibre-label` that has `rel="label"` | An album that the label lists |
| `jimmyv-publisher` | `https://music.jimmyv4v.com/publisher/jimmy-v-publisher-rss.xml` | A publisher feed with `rel="artist"` and `rel="producer"` |
| `jimmyv-produced-album` | the `feedUrl` in `jimmyv-publisher` that has `rel="producer"` | An album that names a different publisher |
| `no-publisher-album` | `https://feed.justcast.com/shows/into-the-blue/audioposts.rss` | An album that names no publisher |

Also fetch `https://wavlake.com/feed/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a`.
Do not store it. Record in `SOURCES.md` whether its body is equal to the body of
`detox-album`.

## Files To Inspect

- `tests/common/mod.rs`
- `tests/adr0043_release_date_tests.rs`: `test_app_state_with_crawl_token`,
  `ingest_payload` and `ingest` show how a test submits a feed
- `tests/fixtures/`
- `stophammer-parser/src/bin/stophammer-parse.rs`: reads XML on standard
  input, writes `IngestFeedData` JSON on standard output

## Files Likely To Change

- `tests/fixtures/adr0049/<name>.xml`, new
- `tests/fixtures/adr0049/<name>.feed_data.json`, new
- `tests/fixtures/adr0049/SOURCES.md`, new
- `tests/common/mod.rs`: one loader function
- `tests/adr0049_fixture_tests.rs`, new

## Do Not Touch

- `src/`. This task changes no production code
- `stophammer-parser` and `stophammer-crawler`
- the existing fixtures in `tests/fixtures/mock-rss/`

## Constraints

- Store each body unchanged, byte for byte.
- Make each JSON file with the parser at task 001 or later:
  `cargo run --manifest-path stophammer-parser/Cargo.toml --bin stophammer-parse < tests/fixtures/adr0049/<name>.xml`.
  Pretty-print it with `jq .`, and do not edit it.
- `SOURCES.md` gives for each file: the URL, the fetch time in UTC, the HTTP
  status, the SHA-256 of the body, and the parser commit.
- The loader in `tests/common/mod.rs` is
  `pub fn adr0049_feed_data(name: &str) -> serde_json::Value`. It reads the JSON
  file. It does not fetch.
- No test in this task touches the network.

## Implementation Steps

1. Fetch the feeds in the table. Stop at the first failure.
2. Make the JSON files with the parser.
3. Write `SOURCES.md`.
4. Add the loader.
5. Add `tests/adr0049_fixture_tests.rs` with the tests below.
6. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- A test proves that each fixture name in the table has an `.xml` file and a
  `.feed_data.json` file.
- A test proves that `detox-artist` has `raw_medium == "publisher"` and at least
  one remote item with `medium == "music"`.
- A test proves that `detox-album` has `feed_guid ==
  "e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9"` and names publisher
  `137aaa9c-75ff-4916-9f23-e02968b2d15e`.
- A test proves that `sirlibre-label` has at least one remote item with
  `rel == "label"`, and `jimmyv-publisher` has one with `rel == "producer"`.
- A test proves that `no-publisher-album` has no remote item with
  `medium == "publisher"`.
- **The probe.** A test submits `detox-artist` through `POST /ingest/feed` on a
  test node with the default verifier chain. It asserts `accepted == true`.

## Test Commands

```bash
cargo build
cargo test --test adr0049_fixture_tests
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
6. whether the two DETOX album URLs give equal bodies

## Escalation Triggers

Stop and report when:

- **the probe fails.** Report the verifier and its reason. Do not change a
  verifier. The plan stops until the operator decides
- a feed fetch fails, or returns a different medium than the table expects
- the parser output has no `rel` key. Task 001 is then not merged
- a publisher feed in the table lists no album of the expected kind

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0049-task-002-real-feed-fixtures.md`, the table "The Feeds"
- `tests/common/mod.rs`
- `tests/adr0043_release_date_tests.rs`, the helpers
  `test_app_state_with_crawl_token`, `ingest_payload` and `ingest`
- `stophammer-parser/src/bin/stophammer-parse.rs`

Goal:
- Store real publisher and album feeds, with their parser JSON, under
  `tests/fixtures/adr0049/`. Prove that the default verifier chain accepts the
  Wavlake artist feed `detox-artist`.

Constraints:
- Fetch each URL in the table once, with a `User-Agent` that names Stophammer
  and at least 2 seconds between requests to one host.
- Store each body unchanged. Make each `.feed_data.json` with `stophammer-parse`
  and `jq .`. Do not edit it.
- Write `tests/fixtures/adr0049/SOURCES.md`: URL, UTC fetch time, HTTP status,
  SHA-256 and parser commit for each file. Also record if
  `https://wavlake.com/feed/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a` gives the same
  body as `detox-album`. Do not store that second body.
- Add `pub fn adr0049_feed_data(name: &str) -> serde_json::Value` to
  `tests/common/mod.rs`. It reads a file and never fetches.
- No test touches the network.

Do not touch:
- `src/`
- `stophammer-parser`, `stophammer-crawler`
- `tests/fixtures/mock-rss/`

Acceptance criteria:
- The gate is green.
- Tests prove:
  - Each fixture has both files.
  - `detox-artist` is `publisher` with a `music` remote item.
  - `detox-album` has GUID `e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9` and names publisher `137aaa9c-75ff-4916-9f23-e02968b2d15e`.
  - `sirlibre-label` has a `rel` of `label`.
  - `jimmyv-publisher` has a `rel` of `producer`.
  - `no-publisher-album` names no publisher.
- A test submits `detox-artist` to `POST /ingest/feed` with the default chain
  and asserts `accepted == true`. If it fails, stop and report the verifier and
  the reason. Do not change a verifier.

Test commands:
- `cargo build`
- `cargo test --test adr0049_fixture_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
