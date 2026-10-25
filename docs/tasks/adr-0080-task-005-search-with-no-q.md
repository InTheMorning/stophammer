# ADR 0080 Task 005: A Search With Filters And No `q`

Owner: [ADR 0080](../adr/0080-a-search-takes-phrases-and-field-filters.md)
§4 (`missing_positive_term`), §6 and §7. The phase plan is
[the search filters plan](../plans/search-filters-phase-plan.md).

Repository: `stophammer`. The operator commits. This task starts after task
003 and task 004.

## Goal

A request can have filters and no word or phrase in `q`. It gives the rows of
the index that match the filters, newest first. Its cursor holds the
date and the rowid. A search with `q` does not change.

## Files To Inspect

- `src/search.rs`: `search`, `SearchFilters`, the conditions of tasks 002 to
  004
- `src/query.rs`: `handle_search`, the check sequence of task 002,
  `encode_cursor` and `decode_cursor` (line 44)
- `src/schema.sql`: `search_entities`, `idx_feeds_newest`,
  `idx_tracks_pub_date`
- `src/openapi.rs`, `docs/API.md`: `/v1/search`
- `tests/adr0080_text_filter_tests.rs`: the interim `missing_positive_term`
  case

## Files Likely To Change

- `src/search.rs`, `src/query.rs`, `src/openapi.rs`, `docs/API.md`
- `tests/adr0080_text_filter_tests.rs`: only the interim case
- `tests/adr0080_no_q_search_tests.rs`, new

## Constraints

- **When it runs.** The handler runs this search when `q` has no word and no
  phrase, and the request has one or more positive filters of §2. A request
  with no word, no phrase and no positive filter continues to give
  `missing_positive_term`.
- **The rows.** The query starts from `search_entities`, with the type filter
  and the joins of task 003. Thus it gives only the rows that the index holds,
  and the medium rules of ADR 0071 §5 apply. Each filter and each exclusion of
  tasks 002 to 004 applies with no change.
- **The date.** The date of a row is `f.newest_item_at` for a `feed` and a
  `publisher` row, and `t.pub_date` for a `track` row.
- **The order.** `ORDER BY date DESC NULLS LAST, e.rowid ASC`. A request with
  no `type` gives feeds and tracks in one query with that order. It does not
  merge two lists.
- **The rank.** `rank` is 0 on each row. `quality_score` comes from the quality
  join of ADR 0078 §4, as for a search with `q`.
- **The cursor.** The cursor of this search encodes `d`, the date or the empty
  text, and the rowid, separated by `\0`. The next page starts after that
  position in the order above. A row with no date has a position after each
  dated row.
- **Two forms.** A cursor with the `d` form on a search with `q` gives `400`.
  A cursor of a search with `q` on this search gives `400`. Use the message
  `invalid cursor format`.
- **The interim case.** Task 002 made a request with a filter and no `q` give
  `missing_positive_term`. Change that test to expect `200`.
- **The contract.** `q` is not required in `src/openapi.rs`. Its description
  says that a request with no word gives its rows newest first, and needs a
  filter. `docs/API.md` gives the order, the date of each type, the rank of 0
  and the rule of the two cursor forms. It says that most publisher feeds have
  no date, so such a search gives them in rowid order.
- **Comments.** Each new item names ADR 0080 §6.

## Do Not Touch

- The order, the rank and the cursor of a search with `q`
- The parse of each filter value, and the error body
- The index functions, the repairs and the migrations
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the tests of the acceptance criteria.
2. Add the query of this search in `src/search.rs`, with the conditions of
   tasks 002 to 004.
3. Add the cursor form and the branch in `handle_search`.
4. Change the interim test, the contract and the document.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0080_no_q_search_tests.rs`.

- ADR 0080 check 13: `type=feed&kind=unknown` gives the feeds newest first by
  `newest_item_at`. A feed with no date comes last. Feeds with the same date
  come in rowid order.
- Check 13: with `limit=2`, the pages of the cursor give each matching row one
  time, with no row two times. This includes the rows with no date.
- `type=track&min_duration=1` gives the tracks newest first by `pub_date`.
- `kind=unknown` with no `type` gives feeds and tracks in one list, newest
  first, and gives no publisher.
- A publisher feed is a row of `type=publisher&host=x.com`, and a music feed is
  not. A feed with no medium is not a row of any type.
- Each row has `rank` 0.
- Check 14: a cursor of a search with `q` gives `400` on a search with no `q`.
  A cursor of a search with no `q` gives `400` on a search with `q`.
- A request with only `not_title=x` gives `missing_positive_term`. A request
  with no parameter gives `missing_positive_term`.
- Each test of the search test files passes, other than the interim case that
  this task changes.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0080_no_q_search_tests
cargo test --test adr0080_text_filter_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A search with no `q` takes more than 200 ms on a copy of the backup of
  2026-10-24.
- The bundled SQLite does not accept `NULLS LAST`.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/AGENTS.md` (the code style, the API steps and the pre-commit gate)
- `~/build/stophammer/docs/adr/0080-a-search-takes-phrases-and-field-filters.md`, §4, §6 and §7
- `~/build/stophammer/docs/tasks/adr-0080-task-005-search-with-no-q.md` (this task)
- `~/build/stophammer/docs/tasks/adr-0080-task-002-text-filters-and-error-codes.md` and `adr-0080-task-003-value-filters.md` (the filters and the joins)
- the files of "Files To Inspect" in this task

Goal:
- A request with filters and no word in `q` gives the matching index rows newest first. Each row has `rank` 0, and the cursor holds the date and the rowid.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Start from `search_entities`, so that only index rows show.
- A cursor of one form gives `400` on the other form.
- Stop at each escalation trigger of this task.

Do not touch:
- the order, the rank and the cursor of a search with `q`, the value parse, the error body, the repairs, the migrations
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0080_no_q_search_tests`
- `cargo test --test adr0080_text_filter_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cargo run --bin gen_openapi > /dev/null`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
