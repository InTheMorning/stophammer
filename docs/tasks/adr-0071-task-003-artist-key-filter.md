# ADR 0071 Task 003: The Releases Of A Name Row

Owner: [ADR 0071](../adr/0071-a-list-row-links-what-it-names.md) section 4.
v4vmm request 8, gap C.

Repository: `stophammer`. The operator commits.

## Goal

`GET /v1/feeds/by-name?artist_key=<key>` gives the music releases that the
`name` row with that `key` counts in `release_count`.

## Files To Inspect

- `src/query.rs`: `ListQuery`, `handle_get_feeds_by_name`, `by_name_feed_rows`,
  `build_artist_name_rows`, `ArtistNameGroup`, `ArtistNameCache`,
  `ArtistNameCacheEntry`, `is_name_level_release`, `sort_key`
- `src/openapi.rs`: the path `/v1/feeds/by-name`
- `tests/adr0070_release_list_tests.rs`, `tests/adr0070_artist_list_tests.rs`

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`
- `tests/adr0071_artist_key_tests.rs`, new
- `docs/API.md`

## Constraints

- **One set.** `ArtistNameGroup` keeps the feed GUIDs that it counts. The copy
  in memory keeps them for each `name` key. The filter reads the set from the
  copy, so the count and the list come from one build.
- **No second rule.** Do not test the name level again in SQL.
- **Order and cursor.** The filtered list keeps the order, `limit`, `cursor` and
  `from` of the release list (ADR 0070 §2).
- **No row.** A key with no `name` row gives an empty list with `200`.
- **Medium.** `artist_key` with a `medium` other than `music` gives `400`.
- Declare `artist_key` in the document.
- No migration, no event change and no ingest change.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- The name level rule of ADR 0070 §4 and the sort key of ADR 0070 §1
- Each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Keep the feed GUIDs of each group in the build and in the copy in memory.
2. Add `artist_key` to `ListQuery`, and filter the rows of
   `handle_get_feeds_by_name` by the set.
3. Write the tests, then `docs/API.md`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0071_artist_key_tests.rs`.

- For a `name` row with `release_count` N, the filter gives N releases over
  all pages, with no gap and no repeat.
- A release of the same artist text whose link has an agreed `artist` role is
  not in the list.
- A key with no `name` row gives an empty list.
- `artist_key` with `medium=publisher` gives `400`.
- The ADR 0044 guards and the ADR 0071 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0071_artist_key_tests
cargo test --test adr0070_release_list_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report, with no workaround, when:

- The filter needs a database read for each release of the set.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns
