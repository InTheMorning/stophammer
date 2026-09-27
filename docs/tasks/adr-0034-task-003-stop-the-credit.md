# ADR 0034 Task 003: The Ingest Stops The Credit, And The Tables Go

Owner: [ADR 0034](../adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md)
§10 and §11, release B. Plan: [the phase plan](../plans/adr-0034-artist-credit-removal-phase-plan.md).

Repository: `stophammer`. The operator commits. Task 002 and
[ADR 0056 task 002](adr-0056-task-002-drop-proof-tables.md) come first.

This task is one commit. `feeds.artist_credit_id` and
`tracks.artist_credit_id` are `NOT NULL` until the migration, so the code
change and the migration cannot go in two commits.

## Goal

The node makes no artist, no credit and no `artist_credit_id`, and signs no
`ArtistUpserted` and no `ArtistCreditCreated`. A node accepts each old artist
event of the log and applies nothing. A migration rebuilds `feeds` and
`tracks` with no `artist_credit_id`, and drops the artist tables.

## Files To Inspect

- `src/api.rs`: the ingest (search for `get_or_create_feed_scoped_source_text_credit`,
  `feed_artist_credit`, `track_credits`, `artist_credit_id:`)
- `src/db.rs`: each function that names `artist`, `artist_credit` or
  `external_ids`. Find them with grep. The event builders
  `build_artist_upserted_event` and `build_artist_credit_event` are near line
  3890 and line 4046. `MIGRATIONS` and `open_db`
- `src/apply.rs`: the arms `ArtistUpserted` and `ArtistCreditCreated`, and
  the helper of task 001
- `src/model.rs`: `Artist`, `ArtistCredit`, `ArtistCreditName`, `Feed`, `Track`
- `src/event.rs`: `ArtistUpsertedPayload`, `ArtistCreditCreatedPayload`
- `src/schema.sql`
- `migrations/0032_feed_scoped_track_identity.sql`: an earlier rebuild
- `migrations/0046_drop_proof_tables.sql`: the last form of
  `trg_feeds_cleanup_before_delete`
- `tests/migration_tests.rs`
- `docs/API.md`: the event type table

## Files Likely To Change

- `src/api.rs`, `src/db.rs`, `src/apply.rs`, `src/model.rs`, `src/event.rs`,
  `src/quality.rs`, `src/schema.sql`
- `migrations/0047_drop_artist_credit.sql`, new
- `docs/API.md`
- `tests/artist_credit_tests.rs` and `tests/external_id_tests.rs`, deleted
- Each other test that builds a credit or reads an artist table
- `tests/adr0034_artist_credit_removed_tests.rs`, new

## Do Not Touch

- The public read routes and the OpenAPI document. No read gives an artist
  ID.
- `release_artist`, `release_artist_sort`, `release_artist_source`,
  `track_artist`, `track_artist_sort`.
- Each event type other than the two artist events.
- `stophammer-crawler/`, `stophammer-parser/`.

## Constraints

### Code

- `Feed` and `Track` lose `artist_credit_id`. The model types have no
  `deny_unknown_fields`, so an old payload with the key still deserializes.
  Check this with a test.
- The ingest calls no credit function, and emits no `ArtistUpserted` and no
  `ArtistCreditCreated`.
- In `apply`, the arms `ArtistUpserted` and `ArtistCreditCreated` do nothing
  after the signature check. The arm counts the event as applied, so the sync
  cursor moves past it.
- `ArtistUpsertedPayload`, `ArtistCreditCreatedPayload`, `Artist`,
  `ArtistCredit` and `ArtistCreditName` stay as wire types. Their doc
  comments say that only events before release 0.3.0 carry them (ADR 0034
  §11). Delete each other use of them.
- Delete each `src/db.rs` function that only serves the artist tables,
  `external_ids` or the credit, and the helper of task 001. Delete
  `cleanup_orphaned_artists`, which has no caller.
- `docs/API.md`: the rows `artist_upserted` and `artist_credit_created` say
  that the log holds them only before release 0.3.0, and that a node applies
  nothing for them. The row `artist_merged` gets the same note, if no code
  emits it.

### Migration

