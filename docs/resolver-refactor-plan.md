# Resolver Refactor Plan

This document describes the staged refactor from today's inline canonical sync
plus manual backfills to a durable, incremental resolver subsystem.

Historical note:

- this document covers the local-resolver rollout that is now in place
- the next architectural step, making the primary the authority for resolved
  replication, is planned separately in
  [primary-resolved-replication-plan.md](/home/citizen/build/stophammer/docs/primary-resolved-replication-plan.md)

## Why

Current behavior is split across two modes:

- ingest/apply rebuilds canonical release/recording state inline for the
  affected feed
- operator backfills (`backfill_canonical`, `backfill_artist_identity`) repair
  or complete derived state after the fact

That leaves an operational gap:

- fresh imports can stop with duplicate or missing artist identities
- there is no durable record of feeds that still need deferred resolution
- there is no background worker that can catch up after import or downtime

Invariant:

- source feed/track rows and staged source claims are the authoritative
  extracted layer
- resolver work may derive canonical/enriched state from those facts, but must
  not rewrite the preserved source layer

The goal is to make resolver work durable, incremental, and eventually
automatic, while keeping the deterministic backfill binaries for repair and
migration use.

## Target Architecture

Keep the resolver inside the main `stophammer` repo and split it into a
first-class internal subsystem:

- `src/resolver/mod.rs`
- `src/resolver/queue.rs`
- `src/resolver/worker.rs`
- `src/bin/resolverd.rs`

The long-term responsibility split is:

- ingest/apply:
  - persist source facts
  - mark affected feeds dirty
  - keep only cheap inline sync where it materially improves queryability
- resolver worker:
  - drain dirty feeds incrementally
  - rebuild canonical release/recording state
  - rebuild canonical promotions and search rows
  - later run targeted artist identity consolidation
- maintenance binaries:
  - full backfill after schema changes, bug fixes, or disaster rebuilds

## Queue Model

Phase 1 adds two operational tables:

- `resolver_queue`
  - one row per dirty feed
  - tracks dirty work, lock state, retries, and last error
- `resolver_state`
  - key/value coordination state
  - first use is `import_active`

Dirty bits are additive:

- `1` canonical state
- `2` canonical promotions
- `4` canonical search
- `8` artist identity

Phase 1 only used the first three bits. The current staged rollout now also
uses the artist-identity bit for targeted feed-scoped cleanup.

## Phases

### Phase 1: Durable canonical queue

Scope:

- add `resolver_queue` and `resolver_state`
- add DB helpers for mark/claim/complete/fail/state read-write
- add `resolverd`
- mark feeds dirty from ingest/apply
- keep existing inline canonical sync in place
- worker resolves only:
  - `sync_canonical_state_for_feed`
  - `sync_canonical_promotions_for_feed`
  - `sync_canonical_search_index_for_feed`

Why this first:

- it creates a durable trail of unfinished resolver work
- it is low risk because current inline behavior remains unchanged
- it turns full canonical rebuilds into a maintenance path instead of a normal
  requirement

### Phase 2: Incremental artist identity

Scope:

- extract deterministic artist merge logic from `backfill_artist_identity`
- derive impacted artist groups from dirty feeds
- resolve only those groups
- add the `artist_identity` dirty bit to normal queue processing

Important constraint:

- ambiguous cases must remain unresolved
- review tooling and overrides stay separate from the automatic resolver

### Phase 3: Import-aware scheduling

Scope:

- have the bulk importer set `resolver_state.import_active=true` while it runs
- `resolverd` pauses heavy work during import
- queue drains after import completes

This avoids expensive cross-feed consolidation competing with a large import.

Status:

- complete in the current operational form
- crawler import mode can set `import_active` automatically when
  `RESOLVER_DB_PATH` is configured
- importer activity is heartbeat-based instead of a one-shot boolean pause
- `resolverd` ignores stale import heartbeats so a crashed importer cannot
  leave the queue paused forever

### Phase 4: Reduce inline work

Scope:

- once the queue path is proven stable, reduce or remove redundant inline
  canonical sync from ingest/apply
- let `resolverd` own the heavier post-ingest consolidation path

This is deliberately later because it changes steady-state write behavior.

Status:

- inline canonical promotions, canonical release/recording state, and canonical
  search have all been moved off the ingest/apply path
- direct source feed/track rows still remain inline, but source feed/track
  search and quality are resolver-backed derived state too
- later phase-4 slices can trim more inline rebuild work once operators are
  comfortable relying on `resolverd` for convergence

### Phase 5: Review and override tooling

Scope:

- unresolved artist/release/recording reports
- manual merge / do-not-merge state
- evidence views built on the stored source claims and mappings

This sits on top of the automatic resolver; it does not replace it.

Status:

- started for feed-scoped artist identity
- `resolverd` now persists durable review items for feed-scoped candidate groups
- operator merge / do-not-merge overrides are now stored durably and checked by
  both targeted resolver batches and whole-db artist-identity backfills
- the current review tool can:
  - list pending review items
  - inspect one review item
  - store merge overrides
  - store do-not-merge overrides

Still deferred:

- review APIs beyond the CLI
- release / recording override workflows
- richer review lifecycle states beyond the current pending / merged / blocked /
  resolved feed-scoped flow

## Phase 1 Status

Phase 1 landed with:

- resolver queue schema and DB helpers
- a minimal `resolverd` worker
- dirty marking from current write paths
- operator docs for running the worker

What Phase 1 still does not do:

- manual override workflow
- removal of the current inline canonical rebuild path

## Phase 2 Status

Phase 2 is complete in its feed-scoped form:

- normal write paths now mark the artist-identity dirty bit
- `resolverd` runs `resolve_artist_identity_for_feed(...)` for touched feeds
- the implementation reuses the existing deterministic merge heuristics from
  `backfill_artist_identity`
- resolver batch output now reports seed-artist and candidate-group counts so
  feed-scoped work is visible before the queue model becomes artist-group-based
- review tooling can now:
  - inspect one feed-scoped artist identity plan
  - list feeds whose targeted plan still has candidate groups to review

What remains deferred beyond Phase 2:

- importer-aware prioritization beyond the coarse `import_active` pause flag
- richer batching keyed by impacted artist groups instead of feed scope
- richer manual override state and review lifecycle beyond the initial
  feed-scoped artist identity review tables
