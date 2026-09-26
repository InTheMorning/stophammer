# ADR 0044 Phase Plan: The Contract Declares Its Fields

Owner: [ADR 0044](../adr/0044-api-contract-declares-its-fields.md), Accepted
2026-09-22.

## Goal

Each documented JSON response declares its field names, and those names come
from the Rust response types rather than a second hand-written copy.

## Non-Goals

- No route is added, renamed or removed.
- No response value changes. This phase changes the document, not the data.
- The custom explorer keeps its present look. This phase does not redesign it.
- The relay routes in `musicindex/relay-api.json` stay hand-kept. This phase
  does not move them.

## Current State

`src/openapi.rs` makes the whole document from a `serde_json::json!` literal in
`spec_value`. The source tree holds zero `#[utoipa::path]` attributes and zero
`ToSchema` derives. Nothing connects a response type to the document.

Measured at commit tip: 54 of 54 JSON responses carry a bare
`{"type":"object"}` schema, and `components.schemas` is empty. The document
records no field name, so a rename changes no part of it.

`src/query.rs` holds 26 `Serialize` structs. `src/api.rs` holds a smaller set.
Not all of them reach a documented response.

## Constraint That Shapes The Work

Neither explorer resolves `$ref`.

- `stophammer/api.html` reads `example`, `examples` and `schema.example` in
  `schemaExample()`. It has no `$ref` handling.
- `musicindex/api.html` fetches the merged `api.json` and holds no schema
  handling at all.

A response that loses its inline `example` renders blank in both. Every
example stays. Schemas are added beside them, not in place of them.

## Affected Modules

| Module | Change |
|---|---|
| `src/query.rs` | `ToSchema` derives on the response types |
| `src/api.rs` | `ToSchema` derives on the response types it owns |
| `src/openapi.rs` | `components.schemas` filled, responses reference them |
| `Cargo.toml` | confirm the `utoipa` derive feature is on |
| `tests/` | a guard that rejects a bare object schema |

## Sequence

1. **Task 001**: derive `ToSchema` on the response types and collect them into
   the document's `components.schemas`. No response references a schema yet.
   The document grows, and nothing reads the new part.
2. **Task 002**: point each documented response at its schema, keeping the
   inline `example` untouched.
3. **Task 003**: add the guards, and correct `AGENTS.md` where it describes
   the procedure.

Each task ends green and deployable on its own.

## Schema And API Implications

None for storage. The published document gains a `components` section. A client
that ignores it is unaffected. A client that reads it can generate a decoder,
which is the outcome ADR 0044 wants: a rename then fails at build time rather
than arriving as a silently absent field.

## Risk Areas

- **The explorers.** Covered by the constraint above. A visual check on both
  pages is a release gate for task 002.
- **`getapi.sh` merge.** The script merges `paths` and `tags` from
  `relay-api.json` and refuses on a path collision. It copies the rest of the
  document, so `components` carries through. This phase adds no path, so the
  collision guard stays quiet. Confirm after task 002.
- **Type churn.** A `ToSchema` derive on a type holding a foreign type may need
  a manual schema. Keep those narrow and documented.

## Test Strategy

- The existing suite must stay green in each task.
- New guard: no documented JSON response carries a schema with no properties.
- New guard: every route in `build_router` and `query_routes` appears in the
  document.
- Manual, task 002 only: load `/api` on the node and `api.html` on the site and
  confirm each endpoint still shows its example.

## Rollback

Each task is one commit and reverts alone. No migration, no stored data, and no
route change, so a revert needs only a redeploy. The published site reverts by
running `getapi.sh && pushsite.sh` from the previous commit.

## Open Questions

- Does any response type hold a field whose type has no `ToSchema`? Task 001
  reports the list rather than inventing a mapping.

## Result Of The Deploy Of 2026-09-26

Tasks 001 to 003 are deployed with the node commit `76487f7`. The live
`/openapi.json` gives 64 JSON responses with named fields. `GET /sync/events`
and `POST /sync/reconcile` keep a plain object, because `Event` has no schema.
The operator did the visual check of both explorers, and each endpoint shows
its example.
