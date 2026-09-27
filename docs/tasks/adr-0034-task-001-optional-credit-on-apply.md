# ADR 0034 Task 001: A Node Accepts An Event With No Credit

Owner: [ADR 0034](../adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md)
§11, release A. Plan: [the phase plan](../plans/adr-0034-artist-credit-removal-phase-plan.md).

Repository: `stophammer`. The operator commits. This task joins release 0.2.0.

## Goal

A node applies a `FeedUpserted` or a `TrackUpserted` event whose `feed` or
`track` has no `artist_credit_id`. The primary still writes the field, so no
event that the primary signs changes.

## Files To Inspect

- `src/model.rs`: `Feed`, `Track`
- `src/apply.rs`: the arms `FeedUpserted` and `TrackUpserted`
- `src/db.rs`: `get_or_create_feed_scoped_source_text_credit`, `upsert_feed`,
  `upsert_track`, and each other reader of `feed.artist_credit_id` or
  `track.artist_credit_id` (use grep)
- `src/api.rs`: `derive_release_artist`, and the ingest code that builds
  `model::Feed` and `model::Track` (search for `artist_credit_id:`)
- `tests/apply_tests.rs`: the helpers that build signed events

## Files Likely To Change

- `src/model.rs`, `src/apply.rs`, `src/db.rs`, `src/api.rs`, `src/quality.rs`
  (only for the type change)
- Each test file that builds a `Feed` or a `Track` literal (the type change)
- `tests/adr0034_optional_credit_tests.rs`, new

## Do Not Touch

- The ingest behavior: the primary still makes each credit, still sets
  `artist_credit_id` to `Some`, and still signs `ArtistCreditCreated` and
  `ArtistUpserted`.
- `src/schema.sql`, `migrations/`. No schema change.
- The SSE registry, the quality rules, the artist tables.
- `stophammer-crawler/`, `stophammer-parser/`.

## Constraints

- `Feed.artist_credit_id` and `Track.artist_credit_id` become `Option<i64>`
  with `#[serde(default, skip_serializing_if = "Option::is_none")]`. With
  `Some`, the serialized JSON is the same as today.
- In `apply`, when `artist_credit_id` is `None`, get or make the credit with
  `get_or_create_feed_scoped_source_text_credit` before the upsert:
  - a feed: the trimmed `release_artist`, or `"Unknown Artist"` when it is
    absent or empty (the placeholder of `derive_release_artist`).
  - a track: the trimmed `track_artist`. When it is absent or empty, use the
    credit of the feed, from the stored feed row.
- Put this rule in one helper in `src/db.rs` or `src/apply.rs`, with a doc
  comment that names ADR 0034 §11. The ingest does not call it.
- Each other reader of the field keeps its behavior. Where the code needs an
  `i64`, it takes the stored column, not the payload.

## Steps

1. Change the two fields. Fix each compile error with the smallest change.
2. Add the helper, and call it in the two apply arms.
3. Write the tests.

## Acceptance

Mechanical, each an integration test in
`tests/adr0034_optional_credit_tests.rs`:

- A signed `FeedUpserted` event with no `artist_credit_id` applies. The
  stored feed has a credit whose display name is the `release_artist`.
- A signed `FeedUpserted` event with no `artist_credit_id` and no
  `release_artist` applies, with the credit "Unknown Artist".
- A signed `TrackUpserted` event with no `artist_credit_id` applies. With a
  `track_artist`, the credit has that name. With none, the track has the feed
  credit.
- A `Feed` with `Some(7)` serializes to the same JSON as before the change:
  the key `artist_credit_id` with the value 7.
- The existing apply and replication tests pass with no change of their
  assertions.
- The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --test adr0034_optional_credit_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report without a workaround when:

- A reader of `artist_credit_id` needs the payload value and cannot use the
  stored column.
- The serialized JSON of a `Feed` with `Some` differs from today.
- The OpenAPI document changes (`cargo run --bin gen_openapi`).

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0034-task-001-optional-credit-on-apply.md
- /home/citizen/build/stophammer/docs/adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md (section 11)
- /home/citizen/build/stophammer/AGENTS.md (Code Style, Tests)
- Only the parts of src/api.rs, src/db.rs and src/apply.rs named in the task file. Use grep. The files are long.

Goal:
- A node applies a FeedUpserted or TrackUpserted event with no artist_credit_id, and makes the feed-scoped credit from the payload text.

Constraints:
- The rules under "Constraints" in the task file.
- The primary still makes each credit and still writes the field.

Do not touch:
- src/schema.sql, migrations/, the SSE registry, the quality rules, stophammer-crawler/, stophammer-parser/.
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
