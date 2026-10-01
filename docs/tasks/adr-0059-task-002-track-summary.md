# ADR 0059 Task 002: An Entry That Names A Track Gives Its Summary

Owner: [ADR 0059](../adr/0059-an-entry-that-names-a-feed-gives-its-summary.md)
§5, amended on 2026-10-01. musicindex.org request 5. Release 0.4.0.

Repository: `stophammer`. The operator commits.

## Goal

Each `remote_items` entry with a `remote_track_guid` gives
`remote_track_title`, `remote_track_duration_secs` and
`remote_track_image_url`.

## Files To Inspect

- `src/query.rs`: `FeedRemoteItemResponse` and `feed_remote_item_response`,
  which looks up `remote_track_guid`
- `src/query.rs`: `web_url_or_none`
- `tests/adr0060_*.rs`: the helpers that store a `musicL` feed and its
  tracks
- `docs/API.md`: the `remote_items` fields of ADR 0060

## Files Likely To Change

- `src/query.rs`, `docs/API.md`
- `tests/adr0059_track_summary_tests.rs`, new

## Do Not Touch

- The other fields of an entry, `TrackRemoteItemResponse`, the storage, the
  ingest.

## Constraints

- Take the three values in the query that already finds `remote_track_guid`.
  Add no second query.
- `remote_track_title` is `tracks.title`. `remote_track_duration_secs` is
  `tracks.duration_secs`. `remote_track_image_url` is `tracks.image_url`
  through `web_url_or_none`, not a resolved image.
- Each of the three fields is null when `remote_track_guid` is null.
- Place the three fields after `remote_track_guid`, each with a doc comment
  that names ADR 0059 §5.
- `docs/API.md` names the three fields. It says that a client can use
  `remote_feed_image_url` when `remote_track_image_url` is null.

## Acceptance

Mechanical, each an integration test in
`tests/adr0059_track_summary_tests.rs`:

- A `musicL` entry that names an indexed track with a title, a duration and
  an image gives the three values.
- An entry that names an indexed track with no image and no duration gives
  its title, and null for the other two.
- An entry whose track is not indexed gives null in each of the three fields.
- An indexed track with a `javascript:` image gives a null image.
- The guards of ADR 0044 pass. The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --test adr0059_track_summary_tests
cargo test --test adr0044_schema_refs_tests
cargo test --test adr0044_contract_guard_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report without a workaround when:

- The lookup of `remote_track_guid` cannot give the three values without a
  second query.
- A `tracks` column of the three does not exist.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0059-task-002-track-summary.md
- /home/citizen/build/stophammer/docs/adr/0059-an-entry-that-names-a-feed-gives-its-summary.md (sections 3, 4 and 5)
- /home/citizen/build/stophammer/AGENTS.md (Code Style, Tests)
- Only the parts of src/query.rs named in the task file. Use grep. The file is long.

Goal:
- Each remote_items entry with a remote_track_guid gives remote_track_title, remote_track_duration_secs and remote_track_image_url, from the same lookup.

Constraints:
- The rules under "Constraints" in the task file.
- Write any scratch file under target/, not /tmp. For a lint exception use #[expect], not #[allow].

Do not touch:
- The other entry fields, TrackRemoteItemResponse, the storage, the ingest, stophammer-crawler/, stophammer-parser/.
- Git: run no git command that writes (no add, commit, stash, checkout, reset, push).

Acceptance criteria:
- The criteria under "Acceptance" in the task file.

Test commands:
- The commands under "Test Commands" in the task file.

At the end, report:
1. files changed
2. the real output of each gate command
3. behavior changed
4. deviations from task
5. unresolved concerns
