# ADR 0056 Phase Plan: Remove The Public Proof Flow

Owner: [ADR 0056](../adr/0056-the-public-proof-flow-is-offline.md). This plan
states no rule.

## Goal

No public route issues a bearer token, and each write route needs the admin
token.

## Non-Goals

- A replacement for publisher self-service.
- A change to the behavior of the SSRF guard.

## Assumptions

- The production tables `proof_challenges` and `proof_tokens` are empty
  (backup of 2026-09-24 20:01 UTC).
- `src/proof.rs` holds two groups of functions. The guard group is used by
  sync registration: `is_url_ssrf_safe`, `validate_node_url`,
  `resolve_and_validate_url`, `validate_feed_url`, `build_ssrf_safe_client`,
  `build_ssrf_safe_client_pinned` and `fetch_with_pinned_redirects`. The other
  functions belong to the proof flow.

## Decisions For The Tasks

1. **The module.** `src/fetch_guard.rs` gets the guard group, unchanged, with
   its tests. `src/proof.rs` is deleted. `src/lib.rs` and the module table of
   `AGENTS.md` change with it. A guard function that only the proof flow calls
   is deleted.
2. **The routes.** `/v1/proofs/challenge` and `/v1/proofs/assert` leave
   `build_router`, `src/openapi.rs` and `docs/API.md`.
3. **The auth helper.** `check_admin_or_bearer_with_conn` becomes a call to
   `check_admin_token` at each of its callers. The bearer extraction and the
   `WWW-Authenticate` helpers are deleted when no other caller uses them. A
   request with only a bearer header answers `403` from `check_admin_token`.
4. **The tables.** No migration in this task. The tables `proof_challenges`
   and `proof_tokens` stay, and no code reads or writes them. The delete of a
   feed stops deleting from them. `src/schema.sql` keeps them with a comment
   that names ADR 0056 section 3. The trigger
   `trg_feeds_cleanup_before_delete` still deletes the rows of a deleted feed
   from the two tables. The migration that removes the tables also changes
   that trigger.
5. **The pruner.** `PROOF_PRUNE_INTERVAL_SECS` and its task in `src/main.rs`
   are deleted. `docs/operations.md` stops naming the variable.
6. **The self-link move.** The move of ADR 0052 stops revoking tokens.
7. **The tests.** A test of the proof flow or of the bearer path is deleted. A
   test of the admin path stays, and changes only where it used a helper that
   is deleted. The report lists each deleted test file and each deleted test.

## Sequence

| Task | Crate | Needs |
|---|---|---|
| [001](../tasks/adr-0056-task-001-remove-proof-flow.md) Remove the proof flow | `stophammer` | ADR 0052 task 001 and ADR 0053 tasks 004 and 006 merged in the working tree |
| Deploy | VPS | With the ADR 0053 deploy |

Task 001 changes `src/api.rs` and many tests. It runs after the other node
tasks of this round, so no two agents change the same file.

## Schema And API Implications

- No migration. Two empty tables stay for one release.
- Two routes are removed from the API and the OpenAPI document.
- Four write routes stop accepting a bearer token.
- One environment variable is removed.

## Risk Areas

- **A client that sends a bearer token.** No evidence shows one. It gets
  `403`.
- **The migration on a community node.** The tables exist there too, and they
  are empty.

## Test Strategy

- The guards of ADR 0056 as a new test file.
- The existing guard tests of the fetch guard, moved with the functions.
- The full gate and the migration tests.

## Rollback

Deploy the previous image. The tables still exist, so that binary works,
including its feed delete. The proof flow returns with it.
