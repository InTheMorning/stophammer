# ADR 0025: Source Claims and Canonical Music Layers

## Status
Accepted

Date: 2026-03-18

## Context
The original Stophammer data model treated RSS-derived `feeds` and `tracks` as if
they were already canonical music entities. That was workable for a small
prototype, but the audited corpus showed it was too naive:

- many releases are mirrored across Wavlake, Fountain, and RSS Blue
- feed-level metadata mixes platform facts (`owner_name`, source URLs) with
  artist/release facts
- rich Podcasting 2.0 metadata such as `podcast:person`,
  `podcast:remoteItem`, typed IDs, links, live items, and alternate enclosures
  is valuable evidence even when it is not yet safe to canonicalize
- canonical identity decisions (artist, release, recording) need to be
  reversible and attributable to source evidence

At the same time, Stophammer's replication model is feed-scoped and event-based.
Any new architecture has to fit the existing signed-event log and community-node
apply flow instead of introducing a separate out-of-band resolver database.

## Decision
Stophammer separates ingest into two layers:

### 1. Source-claim layer

The ingest path stores raw feed-derived evidence in replicated, feed-scoped
snapshot tables. These include:

- `feed_remote_items_raw`
- `live_events`
- `source_contributor_claims`
- `source_entity_ids`
- `source_entity_links`
- `source_release_claims`
- `source_item_enclosures`
- `source_platform_claims`

These rows are source-oriented, not canonical. They preserve provenance such as
extraction path, source tag, and observed order so later resolver logic can be
audited and refined without reparsing the original RSS corpus.

Each feed-scoped source table is replicated with a `*Replaced` event type in the
signed event log. Community nodes therefore receive exactly the same source
evidence as the primary.

### 2. Canonical music layer

Canonical music entities are derived deterministically from current source
evidence and stored separately:

- `artists`
- `releases`
- `recordings`
- `release_recordings`
- `source_feed_release_map`
- `source_item_recording_map`

The current resolver remains conservative:

- feeds only cluster into one canonical release when strong source evidence
  agrees (exact signature or corroborated cross-platform single-track mirrors)
- tracks only cluster into one canonical recording when strong source evidence
  agrees
- otherwise, the canonical layer falls back to identity mappings rather than
  speculative merges

Canonical promotion is intentionally narrow. Today that includes a small set of
high-confidence artist external IDs and provenance links, while most source
claims remain staged rather than auto-promoted.

### 3. Query and product boundary

The source layer is the network-replicated evidence substrate.
The canonical layer is the current best deterministic interpretation of that
evidence.

Future read APIs should expose both perspectives:

- canonical release / recording views for end users
- source-platform views and raw evidence for inspection, debugging, and
  editorial review

## Consequences
- Rich Podcasting 2.0 metadata can be ingested immediately without prematurely
  forcing it into canonical artist/release/recording tables.
- Community nodes stay coherent because both source evidence and canonical
  derivations are represented inside the existing signed event system.
- Resolver behavior becomes inspectable and replaceable: changes to canonical
  logic can be replayed or backfilled from stored source claims.
- The schema becomes more complex, with an explicit distinction between
  source-oriented tables and canonical music tables.
- Query/API work becomes more important: once both layers exist, users need a
  clear way to ask for either canonical results or source/platform detail.
- This ADR does not extend Stophammer to non-music program/show feeds. Those
  remain out of scope for canonical ingest until a separate architecture is
  accepted.
