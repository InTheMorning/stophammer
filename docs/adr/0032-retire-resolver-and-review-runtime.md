# ADR 0032: Retire Resolver and Review Runtime

## Status
Proposed

Date: 2026-04-08

Supersedes [ADR 0029: Primary Resolver Authority for Replicated Read Models](0029-primary-resolved-replication-authority.md)

## Context
The current codebase still carries a large resolver-era runtime surface:

- `stophammer-resolverd`
- `stophammer-resolverctl`
- resolver queue/state tables and worker logic
- review backends, review TUIs, and review-oriented API routes
- resolver-specific docs, packaging, and tests

That architecture assumed Stophammer would preserve source facts first, then
derive canonical artist/release/recording state in a second subsystem.

That second subsystem is now the wrong tradeoff:

- it introduces a second identity model beyond what the feed actually says
- it increases runtime complexity, replication complexity, and operator burden
- it makes canonical state depend on background work rather than ingest-time
  persistence of source facts
- it pushes the project toward heuristic merging before the v1 schema is
  settled

The current vision for Stophammer is different:

- remove the resolver first
- tighten the importer second
- review the schema from a simpler source-first baseline third

Phase 1 therefore needs an ADR that explicitly retires the resolver runtime
instead of continuing to refine it.

## Decision
Stophammer retires the resolver subsystem and its attached review/operator
runtime.

### Runtime scope

The following runtime surfaces are removed:

- `stophammer-resolverd`
- `stophammer-resolverctl`
- resolver worker/module wiring
- review backends and resolver-only TUI/CLI review flows

### API scope

Resolver-only HTTP surfaces are removed, including:

- resolver status endpoints
- resolver-backed diagnostics/review endpoints
- admin review mutation routes tied to resolver workflows

The surviving API surface should expose source-truth read behavior and ingest
behavior only.

### Replication and events

Stophammer continues to preserve and replicate source facts through the signed
event log.

Resolver-authored derived-state events are no longer part of the forward
architecture. New code should not depend on:

- canonical resolved-state replacement events
- resolver queue draining
- review completion events as part of normal convergence

Historical rows/events may still exist in old databases, but they are not part
of the v1 runtime direction.

### Schema and planning boundary

This ADR does not approve a replacement canonical schema.

Phase 1 removes resolver/runtime surfaces without introducing new canonical
artist/release/recording machinery. Schema redesign remains a later phase under
the vision document and future ADRs.

### Documentation, packaging, and tests

Resolver-specific docs, man pages, packaging files, and tests are removed or
rewritten to match the source-first runtime.

## Consequences
- The codebase becomes smaller and operationally simpler before schema work
  begins.
- Source ingest and source-fact preservation become the only approved baseline
  runtime behavior for the next phase.
- Resolver-specific queue/review concepts stop shaping the v1 schema by
  inertia.
- ADR 0029 is superseded because there is no longer a primary-only resolver
  authority to retain.
- Any future cross-source canonicalization effort would require a new ADR and
  must not be smuggled back in as “just a helper” or “just extra IDs.”
