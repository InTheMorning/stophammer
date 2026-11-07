# ADR 0085 Task 001: The Name List Takes The Role Filters

Owner: [ADR 0085](../adr/0085-a-name-list-takes-the-role-filters.md) §1 to §3.

Repository: `stophammer`. This task has no dependency on task 002.

## Goal

`GET /v1/feeds/by-name` takes `stated_rel` and `two_way_links`, with the rules of
ADR 0068 §4 and §5. A request with neither gives the same result as before.

## Files To Inspect

- `src/query.rs`: `LinkFactFilter`, `filtered_recent_rows`, `handle_get_recent_feeds`,
  `by_name_feed_rows`, `handle_get_feeds_by_name`, `feed_list_row_to_response`
- `src/openapi.rs`: the two paths
- `docs/API.md`: the two routes
- ADR 0068 §4 and §5, ADR 0070 §2, ADR 0084 §3 and §7

## Files Likely To Change

- `src/query.rs`
- `src/openapi.rs`
- `docs/API.md`
- New: `tests/adr0085_name_filter_tests.rs`

## Do Not Touch

- The rows, the cursor and the scan limit of `GET /v1/feeds/recent`.
- `src/search.rs`. Task 002 owns it.
- The capability list. ADR 0085 §3 adds no token.
- The ADR files, the index and `AGENTS.md`.

## Constraints

- `LinkFactFilter::from_params` gives the checks. Do not write them again.
- The scan limit and the batch size are those of `filtered_recent_rows`. The limit must not
  drift between the two routes, so share the constant, and share the loop if this stays simple.
- `from` applies to the first batch only, as a request with `cursor` ignores `from`.
- `reverse` and the `r.` cursor work with a filter.
- A filter gives the link facts on the row, as on `GET /v1/feeds/recent`.

## Steps

1. Read the filter in `handle_get_feeds_by_name`.
2. Add the filtered read of the name list.
3. Update the contract and the documents.
4. Write the tests.

## Acceptance Criteria

Mechanical, in `tests/adr0085_name_filter_tests.rs`: ADR 0085 criteria 1 to 4.
`tests/adr0084_sort_tests.rs` and the ADR 0044 guards pass with no edit.

Visual: none.

## Test Commands

```bash
cargo test --test adr0085_name_filter_tests
cargo test --test adr0084_sort_tests
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi
```

## Final Report

State the files changed, the gate result, and the time of a filtered page on a copy of
the production database.

## Escalation Triggers

- A filtered page takes more than approximately 200 ms in a release build.
- A rule of ADR 0085 conflicts with ADR 0068 or ADR 0070.
