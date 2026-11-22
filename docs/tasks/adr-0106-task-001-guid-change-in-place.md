# ADR 0106 Task 001: A Feed Changes Its GUID In Place

Owner: [ADR 0106](../adr/0106-a-feed-changes-its-guid-in-place.md) §1 to §5.

Repository: `stophammer`. `stophammer-crawler/` is a separate repository. Do
not change it.

## Goal

- A submission from the source URL of a record can declare a free new GUID.
  It then changes the GUID of that record in place. The change occurs in the
  ingest transaction, with one signed `FeedGuidChanged` event.
- No pending state, no operator route and no UUIDv5 special case remain.
- The old GUID gives `404` with `superseded_by` at the feed route and at the
  track route of the feed.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Submission classification | reuse | `src/db.rs` | `classify_submission` already gives `SubmissionClass::GuidChange { held_guid }` for a free new GUID at a held URL |
| Ingest transaction | extend | `src/db.rs` | `ingest_transaction` already takes `feed_move: Option<FeedMoveWrite>` and writes it in its transaction. The GUID change joins it the same way |
| GUID change in place | new | `src/db.rs` | Searched `src/db.rs`, `src/api.rs`, `src/apply.rs` and `~/build/musicindex-lib`: no function changes `feeds.feed_guid`. `delete_feed_sql` in `src/db.rs` gives the table list to follow |
| Supersession link | extend | `src/db.rs` | `insert_guid_supersession` writes the link. A revert also needs a delete of the link whose old GUID is the new GUID |
| Search index rows | reuse | `src/search.rs` | `delete_from_search_index` and `populate_search_index` write the feed and track rows |
| Event types | extend | `src/event.rs` | `EventType` and `EventPayload` hold the ADR 0052 events |
| Replica apply | extend | `src/apply.rs` | The match on `EventPayload` applies each event type |
| Superseded 404 | extend | `src/query.rs` | `FeedSupersededBody` and `FeedDetailOutcome::Superseded` give the feed route `404` |
| Migration list | extend | `src/db.rs` | The `include_str!` list of `migrations/` files (ADR 0046) |
| GUID change test helpers | extend | `tests/adr0052_guid_change_tests.rs` | `submit`, `accept`, `stored_feed`, `stored_track_guids`, `guid_supersession` and `get_feed` |

## Files To Inspect

- `docs/adr/0106-a-feed-changes-its-guid-in-place.md`, all of it.
- `src/api.rs`: these items.
- The `SubmissionClass::GuidChange` arm of the ingest handler (about line
  3288), `guid_change_transition` (about line 1272),
  `record_guid_change_pending_for_ingest`, `record_guid_change_return_for_ingest`,
  `guid_change_observed_event_row`, the no-change branch that answers
  `guid_change_pending` (about line 2829), the call of
  `guid_change_transition` before `db::ingest_transaction` (about line 3964),
  `handle_guid_change_decision` and its route (about line 2107).
- `src/db.rs`: `classify_submission`, `ingest_transaction`, `FeedMoveWrite`,
  `delete_feed_sql`, `canonical_track_entity_id`, the `feed_guid_changes`
  and `feed_guid_supersessions` functions (about lines 7978 to 8210), and the
  migration list.
- `src/apply.rs`: the arms for `FeedGuidChangeObserved`,
  `FeedGuidChangeDecided`, `FeedGuidSuperseded` and `FeedRetired`.
- `src/query.rs`: `pending_guid_change_response`, `handle_list_guid_changes`,
  `handle_get_feed`, `handle_get_feed_track`, `FeedSupersededBody`.
- `src/model.rs`: `guid_origin_matches`.
- `src/schema.sql` and `migrations/0042_feed_guid_changes.sql`.
- `src/openapi.rs` (about lines 452, 826, 1042 and 1162) and `docs/API.md`
  (the sections on ingest reasons, `pending_guid_change`, `superseded_by`,
  `GET /v1/guid-changes`, the operator decision and the event types).
- The tests named below.

## Files Likely To Change

- `src/api.rs`, `src/db.rs`, `src/apply.rs`, `src/event.rs`, `src/query.rs`,
  `src/model.rs`, `src/openapi.rs`, `src/schema.sql`
- A new migration `migrations/0066_drop_feed_guid_changes.sql`
- `docs/API.md`
- `tests/adr0052_guid_change_tests.rs`, `tests/adr0051_source_url_tests.rs`,
  `tests/adr0078_retire_index_tests.rs`, `tests/adr0079_retired_source_tests.rs`
- A new test file `tests/adr0106_guid_change_in_place_tests.rs`

## Do Not Touch

