# Phase Plan: Search Phrases And Field Filters

This plan is advisory. It gives the sequence of the tasks of
[ADR 0080](../adr/0080-a-search-takes-phrases-and-field-filters.md), which owns
each rule. The ADR answers items 1 to 7 of
[v4vmm request 10](v4vmm-open-requests.md#10-search-accepts-words-phrases-and-field-filters).

## Goal

- A phrase in double quotes in `q` matches its words in sequence.
- `GET /v1/search` applies each filter and each exclusion of ADR 0080 §2 and
  §3, and gives the error codes of §4.
- A search with filters and no `q` gives its rows newest first.
- The contract and `GET /v1/node/capabilities` state each filter.

## Non-Goals

- v4vmm request 11: the date and the named feeds of a feed hit.
- A stated release kind. Ingest continues to set `unknown` for each feed.
- A prefix match, an OR between two fields, and a parse of field syntax in `q`.
- The other search changes of the research request of 2026-10-23.

## Assumptions

- ADR 0080 is Accepted. A task does not start on a Proposed ADR.
- The next free migration number is 0057. Task 004 checks the number when it
  starts.
- The `url` crate re-exports `form_urlencoded`, so the handler can read a
  repeated parameter with no new dependency.
- The `time` crate builds a `Date` with `Date::from_calendar_date`, so the date
  parse needs no new feature.
- The backup of 2026-10-24 has 10,583 feeds and 27,613 tracks. A scan of each
  index row with its feed has a small cost.

## Affected Modules

All tasks are in the `stophammer` repository.

| Module | Tasks |
|---|---|
| `src/search.rs` (`fts5_query_from_words`, `fts5_phrases`) | 001 |
| `src/search.rs` (`search`, the filter types and the filter table) | 002, 003, 004, 005 |
| `src/query.rs` (`SearchQuery`, `handle_search`, `handle_capabilities`) | 002, 003, 004, 005 |
| `src/api.rs` (the error body of §4) | 002 |
| `migrations/`, `src/schema.sql`, `src/db.rs` (`upsert_feed`, `ingest_transaction`, the `open_db` repairs) | 004 |
| `src/api.rs` (`relocate_feed`) | 004 |
| `src/openapi.rs`, `docs/API.md`, `tests/fixtures/v1-schema-fields.json` | 001 to 005 |

`api.html` reads `/openapi.json` when it opens, so no task changes it.

## Sequence

1. **Task 001, phrases.** It changes `fts5_query_from_words`, and adds
   `fts5_phrases`.
2. **Task 002, the request model and the text filters.** It starts after task
   001, because a text filter uses `fts5_phrases`. It adds the parse of the raw
   query, the filter table, the error body, the text filters and the
   exclusions.
3. **Task 003, the value filters.** `kind`, the counts, the dates and the
   durations. It starts after task 002.
4. **Task 004, the host.** The column, the repair and the `host` filter. It
   starts after task 002. It can run at the same time as task 003 only if the
   two tasks do not edit the filter table at the same time. Run them in
   sequence when one agent does both.
5. **Task 005, a search with no `q`.** It starts after task 003 and task 004,
   because it reads each filter of the table.

Task 001 and task 002 both change `src/search.rs`. Thus they run in sequence.

## Interim Behavior

Each task gives a correct node on its own commit:

- After task 002, `host`, `kind`, the counts, the dates and the durations are
  not in the filter table. Each gives `unsupported_filter`, as §4 states for a
  parameter that the node does not know.
- Before task 005, a request with no word and no phrase in `q` gives
  `missing_positive_term`, also with filters.

## Schema And API Implications

- Task 004 adds `feeds.feed_host` with migration 0057 and an `ensure_` repair
  (ADR 0046). No event payload changes. The primary and the community nodes
  can upgrade in each sequence.
- `GET /v1/search` gets the parameters of §2 and §3, and a `400` body with
  `code` and `filter`. `q` stops being a required parameter in task 005.
- `GET /v1/node/capabilities` gets `search_filters` in task 002.
- A parameter that the node does not know gives `400` from task 002.
- The release is MINOR (ADR 0066).

## Risk Areas

- **The rank.** A filter must not change `rank`. Put each filter in the outer
  `WHERE`, not in the `MATCH` of `q` (ADR 0080 §2 and the alternatives).
- **The feed of a track.** The entity ID of a track is the JSON pair
  `["<feed_guid>","<track_guid>"]` (`canonical_track_entity_id`,
  `src/db.rs:1092`). The SQL reads the feed GUID with `json_extract`.
- **The `release` filter on a track.** The index row of a track does not hold
  the feed title. The filter reads the `title` of the index row of the feed.
- **Parameter numbers.** `search` numbers its SQL parameters by hand, with one
  `match` arm for each case. Task 002 replaces this with one list of values,
  because the filters make too many cases.
- **Unknown parameters.** A test or a client that sends an extra parameter now
  gets `400`. Task 002 searches the tests for such a request.
- **The cursor forms.** Task 005 adds a second cursor form. A cursor of one
  form must give `400` on the other form, not a wrong page.

## Test Strategy

- Each check of ADR 0080 is a test in `tests/`. Each task names its checks.
- New test files: `tests/adr0080_phrase_tests.rs`,
  `tests/adr0080_text_filter_tests.rs`, `tests/adr0080_value_filter_tests.rs`,
  `tests/adr0080_host_filter_tests.rs`, `tests/adr0080_no_q_search_tests.rs`.
- `tests/migration_tests.rs` covers the migration of task 004, and
  `tests/fixtures/migration_sha256.txt` gets its line (ADR 0023).
- The guards of ADR 0044 cover the new schema and properties.
- The gate of `AGENTS.md` runs after each task.

## Rollback Strategy

- Each task is one commit, and a revert of that commit undoes it.
- The column of task 004 stays after a revert. No other code reads it.

## After The Tasks

The orchestrator reviews each diff against
[the review checklist](../reviews/search-filters-review-checklist.md). A
release record gives the release. After the deploy, the orchestrator deletes
this plan and the task packets, and updates both copies of the v4vmm requests.
