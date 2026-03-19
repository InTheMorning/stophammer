# Primary-Authority Resolved Replication Plan

This document plans the next architectural step after the local resolver queue
rollout: make the primary the authority for resolved canonical state and
replicate those decisions to community nodes.

## Why

The current resolver architecture has two properties that are becoming more
expensive than they are useful:

- community nodes still re-run canonical clustering, promotions, search
  rebuilds, and targeted artist identity work from replicated source facts
- resolver review and override state is already primary-local and authoritative,
  which means the most important identity decisions are no longer truly
  "re-derived everywhere"

That creates avoidable duplication and drift risk:

- every community node pays the full resolver cost
- resolver heuristics and override behavior must stay in lock-step everywhere
- operationally, community nodes need `resolverd` even though the primary is
  already the place where review and approval happen

The desired end state is:

- source feed/track data remains preserved and authoritative
- the primary runs the resolver and emits resolved-state events
- community nodes apply those resolved-state events directly
- community nodes no longer need to re-run canonical clustering or artist
  identity logic in normal operation

## Invariants

- source feed/track rows and staged source claims remain preserved and
  authoritative
- resolver work continues to derive state from that preserved source layer; it
  does not rewrite or "normalize away" the source rows
- primary-side review and override decisions are authoritative and replicated
- community nodes remain deterministic appliers of signed state transitions,
  not independent authorities on identity decisions

## Target Architecture

Split replicated state into two layers:

1. source events
   - emitted by ingest and source-preserving mutations
   - keep source feed/track/claim rows reachable everywhere

2. resolved-state events
   - emitted by `resolverd` on the primary after durable resolver work commits
   - carry canonical clustering, promotions, and override-backed decisions as
     signed, replayable snapshots

Community nodes still receive the source layer, but canonical/queryable derived
state converges from primary-authored resolved events instead of local
re-derivation.

## Phase Plan

### Phase 1: Resolved-state event scaffolding

Scope:

- document the authority shift explicitly
- introduce feed-scoped resolved-state event types for canonical state
- teach `apply_events` to replace canonical state from a primary-authored
  snapshot
- let `resolverd` optionally emit those snapshot events after successful
  canonical rebuilds on the primary

Constraints:

- no community cutover yet
- local community resolver behavior remains valid as a fallback
- push fanout is not required for the first slice; poll-based catch-up is
  sufficient

Outcome:

- the primary can begin emitting signed canonical-state snapshots
- replicas can consume them deterministically without invoking canonical
  clustering logic

### Phase 2: Expand resolved-state coverage

Scope:

- promotions and provenance snapshots
- add feed-scoped resolved overlay tables so primary-authored external-ID and
  provenance decisions can be replaced authoritatively on replicas instead of
  only appended additively
- override-backed artist identity decisions
- explicit replication of durable review/override consequences

Outcome:

- canonical release/recording state, artist external IDs, and provenance all
  converge from primary-authored resolved events

Status:

- phase-2 groundwork started
- feed-scoped overlay tables now exist for authoritative resolved external-ID
  and provenance replication:
  - `resolved_external_ids_by_feed`
  - `resolved_entity_sources_by_feed`
- those tables are not the public read model; they are staging tables for
  future primary-authored resolved-state replacement

### Phase 3: Community resolver retirement

Scope:

- stop requiring `resolverd` on community nodes for canonical convergence
- narrow community apply to source facts plus resolved snapshots
- keep only cheap local indexing/projection work that does not make authority
  decisions

Outcome:

- primary runs the resolver
- community nodes apply signed resolved state

### Phase 4: Replication contract cleanup

Scope:

- simplify resolver docs/manpages for community nodes
- remove obsolete local-resolver queue expectations from read-only replicas
- decide whether search stays local-projection work or becomes fully replicated

Outcome:

- replication model matches operational reality cleanly

## Event Design Principles

Resolved-state events should be:

- feed-scoped where practical, to align with the current dirty-queue model
- snapshot-style, not patch-style, so replay remains idempotent and easy to
  reason about
- signed by the primary, just like source events
- explicit about their derived nature so community nodes can distinguish source
  preservation from resolved overlays

For phase 1, the first resolved event is a feed-scoped canonical state
snapshot:

- canonical release rows touched by the feed
- canonical recording rows touched by the feed
- release track ordering rows for those releases
- source feed→release and track→recording maps for the feed

That is enough to start moving canonical clustering authority onto the primary
without attempting the whole resolved stack at once.
