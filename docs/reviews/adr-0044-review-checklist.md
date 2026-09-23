# ADR 0044 Review Checklist

Use this after each task in the
[phase plan](../plans/adr-0044-contract-schema-phase-plan.md). Review the diff
against [ADR 0044](../adr/0044-api-contract-declares-its-fields.md), the plan
and the task packet.

## Mechanical Gates

- [ ] `cargo build` green
- [ ] `cargo test` green, with no test deleted or weakened
- [ ] `cargo clippy --all-targets -- -D warnings` silent
- [ ] `cargo fmt -- --check` silent
- [ ] `cargo run --bin gen_openapi` prints valid JSON
- [ ] `components.schemas` is non-empty
- [ ] Each response carries the same `example` as the previous commit

## Visual Gate

Keep this apart from the mechanical list. It cannot be proved by a test, and a
gate that cannot run is reported as open, never as met.

- [ ] `/api` on the node lists every endpoint and shows each example
- [ ] `api.html` on the site does the same after `getapi.sh && pushsite.sh`

## Drift

- [ ] No route added, renamed or removed
- [ ] No response value changed. This phase changes the document only
- [ ] No field renamed. A rename inside `v1` is the breaking change ADR 0044
      exists to prevent
- [ ] No `#[serde(skip_serializing_if = ...)]` added or removed, so the schema
      matches what the handler sends
- [ ] No handler body, SQL statement or migration touched
- [ ] No opportunistic cleanup outside the task
- [ ] `#[expect(..., reason = ...)]` used rather than `#[allow]`

## Invariants From The ADR

- [ ] A documented JSON response names its fields
- [ ] A `v1` field name is unchanged
- [ ] The published document comes from the response types, not a second
      hand-written copy of the field names

## Merge Decision

- [ ] Pass or fail
- [ ] Required fixes
- [ ] Optional improvements
- [ ] Can this phase merge
- [ ] Does the next task packet need adjustment
