# ADR 0044 Task 001: Derive Schemas On The Response Types

Owner: [ADR 0044](../adr/0044-api-contract-declares-its-fields.md).
Plan: [phase plan](../plans/adr-0044-contract-schema-phase-plan.md).

## Goal

Each type that a documented JSON response returns derives `utoipa::ToSchema`,
and `src/openapi.rs` publishes those schemas under `components.schemas`.

No response references a schema in this task. The document grows a section that
nothing reads yet. This keeps the change reversible and keeps the explorers
untouched.

## Files To Inspect

- `src/openapi.rs` — `spec_value`, and how the literal becomes
  `utoipa::openapi::OpenApi`
- `src/query.rs` — the `Serialize` response structs
- `src/api.rs` — the `Serialize` response structs
- `Cargo.toml` — the `utoipa` dependency

## Files Likely To Change

- `src/query.rs`
- `src/api.rs`
- `src/openapi.rs`
- `Cargo.toml`, only if the derive feature is absent

## Do Not Touch

- `api.html`
- `migrations/`, `src/schema.sql`, `src/db.rs`
- any handler body or SQL statement
- any route registration
- `docs/` other than this file's report

## Constraints

- Add derives. Do not rename a field, reorder a struct or change a type.
- A struct that no documented response returns gets no derive. State which
  ones you skipped and why.
- Keep the `#[serde(skip_serializing_if = ...)]` attributes exactly as they
  are. A schema must describe what the handler sends today.
- Where a field's type has no `ToSchema`, do not invent a mapping. Leave the
  type out, and report it under unresolved concerns.
- `cargo fmt` defaults. No hand formatting.
- Follow `AGENTS.md`. Use `#[expect(..., reason = "...")]` rather than
  `#[allow]` if a lint needs an exception.

## Implementation Steps

1. Confirm `utoipa` provides the `ToSchema` derive with the features already
   declared. Add the feature only if it is missing, and say so in the report.
2. List the response types each documented response returns. Work from the
   handlers named in `spec_value`, not from every `Serialize` struct.
3. Add `ToSchema` to each of those types' derive lists.
4. In `spec_value`, build `components.schemas` from those types.
5. Run the gate.

## Acceptance Criteria

Mechanical, each proved by a command:

- `cargo build` succeeds.
- `cargo test` is green with no new failures.
- `cargo clippy --all-targets -- -D warnings` reports nothing.
- `cargo fmt -- --check` reports nothing.
- `cargo run --bin gen_openapi` prints a document that parses as JSON.
- That document has a non-empty `components.schemas`.
- Every response in that document still carries the `example` it carries today.
  Compare against the committed tip before your change.

## Test Commands

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi | python3 -c "import json,sys; d=json.load(sys.stdin); print('schemas:', len(d.get('components',{}).get('schemas',{})))"
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns, including any type left out for want of a `ToSchema`

## Escalation Triggers

Stop and report rather than deciding, when:

- a response type holds a field whose type has no `ToSchema`
- adding a derive needs a field renamed, reordered or retyped
- a test fails for a reason this task does not explain
- the generated document loses an `example`

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `src/openapi.rs`
- `src/query.rs`
- `src/api.rs`
- `Cargo.toml`
- `AGENTS.md`

Goal:
- Add `utoipa::ToSchema` to each type that a documented JSON response returns,
  and publish those schemas under `components.schemas` in `spec_value`.
- Do not make any response reference a schema yet.

Constraints:
- Add derives only. No field rename, no reorder, no type change.
- Keep every `#[serde(skip_serializing_if = ...)]` attribute as it is.
- Keep every inline `example` in the document exactly as it is. Two separate
  API explorer pages read those examples and neither resolves `$ref`. A lost
  example renders a blank page.
- If a field's type has no `ToSchema`, leave that type out and report it. Do
  not invent a mapping.
- `cargo fmt` defaults. Use `#[expect(..., reason = "...")]` rather than
  `#[allow]`.

Do not touch:
- `api.html`
- `migrations/`, `src/schema.sql`, `src/db.rs`
- any handler body, SQL statement or route registration

Acceptance criteria:
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` and
  `cargo fmt -- --check` are all green.
- `cargo run --bin gen_openapi` prints valid JSON with a non-empty
  `components.schemas`.
- Every response still carries the example it carried before your change.

Test commands:
- `cargo build`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cargo run --bin gen_openapi | python3 -c "import json,sys; d=json.load(sys.stdin); print('schemas:', len(d.get('components',{}).get('schemas',{})))"`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
