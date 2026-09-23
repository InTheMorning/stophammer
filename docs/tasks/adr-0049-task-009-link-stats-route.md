# ADR 0049 Task 009: The Publisher Link Statistics Route

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §8.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md),
decision 9.
Needs: task 005.

## Goal

`GET /v1/publisher-links/stats` gives the number of listed publisher links for
each resolution. The `refresh` pass of task 011 reads it.

## Files To Inspect

- `src/query.rs`: `query_routes` (near line 2247), `QueryResponse`,
  `handle_get_recent_feeds` as a handler pattern, `response_schemas`
- `src/db.rs`: `resolve_listed_feed`
- `src/openapi.rs`: `spec_value`, and how a path is added
- `docs/API.md`: the section for the publisher routes
- `AGENTS.md`: "New API Endpoint"

## Files Likely To Change

- `src/db.rs`: one function
- `src/query.rs`: the handler, the response type, the route
- `src/openapi.rs`: the path and the example
- `docs/API.md`
- `tests/adr0049_link_stats_tests.rs`, new

## Do Not Touch

- the resolver
- the other routes
- `api.html`. It reads `/openapi.json` when it runs

## Constraints

- A listed link is a row of `feed_remote_items_raw` with `medium = "music"`
  whose feed has `raw_medium = "publisher"`.
- Resolve each link with `db::resolve_listed_feed`. Count each resolution.
- The response is `QueryResponse` with this `data`:

```json
{
  "listed_links": 0,
  "resolved_by_guid": 0,
  "resolved_by_feed_url": 0,
  "unresolved": 0
}
```

- `listed_links` is the sum of the other three.
- The route is public and needs no credential, as the other `/v1` reads do.
- The response type derives `ToSchema` and is in `response_schemas`.
- The handler reads on the reader pool in `spawn_blocking`.

## Implementation Steps

1. Add the `src/db.rs` function that gives the four counts.
2. Add the response type, the handler and the route.
3. Add the path to `spec_value` with an example.
4. Add the route to `docs/API.md`.
5. Add the tests below.
6. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- An empty database gives four zeros.
- A test with the task 002 fixtures gives counts that agree with the rows it
  ingests: one `guid` link, one `feed_url` link and one `unresolved` link.
- A test proves that a `medium = "music"` item of a music feed is not counted.
- A test proves that the route needs no credential.
- `cargo run --bin gen_openapi` holds the path `/v1/publisher-links/stats`.

## Test Commands

```bash
cargo build
cargo test --test adr0049_link_stats_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi | grep -c '/v1/publisher-links/stats'
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns
6. the time of one request against a copy of the local database, from a release
   build

## Escalation Triggers

Stop and report when:

- the route path conflicts with a route in `musicindex/relay-api.json`
- one request takes more than 2 seconds against a copy of the local database

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0049-task-009-link-stats-route.md`, the section
  "Constraints"
- `src/query.rs`: `query_routes`, `QueryResponse`, `handle_get_recent_feeds`,
  `response_schemas`
- `src/db.rs` `resolve_listed_feed`
- `src/openapi.rs` `spec_value`
- `AGENTS.md`, the section "New API Endpoint"

Goal:
- Add the public route `GET /v1/publisher-links/stats` that gives the counts of
  listed publisher links by resolution.

Constraints:
- A listed link is a `feed_remote_items_raw` row with `medium = "music"` on a
  feed with `raw_medium = "publisher"`.
- Resolve each one with `db::resolve_listed_feed`.
- `data` has `listed_links`, `resolved_by_guid`, `resolved_by_feed_url`,
  `unresolved`. `listed_links` is the sum of the other three.
- Public, no credential. Reader pool in `spawn_blocking`. The response type
  derives `ToSchema` and is in `response_schemas`.
- Add the path to `spec_value` and the route to `docs/API.md`.

Do not touch:
- the resolver, the other routes, `api.html`

Acceptance criteria:
- The gate is green.
- Tests prove:
  - An empty database gives zeros.
  - The fixtures give one `guid`, one `feed_url` and one `unresolved` link.
  - A music feed item is not counted.
  - No credential is needed.
- `gen_openapi` output holds `/v1/publisher-links/stats`.

Test commands:
- `cargo build`
- `cargo test --test adr0049_link_stats_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cargo run --bin gen_openapi | grep -c '/v1/publisher-links/stats'`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
