# ADR 0049 Task 007: Each Text Field Names Its Source

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §5.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md).
Needs: task 005. Merge after task 004, because of the migration number.

## Goal

1. `release_artist` comes from `itunes:author`, then the `itunes:owner` name,
   then "Unknown Artist". `release_artist_source` names the source.
2. `publisher_text` is the `itunes:owner` name.
3. A feed read gives `publisher_feed_title`, derived when the node reads it.
4. The Wavlake host rules and the publisher repair are deleted.

## Files To Inspect

- `src/api.rs`: `derive_feed_artist_name`, `derive_publisher_name`,
  `is_wavlake_url`, `find_linked_publisher_feed`,
  `has_reciprocal_music_remote_item`, `derive_linked_publisher_name`,
  `feed_remote_item_targets_publisher`, `publisher_repair_text`,
  `publisher_repair_release_artist`, `repair_tracks_for_publisher_text`,
  `music_feed_can_be_repaired_from_publisher`,
  `repair_music_feed_from_publisher`,
  `repair_linked_music_feeds_after_publisher_ingest`,
  `wavlake_artist_name_from_links`, `humanize_slug`, `capitalize_word`,
  `is_platform_owner_name`, `classify_platform_owner`, and in
  `handle_ingest_feed` the block that sets `artist_name`, `publisher` and
  `release_artist`, and the call of the repair near line 2580
- `src/model.rs`: `Feed`
- `src/db.rs`: `upsert_feed`, and each `SELECT` that builds a `Feed`
- `src/query.rs`: `FeedResponse`, and each response type with
  `release_artist`
- `src/openapi.rs`: the feed and track examples
- `tests/api_canonical_query_tests.rs`, `tests/db_tests.rs`,
  `tests/apply_tests.rs`: the Wavlake tests

## Files Likely To Change

- `migrations/0037_feed_release_artist_source.sql`, new
- `src/schema.sql`, `src/model.rs`, `src/db.rs`, `src/api.rs`, `src/query.rs`,
  `src/openapi.rs`
- `tests/adr0049_text_field_tests.rs`, new
- the tests that state a Wavlake rule. Delete them

## Do Not Touch

- `classify_platform_url` and the source platform claims. A platform claim
  records a fact about the host, and it decides no relationship
- the `track_artist` rule. It stays: the item author, then the feed artist name
- the resolver and the rules of tasks 005 and 006
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- The column is `release_artist_source TEXT`, nullable. The migration adds it
  and fills nothing. A feed with no ingest since the migration reads `null`.
- `model::Feed` gains `release_artist_source: Option<String>` with
  `#[serde(default)]`.
- One pure function gives the artist name and its source:
  1. a non-empty trimmed `author_name` gives `"itunes_author"`,
  2. else a non-empty trimmed `owner_name` for which `is_platform_owner_name`
     is false gives `"itunes_owner"`,
  3. else `"Unknown Artist"` gives `"placeholder"`.
- `publisher` is `derive_publisher_name(feed_data)`: the trimmed owner name, or
  `null`.
- Delete each function in the list under "Files To Inspect" from
  `is_wavlake_url` to `capitalize_word`, and the call of the repair. Keep a
  function only if a caller outside the deleted set still needs it, and report
  it.
- `publisher_feed_title` on `FeedResponse`: take the publisher remote item of
  the feed with the lowest `position`. Resolve it with
  `db::resolve_listed_feed`. When it resolves, give the title of that feed.
  Else give `null`. Its doc comment says that the value is derived.
- Add `release_artist_source` to `FeedResponse`, and to each other response
  type that gives the feed `release_artist`. It always sits beside
  `release_artist`, and it means the same on each route (ADR 0042).
- Delete a test only when it asserts a rule that this task deletes. The report
  names each deleted test.

## Implementation Steps

1. Add the migration, `MIGRATIONS` entry, `src/schema.sql` column, model field,
   and the column in `upsert_feed` and each `Feed` reader.
2. Add the pure function and its unit tests.
3. Change the ingest block to use it, and to use `derive_publisher_name` for
   `publisher`.