- `stophammer-crawler/`.
- ADR 0052 §1, §2, §3 and §6: moves, redirects, self links and relocations.
- The classification of `classify_submission`. `RecordConflict`, `Mirror`,
  the ADR 0079 hold check and the ADR 0053 block check keep their answers.
- The `events` table and each stored event. History does not change.
- `docs/adr/`, `AGENTS.md` and `docs/plans/`. The main session owns them.

## Constraints

### The change in place

- Add `GuidChangeWrite { old_guid, new_guid, source_url }` in `src/db.rs`.
  `ingest_transaction` takes `guid_change: Option<GuidChangeWrite>`. When it
  is `Some`, the transaction does these steps before step 3 (the feed upsert):
  1. Call a new function `change_feed_guid_sql(conn, old_guid, new_guid)`.
     It deletes the search index rows of the old keys (feed, publisher,
     playlist, and each track with `canonical_track_entity_id`), as
     `delete_feed_sql` does. It changes the GUID column in each table of the
     list below. It changes the `entity_quality` keys of the feed and of each
     track to the new keys.
  2. Deletes the `feed_guid_supersessions` row whose `old_guid` is the new
     GUID. Then it writes the link from the old GUID to the new GUID. A
     revert must replace an earlier link from the same old GUID, so use an
     upsert, not `INSERT OR IGNORE`.
  3. Signs one `FeedGuidChanged` event with `source_url`, `old_guid` and
     `new_guid`, and `subject_guid` = the new GUID. It is signed before the
     events of the body, so a replica applies it first.
- The rest of `ingest_transaction` then applies the body under the new GUID,
  and writes the search rows again. If any step fails, the transaction rolls
  back and the record keeps its old GUID.
- `PRAGMA foreign_keys` is on, and SQLite ignores a change to it inside a
  transaction. 13 tables reference `feeds`. Use `PRAGMA defer_foreign_keys =
  ON` inside the transaction, or insert the new `feeds` row first, move the
  child rows, and then delete the old `feeds` row. Do not turn the foreign
  keys off.
- `feeds.feed_url` is `UNIQUE`. The record keeps its URL.
- Tables whose GUID column `change_feed_guid_sql` changes: `feeds`, `tracks`,
  `payment_routes`, `feed_payment_routes`, `feed_list_value_raw`,
  `value_time_splits` (`source_feed_guid` only), `feed_remote_items_raw`
  (`feed_guid` only), `track_remote_items_raw` (`feed_guid` only),
  `live_events`, `source_contributor_claims`, `source_entity_ids`,
  `source_entity_links`, `source_release_claims`, `source_item_enclosures`,
  `source_item_transcripts`, `source_platform_claims`,
  `feed_url_observations`, `source_gone_answers`, `feed_copies`,
  `feed_copy_overflow`, `feed_left_urls` and `feed_move_state`.
- Columns that stay, each with its reason in a constant list beside the
  function: `remote_feed_guid` in three tables (a statement of a different
  feed), `events` (signed history), `feed_guid_supersessions` (step 2),
  `retired_source_holds` (a hold names a retired GUID, and the guard keeps a
  held GUID out), and `feed_blocks` (no `feed_guid` column).
- In the ingest handler, the `SubmissionClass::GuidChange { held_guid }` arm
  always sets the GUID change, with the warning
  `GUID changed from <old> to <new> (ADR 0106)`. Remove the approved check,
  the UUIDv5 check and the pending branch. The no-change branch must not
  answer `guid_change_pending`. A `GuidChange` classification there must go
  on to the full ingest, as a move does (`req.force_reingest = true`).
- Delete `guid_change_transition`, `record_guid_change_pending_for_ingest`,
  `record_guid_change_return_for_ingest`, `guid_change_observed_event_row`,
  `handle_guid_change_decision`, its request and response types, its route
  and its OpenAPI entry. Delete `model::guid_origin_matches` if no other code
  calls it. Delete each `db` function of `feed_guid_changes` that has no
  caller.

### The pending table

- Migration `0066_drop_feed_guid_changes.sql` drops `feed_guid_changes`.
  Remove the table from `src/schema.sql`.
- `pending_guid_change` in `GET /v1/feeds/{guid}` stays in the schema and is
  always `null`. `GET /v1/guid-changes` stays with its parameters and always
  gives an empty `data` list with `has_more` `false` (ADR 0044 rule 4).
- The ingest reasons `guid_change_pending` and `guid_change_rejected` go.

### Events and replicas

- `FeedGuidChanged` is a new `EventType` and `EventPayload` variant. Its apply
  arm calls the same `change_feed_guid_sql` and the same link write as the
  primary. It is a no-op when no record holds the old GUID.
