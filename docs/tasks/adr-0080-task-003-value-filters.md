# ADR 0080 Task 003: The Kind, Count, Date And Duration Filters

Owner: [ADR 0080](../adr/0080-a-search-takes-phrases-and-field-filters.md)
§2 (the value filters) and §7. The phase plan is
[the search filters plan](../plans/search-filters-phase-plan.md).

Repository: `stophammer`. The operator commits. This task starts after task
002.

## Goal

`GET /v1/search` applies `kind`, `tracks`, `min_tracks`, `max_tracks`,
`after`, `before`, `min_duration` and `max_duration`, from the stored rows of
the feed and the track of each hit.

## Files To Inspect

- `src/search.rs`: `search` and `SearchFilters` (task 002)
- `src/query.rs`: `SEARCH_FILTERS` and the parse of the raw query (task 002)
- `src/schema.sql`: `feeds` (`release_kind`, `episode_count`,
  `newest_item_at`) and `tracks` (`pub_date`, `duration_secs`)
- `src/api.rs`: the ingest sets `release_kind` to `unknown` (near line 3291)
- `src/openapi.rs`, `docs/API.md`: the parameters of `/v1/search`

## Files Likely To Change

- `src/search.rs`, `src/query.rs`, `src/openapi.rs`, `docs/API.md`
- `tests/adr0080_value_filter_tests.rs`, new

## Constraints

- **The table.** Add these names to `SEARCH_FILTERS`:

  | Name | `feed` | `track` | `publisher` |
  |---|---|---|---|
  | `kind` | yes | yes | no |
  | `tracks`, `min_tracks`, `max_tracks` | yes | yes | no |
  | `after`, `before` | yes | yes | no |
  | `min_duration`, `max_duration` | no | yes | no |

- **The joins.** Join the feed and the track of each hit in the outer query of
  `search`:
  - `LEFT JOIN feeds f ON f.feed_guid = CASE e.entity_type WHEN 'track' THEN
    json_extract(e.entity_id, '$[0]') ELSE e.entity_id END`
  - `LEFT JOIN tracks t ON e.entity_type = 'track' AND t.feed_guid =
    f.feed_guid AND t.track_guid = json_extract(e.entity_id, '$[1]')`
  - Add a join only when a filter of this task needs it.
- **The columns.**
  - `kind` reads `f.release_kind`. The value is a list of `album`, `ep`,
    `single` and `unknown`, with commas. The kinds of the list are an OR.
    `unknown` also matches a NULL kind. An empty item or a different word
    gives `invalid_filter`.
  - `tracks`, `min_tracks` and `max_tracks` read `f.episode_count`. The value
    is an integer from 0. The limits are included.
  - `after` and `before` read `f.newest_item_at` for a feed and `t.pub_date`
    for a track. The value is `YYYY-MM-DD`, a day in UTC. `after` matches a
    value from 00:00:00 of that day. `before` matches a value before 00:00:00
    of the next day. A NULL date does not match.
  - `min_duration` and `max_duration` read `t.duration_secs`. The value is an
    integer number of seconds from 0. The limits are included. A NULL duration
    does not match.
- **The date parse.** Read the three numbers of `YYYY-MM-DD`, and make the day
  with `time::Date::from_calendar_date`. Do not add a feature to the `time`
  crate. A different form, or a day that does not exist, gives
  `invalid_filter`.
- **The integers.** A value that is not a decimal integer from 0 to
  `i64::MAX` gives `invalid_filter`.
- **No order between limits.** `after` later than `before`, or `min_tracks`
  more than `max_tracks`, gives `200` with no rows.
- **The rank.** Each filter is a condition in the outer `WHERE`. The rank does
  not change.
- **The contract.** Declare each new name on `/v1/search` in `src/openapi.rs`,
  with the types that apply it and the form of its value. `docs/API.md` gives
  the same text, and says that the ingest sets `unknown` as the kind of each
  feed now.
- **Comments.** Each new item names ADR 0080 §2.

## Do Not Touch

- The text filters, the exclusions and the error body of task 002, other than
  the table
- `host`. Task 004 adds it.
- A search with no `q`. Task 005 adds it.
- The ingest of `release_kind`. ADR 0080 does not change it.
- The index functions, the repairs and the migrations
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the tests of the acceptance criteria for the value parse.
2. Add the names to the table, and the parse of each value.
3. Add the joins and the conditions.
4. Change the contract and the document.
5. Write the other tests.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0080_value_filter_tests.rs`.

- ADR 0080 check 3, for the value filters: for each "yes" cell of the table of
  this task, one row matches and one row does not match.
- Check 4: `kind=unknown` matches a feed with the kind `unknown` and a feed
  with no kind. `kind=album,single` matches an album and a single, and not an
  EP. A track matches by the kind of its feed.
- Check 5: `after=2026-10-01` matches a value at `2026-10-01T00:00:00Z`, and
  not one second before. `before=2026-10-01` matches a value at
  `2026-10-01T23:59:59Z`, and not one second after. A row with no date does not
  match.
- `tracks=1` matches a feed with one track, and the track of that feed.
- `min_duration=180&max_duration=180` matches a track of 180 seconds only. A
  track with no duration does not match.
- Check 9, for this task: `type=feed&min_duration=1`,
  `type=publisher&kind=album` and `min_duration=1` with no `type` each give
  `unsupported_filter` with the name of the filter.
- Check 10, for this task: `after=2026-13-01`, `after=2026-10`,
  `after=yesterday`, `tracks=-1`, `tracks=1.5`, `min_duration=x`, `kind=lp` and
  `kind=album,` each give `invalid_filter` with the name of the filter.
- `after=2026-10-02&before=2026-10-01` gives `200` with no rows.
- Check 6, for this task: the `rank` of a hit is the same with and without
  `kind=unknown`.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0080_value_filter_tests
cargo test --test adr0080_text_filter_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A track entity ID in the index is not a JSON pair.
- A filter changes the `rank` of a row.
- The date parse needs a new feature of the `time` crate.
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
- `~/build/stophammer/docs/adr/0080-a-search-takes-phrases-and-field-filters.md`, §2 and §7
- `~/build/stophammer/docs/tasks/adr-0080-task-003-value-filters.md` (this task)
- `~/build/stophammer/docs/tasks/adr-0080-task-002-text-filters-and-error-codes.md` (the filter table and the error codes)
- the files of "Files To Inspect" in this task

Goal:
- `GET /v1/search` applies `kind`, the track counts, the dates and the durations from the stored feed and track of each hit.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Put each filter in the outer `WHERE`. The rank does not change.
- A date is `YYYY-MM-DD` in UTC, and each limit is included.
- Stop at each escalation trigger of this task.

Do not touch:
- the text filters and the error body of task 002, other than the table
- `host`, a search with no `q`, the ingest of `release_kind`, the repairs, the migrations
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0080_value_filter_tests`
- `cargo test --test adr0080_text_filter_tests`
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
