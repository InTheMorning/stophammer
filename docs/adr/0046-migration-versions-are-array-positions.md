# ADR 0046: Migration Versions Are Array Positions

## Status
Accepted

## Date
2026-09-22

## Context
A migration's version is its 1-indexed position in the `MIGRATIONS` array in
`src/db.rs`. The file name is not the version. `run_migrations` applies an
entry only when its version is greater than the highest version that
`schema_migrations` records.

The two numbers have separated. The array holds 28 entries. A production
database records version 29. Every entry in the array is therefore already at
or below the recorded version, so a newly appended migration is assigned a
version that the database treats as applied. The runner skips it and reports
nothing.

This is not new. Two repairs already exist for the same fault:

- `ensure_feed_scoped_track_identity_schema`, for migration 0032
- `ensure_source_contributor_npub_schema`, for migration 0033

ADR 0043 added a third, `ensure_feed_last_build_date_schema`, for migration
0034. Without it the `feeds.last_build_date` column would never exist, the
server would start with no error, and each feed query would fail when it read
the missing column.

The cost of the fault is its silence. A skipped migration produces no error at
start and no error at deploy. It produces a defect later, in a response.

## Decision
A migration version stays an array position. The runner is unchanged.

1. When a migration cannot run on an existing database, the author adds an
   `ensure_<name>_schema` function in `src/db.rs` and calls it from `open_db`.
   The function tests for the effect and applies it when absent.
2. A repair does not write a `schema_migrations` row when the version it would
   claim belongs to an earlier migration.
3. `run_migrations` reports the condition at start. When the recorded version
   has reached the migration count, it emits a `tracing::error!` that names
   this ADR and the repair to write.
4. The guard reports and does not refuse to start. The condition is already
   true in production, so a guard that blocked start would stop the node.
5. `migrations_can_advance` holds the condition and carries unit tests.

`AGENTS.md` states this step in the migration procedure.

## Alternatives Considered

### Take the version from the file name

`0034_feed_last_build_date.sql` would become version 34, which is above the
recorded 29 and would run. Files 0030 to 0033 would then appear unapplied and
would run again. `ALTER TABLE ADD COLUMN` fails on a column that exists, so
this needs the same reconciliation as the option below. It costs the same and
returns less. Rejected.

### Record each migration by file name

This is what established migration tools do, and it removes the fault rather
than reporting it. It needs a reconciliation pass: for each of the 28 files,
decide what evidence shows that an existing database already has it. The
position numbering drifted, so the recorded 29 cannot be mapped to a file name.
That pass is the whole cost of this change.

Deferred, not rejected. The guard in this decision removes the urgency, because
the fault announces itself. Revisit this when the reconciliation is affordable.
A new decision record supersedes this one at that time.

### Leave the fault silent

Three repairs were written by hand, each one remembered by its author. A fourth
that is forgotten produces a defect in a response rather than a failure at
deploy. Rejected.

## Consequences

- Adding a column to an existing table needs two changes, not one: the
  migration file and the repair function.
- The error line appears at every start until the numbering is changed. That is
  intended. It is the record of a known fault.
- A fresh database is unaffected. `run_migrations` applies every entry, and
  `src/schema.sql` carries the same shape.
- The reconciliation cost is deferred, not paid.

## Invariants

- A migration version is its 1-indexed position in `MIGRATIONS`.
- A migration that cannot run has a repair that is called from `open_db`.
- The runner reports when a newly appended migration cannot run.

## Guards

This fault reached three migrations. It earns a test.

- `migrations_can_advance` returns false when the recorded version has reached
  the migration count, and true below it.
- `open_db_repairs_feed_last_build_date_when_0034_was_skipped` proves the
  repair path, beside the two equivalent tests for 0032 and 0033.
