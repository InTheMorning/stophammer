# ADR 0044 Task 003: The Contract Guards

Owner: [ADR 0044](../adr/0044-api-contract-declares-its-fields.md). Plan:
[ADR 0044 phase plan](../plans/adr-0044-contract-schema-phase-plan.md),
sequence step 3.

Repository: `stophammer`.

## Goal

A test fails when a route is not in the document, or when a documented JSON
response has a schema with no field names. `AGENTS.md` describes the
procedure as it is after task 002.

## Files To Inspect

- `src/api.rs`: `build_router`, and each router that it merges
- `src/query.rs`: `query_routes`
- `src/openapi.rs`
- `tests/adr0044_schema_refs_tests.rs`, from task 002
- `AGENTS.md`: "New API Endpoint"

## Files Likely To Change

- `tests/adr0044_contract_guard_tests.rs`, new
- `AGENTS.md`
- `docs/adr/0044-api-contract-declares-its-fields.md`: one line in its
  guards section that names the test file

## Constraints

- **Each route is in the document.** Take the list of routes from the
  router, not from a second list in the test. When axum gives no way to list
  the routes of a `Router`, take them from the source: the test reads
  `src/api.rs` and `src/query.rs` and collects each `.route("...")` path.

  Then each path, with each method, is a path of the document. A route that
  is not public, for example `/ingest/feed`, is also in the document today.
  Report each exception, and put it in a constant with its reason.
- **No bare object.** No JSON response has the schema `{ "type": "object" }`
  with no `properties`, except the list of task 002.
- Each failure message names ADR 0044 and gives the fix, in this form:

  ```text
  ADR 0044: GET /v1/example is in the router but not in the OpenAPI document.
  Add the path to spec_value() in src/openapi.rs, with the $ref of its type.
  ```

- **`AGENTS.md`.** Correct step 3 of "New API Endpoint". After task 002, a
  new route adds its path to `spec_value()`, and its response points at the
  schema of its type. The type derives `ToSchema`, and `response_schemas`
  registers it. The guard of this task fails when a step is missing.

  Remove the sentence of "Where The Work Stands" that describes the
  `QueryResponse<T>` alias. Write in ASD-STE100 Simplified Technical English,
  and run
  `python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --check --no-heuristics`
  on each changed document.

## Acceptance Criteria

Mechanical:

- The two guards pass on the tree.
- A manual check shows that each guard fails: remove one path from
  `spec_value()`, and set one response to `json_response(..)` with a bare
  object. Report the two failure messages, then restore the code.
- The gate is green: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`.

## Expected Final Report

1. files changed
2. the real output of each gate command, and the two failure messages
3. the list of exceptions, with the reason of each
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0044-task-003-contract-guards.md`, all of it
- `docs/adr/0044-api-contract-declares-its-fields.md`
- The files in "Files To Inspect"

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.
- Write each test with real assertions. Paste the real output of each gate
  command.

At the end, report the five items of "Expected Final Report".
