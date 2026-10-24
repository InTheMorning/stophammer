# Phase Plan: The Search Index And The Retired Source Hold

This plan is advisory. It gives the sequence of the tasks of two decisions:

- [ADR 0078](../adr/0078-a-search-index-row-holds-only-searchable-text.md): a
  search index row holds only searchable text.
- [ADR 0079](../adr/0079-a-retired-guid-returns-only-from-its-source.md): a
  retired GUID returns only from its source.

Each ADR owns its rules. The two come from
[the research request of 2026-10-23](research-search-index-request.md) and
[the review of 2026-10-23](../reviews/requests-after-release-0.13.2-review.md).

## Goal

- A search finds only the present text of an entity that the node holds.
- Each text of `q` gives `200`, and a `400` gives no SQLite text.
- A feed that a retire with no block removes comes back only through its last
  source URL, or after an operator release.

## Non-Goals

- The other search changes of the research request: the prefix index, one
  request for all types, `effective_rank`, the `ETag`, and the log of the query
  text.
- v4vmm request 10 items 2 to 6 and v4vmm request 11.
- A change to the rowid hash of ADR 0022.
- A change to the operator block of ADR 0053.

## Assumptions

- The two ADRs are Accepted. A task does not start on a Proposed ADR.
- The bundled SQLite is 3.45.0 (`libsqlite3-sys` 0.28.0). The index of
  ADR 0078 §1 keeps its text, so a newer SQLite is not necessary.
- The next free migration numbers are 0054 and 0055. Each task checks the
  number when it starts.
- In the backup of 2026-10-21, one GUID has a `FeedRetired` event and no record.

## Affected Modules

All tasks are in the `stophammer` repository.

| Module | Tasks |
|---|---|
| `migrations/`, `src/schema.sql`, `src/db.rs` (`open_db` repairs) | 0078-001, 0079-001, 0079-003 |
| `src/search.rs` (`populate_search_index`, `delete_from_search_index`) | 0078-001 |
| `src/db.rs` (`delete_feed_sql`, `delete_track_sql`, the stale track removal), `src/api.rs` and `src/apply.rs` (the retire and remove paths) | 0078-002 |
| `src/search.rs` (`search`) | 0078-003 |
| `src/search.rs` (`sanitize_fts5_query`), `src/query.rs` (search errors), `src/openapi.rs`, `docs/API.md` | 0078-004 |
| `src/db.rs` (`delete_feed_with_event`, `classify_submission`), `src/api.rs` (the `NewFeed` arm) | 0079-001 |
| `src/api.rs` (router), `src/openapi.rs`, `docs/API.md`, `tests/fixtures/v1-schema-fields.json` | 0079-002 |

## Sequence

Each task waits for the task before it in its track.

1. **Index track.** ADR 0078 task 001, then task 002, then task 003.
2. **Query track.** ADR 0078 task 004. It can run at the same time as the index
   track. It changes only `sanitize_fts5_query` in `src/search.rs`, and task
   001 does not change that function.
3. **Hold track.** ADR 0079 task 001 starts after ADR 0078 task 002, because
   both change `delete_feed_sql`, `delete_feed_with_event` and the retire paths
   in `src/api.rs`. Then task 002 and task 003, in either order.

The two migrations go in sequence: ADR 0078 task 001 takes the first free
number, and ADR 0079 task 001 takes the next one.

## Schema And API Implications

- The migration of ADR 0078 task 001 replaces `search_index`, and an
  `ensure_` repair fills it one time (ADR 0078 §6, ADR 0046).
- The migration of ADR 0079 task 001 adds `retired_source_holds`. Only the
  primary writes it. It makes no event.
- No event payload changes. The primary and the community nodes can upgrade in
  each sequence.
- `GET /v1/search` changes the description of `q`. No field changes.
- The ingest gets the reason `retired_source_conflict`. The admin route
  `DELETE /v1/retired-source-holds/{guid}` is new (ADR 0079 §3).

## Risk Areas

- **The repair sequence in `open_db`.** On the old table, a `DELETE` by rowid
  fails. The new repair must run before `ensure_search_index_track_identity`
  and `ensure_search_index_publisher_feeds` (ADR 0078 §6).
- **Error propagation.** ADR 0078 task 002 replaces each `let _ =` on an index
  delete. An error now fails the retire. ADR 0078 §3 gives the reason that this
  does not stop the apply loop of a community node.
- **Tests that write the index directly.** Ten test files call
  `populate_search_index` or `delete_from_search_index` with the old
  arguments. Task 001 changes them, and keeps the purpose of each test.
- **The `NewFeed` arm.** ADR 0079 task 001 adds a rejection before the
  ADR 0057 block check. A wrong sequence lets a copy pass, or blocks the true
  source.

## Test Strategy

- Each check of the two ADRs is a test in `tests/`. Each task names its checks.
- New test files: `tests/adr0078_search_index_tests.rs`,
  `tests/adr0078_retire_index_tests.rs`, `tests/adr0078_query_words_tests.rs`,
  `tests/adr0079_retired_source_tests.rs`.
- `tests/issue21_fts5_sanitize_tests.rs` asserts the old sanitizer. ADR 0078
  task 004 deletes it, and its new tests replace it.
- `tests/migration_tests.rs` covers each migration. Each new migration adds its
  line to `tests/fixtures/migration_sha256.txt` (ADR 0023).
- The gate of `AGENTS.md` runs after each task.

## Rollback Strategy

- Each task is one commit, and a revert of that commit undoes it.
- The index is derived data. A revert of ADR 0078 task 001 after a deploy needs
  one rebuild with the old definition. The old repair
  `ensure_search_index_track_identity` does not detect that case, so the
  revert also needs a repair that keys on the definition.
- The table of ADR 0079 holds only holds. A revert leaves the table, and no
  code reads it.

## After The Tasks

The orchestrator reviews each diff against
[the review checklist](../reviews/search-index-and-retired-source-review-checklist.md).
A release record gives the release. After the deploy, the orchestrator deletes
this plan and the task packets.
