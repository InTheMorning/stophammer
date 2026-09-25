# ADR 0053 Task 002: Ingest Check And Admin Routes

Owner: [ADR 0053](../adr/0053-a-correction-stays-applied.md) section 1.
Plan: [phase plan](../plans/adr-0053-durable-corrections-phase-plan.md),
decisions 5 and 6.

## Goal

Ingest rejects a blocked GUID or URL. An operator creates, lists and deletes
blocks through three admin routes, and each change is a signed event.

## Files To Inspect

- `src/db.rs`: `FeedBlockKind`, `FeedBlock`, the block functions (task 001),
  `insert_event`
- `src/api.rs`: `handle_ingest_feed` from its start to the reader phase,
  `sign_event_row`, `feed_url_observed_event_row`, `signed_row_to_event`,
  `check_admin_token`, `handle_remove_track` (as a pattern for a write route
  with fan-out), `build_router`
- `src/openapi.rs`: `spec_value` and one admin route entry
- `docs/API.md`: the admin section
- `tests/adr0051_source_url_tests.rs`: the app state and request helpers

## Files Likely To Change

- `src/api.rs`
- `src/openapi.rs`
- `docs/API.md`
- `tests/adr0053_blocks_api_tests.rs`, new

## Do Not Touch

- `src/db.rs` except a small helper that the handlers need. Report each one.
- `handle_retire_feed` (task 003), `src/verify.rs`, `src/verifiers/`,
  `src/main.rs`, `src/query.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **The ingest check.** In `handle_ingest_feed`, after `authenticate` and
  before the verifier chain, on the reader connection:
  `db::find_feed_block(&reader, guid, &[&req.source_url, &req.canonical_url])`.
  `guid` is the GUID of `feed_data`, when present.

  A match answers `accepted: false`, `no_change: false`,
  `reason: Some("blocked")`, no events, no warnings, `source_url: None`.
  Log it with `tracing::info!` and the fields `feed_guid`, `canonical_url`, `block_id` and `kind`. A short comment
  names ADR 0053 section 1.
- **`POST /v1/blocks`.** Body `{ "kind": "guid" | "url", "value": String,
  "reason": String }`.
  - An empty value or reason after trim answers `400`.
  - On the writer lock, in one transaction, call `get_feed_block_by_pair`.
  - When a row exists, answer `409` with `{ "block_id": ... }`.
  - Otherwise insert the row with a new UUID v4 `block_id` and `blocked_at`
    now. Sign one `FeedBlocked` event with `db::insert_event`.
  - Answer `201` with the row. Fan out the event as `handle_remove_track`
    does.
- **`GET /v1/blocks`.** Answers `200` with `{ "blocks": [FeedBlock] }`, in
  `blocked_at` order.
- **`DELETE /v1/blocks/{block_id}`.** In one transaction: delete the row and
  sign one `FeedUnblocked` event. A missing row answers `404` and signs
  nothing. Answer `204`. Fan out the event.
- Each of the three routes needs `X-Admin-Token` through `check_admin_token`.
  They are in `build_router` only, not in `build_readonly_router`.
- `src/openapi.rs` declares the three routes, the request body and the
  `FeedBlock` schema. `cargo run --bin gen_openapi` shows them.
- `docs/API.md` gives the three routes and the `blocked` ingest reason.

## Implementation Steps

1. Add the ingest check.
2. Add the three handlers and register them.
3. Add the OpenAPI entries and the `docs/API.md` text.
4. Add the tests below.
5. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0053_blocks_api_tests.rs`, through the router:

- A block on a GUID in upper case, then an ingest of that GUID in lower case
  at a new URL: `reason: "blocked"`, and no row in `feeds`.
- A block on a URL, then an ingest where only `source_url` is that URL:
  `blocked`.
- A blocked ingest with a wrong token answers the token error, not `blocked`.
  The token check stays first.
- `POST /v1/blocks` twice with the same pair: `201`, then `409` with the same
  `block_id`. One `feed_blocked` row in `events`.
- `POST` without the admin token: `403`. `POST` with an empty reason: `400`.
- `GET /v1/blocks` lists the row. `DELETE` answers `204`, and a second
  `DELETE` answers `404`. One `feed_unblocked` row in `events`.
- After `DELETE`, the same ingest is accepted.
- The gate is green, and `gen_openapi` shows `/v1/blocks`.

## Test Commands

```bash
cargo build
cargo test --test adr0053_blocks_api_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi | grep -c "/v1/blocks"
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- The ingest handler reads the database before `authenticate`.
- An existing test fails.
- The fan-out pattern of the write routes needs a change to a shared helper.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0053-task-002-ingest-check-and-admin-routes.md`, all of it
- `docs/plans/adr-0053-durable-corrections-phase-plan.md`, decisions 5 and 6
- The files in "Files To Inspect"

Goal:
- Ingest answers `blocked` for a blocked GUID or URL, after the token check
  and before the verifier chain. Add `POST`, `GET` and
  `DELETE /v1/blocks` with the admin token, each write in one transaction
  with its signed event.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `handle_retire_feed`, `src/verify.rs`, `src/verifiers/`, `src/main.rs`,
  `src/query.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0053_blocks_api_tests.rs`.
- The gate is green. `gen_openapi` shows `/v1/blocks`.

Test commands:
- `cargo build`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
