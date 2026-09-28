# ADR 0034 Phase Plan: The Removal Of The Artist Credit

Date: 2026-09-27. This plan states no rule.
[ADR 0034](../adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md)
§10 and §11 own each rule. This plan is item 13 of
[the remaining accepted work plan](remaining-accepted-work-plan.md).

## Goal

The node keeps no artist, no artist credit and no `artist_credit_id`. The
ingest signs no `ArtistCreditCreated` and no `ArtistUpserted`. A community node
can upgrade before or after the primary, in each release.

## Non-Goals

- No change to a public read. No read route gives an artist ID or a credit
  today. `release_artist`, `track_artist` and their sort fields stay.
- No change to the event log. The old artist events stay in it.
- No public SSE route (ADR 0037).
- No change in `stophammer-crawler` or `stophammer-parser`. Neither uses the
  credit.

## Facts

From the production copy of 2026-09-26:

| Count | Value |
|---|---|
| `feeds` | 10,424 |
| `tracks` | 26,883 |
| `artists`, `artist_credit`, `artist_credit_name` | 10,524 each |
| `artist_aliases` | 10,508 |
| `external_ids` | 0 |
| `artist_credit_created` events | 10,424 |
| `artist_upserted` events | 10,424 |
| All events | 370,604 |

In the code on 2026-09-27:

- `model::Feed` and `model::Track` have `artist_credit_id: i64`, and
  `FeedUpsertedPayload` and `TrackUpsertedPayload` carry them. The signature
  covers the stored `payload_json` text, so a change of the Rust type does not
  change the verification of an old event.
- The ingest in `src/api.rs` calls
  `db::get_or_create_feed_scoped_source_text_credit` for the release artist,
  and for each track author.
- `src/apply.rs` applies `ArtistUpserted` and `ArtistCreditCreated`.
- `SseRegistry` in `src/api.rs` keys its channels by artist ID.
  `extract_artist_ids` and `artist_ids_for_feed` find the IDs. No route
  reads the registry (ADR 0037).
- `src/quality.rs`: `compute_feed_quality` gives 10 points when
  `artist_credit_id > 0`. `compute_artist_quality` has no caller outside
  its module and tests.
- `feeds.artist_credit_id` and `tracks.artist_credit_id` are `NOT NULL` and
  reference `artist_credit(id)`. SQLite cannot drop a column that a foreign
  key uses, so the migration rebuilds the two tables. Migration 0032 rebuilt
  `tracks` in the same way.
- The triggers `trg_feeds_cleanup_before_delete` and
  `trg_tracks_cleanup_before_delete` touch no artist table. A rebuild of a
  table drops its triggers, so the migration makes them again.
- About 190 lines in `src/` and 30 test files use the credit.
  `tests/artist_credit_tests.rs` and `tests/external_id_tests.rs` test only
  the removed code.

## Plan Decisions

1. **Release A joins 0.2.0.** Task 001 changes only what a node accepts, so
   it is a compatible change (ADR 0066 §1). 0.2.0 also carries ADR 0061.
2. **Release B is 0.3.0,** after ADR 0056 task 002. Its migration comes after
   migration 0046 of that task, because both make the feed delete trigger.
3. **The migration uses the SQLite rebuild steps:** make `feeds_new`, copy,
   drop `feeds`, rename `feeds_new` to `feeds`. It does not rename the old
   table first. A rename of the old table also changes each foreign key in
   other tables that names it.
4. **The runner turns off foreign keys for the rebuild.** The steps need
   foreign keys off, and SQLite ignores that setting inside a transaction.
   A marker line, `-- stophammer: foreign_keys=off`, turns them off before
   the transaction of that migration, and the runner checks them before its
   commit.

   A test on 2026-09-28 showed two failures without it. With foreign keys
   on, the drop fails. With `defer_foreign_keys`, the drop deletes the rows
   of `source_gone_answers` through its `ON DELETE CASCADE`.
5. **The SSE registry keys by feed GUID.** ADR 0037 keeps the registry, and
   ADR 0064 publishes live frames to it. A feed GUID is the key that each
   event already has.

## Affected Modules

| Task | Module | Change |
|---|---|---|
| 001 | `model`, `apply`, `db` | Optional `artist_credit_id`, and a local credit when it is absent |
| 002 | `api`, `quality` | SSE by feed GUID. The quality rule. No reader uses the credit |
| 003 | `api`, `apply`, `db`, `model`, `event`, `migrations`, `schema.sql` | No credit at ingest. Old artist events apply as no-ops. Rebuild `feeds` and `tracks`. Drop the artist tables |
| 004 | None | Deploy of A and B, and the checks |

## Sequence

| Task | Release | Goal | Needs |
|---|---|---|---|
| [001](../tasks/adr-0034-task-001-optional-credit-on-apply.md) | A, 0.2.0 | A node accepts an event with no `artist_credit_id` | None |
| [002](../tasks/adr-0034-task-002-readers-leave-the-credit.md) | B, 0.3.0 | No reader uses the credit | 001 |
| [003](../tasks/adr-0034-task-003-stop-the-credit.md) | B, 0.3.0 | The ingest stops the credit, and the migration drops it | 002, and ADR 0056 task 002 |
| [004](../tasks/adr-0034-task-004-deploy.md) | A and B | The deploys and the checks | Each task of its release |

Task 002 changes no write and no event, so it is correct alone. Task 003 is
one commit: the columns are `NOT NULL` until its migration, so the writes and
the migration cannot go in two commits. Release B can deploy only when each
known community node runs release A.

## Schema And API

- Release A: no schema change. The OpenAPI document does not change.
- Release B: migration 0047, at array position 41. ADR 0067 is migration
  0045, and ADR 0056 task 002 is migration 0046. `src/schema.sql` loses the artist tables and the two
  columns. `docs/API.md` states that `artist_upserted` and
  `artist_credit_created` appear only in the log before 0.3.0.
- No read route changes in either release.

## Risk Areas

- **An old node and a new event.** Before task 001, a node rejects a
  `FeedUpserted` without `artist_credit_id`. Release B must not reach the
  primary before each known community node runs release A. Task 004 checks
  `GET /node/info` of each node.
- **A new node and an old log.** A new community node reads the log from seq
  0, with 20,848 artist events. Task 003 needs a test that such a log applies.
- **The rebuild.** It copies 10,424 feeds and 26,883 tracks. Task 004 runs the
  migration on a copy of a production backup and records the time.
- **Foreign keys to `feeds`.** Other tables reference `feeds(feed_guid)`. The
  rebuild steps of plan decision 3 keep those references valid. Task 003
  checks `PRAGMA foreign_key_check` after the migration.
- **The feed quality score.** Search ranks by it. Task 002 gives the 10 points
  for `release_artist`, and a test proves that no score changes.

## Test Strategy

- Task 001: apply tests for a feed event and a track event with no
  `artist_credit_id`, on the current schema.
- Task 002: the quality test, and an SSE test keyed by feed GUID.
- Task 003: a log replay test with old artist events. A migration test on a
  database at position 40 with artist rows. A fresh bootstrap from
  `schema.sql`. Delete the tests of the removed code.
- The full gate of the `stophammer` crate after each task.

## Rollback

- Release A: go back to the previous image. Task 001 adds no column and no
  table.
- Release B: the migration drops tables, so an image before release B cannot
  run on the new database. Task 004 takes a backup of the database before the
  deploy. A rollback restores that backup and the previous image. Events that
  the primary signed after the deploy are then lost from the primary, so a
  rollback is possible only in the first hours.

## Review

[The review checklist](../reviews/adr-0034-artist-credit-removal-review-checklist.md)
applies to each task.
