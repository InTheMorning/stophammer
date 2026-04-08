# ADR 0029: Primary Resolver Authority for Replicated Read Models

## Status
Accepted

Date: 2026-03-19

## Context
ADR 0025 split Stophammer into a preserved source layer and a derived canonical
music layer. The first resolver rollout kept that architectural boundary, but
community nodes still ran their own resolver batches to rebuild:

- source feed/track search and quality
- canonical release/recording state
- canonical promotions and provenance
- canonical search
- targeted artist-identity cleanup

That kept replication conceptually symmetric, but it became the wrong
operational tradeoff:

- the primary already owns review and override decisions for artist identity
- community nodes duplicated expensive resolver work after every source event
- resolver heuristics and override behavior had to remain perfectly aligned on
  every replica
- canonical/read-model lag on community nodes depended on local background
  work instead of primary-authored convergence

At this point the primary is the only authority that should make resolver
decisions. Community nodes should preserve source facts, then follow signed
resolved-state changes emitted by the primary.

## Decision
The primary resolver is the sole authority for derived read-model state.

Community nodes:

- preserve and apply replicated source events
- apply signed primary-authored resolved-state events
- do not run `stophammer-resolverd`
- do not re-derive canonical state, promotions, search, or artist-identity
  decisions locally

The primary `stophammer-resolverd` emits signed feed-scoped resolved-state events for:

- canonical release/recording state
  - `canonical_feed_state_replaced`
- canonical promotions and provenance
  - `canonical_feed_promotions_replaced`
- feed-scoped artist-identity completion
  - `artist_identity_feed_resolved`
- override-backed artist merges
  - `artist_merged`

Replica apply logic treats those events as authoritative:

- canonical state events replace canonical release/recording mappings and
  rebuild canonical search
- promotions events replace promoted external IDs and provenance
- artist-identity completion events clear feed-scoped identity backlog without
  local heuristics
- artist-merge events replay primary-approved merges directly

The source layer remains preserved and authoritative. Resolver events enrich
read models on top of that preserved state; they do not rewrite source feed,
track, or staged claim rows.

`stophammer-resolverd` is therefore primary-only. If `NODE_MODE=community`, it exits
immediately.

## Consequences
- Community nodes converge from primary-authored resolved events instead of
  running local resolver batches.
- Primary nodes must run `stophammer-resolverd` if they want canonical/search/promoted
  read models to advance and replicate.
- Resolver review and override decisions now have a single replication
  authority.
- Replication semantics are cleaner: source events preserve facts, resolved
  events publish derived state.
- The primary-facing env var for this emission path is
  `RESOLVER_EMIT_RESOLVED_STATE_EVENTS`.
