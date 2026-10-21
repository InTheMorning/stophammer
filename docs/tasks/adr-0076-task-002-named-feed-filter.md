# ADR 0076 Task 002: The Named Feed Filter Of The Release Lists

Owner: [ADR 0076](../adr/0076-a-named-feed-entry-states-its-confirmation.md)
section 2. v4vmm request 9, gap B.

Repository: `stophammer`. The operator commits.

## Goal

`GET /v1/feeds/by-name` and `GET /v1/feeds/recent` take `named_feed_guid`.
With it, each route gives only the music releases that name that feed.

## Files To Inspect

- `src/query.rs`: `ListQuery`, `handle_get_feeds_by_name`,
  `handle_get_recent_feeds`, `by_name_feed_rows`, `recent_feed_rows`, the
  `artist_key` branch, `load_publisher`
- `src/db.rs`: the queries over `feed_remote_items` with the source
  `podcast_publisher` (ADR 0069 §1a)
- `src/openapi.rs`: the paths `/v1/feeds/recent` and `/v1/feeds/by-name`
- `tests/adr0071_artist_key_tests.rs`: the fixtures and the cursor test

## Files Likely To Change

- `src/query.rs`, `src/db.rs`, `src/openapi.rs`
- `tests/adr0076_named_feed_filter_tests.rs`, new
- `docs/API.md`

## Constraints

- **Set.** The filter keeps each music release with a `named_feeds` entry for
  that GUID. Use the same rows that `named_feeds` uses: each publisher item of
  the album, at each rank, confirmed or not. Do not add a second rule.
- **Not held.** A release that names a feed that the node does not hold is in
  the set for that GUID.
- **Order.** The order, `limit`, `cursor`, `from` and `include` do not change.
- **Unknown GUID.** A GUID that no release names gives an empty list and
  `200`.
- **`400`.** The node gives `400` for `named_feed_guid` with a `medium` other
  than `music`. It also gives `400` for `named_feed_guid` with `artist_key`.
  Each message names the conflict.
- **Contract.** Declare the parameter on both paths of the OpenAPI document.
- A request without the filter gives the rows of today.
- No migration, no event change and no ingest change.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- The `artist_key` set and the artist list in memory (ADR 0070 §4a)
- Each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add `named_feed_guid` to `ListQuery`.
2. Add a query that gives the release GUIDs that name one feed.
3. Give `recent_feed_rows` the same `only_guids` condition that
   `by_name_feed_rows` has.
4. Apply the filter and the two `400` rules in both handlers.
5. Declare the parameter, write the tests, then `docs/API.md`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0076_named_feed_filter_tests.rs`.

- Each route gives each music release that names the feed at rank 1 or rank 2.
  It gives no release that does not name it.
- A release that names the feed, and that the feed does not list, is in the
  result.
- A release that names a feed that the node does not hold is in the result
  for that GUID.
- Pages of `limit=1` over a cursor give each release one time, with no gap.
- `medium=publisher` with the filter gives `400`. `artist_key` with the filter
  gives `400`.
- A GUID that no release names gives an empty list.
- The ADR 0044 guards pass with the parameter declared.

## Test Commands

```bash
cargo build
cargo test --test adr0076_named_feed_filter_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report, with no workaround, when:

- A page with the filter takes more than 0.5 second on the newest backup in
  `backups/`.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. the page time with the filter, on the backup
5. deviations from this task
6. unresolved concerns
