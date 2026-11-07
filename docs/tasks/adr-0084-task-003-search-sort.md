# ADR 0084 Task 003: Search Takes `sort` And `reverse`

Owner: [ADR 0084](../adr/0084-a-search-and-a-list-take-a-sort.md) §2 to §5, §9 to §11.

Repository: `stophammer`. This task needs tasks 001 and 002.

## Goal

`GET /v1/search` takes `sort` and `reverse`. A request with neither parameter gives the
same sequence and fields as before. `GET /v1/node/capabilities` lists `sort`.

## Files To Inspect

- `src/search.rs`: `search_filtered`, `search_by_date`, `ValueFilters::joins`
- `src/query.rs`: `parse_search_request`, `handle_search`, `SearchResponseItem`,
  `handle_capabilities`
- `src/openapi.rs`: `/v1/search`
- `docs/API.md`: the search section and its error codes
- ADR 0080 §4 and §6

## Files Likely To Change

- `src/search.rs`
- `src/query.rs`
- `src/openapi.rs`
- `docs/API.md`
- `api.html`
- `tests/fixtures/v1-schema-fields.json`
- `tests/adr0084_sort_tests.rs`

## Do Not Touch

- The rank expression, and each filter condition. A filter never changes `rank`
  (ADR 0080 §2).
- `migrations/`. Task 001 owns the column.

## Constraints

- The checks of ADR 0080 §4 run first. Then the checks of ADR 0084 §5 run.
- `sort=relevance` and the default sort with a word keep the current query.
- `sort=name` and `sort=date` work with a word and with no word, for each `type` and for
  no `type`. The search with no `type` merges feeds and tracks by the key.
- A row with no key comes last in the two directions (§6).
- The cursor names the sort and the direction (§9). The current rank cursor and the
  current `d` cursor stay valid for the default sort.
- With a word, `rank` keeps its value on each sort. With no word, it stays 0.
- `sort_name` and `sort_date` appear only when the request sends that `sort` (§10).
- The capability `sort` comes last, because it tells a client that the full contract
  exists (§11).

## Steps

1. Read and check `sort` and `reverse` in `parse_search_request`.
2. Add one query form for `name` and `date`, with the direction.
3. Add the cursor forms.
4. Add `sort_name` and `sort_date` to `SearchResponseItem`.
5. Add the capability.
6. Update the contract and the documents, with the three new codes.
7. Write the tests.

## Acceptance Criteria

Mechanical, in `tests/adr0084_sort_tests.rs`:

- ADR 0084 criterion 1, for search.
- Criterion 2: each sort, with a word and with no word, for each `type` and for no
  `type`.
- Criterion 3: a search with no new parameter does not change.
- Criterion 4: each code of §5.
- Criterion 5, on search.
- Criterion 7, on search.
- Criterion 9: the capability.
- Criterion 10: the ADR 0044 guards.

Visual: none in this repository.

## Test Commands

```bash
cargo test --test adr0084_sort_tests
cargo test --test adr0080_no_q_search_tests
cargo test --test adr0080_text_filter_tests
cargo test --test adr0080_value_filter_tests
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi
```

## Final Report

State the files changed, the gate result and the new form of each cursor. Also state the
time of each query form on a copy of the production database.

## Escalation Triggers

- A page of `sort=name` takes more than approximately 2 s after a first read. ADR 0084 §8
  then allows an index, and the operator decides.
- A rule of ADR 0084 conflicts with ADR 0080.
