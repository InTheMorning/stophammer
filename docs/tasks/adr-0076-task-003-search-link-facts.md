# ADR 0076 Task 003: The Link Facts Of A Publisher Search Row

Owner: [ADR 0076](../adr/0076-a-named-feed-entry-states-its-confirmation.md)
section 3. v4vmm request 9, gap C.

Repository: `stophammer`. The operator commits.

## Goal

With `include=link_facts`, each `publisher` row of `GET /v1/search` gives the
link facts of the feed.

## Files To Inspect

- `src/query.rs`: `SearchQuery`, `handle_search`, `link_facts`,
  `load_publisher`, `feed_list_row_to_response`, `FEED_LIST_INCLUDES`,
  `handle_capabilities`
- `src/openapi.rs`: the path `/v1/search`
- `tests/adr0071_publisher_search_tests.rs`,
  `tests/adr0071_artist_list_link_facts_tests.rs`: the fixtures
- `tests/adr0071_row_link_guard_tests.rs`: the guard of ADR 0071 §1c
- `tests/fixtures/v1-schema-fields.json`

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`
- `tests/adr0076_search_link_facts_tests.rs`, new
- `tests/fixtures/v1-schema-fields.json`
- `docs/API.md`

## Constraints

- **Fields.** A `publisher` row gives `two_way_link_count`, `stated_rels`,
  `agreed_roles`, `confirmed_release_artists` and `first_named_count`.
- **Values.** Calculate each value with `link_facts` over `load_publisher`,
  as a list row does. Do not add a second rule.
- **Other rows.** A `feed` row and a `track` row give none of the fields.
- **Include.** Parse `include` on `SearchQuery`. Without `link_facts`, no row
  gives the fields.
- **Rule 1c.** Declare `include` with `link_facts` on `/v1/search` in the
  OpenAPI document. Add a `SEARCH_INCLUDES` constant, and give it as
  `include_params.search` in `handle_capabilities`.
- **Schema.** Add each new property to `tests/fixtures/v1-schema-fields.json`.
- A search without the include gives the results of today.
- No migration, no event change and no change to the search index.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- The search index, its ranking and the `type` rules of ADR 0071 §5
- The link fact rules of ADR 0068 §1 and §5
- Each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add `include` to `SearchQuery`, and the optional fields to the search row.
2. Fill the fields for each `publisher` row when the include is present.
3. Declare the include and the capability.
4. Write the tests, then update the schema fixture and `docs/API.md`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0076_search_link_facts_tests.rs`.

- With the include, a `publisher` row gives the five values of a list row of
  `GET /v1/feeds/by-name?medium=publisher&include=link_facts` for the same
  feed.
- With the include, a `feed` row and a `track` row give no link facts.
- Without the include, a `publisher` row gives no link facts.
- `GET /v1/node/capabilities` gives `include_params.search` with `link_facts`.
- The guard of ADR 0071 §1c and the ADR 0044 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0076_search_link_facts_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report, with no workaround, when:

- A search page of 20 `publisher` rows with the include takes more than 10
  times the time of a page without it. Measure on the newest backup in
  `backups/`.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. the page time with and without the include
5. deviations from this task
6. unresolved concerns
