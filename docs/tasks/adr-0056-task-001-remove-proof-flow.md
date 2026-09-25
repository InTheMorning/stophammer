# ADR 0056 Task 001: Remove The Proof Flow

Owner: [ADR 0056](../adr/0056-the-public-proof-flow-is-offline.md). Plan:
[phase plan](../plans/adr-0056-proof-flow-removal-phase-plan.md), decisions 1
to 7.

## Goal

The public proof flow and its bearer tokens are gone. Its two tables stay,
unused, for one release. The SSRF guard stays, in `src/fetch_guard.rs`.

## Files To Inspect

- `src/proof.rs`, all of it
- `src/api.rs`: each use of `proof::`, `check_admin_or_bearer_with_conn`,
  `extract_bearer_token`, `www_authenticate_challenge`, the two proof
  handlers, `build_router`, and the move step of ADR 0052
- `src/main.rs`: the pruner
- `src/db.rs`: the feed delete that deletes from the proof tables, and the
  `MIGRATIONS` array
- `src/schema.sql`, `src/lib.rs`, `src/openapi.rs`
- `docs/API.md`, `docs/operations.md`, `AGENTS.md` (the module table)
- Each test file that the command
  `grep -rln "proofs/\|issue_token\|Bearer\|proof::" tests` names

## Files Likely To Change

- `src/fetch_guard.rs`, new
- `src/proof.rs`, deleted
- `src/api.rs`, `src/main.rs`, `src/db.rs`, `src/lib.rs`, `src/schema.sql`,
  `src/openapi.rs`
- `docs/API.md`, `docs/operations.md`, `AGENTS.md`
- The test files of the proof flow and the bearer path
- `tests/adr0056_proof_flow_removed_tests.rs`, new

## Do Not Touch

- The behavior of each guard function
- `src/verify.rs`, `src/query.rs`, `src/apply.rs`, `src/event.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- Follow plan decisions 1 to 7 exactly.
- Move the guard functions without a change to their bodies. Update each
  caller to `fetch_guard::`.
- A deleted test is listed in the report, with its file.
- A test that covers the admin path and also a bearer case keeps its admin
  cases.
- `docs/API.md` removes the proof section and the bearer rows, and states
  that each write route needs `X-Admin-Token` (ADR 0056).

## Implementation Steps

1. Create `src/fetch_guard.rs` with the guard group, and update the callers.
2. Delete the proof handlers, routes and OpenAPI entries.
3. Replace `check_admin_or_bearer_with_conn` with `check_admin_token`.
4. Delete the pruner, and remove the proof-table deletes from the feed delete.
5. Delete `src/proof.rs`.
6. Add the comment of plan decision 4 to the two tables in `src/schema.sql`.
7. Remove the token revocation from the ADR 0052 move.
8. Delete or trim the tests of plan decision 7.
9. Add the guard tests below.
10. Update the documents.
11. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0056_proof_flow_removed_tests.rs`, through the
router:

- `POST /v1/proofs/challenge` and `POST /v1/proofs/assert` answer `404`.
- `DELETE /v1/feeds/{guid}`, `PATCH /v1/feeds/{guid}`,
  `DELETE /v1/feeds/{guid}/tracks/{track_guid}` and `PATCH /v1/tracks/{guid}`
  with only `Authorization: Bearer x` answer `403`, and change nothing.
- The same four routes with the admin token behave as before.
- A retire with the admin token writes no row to `proof_challenges` or
  `proof_tokens`.
- A search of `src/` for those table names finds only the migration array and
  `src/schema.sql`.
- `cargo run --bin gen_openapi` output has no `/v1/proofs/` path.
- `grep -rn "proof::" src tests` finds nothing.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0056_proof_flow_removed_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi | grep -c "/v1/proofs/"
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

Add the list of each deleted test file and each deleted test.

## Escalation Triggers

Stop and report when:

- A guard function has a caller that is not sync registration, the proof flow
  or a test.
- A test of the admin path fails for a reason other than a deleted helper.
- The community node code uses the proof tables.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0056-task-001-remove-proof-flow.md`, all of it
- `docs/plans/adr-0056-proof-flow-removal-phase-plan.md`, decisions 1 to 7
- `docs/adr/0056-the-public-proof-flow-is-offline.md`
- The files in "Files To Inspect"

Goal:
- Delete the public proof flow, its bearer path and its pruner. Keep the two
  tables unused. Move the SSRF guard to `src/fetch_guard.rs` unchanged.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- The behavior of each guard function
- `src/verify.rs`, `src/query.rs`, `src/apply.rs`, `src/event.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0056_proof_flow_removed_tests.rs`.
- The gate is green.

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
