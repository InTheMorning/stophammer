# ADR 0044: The API Contract Declares Its Fields

## Status
Accepted

Amended 2026-09-23: the field-declaration rule covers a response that a
client decodes without a credential. It said every documented response, which
obliged the node to publish its internal event model for the sake of four
routes that need the sync token. That obligation served no client.

## Date
2026-09-22

## Context
The v4vmm desktop client reported that a renamed response field disappears
without an error. Stophammer checked the statement and found it correct. The
cause is not the one the client assumed. The evidence is in
[the verification record](../reviews/v4vmm-musicindex-api-change-request-verification.md).

The client believed that the contract marks fields optional. The live contract
marks no field at all.

| Measure of the live contract | Value |
|---|---:|
| JSON responses declared | 54 |
| Responses with a bare `{"type":"object"}` schema | 54 |
| Responses that declare properties | 0 |
| Entries in `components.schemas` | 0 |

`src/openapi.rs` builds the document from a hand-written `serde_json::json!`
literal in `spec_value`. The source tree holds zero `#[utoipa::path]`
attributes and zero `ToSchema` derives. Nothing connects a Rust response type
to the published document.

A rename in a response struct thus changes the response and leaves the
document unchanged. A client decodes the new response, finds no value, and
reports that the field is missing. The defect can stay hidden for months.

The check also found that `AGENTS.md` states two incorrect steps for a new
endpoint. Step 3 tells an author to add a `utoipa` attribute to a handler.
Step 4 tells an author to regenerate `api.html` with
`cargo run --bin gen_openapi`.
That command prints the OpenAPI JSON document to standard output, and
`api.html` is a hand-written page that reads `/openapi.json` at run time.

## Decision
The published contract declares each field that a client decodes, and the
contract comes from the response types.

1. Each type that a documented response returns derives `utoipa::ToSchema`.
2. The document references those schemas. It does not hold a second copy of
   the field names in a hand-written literal.
3. A response that a client decodes without a credential declares its
   fields. A route that needs the sync token serves a node operator, and
   this decision states nothing about it.
4. A field rename or a field removal in a `v1` response is a breaking change.
   It needs a new path version. An added field is not a breaking change.
5. `AGENTS.md` states the correct procedure for a new endpoint in the same
   change that this decision is accepted.

## Alternatives Considered

### Keep the literal and compare it to the types in a test

This holds the field list in two places and asks a test to keep them equal. The
author continues to write each name two times. Rejected.

### Keep the literal and write the field names by hand

This removes the bare schemas and leaves the drift. A rename continues to
change the response and leaves the document unchanged, because nothing
connects them. Rejected.

### Version the whole API on each response change

An added field breaks no client that decodes unknown fields safely. A version
for each added field costs more than it returns. Rejected.

## Consequences

- A client can make a decoder from the document, and a rename then fails at
  build time rather than at run time.
- The custom explorer at `api.html` continues to operate, because it reads
  `/openapi.json` and the document keeps its shape.
- Each response type gains a derive. This touches many types in `src/query.rs`
  and `src/api.rs`.
- The hand-written examples in `src/openapi.rs` stay useful and stop being the
  only record of a field name.

## Invariants

- A documented JSON response names its fields.
- A `v1` field name is stable for the life of `v1`.
- The published document comes from the response types.

## Guards

A silent rename cost a client months of missing data. This rule earns a
test.

- A response that needs no credential holds a schema with properties.
  Enforced by `each_json_response_names_its_fields` in
  `tests/adr0044_schema_refs_tests.rs`.
- Each route in `build_router` and in `query_routes` appears in the document.
  Enforced by `each_router_route_is_in_primary_document` and
  `each_readonly_router_route_is_in_readonly_document` in
  `tests/adr0044_contract_guard_tests.rs`.
- The document that `gen_openapi` prints parses as a correct OpenAPI document.
