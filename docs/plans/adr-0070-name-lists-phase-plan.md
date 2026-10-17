# ADR 0070 Phase Plan: Lists In Name Order

## Status

Open - 2026-10-17. This plan is advisory.
[ADR 0070](../adr/0070-a-name-list-gives-each-identity-one-row.md) owns each
rule. v4vmm request 6 asks for the two lists.

## Goal

The node gives two lists in name order, with a cursor:

- `GET /v1/feeds/by-name`: the releases of one medium.
- `GET /v1/artist-names`: one row for each publisher feed, and one row for each
  name of the name level.

Each row gives its `sort_name`, so v4vmm can merge its Library rows with the
same key.

## Non-Goals

The non-goals of ADR 0070 apply. Also, this plan does not change a crawler or
the parser, and it does not change a signed event.

## Assumptions

- The backup `backups/stophammer-20260926T131633Z.db` is a sufficient copy of
  the production data for the time measurements. Open it with `?immutable=1`,
  or measure on a copy.
- The functions of the publisher view give `role_agreement` and the role
  tokens of each link of an album (ADR 0049 §6a).
- `ensure_search_index_track_identity` in `src/db.rs` is the model for a data
  fill when the database opens.

## Affected Modules

| Module | Task | Change |
|---|---|---|
| `migrations/`, `src/schema.sql` | 001 | Migration 0051 adds `feeds.title_sort` and its index |
| `model` | 001 | `sort_key`, the one function of the key |
| `db` | 001 | The feed upsert writes `title_sort` from `title`. `open_db` fills each null value |
| `query` | 001, 002 | The two routes. The artist count of ADR 0049 §7 uses `sort_key` |
| `openapi` | 001, 002 | The two paths and the row types |

## Proposed Sequence

1. Task 001: the sort key, the migration and `GET /v1/feeds/by-name`.
2. Review of task 001 against the checklist.
3. Task 002: `GET /v1/artist-names`. It uses the sort key function of task 001.
4. Review of task 002 against the checklist.
5. The operator commits, and a release candidate carries both routes.
6. v4vmm checks the deployed routes. Then v4vmm packet 016 can start.

## Schema And API Implications

- Migration 0051 is additive. The fill at open writes one value for each feed.
  A community node gets the column from the migration, and no event carries
  it.
- Two new `v1` routes. No field of an existing route changes (ADR 0044 §4).
- `/v1/node/capabilities` lists the two routes.

## Risk Areas

- **The cost of the artist list.** One full build reads the link facts of
  each music release: 8,641 on the backup, about 200 ms in a release build.
  ADR 0070 §4a keeps the list in memory, and builds it again after a change.
- **A write with no event.** The memory copy uses the highest event `seq` as
  its change marker. A write that changes the list and writes no event leaves
  the copy stale. Task 002 verifies each write.
- **The fill at open.** It must run one time, in one transaction, and it must
  not block a large database for long. Task 001 measures it on the backup.
- **Two sort rules.** A second lowercase in SQL (`lower()`) gives a different
  key for a character that is not ASCII. Only the Rust function makes a key.
- **The cursor of the artist list** has three parts. An error gives a gap or a
  repeat at a page boundary.

## Test Strategy

- Integration tests in `tests/adr0070_release_list_tests.rs` and
  `tests/adr0070_artist_list_tests.rs`, with the fixtures of the ADR 0068
  tests.
- `tests/migration_tests.rs` for migration 0051 and the fill.
- The ADR 0044 guards for the contract.
- The full pre-commit gate of `AGENTS.md`.

## Rollback Strategy

- Before a release: the operator does not commit the change of the task.
- After a release: the next release removes the two routes. The column
  `title_sort` stays, because a migration is never rewritten. A column that no
  code reads costs only disk space.
