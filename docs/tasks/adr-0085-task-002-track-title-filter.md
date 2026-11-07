# ADR 0085 Task 002: A Track Title Filter Reads Its Feed

Owner: [ADR 0085](../adr/0085-a-name-list-takes-the-role-filters.md) §4 to §6.

Repository: `stophammer`. This task has no dependency on task 001.

## Goal

`title` and `not_title` on a track row read the track title or the title of the feed of
the track. Each word and each phrase can match either title.

## Files To Inspect

- `src/search.rs`: `TextFilter::target`, `TextFilter::push_conditions`,
  `TextFilter::expression`
- `tests/adr0080_text_filter_tests.rs`
- `docs/API.md`: the `title` row of the search filters
- `src/openapi.rs`: `search_filter_params`
- ADR 0080 §2 and §3, and ADR 0085 §4 to §6

## Files Likely To Change

- `src/search.rs`
- `src/openapi.rs`
- `docs/API.md`
- `tests/adr0080_text_filter_tests.rs`, only for a test of the old `title` rule
- `tests/adr0085_name_filter_tests.rs`

## Do Not Touch

- `release`, `artist`, `kind` and `not_q`.
- The rank expression. A filter never changes `rank` (ADR 0080 §2).
- `src/query.rs` and the three search query forms, except for what the shared filter
  code needs.
- The ADR files, the index and `AGENTS.md`.

## Constraints

- All three query forms use `TextFilter::push_conditions`, so the change stays there.
- Today a value is one FTS5 `MATCH` with `AND` between its phrases. With this change, each
  phrase has its own "own row or feed row" condition.
- A track with no feed in the index stays under `not_title`, and it can still match `title`
  by its own title. `COALESCE` covers a null result.
- A search with no `type` has feed rows and track rows. A feed row reads only its own row.
- `type=feed` and `type=publisher` keep their current SQL.

## Steps

1. Change the condition for a track row.
2. Change each test that states the old rule, and name each one in the report.
3. Update the documents.
4. Write the tests.

## Acceptance Criteria

Mechanical, in `tests/adr0085_name_filter_tests.rs`: ADR 0085 criteria 5 to 9.

Visual: none.

## Test Commands

```bash
cargo test --test adr0085_name_filter_tests
cargo test --test adr0080_text_filter_tests
cargo test --test adr0084_sort_tests
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi
```

## Final Report

State the files changed, the gate result and each test that changed. Also state the time
of a `title` search on a copy of the production database. Measure a common word and a rare
word, with `q` and with no `q`.

## Escalation Triggers

- A `title` page with a common word takes more than approximately 2 s in a release build
  after a first read.
- A test outside the old `title` rule must change.
