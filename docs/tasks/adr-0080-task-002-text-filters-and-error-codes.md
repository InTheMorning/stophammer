# ADR 0080 Task 002: The Text Filters, The Exclusions And The Error Codes

Owner: [ADR 0080](../adr/0080-a-search-takes-phrases-and-field-filters.md)
§2 (the text filters), §3, §4 and §7. The phase plan is
[the search filters plan](../plans/search-filters-phase-plan.md).

Repository: `stophammer`. The operator commits. This task starts after task
001.

## Goal

`GET /v1/search` reads its parameters from the raw query. It applies `artist`,
`title`, `release` and the five `not_` parameters. It gives the three error
codes of §4. One filter table gives the filters of each type to the handler,
to `GET /v1/node/capabilities` and to the contract.

## Files To Inspect

- `src/query.rs`: `SearchQuery` (line 137), `SEARCH_INCLUDES` (line 232),
  `handle_search` (near line 5082), `handle_capabilities` and
  `CapabilitiesResponse` (near line 5330), `encode_cursor` and
  `decode_cursor` (line 44)
- `src/search.rs`: `search` (near line 227), `fts5_phrases` (task 001)
- `src/api.rs`: `AmbiguousTrackGuidBody` and
  `ambiguous_track_guid_response` (near line 96)
- `src/db.rs`: `canonical_track_entity_id` (line 1092),
  `sync_source_read_models_for_feed` (the text of each index row, near line
  1471)
- `src/openapi.rs`: `/v1/search` (near line 520), `/v1/node/capabilities`,
  `query_param` (line 1356), the registered schemas
- `docs/API.md`: the sections `GET /v1/search` and
  `GET /v1/node/capabilities`
- `tests/fixtures/v1-schema-fields.json`
- `tests/client_requests_capabilities_tests.rs`, `tests/search_tests.rs`

## Files Likely To Change

- `src/search.rs`, `src/query.rs`, `src/api.rs`, `src/openapi.rs`
- `docs/API.md`, `tests/fixtures/v1-schema-fields.json`
- `tests/adr0080_text_filter_tests.rs`, new

## Constraints

- **The raw query.** `handle_search` reads `axum::extract::RawQuery` and
  decodes it with `url::form_urlencoded::parse`. No new dependency.
  - The known names are `q`, `type`, `limit`, `cursor`, `include`, and each
    name of the filter table.
  - A second value of `q`, `type`, `limit`, `cursor` or `include` gives `400`
    with no code, as a duplicate gives now.
  - A missing `q` is the same as an empty `q`.
- **The typed request.** Put a `SearchFilters` type in `src/search.rs`. It
  holds the parsed text filters and the exclusions, each as a list of FTS5
  phrases from `fts5_phrases`. `search` takes it in place of the separate
  arguments that only the handler uses. Keep the type filter, the limit and
  the cursor.
- **The filter table.** Add `pub const SEARCH_FILTERS` in `src/query.rs`, adjacent
  to `SEARCH_INCLUDES`. Each entry gives a filter name and the types that apply
  it. In this task the table has these names:

  | Name | `feed` | `track` | `publisher` |
  |---|---|---|---|
  | `artist`, `not_artist` | yes | yes | yes |
  | `title`, `not_title` | yes | yes | yes |
  | `release`, `not_release` | yes | yes | no |
  | `not_q` | yes | yes | yes |

  A request with no `type` accepts a name only when `feed` and `track` each
  apply it. Tasks 003 and 004 add names to this table.
- **The text match.** Each text filter is a condition in the outer `WHERE` of
  the search SQL. It is never part of the `MATCH` of `q`, so the rank does not
  change.
  - The condition is
    `m.rowid IN (SELECT rowid FROM search_index WHERE search_index MATCH ?n)`.
  - Its expression puts the column before each phrase and joins them with
    `AND`, for example `title : "night drive" AND title : "cut"`.
  - The column is `name` for `artist` on a feed and a track, and `title` for
    `title` and for `release` on a feed. On a publisher, `artist` and `title`
    each use `title`, the publisher title.
  - `release` on a track reads the index row of the feed of the track:
    `(SELECT fe.rowid FROM search_entities fe WHERE fe.entity_type = 'feed'
    AND fe.entity_id = json_extract(e.entity_id, '$[0]'))`, with the same
    `IN` condition on `title`.
  - An exclusion is the same condition with `NOT IN`. `not_q` has no column,
    so it matches `name`, `title` and `description`.
- **The SQL parameters.** Replace the four `match` arms of `search` with one
  list of `rusqlite::types::Value`, given with `params_from_iter`. Each value
  from the request is a bound parameter. No request text goes into the SQL
  text.
