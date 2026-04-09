# 0035: Add Track-Level Publisher Text

- Status: Accepted
- Date: 2026-04-09

## Context

Phase 3 moved the public API to a source-first feed/track model. `feeds.publisher`
already stores strict publisher text, with the narrow Wavlake exception where
linked publisher metadata may also provide artist text while the stored feed
publisher remains `"Wavlake"`.

That leaves one gap: track rows do not currently persist publisher text. This
prevents item-level reads from carrying the same publisher truth as the parent
feed, even when the operator wants to inspect or search music items by
publisher.

## Decision

Add `tracks.publisher` as an optional source-first field.

In v1:

- `tracks.publisher` stores publisher text, not artist identity
- when ingest creates or updates a track, `tracks.publisher` inherits the
  resolved publisher text chosen for the parent feed
- Wavlake tracks therefore store `"Wavlake"` as publisher text while their
  `track_artist` may still come from linked publisher metadata
- item-level publisher-specific namespace work remains deferred; when a future
  item-level publisher field exists, it can override inherited feed publisher
  text

## Consequences

- track API responses can expose `publisher_text`
- item-level search and inspection can filter or display by publisher without
  recomputing from the feed at read time
- the change is additive and safe for rebuild-first Phase 3 work
