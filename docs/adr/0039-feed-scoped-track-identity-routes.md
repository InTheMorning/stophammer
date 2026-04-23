# 0039: Feed-Scoped Public Track Identity Routes

- Status: Accepted
- Date: 2026-04-22

## Context

Stophammer currently exposes flat track endpoints keyed only by
`track_guid`:

- `GET /v1/tracks/{guid}`
- `PATCH /v1/tracks/{guid}`

That shape assumes `track_guid` is globally unique across all feeds. In
practice, RSS item GUIDs are source publication identities and can collide
across feeds. The source-first schema and docs already treat `track_guid` as
raw source truth rather than a synthetic globally unique ID.

When duplicate raw item GUIDs are eventually present in the store, the flat
route shape becomes unsafe because it can no longer identify a single track
unambiguously.

## Decision

Adopt `(feed_guid, track_guid)` as the canonical public locator for tracks.

### Canonical routes

Add feed-scoped canonical routes:

- `GET /v1/feeds/{feed_guid}/tracks/{track_guid}`
- `PATCH /v1/feeds/{feed_guid}/tracks/{track_guid}`

The existing delete route already follows this model:

- `DELETE /v1/feeds/{feed_guid}/tracks/{track_guid}`

### Flat-route compatibility

Keep the flat routes as compatibility shims:

- `GET /v1/tracks/{guid}`
- `PATCH /v1/tracks/{guid}`

Behavior:

- if exactly one track matches the raw `track_guid`, proceed normally
- if no track matches, return `404`
- if multiple tracks match, return `409 Conflict`

The `409` response must be machine-readable and include canonical recovery
targets:

- `code = "ambiguous_track_guid"`
- the raw `track_guid`
- candidate `feed_guid` values
- canonical `href` values

### Search disambiguators

Track search hits remain source-first and keep `entity_id = track_guid`, but
they also include:

- `feed_guid`
- canonical `href`

This lets callers move from search results directly to canonical track URLs
without assuming global uniqueness of raw item GUIDs.

## Consequences

### Positive

- preserves raw source `track_guid` semantics
- gives callers a stable migration path before storage-layer dedup changes
- aligns read and write routes with feed-scoped bearer-token authorization

### Negative

- flat routes remain a compatibility surface that callers should migrate away
  from
- this ADR does not by itself remove the current storage assumption that
  `tracks.track_guid` is globally unique
- search and event internals still require a later storage/identity refactor
  to support true duplicate raw `track_guid` rows end-to-end

## Follow-up

A later ADR/change set must complete the storage refactor so duplicate raw
`track_guid` values can coexist safely in the database and replication model
without synthetic rewriting of the source GUID.
