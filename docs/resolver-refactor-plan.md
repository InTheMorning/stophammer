# Resolver Refactor Plan

This document describes the staged refactor from today's inline canonical sync
plus manual backfills to a durable, incremental resolver subsystem.

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

Phase 1 only uses the first three bits.

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

### Phase 4: Reduce inline work

Scope:

- once the queue path is proven stable, reduce or remove redundant inline
  canonical sync from ingest/apply
- let `resolverd` own the heavier post-ingest consolidation path

This is deliberately later because it changes steady-state write behavior.

### Phase 5: Review and override tooling

Scope:

- unresolved artist/release/recording reports
- manual merge / do-not-merge state
- evidence views built on the stored source claims and mappings

This sits on top of the automatic resolver; it does not replace it.

## Phase 1 Status

Phase 1 starts in this branch/repo state:

- resolver queue schema and DB helpers
- a minimal `resolverd` worker
- dirty marking from current write paths
- operator docs for running the worker

What Phase 1 does not do yet:

- importer auto-pause integration
- incremental artist identity
- manual override workflow
- removal of the current inline canonical rebuild path
