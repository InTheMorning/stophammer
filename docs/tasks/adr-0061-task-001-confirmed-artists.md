# ADR 0061 Task 001: The Confirmed Artists Of A Publisher

Owner: [ADR 0061](../adr/0061-a-publisher-read-counts-its-listed-artists.md).
Plan: item 15 of the [remaining accepted work
plan](../plans/remaining-accepted-work-plan.md).

Repository: `stophammer`. The operator commits.

## Goal

A feed read of a publisher feed gives `confirmed_release_artists`,
`confirmed_release_artist_count`, `unconfirmed_release_artists` and
`unconfirmed_release_artist_count` (ADR 0061 §1). A test guards §5: an album
read gives only the publishers that the album names.

## Files To Inspect

- `src/query.rs`: `FeedResponse` (the fields `distinct_release_artist_count`
  and `distinct_release_artists`), `publisher_artist_count`,
  `normalize_release_artist`, `build_feed_response` (the block
  `medium::is_publisher`), `load_publisher`, `PublisherResponse`
  (`direction`, `music_names_publisher`, `publisher_link_resolution`,
  `remote_release_artist`, `remote_release_artist_source`)
- `src/openapi.rs`: the entry of `/v1/feeds/{guid}`
- `tests/adr0049_artist_count_tests.rs`, `tests/adr0059_entry_summary_tests.rs`:
  helpers to build publisher and album feeds
- `docs/API.md`: the section on the publisher artist count

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs` (only when the schema does not follow the
  struct), `docs/API.md`
- `tests/adr0061_confirmed_artists_tests.rs`, new

## Do Not Touch

- The meaning of `distinct_release_artist_count` and `distinct_release_artists`.
- `load_publisher` and the row builders. The album read already follows §5.
- `src/db.rs`, migrations, `docs/adr/`, `stophammer-crawler/`,
  `stophammer-parser/`.

## Constraints

- Build the four fields from the rows that `load_publisher` gives for the
  publisher feed. No new database query.
- A listed album is a row with `direction` `publisher_to_music` and a
  `publisher_link_resolution` other than `unresolved`. It is confirmed when
  `music_names_publisher` is true.
- Take the artist from `remote_release_artist`. Skip a row with no value, or
  with `remote_release_artist_source` `placeholder`.
- Two values are the same when `normalize_release_artist` gives the same
  result. Keep the raw value of the first row, in the order of the rows.
- An artist of a confirmed album is not in the unconfirmed list.
- Each field is present only on a publisher feed, like
  `distinct_release_artist_count` (`skip_serializing_if`). Each has a doc
  comment that names ADR 0061 §1.
- `docs/API.md` states the difference between `distinct_*` (albums that name
  the publisher) and `confirmed_*` / `unconfirmed_*` (albums that the
  publisher lists).

## Steps

1. Add the four fields to `FeedResponse`.
2. Write a pure helper that takes the rows and gives the two lists. Reuse the
   rows that the read already built, or call `load_publisher` one time.
3. Fill the fields in the `medium::is_publisher` block.
4. Update the OpenAPI entry if needed, and `docs/API.md`.
5. Write the tests.

## Acceptance

Mechanical, each an integration test:

- A publisher that lists 2 albums by 2 artists, of which no album names it,
  gives `confirmed_release_artist_count` 0 and
  `unconfirmed_release_artist_count` 2.
- A publisher that lists 2 albums by 2 artists, of which one names it, gives 1
  and 1.
- A listed album with a `placeholder` artist is not counted.
- Two listed albums by "Ann Lee" and " ann  lee " count as one artist.
- An unresolved listed album is not counted.
- The read of an album that a publisher lists, and that does not name the
  publisher, gives no `publisher` row for that publisher.
- A music feed read has none of the four fields.
- The guards of ADR 0044 pass. The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --test adr0061_confirmed_artists_tests
cargo test --test adr0044_schema_refs_tests
cargo test --test adr0044_contract_guard_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Escalation Triggers

Stop and report without a workaround when:

- `load_publisher` does not give `remote_release_artist_source`, so a
  placeholder cannot be told apart.
- The album read test fails. Then the code does not follow §5, and the
  planner decides.
- A guard of ADR 0044 needs a change to the guard.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0061-task-001-confirmed-artists.md
- /home/citizen/build/stophammer/docs/adr/0061-a-publisher-read-counts-its-listed-artists.md
- /home/citizen/build/stophammer/AGENTS.md (Code Style, Tests)
- Only the parts of src/query.rs named in the task file. Use grep. The file is long.

Goal:
- Add the four confirmed and unconfirmed artist fields to the feed read of a publisher feed, and a test that an album read gives only the publishers that it names.

Constraints:
- The rules under "Constraints" in the task file.
- No new database query. No change to load_publisher or to the distinct_* fields.

Do not touch:
- src/db.rs, migrations, docs/adr/, stophammer-crawler/, stophammer-parser/.
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
