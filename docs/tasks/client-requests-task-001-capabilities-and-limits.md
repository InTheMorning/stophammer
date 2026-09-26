# Client Requests Task 001: Capabilities And Limits

Plan: [client requests work plan](../plans/client-requests-work-plan.md),
items 1 and 2. Owner of item 2: ADR 0044. Item 1 corrects a defect.

Repository: `stophammer`.

## Goal

`/v1/node/capabilities` gives the includes that each route accepts. The
contract gives the maximum of each `limit`. `/v1/publishers` gives a correct
`has_more`.

## Files To Inspect

- `docs/plans/client-requests-work-plan.md`, items 1 and 2
- `src/query.rs`: `handle_capabilities`, the include parsing of the feed and
  track routes, `capped_limit`, `handle_search`, `handle_publisher_search`
- `src/openapi.rs`: `spec_value()`, each `limit` parameter, the capabilities
  example
- `docs/API.md`: the capabilities route, the list routes

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`, `docs/API.md`
- `tests/client_requests_capabilities_tests.rs`, new

## Do Not Touch

- The behavior for an unknown include name, and for a `limit` above the
  maximum. Neither answers `400`.
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **One list for each entity.** Declare `FEED_INCLUDES` and `TRACK_INCLUDES`
  in `src/query.rs`. The include parsing of each route and
  `handle_capabilities` read the same list. The parsing can use a `match`.
  Then a test must show that each name in the list gives its key in the
  response, so that the two stay equal.
- Correct the capabilities example in `src/openapi.rs`.
- `docs/API.md` states that a route ignores an include name that it does not
  know.
- **The maximums.** Each `limit` parameter in `spec_value()` gets
  `"minimum": 1` and a `"maximum"` equal to the value in the code for that
  route: 200 for the routes that use `capped_limit`, and 100 for
  `/v1/search`, `/v1/publishers`, `/v1/copies` and `/v1/guid-changes`. Use a
  constant for each value, and read the same constant in the code and in
  `spec_value()`.
- **`/v1/publishers`.** Read `limit + 1` rows. Return `limit` rows, and set
  `has_more` to true when the extra row exists. No cursor.

## Implementation Steps

1. Add the include lists, and use them in the parsing and the capabilities
   route.
2. Add the limit constants, and use them in the code and the document.
3. Correct `/v1/publishers`.
4. Update `docs/API.md`.
5. Add the tests below.
6. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/client_requests_capabilities_tests.rs`:

- The test reads `/v1/node/capabilities`. For each track include that it
  lists, `GET /v1/tracks/{guid}?include=<name>` gives the key of that include.
  The same for each feed include on `GET /v1/feeds/{guid}`. The failure
  message names v4vmm request 3 and `TRACK_INCLUDES` or `FEED_INCLUDES`.
- The track list includes `remote_items` and `publisher`.
- The test reads the generated document from `openapi::spec_value()` or its
  equivalent. Each `limit` parameter has a `maximum` equal to the constant of
  its route. The failure message names musicindex request 4.
- With 3 publishers in the database, `/v1/publishers?limit=2` gives 2 rows
  and `has_more: true`. With `limit=3` it gives `has_more: false`.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test client_requests_capabilities_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- A route accepts an include name that is in neither list, or a list names an
  include that a route does not accept. List each one before you change it.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/client-requests-task-001-capabilities-and-limits.md`, all of it
- `docs/plans/client-requests-work-plan.md`, items 1 and 2
- The files in "Files To Inspect"

Goal:
- The capabilities route gives the accepted includes, each `limit` has a
  stated maximum, and `/v1/publishers` gives a correct `has_more`.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- The behavior for an unknown include or a large `limit`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria".
- The gate is green.

Test commands:
- The commands of "Test Commands".

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