4. Delete the Wavlake rules and the repair.
5. Add `release_artist_source` and `publisher_feed_title` to the responses and
   the OpenAPI examples.
6. Delete the tests of the deleted rules. Add the tests below.
7. Add a guard test: `src/api.rs` holds no `fn is_wavlake_url`, no
   `fn wavlake_artist_name_from_links` and no `remote_feed_guid ==`. The
   failure message names ADR 0049 §5 and says: "Take the value from one RSS
   element, and put a derived value in a separate field."
8. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- A migration test proves the new column, and that an existing row reads `null`.
- Unit tests prove the three branches of the pure function, and that owner
  "Wavlake" gives `"placeholder"` when there is no author.
- DETOX test: ingest `detox-artist` and `detox-album`. The album gives
  `release_artist` equal to its `itunes:author`,
  `release_artist_source = "itunes_author"`, `publisher_text = "Wavlake"`, and
  `publisher_feed_title` equal to the title of `detox-artist`.
- RSS Blue test: `rssblue-album` gives `publisher_text` equal to its owner name,
  not the publisher feed title.
- `no-publisher-album` gives `publisher_feed_title = null`.
- A test proves that the ingest of a publisher feed emits no event for a
  different feed.
- A track read gives `release_artist_source` beside `release_artist`.
- The guard test passes.

## Test Commands

```bash
cargo build
cargo test --test migration_tests
cargo test --test adr0049_text_field_tests
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
6. each deleted test, with the rule that it asserted
7. each deleted function

## Escalation Triggers

Stop and report when:

- a test that does not state a Wavlake rule fails
- a response type gives a `release_artist` that does not come from the feed
- migration number 0037 is taken
- a `Feed` reader is outside `src/db.rs`

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0049-task-007-text-fields.md`, the sections "Files To
  Inspect" and "Constraints", in full
- `src/api.rs`: the functions in that list, and the ingest block that sets
  `artist_name`, `publisher` and `release_artist`
- `src/model.rs` `Feed`
- `src/db.rs` `upsert_feed` and the `Feed` readers
- `src/query.rs` `FeedResponse`, and each response with `release_artist`

Goal:
- `release_artist` from `itunes:author`, then the non-platform owner name, then
  "Unknown Artist", with `release_artist_source`. `publisher_text` from the
  owner name. `publisher_feed_title` derived when read. The Wavlake rules and
  the publisher repair deleted.

Constraints:
- `migrations/0037_feed_release_artist_source.sql` adds nullable
  `release_artist_source TEXT` to `feeds` and fills nothing.
- `Feed.release_artist_source` is `Option<String>` with `#[serde(default)]`.
- One pure function gives the name and its source: `"itunes_author"`,
  `"itunes_owner"` or `"placeholder"`.
- Delete the functions from `is_wavlake_url` to `capitalize_word` in the task
  list, and the repair call. Report any function that you keep, and why.
- `publisher_feed_title` resolves the lowest-position publisher item through
  `db::resolve_listed_feed`, and gives that feed's title, or `null`.
- `release_artist_source` sits beside each feed `release_artist` on each route.
- Delete a test only when it asserts a deleted rule.
- Add a guard test on the text of `src/api.rs`: no `fn is_wavlake_url`, no
  `fn wavlake_artist_name_from_links`, no `remote_feed_guid ==`. Its message
  names ADR 0049 §5 and says: "Take the value from one RSS element, and put a
  derived value in a separate field."

Do not touch:
- `classify_platform_url` and the source platform claims
- the `track_artist` rule
- the resolver and the rules of tasks 005 and 006
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- Tests prove:
  - The column and a `null` for an existing row.
  - The three branches, and owner "Wavlake" with no author gives `"placeholder"`.
  - The DETOX, RSS Blue and no-publisher values in the task file.
  - A publisher ingest emits no event for a different feed.
  - A track read gives `release_artist_source`.
  - The guard.

Test commands:
- `cargo build`
- `cargo test --test migration_tests`
- `cargo test --test adr0049_text_field_tests`
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
