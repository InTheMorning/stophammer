# ADR 0042: Query Responses Name The Owner Of Each Field

## Status
Proposed

## Date
2026-09-22

## Context
The v4vmm desktop client reported two defects in the read API. Stophammer
checked the two statements and found each one correct. The evidence is in
[the verification record](../reviews/v4vmm-musicindex-api-change-request-verification.md).

`SearchResponseItem` at `src/query.rs:306` returns `entity_type`, `entity_id`,
`rank`, `quality_score`, `feed_guid` and `href`. It returns no title. A client
cannot show a result row from a search response. It must then read the full
record of each hit.

Five queries select `COALESCE(t.image_url, f.image_url)` and return the result
as `image_url`. The response does not say which owner supplied the value. In
the measured data, 1,781 of 23,960 tracks hold their own artwork. The other
22,179 tracks return the feed artwork through a field that reads as the track's
own artwork. Matching URLs do not show the difference, so no client
can repair this.

The check found a third defect. `build_feed_response` at `src/query.rs:661`
does not apply the `COALESCE`. One live track returns an empty `image_url` on
that route and the feed artwork URL on two other routes. The same field name
thus holds two meanings.

This is a source-first index. A response that composes two owners into one
unnamed value contradicts that model.

## Decision
A query response field takes its value from one owner, or its name states that
the value is resolved.

1. Add `track_image_url` and `feed_image_url` to each response that holds a
   track. `track_image_url` comes from `tracks.image_url` alone.
   `feed_image_url` comes from `feeds.image_url` alone. `track_image_url` is
   null when the track holds no artwork.
2. Keep the field `image_url` and define it as resolved display artwork: the
   track artwork when the track holds one, and the feed artwork when it does
   not. Apply this definition on each route that returns a track, and include
   `build_feed_response`. This removes the second meaning of the name.
3. Add summary fields to `SearchResponseItem`. The response always holds
   `title`. The response holds `feed_title`, `track_image_url`,
   `feed_image_url` and `pub_date` when the row holds them. The handler loads
   these values in the loop that reads the row.
4. A missing summary field is not evidence that the value is missing in the
   full record.

No change to storage and no reingest are necessary. The two artwork values
and the search summary values are stored.

## Alternatives Considered

### Freeze `image_url` with its current per-route behavior

This keeps two meanings in one name. A client must then learn the behavior
of each route. Rejected.

### Remove the `COALESCE` from all five queries

A client that shows track artwork today would no longer show the artwork for
22,179 tracks. Rejected.

### Return a full provenance record with each field

The v4vmm client models the owner, the source and the observation time of each
value. That model belongs to the client. The client did not tell Stophammer to
use it. Rejected as out of scope.

## Consequences

- A client can show a search result row from one call.
- A client can tell album artwork from track artwork, and can show that a track
  holds no artwork of its own.
- `build_feed_response` changes its output. A track with no artwork of its own
  returns the feed artwork in `image_url` and not an empty value. A client
  that read the empty value as "no track artwork" must read `track_image_url`.
- The change adds fields and renames none. ADR 0044 does not classify an
  added field as a breaking change, so the change stays in `v1`.

## Invariants

- A response field takes its value from one owner, or its name states that the
  value is resolved.
- `track_image_url` is null when the track holds no artwork of its own.
- A search result holds the fields that a client needs to show a row.

## Guards

These rules broke in service. Each one earns a test.

- A search response item holds a title.
- A track with no artwork of its own returns a null `track_image_url` and a
  populated `feed_image_url`.
- A track and its feed that assert the same artwork URL continue to return
  the two fields.
- Each route that returns a track returns the same `image_url` for the same
  track.
