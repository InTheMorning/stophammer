# ADR 0078 Task 002: A Retire Deletes Each Index Row

Owner: [ADR 0078](../adr/0078-a-search-index-row-holds-only-searchable-text.md)
§3. The phase plan is
[the search index and retired source plan](../plans/search-index-and-retired-source-phase-plan.md).

Repository: `stophammer`. The operator commits. This task starts after task
001.

## Goal

The feed delete `delete_feed_sql` deletes the index rows of the types `feed`
and `publisher` of the feed, and the index rows of its tracks. The track delete
`delete_track_sql` deletes the index row of the track. No other path deletes
index rows, and no path discards the result of a delete.

## Files To Inspect

- `src/db.rs`: `delete_feed_sql` (near line 2687), `delete_feed`,
  `delete_feed_with_event` (near line 2830), `delete_track_sql` (near line
  2629), `delete_track`, and the stale track removal of the ingest transaction
  (near line 6900)
- `src/api.rs`: the GUID change retire (near lines 1240 to 1280), the gone
  source retire `retire_for_source_gone` (near line 2180), the `podcast:block`
  retire (near line 2870), `handle_retire_feed` (near line 4748),
  `handle_remove_track` (near line 4897)
- `src/apply.rs`: the `FeedRetired` and `TrackRemoved` arms (near lines 289 to
  340)
- `src/community.rs`: the apply loop near line 229
- `src/search.rs`: `delete_from_search_index` after task 001
- `tests/retire_remove_tests.rs`, `tests/adr0057_podcast_block_tests.rs`,
  `tests/adr0067_source_gone_tests.rs`, `tests/adr0052_guid_change_tests.rs`

## Files Likely To Change

- `src/db.rs`, `src/api.rs`, `src/apply.rs`
- `tests/adr0078_retire_index_tests.rs`, new

## Constraints

- **The feed delete.** At its start, `delete_feed_sql` reads the track GUIDs of
  the feed, as it does now. It calls `delete_from_search_index` for the entity
  `feed`, for the entity `publisher`, and for each track with
  `canonical_track_entity_id`. It uses `?` on each call. It does this before it
  deletes the feed row.
- **The track delete.** `delete_track_sql` calls `delete_from_search_index` for
  the track with `canonical_track_entity_id`, with `?`.
- **The callers.** Remove each separate call of `delete_from_search_index` in
  the retire and remove paths of "Files To Inspect". The delete functions now
  do it. Each of these paths calls `delete_feed_sql` or `delete_track_sql`.
  When a path does not, stop at the escalation trigger.
- **The medium change.** The two calls of `delete_from_search_index` in
  `sync_source_read_models_for_feed` (near line 1406 and line 1433) stay. They
  delete the row of the type that a feed left. Change them from `let _ =` to
  `?` only if they use `let _ =`.
- **Errors.** No call of `delete_from_search_index` uses `let _ =` or `.ok()`
  after this task.
- **Comments.** Remove each "best-effort" comment on a removed call. The new
  calls name ADR 0078 §3.

## Do Not Touch

- `populate_search_index`, `delete_from_search_index` and the table. Task 001
  owns them.
- The `search` function and `src/query.rs`
- The retire rules: the reasons, the events, the blocks of ADR 0053, the
  sequence of the checks of ADR 0051 and ADR 0057
- `src/community.rs`
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add the calls to `delete_feed_sql` and `delete_track_sql`.
2. Remove the separate calls in each path.
3. Search the crate for `delete_from_search_index`. Each remaining call uses
   `?`.
4. Write the tests.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0078_retire_index_tests.rs`.

- ADR 0078 check 4: a test retires a music feed and a publisher feed by each
  path of ADR 0078 §3: `DELETE /v1/feeds/{guid}`, a `podcast:block` ingest, two
  gone answers, a GUID change of ADR 0052, and the apply of a `FeedRetired`
  event. After each retire, `search_entities` has no row for the feed or its
  tracks. A search for its title, its artist or a track title gives no hit.
- A test removes a track in three ways: `handle_remove_track`, the stale track
  removal of an ingest, and the apply of `TrackRemoved`. After each removal,
  `search_entities` has no row for the track.
- Check 5: a retire that receives an error from a delete gives that error, and
  the transaction writes no row. One method: drop `search_index` in the test
  database before the retire.
- A source search of `src/` finds no `let _ = crate::search::delete_from_search_index`.
  A test in the file reads the source files and asserts this. Its failure
  message names ADR 0078 §3 and the fix.
- Each test of the four test files of "Files To Inspect" passes.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0078_retire_index_tests
cargo test --test retire_remove_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A retire or remove path deletes a feed or a track with SQL that is not
  `delete_feed_sql` or `delete_track_sql`.
- A test expects a retire to succeed when the index fails.
- The apply of an existing event fails after this change.
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
- `~/build/stophammer/docs/adr/0078-a-search-index-row-holds-only-searchable-text.md`, §3
- `~/build/stophammer/docs/tasks/adr-0078-task-002-retire-deletes-each-index-row.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- `delete_feed_sql` and `delete_track_sql` delete each index row of the feed or the track. No other path deletes index rows. No path discards the result of a delete.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Use `?` on each call of `delete_from_search_index`.
- Do not change a retire rule, a reason, an event or a block.
- Stop at each escalation trigger of this task.

Do not touch:
- `populate_search_index`, `delete_from_search_index`, the table, the `search` function, `src/query.rs`, `src/community.rs`
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0078_retire_index_tests`
- `cargo test --test retire_remove_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
