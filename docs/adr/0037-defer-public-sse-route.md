# ADR 0037: Defer the Public SSE Route

## Status
Accepted

## Context

ADR 0020 described a public `GET /v1/events?artists=...` Server-Sent Events
endpoint. The current runtime still contains `SseRegistry` and post-commit
publish wiring for events that pass through ingest, mutation, and community
apply paths, but neither the primary router nor the read-only community router
registers `/v1/events`.

The API reference should only document routes actually exposed by the runtime.

## Decision

Do not document `GET /v1/events` as a public HTTP API route until an Axum handler
and route are present.

Keep the internal `SseRegistry` code and tests as implementation groundwork.
Those internals are not a client-facing API contract.

## Consequences

- `docs/API.md` remains focused on the currently registered HTTP routes.
- Operations docs should not describe community nodes as serving public SSE
  events.
- Security reports that discuss SSE should be read as coverage of the internal
  registry limits unless a later ADR or code change reintroduces the public
  endpoint.
- Adding a public SSE route later requires updating `api.rs`, tests, and the API
  and operations docs in the same change.
