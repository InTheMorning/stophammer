# ADR 0059 Task 004: A Remote Value Time Split Gives Its Summary

Owner: [ADR 0059](../adr/0059-an-entry-that-names-a-feed-gives-its-summary.md)
§7. The phase plan is [the summary rows plan](../plans/summary-rows-phase-plan.md).

Repository: `stophammer`. The operator commits. This task does not depend on
task 003.

## Goal

Each row of `include=value_time_splits` on a track read gives the five feed
fields of ADR 0059 §1 and the four track fields of §5. The names are the names
that a `remote_items` entry uses.

## Files To Inspect

- `src/query.rs`:
  - `VtsResponse` and `vts_response_from_split` (ADR 0073 §4)
  - the `"value_time_splits"` arm of the track includes (near line 2353)
  - `NamedFeedSummary` and `named_feed_summary` (ADR 0059 §1)
  - `feed_remote_item_response`: the track lookup of ADR 0059 §5
  - `FeedRemoteItemResponse`: the doc comments of the summary fields
- `src/model.rs`: `ValueTimeSplit`
- `tests/fixtures/v1-schema-fields.json`: `VtsResponse`
- `docs/API.md`: the rules of `value_time_splits` (near line 1885)
- `tests/adr0073_split_response_tests.rs`: the test setup of a split
- `tests/adr0059_track_summary_tests.rs`: the test of the §5 fields

## Files Likely To Change

- `src/query.rs`, `tests/fixtures/v1-schema-fields.json`, `docs/API.md`
- `tests/adr0059_split_summary_tests.rs`, new

## Constraints

- **The fields.** Add these fields to `VtsResponse`, after
  `remote_item_guid`, each `Option`:
  - `remote_feed_title`, `remote_feed_image_url`, `remote_release_artist`,
    `remote_release_artist_source` and `remote_episode_count`
  - `remote_track_guid`, `remote_track_title`, `remote_track_duration_secs`
    and `remote_track_image_url`
- **The feed.** Read the feed with
  `named_feed_summary(conn, Some(&remote_feed_guid))`. A split gives no URL, so
  do not call `resolve_listed_feed` with a URL. A feed that is not indexed
  gives null in each feed field and each track field.
- **The track.** Move the track lookup of `feed_remote_item_response` into one
  new function, for example `named_track_summary(conn, feed_guid, item_guid)`.
  `feed_remote_item_response` and the split row both call it. Its query and its
  `web_url_or_none` of the image do not change. The split row calls it only
  when the feed is indexed and `remote_item_guid` is not `""`. In each other
  case, the four track fields are null.
- **The function.** `vts_response_from_split` takes `conn` and gives
  `Result<Option<VtsResponse>, api::ApiError>`. A local split gives `None`
  with no query. Each field of ADR 0073 §4 keeps its value and its type.
- **No other change.** `include=local_value_time_splits` and
  `LocalVtsResponse` do not change. A `remote_items` entry gives the same values
  as before the move.
- **The contract.** Add each new property of `VtsResponse` to
  `tests/fixtures/v1-schema-fields.json`. `docs/API.md` gives the new fields,
  the GUID-only rule, and the null rule for `remote_item_guid` `""`.
- **Comments.** A doc comment on each new field names ADR 0059 §7.

## Do Not Touch

- `CopyRecordResponse` and `LiveItemListResponse`. Task 003 changes them.
- `LocalVtsResponse`, `local_vts_response_from_split`, and the store of splits
- `db::resolve_listed_feed` and the `publisher` view
- The migrations, `src/schema.sql`, `src/db.rs` and the events
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the tests of the acceptance criteria.
2. Move the track lookup into its function, and run
   `cargo test --test adr0059_track_summary_tests`.
3. Add the fields, and the `conn` argument of `vts_response_from_split`.
4. Change the fixture and `docs/API.md`.
5. Run the test commands.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0059_split_summary_tests.rs`, through
the HTTP router.

- A remote split names an indexed feed and an indexed track. Its row gives the
  five feed fields of that feed and the four track fields of that track.
- A remote split with `remote_item_guid` `""` that names an indexed feed gives
  the feed fields, and null in each track field.
- A remote split that names an indexed feed and a track that is not indexed
  gives the feed fields, and null in each track field.
- A remote split that names a feed that is not indexed gives null in each of
  the nine fields.
- A named feed with a `javascript:` image gives `remote_feed_image_url` null.
  A named track with a `javascript:` image gives `remote_track_image_url` null.
- The fields of ADR 0073 §4 keep their values: the tests of
  `adr0073_split_response_tests.rs` pass with no change.
- The `remote_items` tests of `adr0059_entry_summary_tests.rs` and
  `adr0059_track_summary_tests.rs` pass with no change.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0059_split_summary_tests
cargo test --test adr0059_track_summary_tests
cargo test --test adr0059_entry_summary_tests
cargo test --test adr0073_split_response_tests
cargo test --test adr0044_field_removal_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A remote split row needs more than two more queries.
- The move of the track lookup changes a value of a `remote_items` entry.
- A remote split has no `remote_feed_guid` in the store.
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
- `~/build/stophammer/AGENTS.md` (the code style, the API steps and the pre-commit gate)
- `~/build/stophammer/docs/adr/0059-an-entry-that-names-a-feed-gives-its-summary.md`, §1, §3, §5 and §7
- `~/build/stophammer/docs/adr/0073-a-value-time-split-keeps-both-forms.md`, §4
- `~/build/stophammer/docs/tasks/adr-0059-task-004-split-summary.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- Each row of `include=value_time_splits` gives the five feed fields of ADR 0059 §1 and the four track fields of §5.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Read the feed by `remote_feed_guid` only, with `named_feed_summary`.
- Move the track lookup of `feed_remote_item_response` into one function, and call it from both places.
- `remote_item_guid` `""` gives null track fields. Each field of ADR 0073 §4 stays the same.
- Stop at each escalation trigger of this task.

Do not touch:
- `CopyRecordResponse`, `LiveItemListResponse`, `LocalVtsResponse`, the store of splits
- `db::resolve_listed_feed`, the migrations, `src/schema.sql`, `src/db.rs`
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0059_split_summary_tests`
- `cargo test --test adr0059_track_summary_tests`
- `cargo test --test adr0059_entry_summary_tests`
- `cargo test --test adr0073_split_response_tests`
- `cargo test --test adr0044_field_removal_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cargo run --bin gen_openapi > /dev/null`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
