# ADR 0034 Task 002: No Reader Uses The Artist Credit

Owner: [ADR 0034](../adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md)
§11, release B. Plan: [the phase plan](../plans/adr-0034-artist-credit-removal-phase-plan.md).

Repository: `stophammer`. The operator commits. Task 001 comes first.

## Goal

Two readers use the artist credit today: the internal SSE registry and the
quality scores of a feed and a track. After this task, neither reads an artist ID, a credit or
`artist_credit_id`. No write and no event changes.

## Files To Inspect

- `src/api.rs`: `SseRegistry` and its methods, `extract_artist_ids`,
  `artist_ids_for_feed`, `build_live_sse_frames_for_feed`,
  `publish_sse_frames`, `publish_events_to_sse`
- `src/quality.rs`: `compute_feed_quality`,
  `compute_track_quality_for_feed_track`, `compute_artist_quality`, and the
  two structs with `artist_credit_id`
- `src/event.rs`: the payload types, to find the feed GUID of each event
- `tests/sse_publish_tests.rs`, `tests/quality_tests.rs`, and each test that
  calls `SseRegistry` (use grep)

## Files Likely To Change

- `src/api.rs`, `src/quality.rs`
- `tests/sse_publish_tests.rs`, `tests/quality_tests.rs`, and each test that
  uses the removed functions
- `tests/adr0034_readers_tests.rs`, new

## Do Not Touch

- The ingest, the apply step, the events, `src/db.rs` writes, the schema,
  `migrations/`. Task 003 owns them.
- The public routes. ADR 0037 keeps the SSE route unpublished.
- `stophammer-crawler/`, `stophammer-parser/`.

## Constraints

- **SSE.** The registry keys each channel by feed GUID. Rename the key
  parameters and the doc comments from artist to feed.
  - An event goes to the channel of the feed GUID that it names: the
    `feed_guid` of the `feed` or the `track` in `FeedUpserted` and
    `TrackUpserted`, else the `feed_guid` field of the payload. An event that
    names no feed GUID goes to no channel.
  - `build_live_sse_frames_for_feed` keys its frames by the feed GUID, and
    reads no artist.
  - Delete `extract_artist_ids` and `artist_ids_for_feed`.
- **Quality.** `compute_feed_quality` gives the 10 points when the stored
  `release_artist` is not null and not empty, in place of
  `artist_credit_id > 0`. The track score gives its 5 points for a stored
  `track_artist` that is not empty. With no such value, it gives them for a
  `release_artist` of its feed that is not empty. Delete `compute_artist_quality` and each
  test that tests only it. No quality function reads `artist_credit_id`.
- Each doc comment that you change names ADR 0034 §11.

## Steps

1. Change the SSE key and the event routing.
2. Change the quality rule, and delete `compute_artist_quality`.
3. Correct the tests, and write the new ones.
4. Check with grep: `src/api.rs` and `src/quality.rs` contain no
   `artist_id` and no `artist_credit` in the SSE and quality code.

## Acceptance

Mechanical, each an integration test in `tests/adr0034_readers_tests.rs`:

- A `FeedUpserted` event reaches a subscriber of the channel of its feed
  GUID.
- A `TrackUpserted` event reaches the channel of the feed GUID of its track.
- A live frame of a feed reaches the channel of that feed GUID.
- For a feed built by the ingest test helpers, `compute_feed_quality` gives
  the same score before and after the change. Compute the expected value from
  the scoring rules, not from the new code.
- The same for a track with a `track_artist`, and for a track without one in
  a feed with a `release_artist`.
- The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --test adr0034_readers_tests
cargo test --test sse_publish_tests
cargo test --test quality_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report without a workaround when:

- A public route reads the SSE registry.
- A feed in the tests has a credit and a null or empty `release_artist`, so
  its score changes.
- A reader of the credit exists outside the SSE code and the quality code.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0034-task-002-readers-leave-the-credit.md
- /home/citizen/build/stophammer/docs/adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md (section 11)
- /home/citizen/build/stophammer/AGENTS.md (Code Style, Tests)
- Only the parts of src/api.rs named in the task file. Use grep. The file is long.

Goal:
- The internal SSE registry keys by feed GUID, and the feed quality score reads release_artist in place of artist_credit_id. No write and no event changes.

Constraints:
- The rules under "Constraints" in the task file.

Do not touch:
- The ingest, the apply step, the events, db writes, the schema, migrations/, public routes, stophammer-crawler/, stophammer-parser/.
- Git: run no git command that writes (no add, commit, stash, checkout, reset, push).

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
