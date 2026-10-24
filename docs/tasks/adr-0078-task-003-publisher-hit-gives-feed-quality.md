# ADR 0078 Task 003: A Publisher Hit Gives The Quality Of Its Feed

Owner: [ADR 0078](../adr/0078-a-search-index-row-holds-only-searchable-text.md)
§4. The phase plan is
[the search index and retired source plan](../plans/search-index-and-retired-source-phase-plan.md).

Repository: `stophammer`. The operator commits. This task starts after task
002.

## Goal

A `publisher` hit of `GET /v1/search` gives the quality score of the `feed`
quality row of the same feed. The rank and the cursor use that score.

## Files To Inspect

- `src/search.rs`: `search` (near line 263), the `eff_rank_expr`, the keyset
  `WHERE` clause and the join on `entity_quality` (near line 337)
- `src/quality.rs`: `store_quality`
- `src/db.rs`: the `store_quality` call near line 1419
- `src/query.rs`: the search handler (near line 5080) and the cursor encode
- `tests/adr0071_publisher_search_tests.rs`

## Files Likely To Change

- `src/search.rs`
- `tests/adr0078_search_index_tests.rs`: new tests at the end of the file

## Constraints

- **The join.** The join on `entity_quality` uses the type `feed` when the hit
  has the type `publisher`. One method:
  `q.entity_type = CASE e.entity_type WHEN 'publisher' THEN 'feed' ELSE e.entity_type END`.
- **One expression.** The `SELECT`, the `ORDER BY` and the keyset `WHERE`
  clause use the same quality value. A difference makes a cursor skip or repeat
  a row.
- **No new rows.** No code writes a quality row of the type `publisher`.
- **Comments.** The change names ADR 0078 §4.

## Do Not Touch

- `src/quality.rs` and each `store_quality` call
- The MATCH query, `sanitize_fts5_query` and the index functions
- The response fields of `GET /v1/search`
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Change the join and each place that reads the quality value.
2. Write the tests.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0078_search_index_tests.rs`.

- ADR 0078 check 6: the `quality_score` of a `publisher` hit equals the score
  of the `feed` quality row of that feed. It is not 0 when that score is above
  0.
- A test makes three publisher feeds with different scores. A search with
  `type=publisher` and `limit=1` then gives each feed one time across the pages
  of the cursor, in the sequence of the effective rank.
- The quality of a `feed` hit and of a `track` hit does not change.
- Each test of `tests/adr0071_publisher_search_tests.rs` passes.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0078_search_index_tests
cargo test --test adr0071_publisher_search_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A test asserts that a publisher hit gives `quality_score` 0.
- A code path writes or reads a quality row of the type `publisher`.
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
- `~/build/stophammer/AGENTS.md` (the code style and the pre-commit gate)
- `~/build/stophammer/docs/adr/0078-a-search-index-row-holds-only-searchable-text.md`, §4
- `~/build/stophammer/docs/tasks/adr-0078-task-003-publisher-hit-gives-feed-quality.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- A `publisher` search hit gives the quality of the `feed` quality row of its feed, and the rank and the cursor use that value.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Use one quality expression in the select, the order and the keyset filter.
- Write no quality row of the type `publisher`.
- Stop at each escalation trigger of this task.

Do not touch:
- `src/quality.rs`, the MATCH query, `sanitize_fts5_query`, the index functions, the response fields
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0078_search_index_tests`
- `cargo test --test adr0071_publisher_search_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