- **The sequence of the checks.** The handler checks in this sequence, and
  gives the first error:
  1. A name that the node does not know: `unsupported_filter`.
  2. A filter that the type of the request does not apply:
     `unsupported_filter`.
  3. A second value of a positive filter, a value with no word and no phrase,
     or the 11th exclusion: `invalid_filter`.
  4. No word or phrase in `q` and no positive filter: `missing_positive_term`.
     In this task, a request with no word or phrase in `q` also gives this
     code when it has a filter. Task 005 changes this.
- **The error body.** Add `SearchFilterErrorBody` in `src/api.rs`, adjacent to
  `AmbiguousTrackGuidBody`, with `error`, `code` and `filter`. `filter` is
  absent for `missing_positive_term`. Give it `#[derive(ToSchema)]` and
  register it. `handle_search` gives the body with status `400`, as
  `ambiguous_track_guid_response` does.
- **The capabilities.** `CapabilitiesResponse` gets `search_filters`, a map
  from `feed`, `track` and `publisher` to the names of the table. The handler
  and the capabilities read the same table.
- **The contract.** In `src/openapi.rs`:
  - Declare each name of the table on `/v1/search`. Each description names the
    types that apply it, and says when it can repeat.
  - Give the `400` response the schema of `SearchFilterErrorBody`.
  - Add `search_filters` to the example of `/v1/node/capabilities`.
  - Add `SearchFilterErrorBody` and the `search_filters` property to
    `tests/fixtures/v1-schema-fields.json`.
- **The documents.** `docs/API.md` gives the filters, the exclusions, the
  three codes and `search_filters`.
- **Comments.** Each new item names ADR 0080 and its section.

## Do Not Touch

- `fts5_query_from_words` and `fts5_phrases`, other than a call
- The order, the rank, the quality join and the cursor form of a search with
  `q`
- `kind`, the counts, the dates, the durations and `host`. Tasks 003 and 004
  add them.
- The index functions, the table, the repairs and the migrations
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the tests of the acceptance criteria that read only the error codes.
2. Add the raw query parse, the filter table and the error body.
3. Add `SearchFilters` and change `search` to one list of SQL values. Run the
   existing search tests. Each must pass with no change.
4. Add the text conditions and the exclusions.
5. Add `search_filters`, the contract and the documents.
6. Write the other tests of the acceptance criteria.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0080_text_filter_tests.rs`, unless
the item names a different file.

- ADR 0080 check 3, for the text filters: for each "yes" cell of the table of
  this task, one row matches and one row does not match.
- `artist=jsmo` does not match the artist `jSmovie`. `artist=jsmovie` matches
  it.
- `title="night drive"` matches `Night Drive`, and not `Drive Night`.
- Check 6: a feed and a track give the same `rank` with and without
  `artist`.
- Check 7: each `not_` parameter removes the rows that match its value. Two
  values of `not_title` remove the rows of each value. `not_q=night drive`
  removes a row with the two words, and keeps a row with only `night`.
- Check 8: 11 exclusions give `invalid_filter` with the name of the 11th. A
  request with only `not_title` gives `missing_positive_term` and no `filter`.
- Check 9, for this task: `type=publisher&release=x` gives
  `unsupported_filter` with `filter` set to `release`. `genre=x` gives
  `unsupported_filter` with `filter` set to `genre`.
- `artist=a&artist=b` gives `invalid_filter`. `artist=` and `artist=""` give
  `invalid_filter`.
- Check 15, for this task: each name of `SEARCH_FILTERS` is a parameter of
  `/v1/search` in `/openapi.json`, and is in `search_filters` of
  `GET /v1/node/capabilities` for each type that applies it.
- Each test of `tests/search_tests.rs` and the other search test files passes
  with no change.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0080_text_filter_tests
cargo test --test client_requests_capabilities_tests
cargo test --test adr0044_field_removal_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A filter changes the `rank` of a row.
- An existing test sends a parameter that is not in the known names.
- The FTS5 column filter does not accept a phrase with a column before it.
- A step needs a new dependency, a change to an ADR, or a change to a file of
  "Do Not Touch".

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
- `~/build/stophammer/docs/adr/0080-a-search-takes-phrases-and-field-filters.md`, §2, §3, §4 and §7
- `~/build/stophammer/docs/adr/0078-a-search-index-row-holds-only-searchable-text.md`, §1 and §5
- `~/build/stophammer/docs/tasks/adr-0080-task-002-text-filters-and-error-codes.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- `GET /v1/search` applies `artist`, `title`, `release` and the `not_` parameters. It gives the codes `unsupported_filter`, `invalid_filter` and `missing_positive_term`. The contract and the capabilities state its filters.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Put each filter in the outer `WHERE`, never in the `MATCH` of `q`.
- Bind each request value as an SQL parameter.
- The handler, the capabilities and the contract read one filter table.
- Stop at each escalation trigger of this task.

Do not touch:
- `fts5_query_from_words`, `fts5_phrases`, the order and the cursor of a search with `q`
- the value filters and `host`, the index functions, the repairs, the migrations
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0080_text_filter_tests`
- `cargo test --test client_requests_capabilities_tests`
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
