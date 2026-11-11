# ADR 0094 Task 001: The Index Holds A Playlist As Its Own Row Type

Owner: [ADR 0094](../adr/0094-a-feed-hit-states-its-medium-kind-and-date.md) §1a.

Repository: `stophammer`. This task has no dependency. Task 002 needs it.

## Goal

The search index holds each `musicL` feed as a row of the type `playlist`. No
search reads such a row. Task 002 adds the parameter that reads it.

## Files To Inspect

- `src/db.rs`: `sync_source_read_models_for_feed`, `ensure_search_index_publisher_feeds`
  and its call in the open sequence, `delete_feed_sql`
- `src/medium.rs`: `is_musicl`
- `src/search.rs`: `RankScope`, `match_source`, `rank_search`, `search_filtered`,
  `search_by_date`, `search_sorted`, `FEED_ROW`, `TextFilter::target`
- `src/query.rs`: `handle_search`, the three calls of the search functions
- `tests/adr0071_publisher_search_tests.rs`, `tests/adr0078_retire_index_tests.rs`
- ADR 0071 §5, ADR 0078 §3

## Files Likely To Change

- `src/db.rs`
- `src/search.rs`, only if a scope can read a `playlist` row
- New: `tests/adr0094_playlist_index_tests.rs`

## Do Not Touch

- The search parameters and the response. Tasks 002 and 003 own them.
- The capability list.
- The ADR files, the index and `AGENTS.md`.

## Constraints

- A feed is at most one of the row types `feed`, `publisher` and `playlist`. Add
  `("playlist", is_musicl)` to the loop of `sync_source_read_models_for_feed`.
  Then a change of the medium deletes the row of the type that the feed left. Correct the
  comment of the loop, which states that a `musicL` feed holds no row.
- `delete_feed_sql` deletes the `playlist` row of the feed, as it deletes the
  `feed` row and the `publisher` row (ADR 0078 §3).
- Make one repair for the two row types: a missing `publisher` row and a missing
  `playlist` row. Do not copy the function. The repair only adds rows, and a
  second open adds none.
- `RankScope::All` reads each row type. Find each path that can use it, and prove
  that no search with a valid `type`, or with no `type`, reads a `playlist` row.
  If a path can read one, restrict it in this task.
- The repair reads only stored rows. It does not crawl and does not emit an event.

## Steps

1. Add the row type to `sync_source_read_models_for_feed` and `delete_feed_sql`.
2. Make the repair cover the `playlist` rows.
3. Examine each search scope for `playlist` rows.
4. Write the tests.

## Acceptance Criteria

Mechanical, in `tests/adr0094_playlist_index_tests.rs`:

- ADR 0094 criterion 1: a `musicL` feed is not a hit of `type=feed`, of
  `type=publisher` or of no `type`. Test each of the three search paths: a
  search with `q`, a search with no `q`, and a search with `sort=name`.
- ADR 0094 criterion 4: a change of the medium from `musicL` to `music` and back
  leaves one row of the correct type in `search_entities`.
- ADR 0094 criterion 5: the first open of a database with a `musicL` feed and no
  `playlist` row adds the row, and a second open adds none.
- A retire of a `musicL` feed deletes its `playlist` row.

`tests/adr0071_publisher_search_tests.rs` and `tests/adr0078_retire_index_tests.rs`
pass with no edit.

Visual: none.

## Test Commands

```bash
cargo test --test adr0094_playlist_index_tests
cargo test --test adr0071_publisher_search_tests
cargo test --test adr0078_retire_index_tests
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Final Report

State the files changed and the gate result. Name each search path that you
examined for `playlist` rows, and the result. Give the time of the repair on a
copy of the production database.

## Escalation Triggers

- A search path reads `playlist` rows, and a restriction changes the rows of a
  present search.
- The repair takes more than approximately 1 s on a copy of the production
  database.
