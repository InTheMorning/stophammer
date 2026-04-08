# V4V Music Metadata Vision & Master Refactoring Plan

This document is the single source of truth for the current Stophammer
refactor sequence.

The immediate objective is not to add new resolver logic. The immediate
objective is to simplify the system, tighten the importer, and then review the
schema from a clean baseline so a definitive v1 schema plan can be chosen.

## Ordered Sequence

Work proceeds in this order and no later phase should start early:

1. Remove the resolver and its associated UI/API/operator surfaces.
2. Apply the importer/crawler changes.
3. Review the schema and produce a definitive v1 schema plan.

## Foundational Rules

### 1. One phase per session

Do not mix phases in one coding session. Finish one phase, verify it, then
start a fresh session for the next phase.

### 2. ADR first

Before implementing any architectural, runtime, API, or schema decision from
this plan, create or update the relevant ADR in `docs/adr/`.

This vision document defines sequence and scope. ADRs record the concrete
decision that will actually be implemented.

### 3. Green commit rule

After every code-editing session:

- run `cargo check`
- run `cargo fmt --check`
- run focused tests for the touched area

Per repo policy, successful verification is reported as `Green`.

### 4. Provenance-first data handling

Do not discard source facts because a heuristic prefers one metadata field over
another. Preserve conflicts and defer interpretation until the schema plan
explicitly defines how they should be represented.

### 5. No premature schema invention

Until Phase 3 is complete, do not add new metadata columns, canonical tables,
or lineage heuristics just because they seem useful. Candidate ideas may be
listed, but they are not approved implementation scope.

## Phase 1: Resolver Removal

### Goal

Remove the resolver as a runtime subsystem and remove the review surfaces that
exist only to support it.

The result should be a simpler source-first ingest/query system with no
background resolver worker, no resolver control plane, and no resolver-specific
operator UI.

### In Scope

#### Runtime and build surface

- remove `stophammer-resolverd`
- remove `stophammer-resolverctl`
- remove `backfill_canonical` if it remains resolver-only
- remove resolver module wiring from the library and main runtime

#### Resolver-specific codepaths

- remove `src/resolver/`
- remove `src/resolver_coordination.rs`
- remove `src/review_backend.rs`
- remove review TUI/CLI binaries that exist only for resolver workflows
- remove `src/tui.rs` if it becomes orphaned after the review tools are gone

#### API surface

- remove resolver status endpoints such as `/v1/resolver/status`
- remove resolver-backed diagnostics/review endpoints under `/v1/diagnostics/*`
- remove admin review mutation routes and handlers from `src/api.rs`
- keep only source-truth API surface needed after resolver retirement

#### Packaging, docs, and tests

- remove resolver systemd/env packaging
- remove resolver man pages
- update docs that currently describe resolver-backed reads as part of normal
  operation
- delete or rewrite tests that only validate resolver behavior

### Explicit Non-Goals

- do not redesign the metadata schema in this phase
- do not introduce replacement heuristics in this phase
- do not assume that every current canonical table must be dropped immediately;
  schema retention or removal is decided in Phase 3 unless Phase 1 cleanup
  makes a specific removal unavoidable

### Deliverable

A codebase that compiles and runs without the resolver subsystem or its
attached operator surfaces.

### Exit Criteria

- resolver binaries and review tools are gone
- resolver-only API endpoints are gone
- resolver packaging/docs/tests are removed or rewritten
- `cargo check` and `cargo fmt --check` are Green

## Phase 2: Importer / Crawler Changes

### Goal

Improve the PodcastIndex import path now that resolver work is out of the way.
This phase is limited to importer/crawler behavior, not schema redesign.

### Baseline

ADR 0030 already establishes that the importer lives in
`stophammer-crawler` and uses durable attempt memory in `import_state.db`.
Phase 2 builds on that baseline rather than reopening the runtime split.

### Planned Changes

#### 1. Music-first cursor jump

Target: `stophammer-crawler/src/modes/import.rs`

- define a hard lower bound for the music-first scan window
- if the stored cursor is below that bound, log the jump and start from the
  music-first ID instead of replaying millions of obviously irrelevant rows

#### 2. Snapshot staleness detection

Target: `stophammer-crawler/src/modes/import.rs`

- stop blindly re-downloading the PodcastIndex snapshot when a local copy
  already exists
- use local file modification time plus conditional HTTP request semantics
- skip download/extract work when the remote snapshot is unchanged

#### 3. Preserve importer observability

- keep ADR 0030 durable attempt memory intact
- any skip/jump behavior must remain auditable through importer state rather
  than becoming invisible control flow

### Explicit Non-Goals

- no feed metadata schema expansion in this phase
- no canonical matching logic in this phase
- no new review UI in this phase

### Deliverable

A faster, more restart-friendly importer that avoids wasteful snapshot and
pre-music scanning work.

### Exit Criteria

- importer behavior matches the approved Phase 2 ADR scope
- touched importer tests are Green
- `cargo check` and `cargo fmt --check` are Green

