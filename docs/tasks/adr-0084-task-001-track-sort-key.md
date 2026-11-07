# ADR 0084 Task 001: A Track Stores Its Sort Key

Owner: [ADR 0084](../adr/0084-a-search-and-a-list-take-a-sort.md) §8.

Repository: `stophammer`. This task has no dependency, and task 003 needs it.

## Goal

Each track row has `title_sort`, equal to `model::sort_key(title)`. No API changes.

## Files To Inspect

- `migrations/0051_feed_title_sort.sql`
- `src/db.rs`: `ensure_feed_title_sort_schema`, `ensure_feed_title_sort_filled`, and
  the `INSERT INTO tracks` and `title_lower = excluded.title_lower` statements
- `src/api.rs` and `src/apply.rs`: where a write builds a track
- `AGENTS.md`, "New Database Migration"

## Files Likely To Change

- New: `migrations/0062_track_title_sort.sql`
- `src/db.rs`
- `src/schema.sql`
- `tests/fixtures/migration_sha256.txt`
- `docs/schema-reference.md`
- New: `tests/adr0084_sort_tests.rs`

## Do Not Touch

- `src/query.rs` and `src/search.rs`. Tasks 002 and 003 own them.
- Each committed migration (ADR 0023).

## Constraints

- The column has no index (ADR 0084 §8).
- SQLite cannot make the key. A Rust fill in `open_db` sets each null value, as
  `ensure_feed_title_sort_filled` does for feeds.
- Each write of a track derives the value, also a write that applies a signed event.
- The event format does not change. The value is local to each node.

## Steps

1. Add the migration and its line in `MIGRATIONS` and in the hash fixture.
2. Add `ensure_track_title_sort_schema` and `ensure_track_title_sort_filled`, and call
   them from `open_db` (ADR 0046 item 1).
3. Set `title_sort` in each statement that writes `tracks.title`.
4. Add the column to `src/schema.sql` and `docs/schema-reference.md`.
5. Write the tests.

## Acceptance Criteria

Mechanical:

- ADR 0084 criterion 8: `tracks.title_sort` equals `model::sort_key(title)` after the
  fill and after each form of write.
- `tests/adr0046_schema_sql_tests.rs` and `tests/adr0023_migration_hash_tests.rs` pass.

Visual: none.

## Test Commands

```bash
cargo test --test adr0084_sort_tests
cargo test --test migration_tests
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Final Report

State the files changed, the gate result, and each statement that writes
`tracks.title`.

## Escalation Triggers

- A write path sets a track title with no statement that this task can change.
- The fill takes more than a few seconds on a copy of the production database.
