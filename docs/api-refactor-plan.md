# API Refactor Plan

## Status

Draft implementation plan for exposing the new source/canonical model through
the public read API.

## Why this refactor is needed

The database and ingest pipeline now distinguish between:

- source-oriented feed/item data and staged source claims
- canonical artists, releases, and recordings derived from that evidence

The current `/v1/*` read API still mostly reflects the older feed-centric
model:

- `GET /v1/feeds/{guid}` returns a source feed as if it were the main release view
- `GET /v1/tracks/{guid}` returns a source item as if it were the main recording view
- search and recent endpoints are still source-heavy
- there is no direct canonical read surface for `releases` or `recordings`
- there is no clear way for clients to ask for source-platform evidence for one
  canonical work/release

The next API phase should make the source/canonical split explicit without
breaking existing clients.

## Guiding rules

1. Keep current `/v1/feeds/*` and `/v1/tracks/*` semantics source-oriented for now.
2. Add canonical endpoints before changing existing endpoints.
3. Make provenance explicit instead of hiding source-platform detail.
4. Prefer additive includes over bloated default responses.
5. Do not expose unstable resolver internals unless they help inspection.

## Current API shape

Current read routes in [src/query.rs](/home/citizen/build/stophammer/src/query.rs):

- `GET /v1/artists/{id}`
- `GET /v1/artists/{id}/feeds`
- `GET /v1/feeds/{guid}`
- `GET /v1/tracks/{guid}`
- `GET /v1/recent`
- `GET /v1/search`
- `GET /v1/node/capabilities`
- `GET /v1/peers`

These routes currently serialize:

- artists
- source feeds
- source tracks
- search/recent results over those source entities

The missing public objects are:

- canonical `release`
- canonical `recording`
- source-to-canonical mapping views
- staged source evidence views for inspection

## Target API model

The read API should expose two explicit layers.

### 1. Source endpoints

These remain the authoritative way to inspect what one source feed or one
source item currently says.

Keep:

- `GET /v1/feeds/{guid}`
- `GET /v1/tracks/{guid}`

Evolve them to optionally include:

- source links
- source IDs
- source contributors
- source platform claims
- source enclosure variants
- canonical mapping summary

Suggested include keys:

- `tracks`
- `payment_routes`
- `tags`
- `source_links`
- `source_ids`
- `source_contributors`
- `source_platforms`
- `source_release_claims`
- `source_enclosures`
- `canonical`

### 2. Canonical endpoints

Add:

- `GET /v1/releases/{id}`
- `GET /v1/recordings/{id}`
- `GET /v1/artists/{id}/releases`

Likely follow-up endpoints:

- `GET /v1/releases/{id}/sources`
- `GET /v1/recordings/{id}/sources`

Canonical endpoints should default to canonical fields only, with optional
source expansion.

Suggested include keys for releases:

- `artist_credit`
- `tracks`
- `sources`
- `source_links`
- `source_platforms`

Suggested include keys for recordings:

- `artist_credit`
- `sources`
- `source_links`
- `source_enclosures`
- `releases`

## Response shape

Canonical endpoints should not reuse the source feed/track response structs.

Add dedicated response types:

- `ReleaseResponse`
- `ReleaseTrackResponse`
- `RecordingResponse`
- `CanonicalSourceSummary`

Recommended `ReleaseResponse` fields:

- `release_id`
- `title`
- `artist_credit`
- `description`
- `image_url`
- `release_date`
- `created_at`
- `updated_at`
- optional `tracks`
- optional `sources`

Recommended `RecordingResponse` fields:

- `recording_id`
- `title`
- `artist_credit`
- `duration_secs`
- `created_at`
- `updated_at`
- optional `sources`
- optional `releases`

Recommended source summary fields under canonical views:

- `feed_guid` or `track_guid`
- `match_type`
- `confidence`
- `platforms`
- `links`
- `primary_enclosure`

## Search and recent strategy

Do not switch `/v1/search` or `/v1/recent` to canonical-only immediately.

Stage the change:

### First slice

- keep `recent` source-oriented
- keep `search` source-oriented
- optionally add a `type=release` or `type=recording` branch once canonical
  endpoints exist

### Later slice

- decide whether default search should rank canonical entities first
- possibly add explicit canonical search endpoints if mixed search becomes noisy

## Implementation order

### Phase A: Add canonical read helpers

In [src/db.rs](/home/citizen/build/stophammer/src/db.rs), add helper functions for:

- loading one release
- loading one recording
- loading release tracks from `release_recordings`
- loading source feed mappings for a release
- loading source item mappings for a recording
- loading staged source links/platforms/enclosures for mapped entities

Exit criteria:

- no handler changes yet
- DB helpers can build complete canonical read models

### Phase B: Add response structs and handlers

In [src/query.rs](/home/citizen/build/stophammer/src/query.rs):

- add `ReleaseResponse`
- add `RecordingResponse`
- add new handlers:
  - `handle_get_release`
  - `handle_get_recording`
  - `handle_get_artist_releases`

Route additions:

- `/v1/releases/{id}`
- `/v1/recordings/{id}`
- `/v1/artists/{id}/releases`

Exit criteria:

- canonical objects are readable without changing existing source endpoints

### Phase C: Add source/canonical cross-links

Extend existing source endpoints so clients can discover canonical mappings.

For `GET /v1/feeds/{guid}`:

- optional canonical release summary

For `GET /v1/tracks/{guid}`:

- optional canonical recording summary

Exit criteria:

- clients can navigate from source objects to canonical objects

### Phase D: Add source expansions under canonical endpoints

Support `include=sources` and related include keys on releases/recordings.

This is the first slice that enables the product goal:

- work/release
- all platforms for this work
- different enclosures for this work

Exit criteria:

- canonical release view can show all mapped source feeds
- canonical recording view can show all mapped source items and enclosure variants

### Phase E: Revisit search/recent semantics

Only after the canonical endpoints have settled:

- add canonical search branches
- decide whether `recent` needs a canonical variant
- document the long-term default behavior

## Explicit non-goals for the first refactor slice

Do not do these in the first implementation pass:

- remove or rename `/v1/feeds/*` and `/v1/tracks/*`
- switch default search ranking to canonical entities
- expose every staged claim table directly
- add a full review/moderation API
- redesign the sync or ingest APIs

## First implementation slice

The smallest safe first slice is:

1. DB read helpers for releases and recordings
2. `GET /v1/releases/{id}`
3. `GET /v1/recordings/{id}`
4. `GET /v1/artists/{id}/releases`
5. source-to-canonical summaries on `GET /v1/feeds/{guid}` and `GET /v1/tracks/{guid}`

That gives clients a stable canonical API surface without forcing a breaking
change to the existing source-oriented endpoints.