## Phase 3: Schema Review For Definitive v1 Plan

### Goal

Review the existing schema from the simplified post-Phase-2 baseline and
decide what the actual v1 schema should be.

This phase is a review and planning phase, not a migration-writing phase.

### Questions To Answer

#### 1. What remains as core source truth?

Review the tables that preserve feed, track, and source-evidence facts:

- `feeds`
- `tracks`
- `feed_remote_items_raw`
- `source_contributor_claims`
- `source_entity_ids`
- `source_entity_links`
- `source_release_claims`
- `source_item_enclosures`
- `source_platform_claims`
- `events` and sync tables

The schema review must also decide and document the strict artist-field rules
for v1:

- `release_artist` and `track_artist` are separate fields
- feed-level `itunes:author` maps to `release_artist`
- `podcast:person` remains contributor metadata, not release-artist truth
- `track_artist` remains separate even when v1 defaults it from
  `release_artist`
- artist sort-order metadata is optional and separate from display artist text
- sort fields stay null unless reliable published sort metadata exists
- any internal derived sort key is an indexing helper, not authoritative
  source truth
- `publisher` retains its normal meaning by default; do not globally treat
  publisher data as artist truth
- Wavlake is a narrow compatibility exception where current feed-level and
  track-level publisher data may provide artist text, but that text still does
  not become a stable unique artist identity
- minimum v1 does not include an `artists` table or `artist_id`
- a future `artists` table should only appear once there is a direct
  artist-claim / feed-link workflow built on feed ownership proof
- future `artist_id` values must come from explicit claim/link flows, not from
  artist-name inference

The schema review must also document the strict title mapping rules for v1:

- feed title = release title
- item title = track title
- minimum v1 should not add duplicate release-title / track-title columns when
  `feeds.title` and `tracks.title` already carry those meanings

The schema review must also document the core v1 field rules:

- `release_date` = feed `pubDate`
- artwork is stored as URL fields at feed and track level
- `publisher` means publisher by default, except for the narrow Wavlake
  compatibility exception already noted
- enclosures are stored as published RSS enclosure data
- value blocks mirror RSS tags, route types, and arguments as-is
- links and external IDs are stored so users can search by values such as
  `npub`
- ordering follows RSS ordering or `pubDate`
- `language` and `explicit` are stored per track, inheriting from the feed
  when missing
- unknown values stay `unknown` unless explicitly present or safely derivable

The schema review must also document the v1 identifier policy:

- `feed_guid` and item/track GUIDs are source publication identities
- child source-fact tables should prefer composite natural keys over new global
  IDs
- `event_id` is an operational mutation-log identifier, not music metadata
- canonical IDs such as `release_id`, `recording_id`, `work_id`, or
  `artist_id` should be deferred until those concepts actually exist as
  first-class schema entities

#### 2. What is resolver-derived and therefore suspect for v1?

Review the tables and concepts that exist because of the resolver/canonical
layer:

- `releases`
- `recordings`
- `release_recordings`
- `source_feed_release_map`
- `source_item_recording_map`
- `entity_source` where it is used only as canonical provenance
- `resolver_queue`
- `resolver_state`
- `artist_identity_override`
- `artist_identity_review`

#### 3. Which artist/relationship tables survive independently of the resolver?

Review whether these remain part of v1 as first-class schema or should be
deferred/simplified:

- `artists`
- `artist_aliases`
- `artist_credit`
- `artist_credit_name`
- `artist_artist_rel`
- `artist_id_redirect`
- tags, relationships, and external ID tables

#### 4. What API shape follows from the v1 schema?

Classify each current API surface as one of:

- keep as source-truth v1 API
- remove with resolver retirement
- defer until after the v1 schema is approved

### Required Output

Phase 3 must end with a concrete v1 schema decision artifact that tells the
user:

- which tables/columns are kept unchanged
- which tables are removed
- which tables are deferred to post-v1
- which migrations will be required
- which API/docs changes follow from that schema choice
- which open questions still need an ADR before implementation

### Explicit Non-Goals

- no schema migration implementation in this phase
- no speculative parser changes in this phase
- no new canonicalization rules in this phase

## Deferred Candidate Topics

These ideas may be revisited after Phase 3, but they are not approved work
before the schema review completes:

- a future RSS / Podcasting 2.0 `artist_credit` tag, because `podcast:person`
  is contributor metadata and not a strict release-artist field
- a future RSS / Podcasting 2.0 `kind` or `release_kind` tag, because the
  current namespace distinguishes `medium=music` but does not formally encode
  album/EP/single-style release kind
- generator or lineage capture such as `generator` / `generator_lineage`
- lineage-aware ingest heuristics
- equal-weight treatment of conflicting contributor evidence
- Wavlake/MSP/De-Mu specific normalization rules
- any replacement for the removed resolver UI beyond narrowly scoped source
  inspection tooling

## Current Status

The next executable phase is Phase 1: resolver removal.
