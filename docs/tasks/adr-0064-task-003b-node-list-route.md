# ADR 0064 Task 003b: The List Of Live Items

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) section 6. Plan:
[ADR 0064 phase plan](../plans/adr-0064-live-items-phase-plan.md).

Repository: `stophammer`. The operator commits.

## Goal

`GET /v1/live-items` gives the rows of all public feeds, with `view` (`now` by
default, `upcoming`, `all`), the raw filters for `all`, and paging in the order
`feed_guid`, then `live_item_guid`.

## Files To Inspect

- `src/live.rs` as task 003 left it: `in_now_view`, `in_upcoming_view`,
  `is_confirming_relay`, the host list
- `src/query.rs`: `LiveItemResponse`, `Pagination`, `encode_cursor`,
  `decode_cursor`, `handle_list_copy_records` (a paged list to copy), the
  envelope type, `query_routes()`, `response_schemas()`
- `src/openapi.rs`: `spec_value()`, `envelope_schema`
- `src/db.rs`: `get_live_events_for_feed`, and how a read excludes a feed that
  is not public (search the feed read for the check that it uses)
- `tests/adr0044_contract_guard_tests.rs`
- `docs/plans/adr-0064-live-item-cases.md`

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`, `src/db.rs` (one list query)
- `docs/API.md`, `api.html`
- `tests/adr0064_live_list_tests.rs`, new

## Do Not Touch

- `src/live.rs` other than a small helper that the route needs.
- The ingest in `src/api.rs`, `AppState`.
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`.

## Constraints

- One new database function reads the rows of all feeds after a cursor, in
  the order `feed_guid, live_item_guid`. It joins the feed, so a feed that is
  not public gives no row. Add an index only if the query plan needs
  one, and report it.
- The handler takes `view`, `status`, `live_value`, `ends_after`,
  `starts_after`, `cursor` and `limit`. The raw filters apply only when `view`
  is `all`. With another `view`, a raw filter gives `400`.
- `now` and `upcoming` call the view functions of `src/live.rs` with the
  current time and `is_confirming_relay`. The handler reads the clock one
  time, and passes it in.
- An unknown `view` or filter value gives `400`.
- The cursor encodes `feed_guid` and `live_item_guid`, as the other list
  routes encode their keys.
- The response uses the envelope of the other read routes, with `data` of
  `LiveItemResponse` rows that also give `feed_guid`, and `pagination`.
- Register the route in `query_routes()` and the path in `spec_value()` with
  `envelope_schema`.
- `docs/API.md` gives the route, the three views, what `confirming_relay`
  means, and how a client asks the relay. `api.html` shows the route.

## Steps

1. Write the database function.
2. Write the handler, its query type and the validation.
3. Register the route and the OpenAPI path.
4. Update `docs/API.md` and `api.html`.
5. Write the integration tests.

## Acceptance

Mechanical, each an integration test with a fixed time where the route needs
one:

- With no `view`, the route gives the `now` rows only.
- Each row of the case table is in the view that the table gives, and not in
  the others.
- `view=all` with each raw filter selects the expected rows.
- A raw filter with `view=now` gives `400`. An unknown `view` gives `400`.
- Two pages with `limit=1` give each row one time, in the order `feed_guid`,
  `live_item_guid`.
- A deleted feed gives no row.
- The guards of ADR 0044 pass.
- The gate is green.

Visual, for the operator:

- `api.html` shows the route and the three views.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --test adr0064_live_list_tests
cargo test --test adr0044_schema_refs_tests
cargo test --test adr0044_contract_guard_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Escalation Triggers

Stop and report without a workaround when:

- The route cannot use a fixed time in a test without a change to the handler
  signature. Report the options.
- The check for a public feed is not one query condition, and needs a call for
  each row.
- The envelope of the other read routes cannot carry `pagination`.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0064-task-003b-node-list-route.md
- /home/citizen/build/stophammer/docs/plans/adr-0064-live-item-cases.md
- /home/citizen/build/stophammer/AGENTS.md (Code Style, New API Endpoint)
- /home/citizen/build/stophammer/src/live.rs
- Only the parts of src/query.rs, src/openapi.rs and src/db.rs named in the task file. Use grep.

Goal:
- Add `GET /v1/live-items` with the views `now` (default), `upcoming` and `all`.
- Apply the raw filters for `all`.
- Page in the order `feed_guid`, `live_item_guid`.

Constraints:
- The views call the functions of `src/live.rs`. Read the clock one time in the handler.
- Raw filters only with `view=all`. Unknown values give 400.
- One list query, joined so a feed that is not public gives no row.
- Follow the envelope, cursor and OpenAPI patterns of the other list routes.

Do not touch:
- The ingest in `src/api.rs`, `AppState`, `docs/adr/`, `stophammer-crawler/`, `stophammer-parser/`.
- Git: run no git command that writes (no add, commit, stash, checkout, reset).

Acceptance criteria:
- The criteria under "Acceptance" in the task file.

Test commands:
- cargo build
- cargo test --test adr0064_live_list_tests
- cargo test --test adr0044_schema_refs_tests
- cargo test --test adr0044_contract_guard_tests
- cargo test
- cargo clippy --all-targets -- -D warnings
- cargo fmt -- --check
- cargo run --bin gen_openapi > /dev/null

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
