# 0040: Store Track Identity As Feed-Scoped

- Status: Accepted
- Date: 2026-04-22

## Context

ADR 0039 introduced feed-scoped public track routes so callers can stop
assuming global uniqueness of raw RSS item GUIDs. That change was only a
compatibility layer. The storage model still treats `tracks.track_guid` as a
global primary key and several child tables still reference tracks by raw
`track_guid` alone.

That means the database still cannot store two tracks with the same raw
`track_guid` under different feeds, even though that collision is valid at the
source-publication layer.

## Decision

Promote `(feed_guid, track_guid)` to the authoritative storage identity for
track-shaped source rows.

### Core track table

Change `tracks` from:

- primary key: `track_guid`

to:

- composite primary key: `(feed_guid, track_guid)`

The raw `track_guid` stays verbatim and remains the source publication
identity. No synthetic public rewrite is introduced.

### Child tables

Any live child table that currently references a track by raw `track_guid`
alone must become feed-scoped as well:

- `payment_routes` references `(feed_guid, track_guid)`
- `value_time_splits` adds `source_feed_guid` and references
  `(source_feed_guid, source_track_guid)`
- `track_remote_items_raw` adds `feed_guid` and references
  `(feed_guid, track_guid)`

### Search and quality

Track search and quality rows use an internal canonical key derived from
`feed_guid` plus raw `track_guid`. This key is internal-only and is not exposed
as the public `track_guid`.

Public search responses continue to expose:

- raw `track_guid` as `entity_id`
- `feed_guid`
- canonical `href`

### Event apply

Replication apply paths must use feed-scoped identifiers whenever the event
payload provides them. Track upserts already carry `track.feed_guid`. Track
removals already carry `feed_guid`. Track-remote-item replacement payloads are
extended to carry `feed_guid` as well.

## Consequences

### Positive

- duplicate raw RSS item GUIDs can coexist across feeds
- public API remains source-first
- caller migration work from ADR 0039 becomes aligned with true storage rules

### Negative

- schema migration rebuilds several tables
- internal helpers and tests that assumed global raw `track_guid` uniqueness
  must be updated
- search and quality code must decode internal track entity keys before
  returning public results

## Notes

This ADR intentionally does not invent a new synthetic public track ID. The
public contract remains source-first:

- `track_guid` is raw source identity
- `(feed_guid, track_guid)` is the canonical locator
