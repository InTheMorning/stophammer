# ADR 0084 Task 002: The Three List Routes Take `reverse`

Owner: [ADR 0084](../adr/0084-a-search-and-a-list-take-a-sort.md) §3, §6, §7 and §9.

Repository: `stophammer`. This task has no dependency on task 001.

## Goal

`GET /v1/feeds/recent`, `GET /v1/feeds/by-name` and `GET /v1/artist-names` take
`reverse=true`. A request with no `reverse` gives the same result as before.

## Files To Inspect

- `src/query.rs`: `ListQuery`, `recent_feed_rows`, `filtered_recent_rows`,
  `parse_recent_cursor`, `by_name_feed_rows`, `parse_by_name_cursor`,
  `handle_get_artist_names`, `parse_artist_names_cursor`
- `src/openapi.rs`: the three paths
- `docs/API.md` and `api.html`: the three routes
- `tests/fixtures/v1-schema-fields.json`

## Files Likely To Change

- `src/query.rs`
- `src/openapi.rs`
- `docs/API.md`
- `api.html`
- `tests/fixtures/v1-schema-fields.json`
- `tests/adr0084_sort_tests.rs`

## Do Not Touch

- `src/search.rs` and `handle_search`. Task 003 owns them.
- `GET /v1/node/capabilities`. Task 003 adds `sort` when the full contract exists.
- The filters of each route.

## Constraints

- A row with no `newest_item_at` comes last in the two directions (§6). The key
  `COALESCE(newest_item_at, -1)` does not give this rule in the reversed direction.
- With `reverse=true`, `from` starts at the last row whose key starts with the key of
  `from` (§7). One function gives this limit to `by_name_feed_rows` and to the artist
  list. Comparison is by code point.
- The tie key follows the direction (§6).
- A reversed cursor names its direction. A cursor with a different direction gives
  `400`, and a cursor from before this change continues in the usual direction (§9).
- Each form of each cursor must fail cleanly. The name in a by-name cursor is any text,
  so the mark of the direction must not be read as part of a name.
- `filtered_recent_rows` follows the direction, and keeps its scan limit (ADR 0068 §4).
- `reverse` accepts only `true` and `false`, and any other value gives `400`.

## Steps

1. Add `reverse` to `ListQuery`, and give the direction to each row function.
2. Change each cursor, with the old form still accepted.
3. Add the `from` limit.
4. Update the contract and the documents.
5. Write the tests.

## Acceptance Criteria

Mechanical, in `tests/adr0084_sort_tests.rs`:

- ADR 0084 criterion 1, for the three routes: each page in sequence gives the reversed
  list, with no row that repeats or goes missing.
- Criterion 3, for the three routes: a request with no `reverse` does not change.
- Criterion 5, on `GET /v1/feeds/recent`.
- Criterion 6: `from=M&reverse=true` starts at the last title that starts with "m", on
  `GET /v1/feeds/by-name` and `GET /v1/artist-names`.
- Criterion 7, for the three routes.
- The ADR 0044 guards pass.

Visual: none.

## Test Commands

```bash
cargo test --test adr0084_sort_tests
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi
```

## Final Report

State the files changed, the gate result, and the new form of each cursor.

## Escalation Triggers

- A reversed page takes more than approximately 50 ms on a copy of the production
  database.
- A rule of ADR 0084 conflicts with ADR 0068 §4 or ADR 0070.
