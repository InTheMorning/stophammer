# ADR 0044 Task 002: Each Response Points At Its Schema

Owner: [ADR 0044](../adr/0044-api-contract-declares-its-fields.md). Plan:
[ADR 0044 phase plan](../plans/adr-0044-contract-schema-phase-plan.md),
sequence step 2. Review:
[ADR 0044 review checklist](../reviews/adr-0044-review-checklist.md).

Repository: `stophammer`.

## Goal

Each documented JSON response has a schema that names its fields. The inline
`example` of each response does not change.

## Current State

- `components.schemas` holds the schemas of the response types. Task 001
  added them, and each later ADR added its types.
- `json_response` in `src/openapi.rs` gives each response the schema
  `{ "type": "object" }`. `error_response` does the same.
- The read routes send the envelope `QueryResponse<T>`: `data`, `pagination`
  and `meta`. The `ToSchema` derive removes the type parameter, so no schema
  names the envelope of one route.

## Files To Inspect

- `src/openapi.rs`: `json_response`, `error_response`,
  `query_envelope_example`, `schemas_object`, each call of `json_response`
- `src/query.rs`: `QueryResponse`, `response_schemas`, the type that each
  read route puts in `data`
- `src/api.rs`, `src/ingest.rs`, `src/sync.rs`: `response_schemas`, and the
  type that each route sends
- `docs/reviews/adr-0044-review-checklist.md`

## Files Likely To Change

- `src/openapi.rs`
- `src/query.rs`, `src/api.rs`, `src/ingest.rs`, `src/sync.rs`: only to
  register a response type that no schema holds yet
- `tests/adr0044_schema_refs_tests.rs`, new

## Do Not Touch

- A handler body, an SQL statement, a migration, a field name, or a
  `serde` attribute
- Each inline `example`
- `api.html`

## Constraints

- **The schema of a response.** Give `json_response` a schema argument. Each
  call passes the schema of the type that its handler sends:
  - A body that is one registered type: `{ "$ref": "#/components/schemas/Name" }`.
  - A read route with the envelope: an inline object with three properties.
    `data` is a `$ref` to the item type, or an array of that `$ref`.
    `pagination` is a `$ref` to `Pagination`. `meta` is a `$ref` to
    `ResponseMeta`.

    Write one helper for this object, for example
    `envelope_schema(data_schema)`. This helper replaces the utoipa alias
    that `AGENTS.md` names. It gives the same result with no alias for each
    instantiation.
- **Errors.** `error_response` points at the `ErrorBody` schema.
- **A type with no schema.** When a handler sends a type that
  `components.schemas` does not hold, derive `ToSchema` on it and register
  it. Do not write a schema by hand. When a handler sends a
  `serde_json::Value` or a `json!` literal, report the route and keep
  `{ "type": "object" }` for it.
- Read the handler to find each type. Do not guess a type from the example.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0044_schema_refs_tests.rs`. Each failure
message names ADR 0044 and the route:

- Each `$ref` in the document names a schema in `components.schemas`.
- Each JSON response of the document has a `$ref` or an envelope object,
  except the routes that the final report lists as `serde_json::Value`
  bodies. The test holds that list as a constant.
- For 5 routes, the test reads the response of the router in a test node.
  Each key of `data` is a property of the schema that the document gives. Use `GET /v1/feeds/{guid}`, `GET /v1/search`, `GET /v1/copies`,
  `GET /node/info` and `GET /v1/node/capabilities`.
- Each inline `example` is equal to the example of the commit before the
  change. The test compares against a copy of the document that the task
  saves in `tests/fixtures/openapi-before-adr0044-task-002.json`.
- The gate is green: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`.

Visual, and open until the operator checks it:

- `/api` on the node, and `api.html` on the site after
  `getapi.sh && pushsite.sh`, show each endpoint with its example.

## Expected Final Report

1. files changed
2. the real output of each gate command
3. the list of routes that keep `{ "type": "object" }`, with the reason
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0044-task-002-responses-point-at-schemas.md`, all of it
- `docs/plans/adr-0044-contract-schema-phase-plan.md`
- `docs/reviews/adr-0044-review-checklist.md`
- The files in "Files To Inspect"

Constraints:
- Follow "Constraints" of the task file exactly.
- Save the fixture of the document before the first change to
  `src/openapi.rs`.
- Do not run any git command that writes. Do not commit.
- Write each test that the criteria name, with real assertions. Paste the
  real output of each gate command.

At the end, report the five items of "Expected Final Report".
