# ADR 0070 Task 002: The Artist List

Owner: [ADR 0070](../adr/0070-a-name-list-gives-each-identity-one-row.md)
sections 3 to 6, with section 4a. v4vmm request 6. Task 001 comes first.

Repository: `stophammer`. The operator commits.

## Goal

`GET /v1/artist-names` gives one `publisher` row for each publisher feed, and
one `name` row for each sort key of the releases of the name level. The rows
come in the order of `sort_name`, then `kind`, then `key`, with `cursor` and
`from`.

## Files To Inspect

- `src/query.rs`: `load_publisher`, `music_to_publisher_facts`,
  `role_agreement`, `rel_tokens`, `link_facts`, `web_url_or_none`, the
  handler of task 001
- `src/model.rs`: `sort_key`, from task 001
- `src/db.rs`: `resolve_listed_feed`, `get_feed_remote_items_for_feed`
- `src/openapi.rs`: the path of task 001
- `tests/adr0068_link_facts_tests.rs` and `tests/adr0069_*`: the fixtures

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`
- `tests/adr0070_artist_list_tests.rs`, new
- `docs/API.md`

## Constraints

- **Name level.** A music release is of the name level in two conditions.
  The first condition: no `music_to_publisher` row of its publisher view
  resolves to a feed that the node holds. The second condition: each such row
  has `role_agreement` `both`, and its role tokens do not include `artist`.
- **One rule.** Read the facts with the functions of the publisher view. Do not write a
  second rule.
- **A `name` row.** The key is the sort key of task 001 of the
  `release_artist`. `name` is the raw value that the most releases of the key
  state. With a tie, use the first raw value in code point order. A release
  with a null or empty `release_artist` gives no row.
- **A `publisher` row.** Each feed with the medium `publisher`, also with no
  music feed. `name` is the feed title, and `image_url` passes
  `web_url_or_none`.
- **Order and cursor.** `(sort_name, kind, key)` in code point order. `kind`
  `name` comes before `publisher`. The cursor encodes the three values of the
  last row. `from` works as in task 001.
- **Fields.** `kind`, `name`, `sort_name`, `key`, `feed_guid`, `image_url`,
  `release_count`, as ADR 0070 §3 gives. Register the row type with
  `ToSchema`.
- **Memory copy (ADR 0070 §4a, amended 2026-10-17).** `AppState` holds the
  built list and the highest `events.seq` at its build. A request reads
  `SELECT max(seq) FROM events`. When the value is the same, the request uses
  the copy. When it differs, and the last build is 30 seconds old or more,
  the request builds the list again. Else it uses the old copy. One lock lets
  only one build run. Give the 30 seconds as a constant, and let a test give a
  shorter interval, so that no test waits 30 seconds.
- **The change marker.** Each write that can change the list must also write
  an event row in the same transaction: the feed upsert, `ingest_transaction`,
  the three writes of `feed_remote_items_raw`, the feed delete, and the
  `feed_url` update in `src/api.rs`. Verify each one, on the primary and on
  the apply path of a community node.
- **Cost.** The first build takes about 200 ms on the backup. Measure a page
  from the copy in a release build, and report it.
- **Contract.** The route is in `spec_value()` with an envelope from
  `envelope_schema`, and `/v1/node/capabilities` lists it.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- Each existing migration file. Never rewrite a merged migration.
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- Each existing route, response field and column, except where a step below
  names it
- `backups/`. Read a backup only with `?immutable=1`, or from a copy in the
  scratch directory.
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Read how `load_publisher` gives the `music_to_publisher` rows of an album,
   with `role_agreement` and `role`. Write one function that gives true when a
   release is of the name level (ADR 0070 §4). It reads those rows. Unit test
   it with the four cases of the acceptance criteria.
2. Write one function that builds the full sorted row list: the `publisher`
   rows from the feeds with the medium `publisher`, and the `name` rows from
   the music feeds of the name level, grouped by `sort_key` of task 001.
3. Add the row type `ArtistNameRow` with `ToSchema`, and register it.
4. Add the handler and the route `/v1/artist-names`. Apply `from` or the
   cursor to the sorted list, then take `limit + 1` rows.
5. Add the path to `spec_value()`, and the route to `handle_capabilities`.
6. Write the tests, then `docs/API.md`.
7. Add the memory copy of ADR 0070 §4a, and its tests.
8. Verify the change marker for each write of "Constraints".
9. Measure a page from the copy in a release build.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0070_artist_list_tests.rs`.

- A publisher feed that no music feed names gives a `publisher` row.
- An album that names a held publisher feed, with no `rel`, gives no `name`
  row.
- An album with no publisher link gives a `name` row with `release_count` 1.
- An album whose only link has `role_agreement` `both` and the role `label`
  gives a `name` row.
- An album with a link with the role `artist, label` on both sides gives no
  `name` row.
- Two albums with the artist texts `"Haleen"` and `" haleen "` give one `name`
  row with `release_count` 2.
- A publisher feed with the title `"Haleen"` and a `name` row `"Haleen"` give
  two rows, with the `name` row first.
- Pages of size 2 give each row one time, and `from=h` starts at the first key
  that starts with `h`.
- Two requests with no event between them build the list one time.
- After an ingest of a new album, a request after the interval gives its row.
- A request before the interval ends gives the old copy.
- The ADR 0044 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0070_artist_list_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A step needs a change to an ADR, or to a file of "Do Not Touch".
- A constraint of this task disagrees with the code.
- An existing test fails after the change, and the cause is not clear.
- A write of "Constraints" changes the list and writes no event row.
- A page from the copy takes more than 10 ms in a release build.
- The functions of the publisher view cannot give the role facts of one album
  without a full publisher read.
- The name level needs a rule that ADR 0070 §4 does not state.

## Expected Final Report

1. files changed
2. tests run and their results, the time of a page from the copy, and the
   result of the change marker check
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `AGENTS.md` (the code style and the pre-commit gate)
- `docs/adr/0070-a-name-list-gives-each-identity-one-row.md`, sections 3 to 6
- `docs/adr/0049-publisher-relationships-are-rss-facts.md`, section 6a
- `docs/tasks/adr-0070-task-002-artist-list.md` (this task: the constraints, the steps and the triggers)
- the files of "Files To Inspect" in this task

Goal:
- `GET /v1/artist-names` gives one `publisher` row for each publisher feed and one `name` row for each sort key of the releases of the name level, in the order of `(sort_name, kind, key)`, with `cursor` and `from`.

Constraints:
- Follow the section "Constraints" and the section "Implementation Steps" of this task.
- Use `model::sort_key` of task 001. Do not write a second key rule.
- The name level reads the link facts of the publisher view. Do not write a second role rule.
- Add no table and no migration. Keep the list in memory (ADR 0070 §4a).
- Stop at each escalation trigger of this task.

Do not touch:
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`
- each existing migration file
- each existing route, response field and column, except as this task names
- `backups/` except for read-only measurement
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0070_artist_list_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
