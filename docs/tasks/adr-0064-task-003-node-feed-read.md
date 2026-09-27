# ADR 0064 Task 003: The Relay Setting And The Feed Read

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) sections 3 and 6.
Plan: [ADR 0064 phase plan](../plans/adr-0064-live-items-phase-plan.md).

Repository: `stophammer`. The operator commits.

## Goal

The node reads `CONFIRMING_RELAY_HOSTS` at start. `src/live.rs` decides
`confirming_relay` and the two views as pure functions. `GET /v1/feeds/{guid}`
gives `live_items`, with each row of the feed.

## Files To Inspect

- `src/live.rs` as task 002b left it
- `src/main.rs`: the start, near `blocks::blocks_from_env`
- `src/blocks.rs`: `read_csv_env`
- `src/query.rs`: `FeedResponse`, `handle_get_feed`, `response_schemas()`
- `src/model.rs`: `web_url_or_none`
- `src/openapi.rs`: the entry of `/v1/feeds/{guid}`
- `tests/adr0044_schema_refs_tests.rs`, `tests/adr0044_contract_guard_tests.rs`
- `docs/plans/adr-0064-live-item-cases.md`: "The Client Views"

## Files Likely To Change

- `src/live.rs`, `src/main.rs`, `src/query.rs`, `src/openapi.rs`
- `docs/API.md`, `docs/operations.md`
- `tests/adr0064_live_feed_read_tests.rs`, new

## Do Not Touch

- `AppState` and its constructors. The setting does not go into `AppState`.
- The ingest in `src/api.rs`.
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`.

## Constraints

- `src/live.rs` holds the host list in a `std::sync::OnceLock<Vec<String>>`.
  `set_confirming_relay_hosts(hosts)` sets it one time. `main.rs` calls it at
  start with the comma-separated value of `CONFIRMING_RELAY_HOSTS`. When it is
  not set, the list is empty.
- `is_confirming_relay(uri, hosts) -> bool` is pure: true only when `uri`
  parses as an `https` URL (use the `url` crate) and its host is equal to a
  listed host, with no case difference. An identifier-only `uri` gives false.
- `in_now_view(row, now, confirming) -> bool` and `in_upcoming_view(row, now)
  -> bool` are pure and follow ADR 0064 §6 with `LIVE_END_MARGIN_SECS = 3600`
  and `PENDING_START_MARGIN_SECS = 3600`:
  - `now`: status `live`, and a confirming relay, or no `scheduled_end`, or
    `now < scheduled_end + 3600`.
  - `upcoming`: status `pending`, and `now < scheduled_end`. With no
    `scheduled_end`: `now < scheduled_start + 3600`. With no time: false.
- A new response type `LiveItemResponse` with `#[derive(Serialize,
  ToSchema)]` gives the fields of ADR 0064 §6. `content_link` and a
  `live_value_uri` with a scheme go through `web_url_or_none`. An
  identifier-only `uri` is given as it is.
- `FeedResponse` gains `live_items: Vec<LiveItemResponse>`, always present,
  in the order `live_item_guid`.
- Register `LiveItemResponse` in `response_schemas()`, and update the
  document for `/v1/feeds/{guid}`.
- `docs/operations.md` gives `CONFIRMING_RELAY_HOSTS`. `docs/API.md` gives the
  new field.

## Steps

1. Add the setting, `is_confirming_relay` and the two view functions to
   `src/live.rs`, with unit tests. The view functions are for task 003b and
   need tests now.
2. Call `set_confirming_relay_hosts` in `main.rs`.
3. Add `LiveItemResponse` and `live_items` to the feed read.
4. Update the OpenAPI document and the two reference documents.
5. Write the integration tests.

## Acceptance

Mechanical:

- Unit tests of `is_confirming_relay`:
  - A listed `https` host gives true.
  - An unlisted host, an `http` URL, and `event-one` give false.
  - The host compare has no case difference.
- Unit tests of the two view functions cover each row of "The Client Views"
  with a fixed `now`, including the boundary at exactly 1 hour.
- An integration test: the feed read gives `live_items` with a `pending`, a
  `live` and an `ended` row, with the relay link, and `confirming_relay:
  false` when the list is empty.
- A `javascript:` `content_link` gives `null` in the read.
- The guards of ADR 0044 pass. `cargo run --bin gen_openapi` gives the field.
- The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --lib live
cargo test --test adr0064_live_feed_read_tests
cargo test --test adr0044_schema_refs_tests
cargo test --test adr0044_contract_guard_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Escalation Triggers

Stop and report without a workaround when:

- The feed read builds its response in a way that makes `live_items` need a
  second database connection or a change to `AppState`.
- A guard of ADR 0044 needs a change to the guard itself.
- A test of the feed read outside this task fails because of the new field.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0064-task-003-node-feed-read.md
- /home/citizen/build/stophammer/docs/plans/adr-0064-live-item-cases.md ("The Client Views")
- /home/citizen/build/stophammer/AGENTS.md (Code Style, New API Endpoint steps 3 to 7)
- /home/citizen/build/stophammer/src/live.rs
- Only the parts of src/query.rs, src/main.rs and src/openapi.rs named in the task file. Use grep.

Goal:
- Add the `CONFIRMING_RELAY_HOSTS` setting, `is_confirming_relay`, and the pure `now` and `upcoming` view functions to `src/live.rs`, and give `live_items` in `GET /v1/feeds/{guid}`.

Constraints:
- The host list lives in a `OnceLock` in `src/live.rs`, not in `AppState`.
- The function names, constants and rules given in the task file.
- Links go through `web_url_or_none`, except an identifier-only `uri`.

Do not touch:
- `AppState` and its constructors, the ingest in `src/api.rs`, `docs/adr/`, `stophammer-crawler/`, `stophammer-parser/`.
- Git: run no git command that writes (no add, commit, stash, checkout, reset).

Acceptance criteria:
- The criteria under "Acceptance" in the task file.

Test commands:
- cargo build
- cargo test --lib live
- cargo test --test adr0064_live_feed_read_tests
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
