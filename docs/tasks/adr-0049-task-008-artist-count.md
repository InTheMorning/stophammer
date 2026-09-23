# ADR 0049 Task 008: The Derived Artist Count

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §7.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md),
decision 6.
Needs: task 005 and task 007.

## Goal

A feed read of a publisher feed gives `distinct_release_artist_count` and
`distinct_release_artists`.

## Files To Inspect

- `src/query.rs`: `FeedResponse`, `handle_get_feed`, and how `raw_medium` is
  read
- `src/db.rs`: `resolve_listed_feed`, and the index
  `idx_feed_remote_items_guid`
- `src/medium.rs`: `is_publisher`
- `src/openapi.rs`: the feed examples

## Files Likely To Change

- `src/db.rs`: one query function
- `src/query.rs`
- `src/openapi.rs`
- `tests/adr0049_artist_count_tests.rs`, new

## Do Not Touch

- the ingest path
- the stored `release_artist` and `release_artist_source`
- the resolver

## Constraints

- The albums of publisher feed P are the music feeds that have a remote item
  with `medium = "publisher"` that resolves to P through
  `db::resolve_listed_feed`. Use the index on `remote_feed_guid` for the GUID
  branch. For the URL branch, find the observations of P and match their URLs
  to `remote_feed_url`.
- Count only the albums with `release_artist_source = "itunes_author"`.
- Normalize each `release_artist` in this sequence:
  1. Remove the white space at the start and at the end.
  2. Replace each internal run of white space with one space.
  3. Apply `str::to_lowercase`.
- Do not split "feat." or a similar credit.
- `distinct_release_artist_count` is the number of distinct normalized values.
- `distinct_release_artists` gives one raw value for each normalized value: the
  value of the album with the lowest `feed_guid`. Sort the list by the
  normalized value.
- Both fields are `Option` with `skip_serializing_if = "Option::is_none"`. They
  are present only when the feed is a publisher feed. A publisher feed with no
  album gives `0` and `[]`.
- Their doc comments give three facts. The values are derived. They include
  only the albums with an artist from `itunes:author`. A "feat." credit can
  count as a different artist.
- Put the normalization in one pure function with unit tests.

## Implementation Steps

1. Add the pure normalization function and its tests.
2. Add a `src/db.rs` function that gives the `release_artist` values of the
   albums of P, with the constraints above.
3. Fill the two fields in `handle_get_feed` for a publisher feed only.
4. Add the OpenAPI example.
5. Add the tests below.
6. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- Unit tests prove that `"  Jimmy   V "` and `"jimmy v"` give one normalized
  value, and that `"A feat. B"` and `"A"` give two.
- A test with three albums that name one publisher, with authors `"X"`,
  `" x "` and `"Y"`, gives a count of 2 and the list `["X", "Y"]`.
- A test proves that an album with `release_artist_source = "itunes_owner"` is
  not counted.
- A test proves that an album that names P through a `feed_url` observation is
  counted.
- A music feed read has neither field.
- A publisher feed with no album gives `0` and `[]`.

## Test Commands

```bash
cargo build
cargo test --test adr0049_artist_count_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- the URL branch needs a full scan of `feed_remote_items_raw`
- a feed read cannot tell a publisher feed from `raw_medium`

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0049-task-008-artist-count.md`, the section "Constraints"
- `src/query.rs` `FeedResponse` and `handle_get_feed`
- `src/db.rs` `resolve_listed_feed`
- `src/medium.rs` `is_publisher`

Goal:
- A publisher feed read gives `distinct_release_artist_count` and
  `distinct_release_artists`.

Constraints:
- The albums of P are the music feeds with a `medium = "publisher"` remote item
  that resolves to P. Count only `release_artist_source = "itunes_author"`.
- Normalize: trim, collapse internal white space to one space,
  `str::to_lowercase`. Do not split credits.
- The list gives, for each normalized value, the raw value of the album with the
  lowest `feed_guid`, sorted by the normalized value.
- Both fields are `Option` and skipped when `None`. Only a publisher feed has
  them. No album gives `0` and `[]`.
- Doc comments say derived, `itunes:author` only, and that "feat." can count
  twice.

Do not touch:
- the ingest path, the stored artist fields, the resolver

Acceptance criteria:
- The gate is green.
- Tests prove:
  - `"  Jimmy   V "` equals `"jimmy v"`.
  - `"A feat. B"` differs from `"A"`.
  - Authors `"X"`, `" x "`, `"Y"` give 2 and `["X", "Y"]`.
  - An `itunes_owner` album is not counted.
  - A `feed_url` link is counted.
  - A music feed has neither field.
  - No album gives `0` and `[]`.

Test commands:
- `cargo build`
- `cargo test --test adr0049_artist_count_tests`
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