- `FeedGuidChangeObserved` and `FeedGuidChangeDecided` stay parseable. Their
  apply arms do nothing, because the table is gone. A comment names ADR 0106
  §5.
- `FeedGuidSuperseded` and `FeedRetired` keep their apply arms for history.

### The old GUID

- `GET /v1/feeds/{old_guid}/tracks/{track_guid}` answers `404` with the body
  of `FeedSupersededBody` when no record holds `old_guid` and a link names
  it. `superseded_by` gives the new feed GUID. Use the lookup of
  `handle_get_feed`. Do not duplicate it: move it into one function that
  both handlers call.

### Text

- Each new or changed comment names ADR 0106. Remove comments that describe
  the pending row or the operator decision.
- Write each comment and each `docs/API.md` change in Simplified Technical
  English. Read `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Read the `rust-dev` skill rules that `AGENTS.md` names.
- Do not commit. Do not run a Git command that writes.

## Tests To Delete

These assert the rule that ADR 0106 supersedes:

- `tests/adr0052_guid_change_tests.rs`:
  `a_guid_change_that_is_not_the_uuidv5_is_pending_and_public`,
  `a_repeated_pending_guid_signs_no_new_event_and_touches_last_seen`,
  `a_submission_with_the_old_guid_deletes_the_pending_row`,
  `reject_holds_for_the_same_guid_and_a_different_guid_opens_a_new_row`,
  `approve_runs_the_transition_at_the_next_submission`,
  `the_guid_change_route_answers_403_with_no_admin_token`.
- `tests/adr0051_source_url_tests.rs`:
  `a_new_guid_at_a_held_url_answers_guid_change_pending`,
  `feed_count_is_unchanged_by_guid_change_pending`.
- `tests/adr0079_retired_source_tests.rs`: `a_guid_change_retire_writes_no_hold`.

Move the tests that stay in `tests/adr0052_guid_change_tests.rs` and the new
tests into `tests/adr0106_guid_change_in_place_tests.rs`. Delete
`tests/adr0052_guid_change_tests.rs` when it holds no test.

## Tests To Change

- `a_uuidv5_guid_transitions_at_once_and_the_new_guid_then_updates`: a free
  GUID that is not the UUIDv5 changes in place at the first submission. Give
  it a name that says this.
- `a_uuidv5_guid_change_over_the_track_limit_does_not_retire_the_old_record`:
  a body over the track limit leaves the record and its GUID unchanged.
- `a_replica_applying_the_events_has_the_same_rows_and_supersession`: see
  criterion 2.
- `tests/adr0078_retire_index_tests.rs` `guid_change_retire_deletes_each_index_row`:
  after the change, no search row has an old key, and the feed and each track
  have a row with the new key.

## Acceptance Criteria (mechanical)

Each criterion is a test. The orchestrator gave criteria 1 to 4.

1. A schema test reads each table and column from `sqlite_master` and
   `pragma_table_info` of a migrated database. It fails when a column whose
   name contains `feed_guid` is in neither the change list nor the list of
   columns that stay. The message is:
   `ADR 0106 §2: <table>.<column> holds a feed GUID. Add it to the change of change_feed_guid_sql, or to the list of columns that stay with its reason.`
2. A replica that applies the events of the primary after a GUID change has
   the same rows as the primary in `feeds`, `tracks`, `payment_routes` and
   `feed_guid_supersessions`.
3. A revert to the old GUID applies at the next submission. After it, the
   record has the old GUID, and `feed_guid_supersessions` holds no row whose
   `old_guid` is the GUID of the record.
4. An error inside the change leaves the record unchanged: the old GUID, its
   tracks and its payment routes, and no `FeedGuidChanged` event. Force the
   error, for example with a body over the track limit or with a test trigger
   on a child table.
5. After a change, the record count does not change, each `track_guid` and
   each payment route stays, and no row of the change list holds the old GUID.
6. `GET /v1/feeds/{old_guid}` and `GET /v1/feeds/{old_guid}/tracks/{track_guid}`
   give `404` with `superseded_by` equal to the new GUID.
7. A new GUID that a different record holds gives `record_conflict`, and
   changes nothing.
8. `GET /v1/guid-changes` gives an empty list, and `pending_guid_change` is
   `null`, after a GUID change and after the migration on a database that had
   pending rows.
9. `POST /v1/feeds/{guid}/guid-change` gives `404` or `405`.

Visual: none.

## Checks

Run in the repository root:

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
python3 ~/.agents/skills/project-baseline/scripts/check_adr_links.py . --bar-from 0101
```

Report each result. Report each test that you deleted or renamed.
