# ADR 0078 Task 001: The Index Holds Only Searchable Text

Owner: [ADR 0078](../adr/0078-a-search-index-row-holds-only-searchable-text.md)
§1, §2 and §6. The phase plan is
[the search index and retired source plan](../plans/search-index-and-retired-source-phase-plan.md).

Repository: `stophammer`. The operator commits.

## Goal

`search_index` has the three columns of ADR 0078 §1, and it keeps a copy of
its text.
A delete is a `DELETE` by rowid with no old text. On the first open, one repair
replaces the old table and fills it from the source tables.

## Files To Inspect

- `src/schema.sql`: `search_index`, `search_entities`
- `src/search.rs`: `populate_search_index`, `delete_from_search_index`,
  `rowid_for`
- `src/db.rs`: `MIGRATIONS` (line 159), `open_db` (the repair calls near line
  821), `ensure_search_index_track_identity`,
  `ensure_search_index_publisher_feeds`, `sync_source_read_models_for_feed`
  (near line 1380), and each caller of the two index functions
- `src/api.rs`, `src/apply.rs`: each caller of the two index functions
- `docs/adr/0022-fts5-search-architecture.md`
- `docs/adr/0046-migration-versions-are-array-positions.md`
- The ten test files that call the two index functions: `tests/e2e_tests.rs`,
  `tests/availability_tests.rs`, `tests/hash_collision_tests.rs`,
  `tests/search_tests.rs`, `tests/search_one_hit_per_entity_tests.rs`,
  `tests/retire_remove_tests.rs`, `tests/adr0071_publisher_search_tests.rs`,
  `tests/crypto_security_tests.rs`, `tests/coverage_gap_tests.rs`,
  `tests/issue21_fts5_sanitize_tests.rs`

## Files Likely To Change

- `migrations/0054_search_index_text_columns.sql`, new. Use the next free
  number if 0054 is taken.
- `tests/fixtures/migration_sha256.txt`: one new line
- `src/schema.sql`, `src/search.rs`, `src/db.rs`
- `src/api.rs`, `src/apply.rs`: only the arguments of each call
- The test files above: only the arguments of each call
- `tests/migration_tests.rs`: each recorded version moves by one
- `tests/adr0078_search_index_tests.rs`, new

## Constraints

- **The table.** Use the definition of ADR 0078 §1 exactly. `search_entities`
  does not change.
- **The migration.** It drops `search_index`, creates the new table, and
  deletes each row of `search_entities`. It does not fill the table.
- **The repair.** Add `ensure_search_index_text_columns` in `src/db.rs`.
  - It runs when the `sql` of `search_index` in `sqlite_master` has
    `content=''`, or when `search_entities` has no row and `feeds` has a row
    (ADR 0078 §6). Otherwise it returns. It takes no flag from `open_db`.
  - In one transaction, it drops the table, creates it, deletes each row of
    `search_entities`, and calls `sync_source_read_models_for_feed` for each
    feed. This is the pattern of `ensure_search_index_track_identity`.
  - It logs one `tracing::info!` with the feed count and the time, as the other
    repairs do.
  - Call it in `open_db` before `ensure_search_index_track_identity` and
    before `ensure_search_index_publisher_feeds`.
- **The writer.** `populate_search_index` takes `conn`, `entity_type`,
  `entity_id`, `name`, `title` and `description`. An update runs
  `DELETE FROM search_index WHERE rowid = ?1` and then the insert. Keep the
  rowid collision check and the `search_entities` existence guard.
- **The delete.** `delete_from_search_index` takes `conn`, `entity_type` and
  `entity_id` only. It runs `DELETE FROM search_index WHERE rowid = ?1` and
  deletes the `search_entities` row. Keep the existence guard.
- **The medium.** No caller sends the medium to the index.
- **The callers.** Change only the arguments. Keep each `let _ =` as it is.
  Task 002 changes the error handling and the retire paths.
- **Comments.** Each new item names ADR 0078 and its section. Correct the
  comments that describe a delete with the old column values.

## Do Not Touch

- `sanitize_fts5_query` and its tests. Task 004 changes them.
- The `search` function, the quality join and `src/query.rs`. Task 003 and
  task 004 change them.
- The retire and remove paths, except the call arguments. Task 002 changes
  them.
- `rowid_for` and the hash of ADR 0022
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`, each existing migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the first test: an in-memory connection of the crate creates the table
   of §1.
2. Write the migration, its `MIGRATIONS` entry, its hash line and the change to
   `src/schema.sql`.
3. Change the two index functions and each caller.
4. Write the repair and its call in `open_db`.
5. Change the arguments in the ten test files. A test keeps its purpose.
6. Write the tests of the acceptance criteria.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0078_search_index_tests.rs`, unless the
item names a different file.

- ADR 0078 check 1: the `sql` of `search_index` in `sqlite_master` has
  `name`, `title` and `description`, and no `content=''`,
  `contentless_delete`, `entity_type`, `entity_id` or `tags`.
- Check 2: a test changes the title of a feed and of a track. A search for a
  word that only the old title had gives no hit. A search for a word of the new
  title gives the entity one time.
- Check 3: after the change, the bm25 rank of an unchanged entity equals its
  rank in a new database that holds the same rows.
- Check 7: `q=track`, `q=feed` and `q=music` give no hit for an entity that
  does not have the word in its text.
- Check 10: a database with the old definition opens with the new definition,
  and with one `search_entities` row for each entity of the source tables. A
  second open runs no repair.
- Check 11: a database with the old definition and a bare-GUID track row opens
  with no error.
- Check 12: a database with the old definition and a stale index row of a
  deleted feed opens with no row for that feed.
- `tests/migration_tests.rs` passes with the new migration.
- Each test of the ten changed files passes.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0078_search_index_tests
cargo test --test migration_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A test asserts that a medium word or an entity type word finds an entity.
- A caller needs the old text for a different purpose than the delete.
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
- `~/build/stophammer/AGENTS.md` (the code style, the migration steps and the pre-commit gate)
- `~/build/stophammer/docs/adr/0078-a-search-index-row-holds-only-searchable-text.md`, §1, §2 and §6
- `~/build/stophammer/docs/adr/0022-fts5-search-architecture.md`
- `~/build/stophammer/docs/adr/0046-migration-versions-are-array-positions.md`
- `~/build/stophammer/docs/tasks/adr-0078-task-001-index-holds-searchable-text.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- The index has only `name`, `title` and `description`, and keeps a copy of its text. A delete needs no old text. One repair at the first open replaces and fills the old table.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Run the new repair before each other repair in `open_db` that writes the index.
- Change only the arguments of each caller. Keep each `let _ =`.
- Stop at each escalation trigger of this task.

Do not touch:
- `sanitize_fts5_query`, the `search` function, `src/query.rs`, the retire logic, `rowid_for`
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`, each existing migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0078_search_index_tests`
- `cargo test --test migration_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