- File `migrations/0047_drop_artist_credit.sql`, at the next array position
  of `MIGRATIONS` after migration 0046.
- `PRAGMA foreign_keys = OFF` at the start, as in migration 0032.
- For `feeds` and then `tracks`: `CREATE TABLE feeds_new` with each column
  except `artist_credit_id`, `INSERT INTO feeds_new SELECT` the same columns,
  `DROP TABLE feeds`, `ALTER TABLE feeds_new RENAME TO feeds`. Do not rename
  the old table first: that also changes each foreign key in other tables
  that names it.
- Take each column, each constraint and each `STRICT` from the current
  `schema.sql` form of the table. Make each index of the table again, except
  `idx_feeds_credit` and `idx_tracks_credit`.
- Make `trg_feeds_cleanup_before_delete` again, in its form of migration
  0046, and `trg_tracks_cleanup_before_delete` in its last form.
- Drop `artist_credit_name`, `artist_credit`, `artist_aliases`, `artists`,
  `artist_type`, `rel_type` and `external_ids`, children first.
- `src/schema.sql` gives the same final schema. `tests/migration_tests.rs`
  compares them, if it has such a test.

## Steps

0. Before a change, run
   `cargo run --bin gen_openapi > target/openapi-before.json`.
1. Change the models and the ingest. Fix each compile error.
2. Change the apply arms. Delete the dead functions.
3. Write the migration and change `schema.sql`.
4. Delete the two test files. Correct each other test.
5. Write the new tests. Update `docs/API.md`.
6. Check with grep: `src/` names `artist_credit` and `ArtistUpserted` only in
   the wire types, the apply no-op arms and the migration list.

## Acceptance

Mechanical, each an integration test in
`tests/adr0034_artist_credit_removed_tests.rs` or `tests/migration_tests.rs`:

- An ingest of a feed with two tracks emits no `artist_upserted` and no
  `artist_credit_created` event.
- A node applies a log of signed events: `ArtistUpserted`,
  `ArtistCreditCreated`, then a `FeedUpserted` and a `TrackUpserted` whose
  JSON has `artist_credit_id`. Each event counts as applied, and the feed and
  the track are stored.
- A database at the array position of migration 0046, with artist rows and
  credits, migrates. After it:
  - `feeds` and `tracks` have no `artist_credit_id`.
  - The seven tables are gone.
  - The row counts of `feeds` and `tracks` are the same.
  - `PRAGMA foreign_key_check` gives no row.
  - The delete of a feed still deletes its tracks.
- A fresh database from `schema.sql` has no artist table and no
  `artist_credit_id` column.
- The quality score of a feed is the same as before the change.
- The OpenAPI document does not change.
- The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --test adr0034_artist_credit_removed_tests
cargo test --test migration_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > target/openapi-after.json
# Compare with target/openapi-before.json, made before the first change.
```

## Escalation Triggers

Stop and report without a workaround when:

- Migration 0046 does not exist yet.
- A public route, a verifier or the crawler reads an artist table.
- A table other than the seven has a foreign key to an artist table.
- A model type has `deny_unknown_fields`.
- An old event of the log fails to apply.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0034-task-003-stop-the-credit.md
- /home/citizen/build/stophammer/docs/adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md (sections 10 and 11)
- /home/citizen/build/stophammer/AGENTS.md (Code Style, Tests, New Database Migration)
- Only the parts of src/api.rs and src/db.rs named in the task file. Use grep. The files are long.

Goal:
- The node makes no artist and no credit, applies old artist events as no-ops, and migration 0047 rebuilds feeds and tracks with no artist_credit_id and drops the seven artist tables.

Constraints:
- The rules under "Constraints" in the task file.
- One logical change. The code and the migration must pass together.

Do not touch:
- Public read routes, the OpenAPI document, the release_artist and track_artist fields, other event types, stophammer-crawler/, stophammer-parser/.
- Git: run no git command that writes (no add, commit, rm, stash, checkout, reset, push). Use rm for the two deleted test files.

Acceptance criteria:
- The criteria under "Acceptance" in the task file.

Test commands:
- The commands under "Test Commands" in the task file.

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
