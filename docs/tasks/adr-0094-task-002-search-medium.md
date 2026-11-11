# ADR 0094 Task 002: A Search Takes The Medium

Owner: [ADR 0094](../adr/0094-a-feed-hit-states-its-medium-kind-and-date.md) §1 and
the capability `search_medium` of §4.

Repository: `stophammer`. Do this task after task 001 and task 003.

## Goal

`GET /v1/search` takes `medium`, a comma list of `music` and `musicL`. A value
that names `musicL` adds the `playlist` rows of task 001 to the feed rows. A
playlist hit gives the `entity_type` `feed`. A search with no `medium` gives the
same rows as before.

## Files To Inspect

- `src/query.rs`: `parse_search_request` and its two checks, `SEARCH_FILTERS`,
  `handle_search`, the map of each hit, `handle_capabilities`
- `src/search.rs`: `RankScope`, `match_source`, `rank_search`, `search_filtered`,
  `search_by_date`, `search_sorted`, `FEED_ROW`, `TextFilter::target`, the quality
  join `CASE e.entity_type WHEN 'publisher' THEN 'feed'`, the field groups of
  ADR 0087
- `src/openapi.rs`: the parameters of `GET /v1/search`
- `docs/API.md`: `GET /v1/search` and `GET /v1/node/capabilities`
- `tests/adr0080_value_filter_tests.rs`, `tests/adr0087_match_group_tests.rs`,
  `tests/adr0084_sort_tests.rs`
- ADR 0080 §2 and §7, ADR 0084, ADR 0086, ADR 0087

## Files Likely To Change

- `src/query.rs`
- `src/search.rs`
- `src/openapi.rs`
- `docs/API.md`
- New: `tests/adr0094_search_medium_tests.rs`

## Do Not Touch

- The index writes and the repair of task 001.
- The three fields of task 003.
- The ADR files, the index and `AGENTS.md`.

## Constraints

- `medium` changes which rows a search reads. It is not a text filter or a value
  filter. Do not add it to `SEARCH_FILTERS`. Check 2 of `parse_search_request`
  requires a filter with no `type` to apply to `feed` and to `track`. Read
  `medium` in the same way as `sort` and `include`.
- With no `type`, `medium` changes only the feed rows. The track rows stay.
  `medium=musicL` with no `type` thus gives the playlists and the tracks.
- Errors, each with `search_filter_error_response`:
  - `medium` with `type=track` or `type=publisher` gives `400` with
    `unsupported_filter`.
  - A list with a value other than `music` and `musicL`, an empty value or an
    empty item gives `400` with `invalid_filter`.
  - A repeated `medium` parameter gives the error of each other repeated
    parameter.
- The row scope is a set of row types. Each of the three search paths reads the
  same set: the search with `q`, the search with no `q`, and the search with a
  `sort`.
- A `playlist` row acts as a `feed` row in each part of a search. These parts
  are the text filters, the value filters and the host filter of ADR 0080. They
  are also the quality join, the field groups of ADR 0087 and the sort keys of
  ADR 0084.
- A playlist hit gives the `entity_type` `feed`. Map the row type in the response
  only. The cursor stays the cursor of the row.
- Add `search_medium` to the end of the capability list, after the capability of
  task 003.
- Regenerate the OpenAPI document with `cargo run --bin gen_openapi`.

## Steps

1. Read and check `medium` in `parse_search_request`.
2. Give the row scope to each of the three search paths.
3. Make a `playlist` row act as a `feed` row in each part of a search.
4. Map a `playlist` row to the `entity_type` `feed` in the response.
5. Add the capability.
6. Update the contract and the documents.
7. Write the tests.

## Acceptance Criteria

Mechanical, in `tests/adr0094_search_medium_tests.rs`:

- ADR 0094 criterion 2: a `musicL` feed is a hit of
  `type=feed&medium=music,musicL` and of `type=feed&medium=musicL`, by a word of
  its title and by a prefix. A music feed is a hit of the first and not of the
  second. Test each of the three search paths.
- A playlist hit gives the `entity_type` `feed` and the `medium` `musicL`.
- With no `type`, `medium=musicL` gives the playlist and the tracks of a music
  feed, and not the music feed.
- `medium=music,musicL&min_tracks=1` gives no playlist.
- ADR 0094 criterion 3: `medium` with `type=track` and with `type=publisher`
  gives `400` with `unsupported_filter`. `medium=publisher`, `medium=` and
  `medium=music,,musicL` give `400` with `invalid_filter`.
- A cursor over pages of `type=feed&medium=music,musicL` gives each row one time.
- ADR 0094 criterion 7, the part of this task: the capabilities list
  `search_medium`.

The present search tests pass with no edit. The tests of task 001 pass, and they
prove that a search with no `medium` reads no `playlist` row.

Visual: none.

## Test Commands

```bash
cargo test --test adr0094_search_medium_tests
cargo test --test adr0094_playlist_index_tests
cargo test --test adr0087_match_group_tests
cargo test --test adr0084_sort_tests
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi
```

## Final Report

State the files changed and the gate result. Give the hits of
`q=Deck%20Darlings&type=feed&medium=music,musicL` on a copy of the production
database. Give the time of that search and of the same search with no `medium`.

## Escalation Triggers

- A search with no `medium` gives different rows or a different rank than before.
- A search with `medium` takes more than approximately 2 times the time of the
  same search with no `medium`.
- A part of a search cannot treat a `playlist` row as a `feed` row with a small
  change.
