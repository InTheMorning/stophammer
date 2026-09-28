# ADR 0034: Adopt Rebuild-First Source-First v1 Music Schema

## Status
Accepted on 2026-09-27, as narrowed by the amendment below.

Amended on 2026-09-27: section 10 keeps the search and quality tables, and
names the parts of section 4 that are not yet done. The amendment reverses no
other decision.

Amended on 2026-09-27: section 11 gives the sequence that removes the
compatibility artist credit in two releases.

Date: 2026-04-08

Supersedes [ADR 0025: Source Claims and Canonical Music Layers](0025-source-claims-and-canonical-music-layers.md) for the v1 schema direction.

## Context
After ADR 0032 retired the resolver runtime and ADR 0033 tightened the
importer, Stophammer still carries a schema shaped by the old canonical-layer
plan:

- `feeds` and `tracks` still depend on `artist_credit_id`
- canonical graph tables such as `artists`, `releases`, `recordings`, and
  mapping tables still exist
- search, quality, resolver, and wallet tables still sit beside the source
  ingest layer
- the API surface still documents canonical artist/release/recording reads

That is too much schema opinion for the current product goal.

The current goal is narrower:

- preserve RSS / Podcasting 2.0 source facts
- expose a music-first read shape for release and track display
- avoid any v1 schema that requires canonical artist/release/recording
  resolution
- leave future artist claiming and canonicalization as explicit later work

The user also set strict v1 semantics during schema review:

- feed = release-shaped source object
- item = track-shaped source object
- feed-level `itunes:author` = release artist text
- `podcast:person` remains contributor evidence, not release-artist truth
- `publisher` retains its normal meaning, except for the narrow Wavlake
  compatibility rule where current publisher data may also provide artist text
- release date = feed `pubDate`
- artwork is URL-based at feed and track level
- track `language` and `explicit` inherit from the feed when missing
- unknown values remain `null` or `unknown`

## Decision
Stophammer adopts a rebuild-first, source-first v1 music schema.

### 1. Rollout shape

The v1 music schema is planned as a rebuild of the music metadata database, not
as an in-place preservation of every legacy table.

This means:

- source-truth tables define what survives into v1
- resolver-era and canonical derived tables are disposable unless explicitly
  retained
- non-database persistent assets such as signing keys are out of scope for the
  rebuild and must not be touched by schema work

### 2. Core v1 entity model

The v1 schema uses source publication objects directly:

- one `feed` row is the release-shaped object
- one `track` row is the track-shaped object

The v1 schema does not include first-class canonical entities for:

- artists
- releases separate from feeds
- recordings separate from tracks
- works
- artist credits

### 3. Tables preserved for v1

The approved v1 base tables are:

- `schema_migrations`
- `events`
- `feed_crawl_cache`
- `node_sync_state`
- `peer_nodes`
- `feeds`
- `tracks`
- `feed_payment_routes`
- `payment_routes`
- `value_time_splits`
- `feed_remote_items_raw`
- `source_contributor_claims`
- `source_entity_ids`
- `source_entity_links`
- `source_release_claims`
- `source_item_enclosures`
- `source_item_transcripts`

### 4. Tables removed from the v1 music schema

The following tables are not part of the v1 music schema and should be dropped
in the rebuild:

- resolver/review tables
- canonical artist/release/recording tables
- canonical relationship/tag tables
- canonical promoted ID/provenance tables
- search/quality tables tied to the old schema

Concretely, that includes:

- `resolver_queue`
- `resolver_state`
- `artist_identity_override`
- `artist_identity_review`
- `resolved_external_ids_by_feed`
- `resolved_entity_sources_by_feed`
- `artists`
- `artist_aliases`
- `artist_credit`
- `artist_credit_name`
- `releases`
- `recordings`
- `release_recordings`
- `source_feed_release_map`
- `source_item_recording_map`
- `artist_artist_rel`
- `artist_id_redirect`
- `track_rel`
- `feed_rel`
- `tags`
- `artist_tag`
- `feed_tag`
- `track_tag`
- `external_ids`
- `entity_source`
- `search_index`
- `search_entities`
- `entity_quality`
- `entity_field_status`

### 5. Tables deferred from the minimum v1 cut

These tables are deferred from the minimum v1 music schema:

- `proof_challenges`
- `proof_tokens`
- `live_events`
- `source_platform_claims`
- all wallet tables
- lookup tables used only by deferred canonical layers

If proof-of-possession survives as a runtime feature for non-music mutations,
that subsystem must be justified separately and must not shape the minimum
music schema.

### 6. Approved v1 feed and track fields

`feeds` remains the release-shaped row with these user-facing semantics:

- `title` = release title
- `release_artist` = release artist text
- `release_artist_sort` = optional published sort form
- `publisher` = publisher text
- `release_date` = feed `pubDate`
- `release_kind` = optional strict release classification, defaulting to
  `unknown` when not explicitly published
- `image_url` = release artwork URL
- `language` = feed language
- `explicit` = feed explicit flag
- `raw_medium` = source `podcast:medium`

`tracks` remains the track-shaped row with these user-facing semantics:

- `title` = track title
- `publisher` = track publisher text
- `track_artist` = track artist text
- `track_artist_sort` = optional published sort form
- `image_url` = track artwork URL
- `language` = track language
- `explicit` = track explicit flag
- `pub_date` = track chronology and fallback ordering
- `duration_secs` = track duration when published
- `enclosure_url`, `enclosure_type`, `enclosure_bytes` = published primary
  media
- `track_number` = published sequencing when present

Fields such as `title_lower`, `artist_credit_id`, `itunes_type`,
`episode_count`, `newest_item_at`, `oldest_item_at`, and `season` are not part
of the minimum v1 music model.

### 7. Artist and publisher rules

The v1 schema uses strict field rules:

- feed-level `itunes:author` maps to `feeds.release_artist`
- `tracks.track_artist` stays separate even when it defaults from
  `feeds.release_artist`
- `podcast:person` remains contributor evidence in
  `source_contributor_claims`
- sort-order metadata stays null unless explicitly published
- artist text is display metadata, not a stable artist identity
- `publisher` means publisher by default everywhere
- `tracks.publisher` exists for item-level publisher search/display and
  inherits the resolved feed publisher in v1
- Wavlake is a narrow compatibility exception where current feed-level and
  track-level publisher data may also supply artist text, but that text still
  does not become a stable unique artist ID

### 8. Identity rules

The v1 schema distinguishes source identities from future canonical identities:

- `feed_guid` and `track_guid` are source publication identities
- child source-fact rows should prefer composite natural keys over synthetic
  global IDs
- `events.event_id` remains an operational mutation-log identifier
- v1 does not include `artist_id`, `release_id`, `recording_id`, or `work_id`

A future `artists` table is allowed only after StopHammer has a direct artist
claim / feed-link workflow. Artist names alone must not mint artist entities.

### 9. API and documentation boundary

The v1 schema decision implies a source-first API direction:

- feed and track reads stay primary
- source evidence reads stay primary
- canonical artist/release/recording reads are not part of the minimum v1
  schema and should be removed or deferred with the corresponding code/docs
- search should be reconsidered only after the v1 source-first schema is
  implemented

### 10. The state on 2026-09-27

The resolver, review, release, recording, tag and relationship tables are
gone, and the seven `source_*` tables exist. These parts of section 4 are
changed or open:

- **Search and quality stay.** `search_index`, `search_entities` and
  `entity_quality` now hold only feeds and tracks, the source objects of
  section 2. They back `GET /v1/search`, and
  [ADR 0042](0042-query-responses-name-the-field-owner.md) sets rules for its
  results. So section 4 no longer drops them, and section 9 no longer defers
  search.
- **The compatibility artist credit is open work.** The ingest still makes one
  feed-scoped `artists` and `artist_credit` row for each release artist text
  and each track author, because `feeds.artist_credit_id` and
  `tracks.artist_credit_id` are required. It also signs
  `ArtistCreditCreated`. No public read route uses these rows. The removal
  needs a rebuild of `feeds` and `tracks`, and the end of
  `ArtistCreditCreated`. A node still applies the old events of the log.
- **The unused tables are open work.** `artist_type` and `rel_type` have no
  use, and `external_ids` is empty.

Item 13 of the remaining accepted work plan holds the open work.

### 11. The removal of the artist credit takes two releases

Amendment of 2026-09-27. The operator decided the sequence.

`Feed` and `Track` carry `artist_credit_id` in the payloads of
`FeedUpserted` and `TrackUpserted`. The field is required today, so a node
rejects an event without it. If the primary stops the field in one release,
each community node must upgrade first. Two releases remove that order:

1. **Release A.** A node accepts a `Feed` or a `Track` with no
   `artist_credit_id`. When the field is absent, the node makes the
   feed-scoped credit from the text of the payload, as the ingest does:
   `release_artist` for a feed, and `track_artist`, else the feed credit, for
   a track. The primary still writes the field and still signs
   `ArtistCreditCreated` and `ArtistUpserted`. Release A changes nothing that
   an older node reads.
2. **Release B.** The primary makes no artist, no credit and no
   `artist_credit_id`, and signs no `ArtistCreditCreated` and no
   `ArtistUpserted`. A migration rebuilds `feeds` and `tracks` with no
   `artist_credit_id`, and drops the tables of section 10. The release notes
   state that each community node must run release A or later before the
   primary runs release B.

After release B, these rules hold:

- The event log keeps each old `ArtistCreditCreated` and `ArtistUpserted`
  event. A node that reads the log from the start accepts each one, verifies
  its signature, and applies nothing. The wire types of the two payloads stay
  for this purpose only.
- A node ignores `artist_credit_id` in an old `FeedUpserted` or
  `TrackUpserted` event.
- The internal SSE registry of ADR 0037 keys its channels by feed GUID, not
  by artist ID.
- The feed quality score gives its 10 points for a stated `release_artist`,
  in place of a present `artist_credit_id`. The track quality score gives its
  5 points for a stated `track_artist`, or for a stated `release_artist` of
  its feed. Each feed and each track has a credit today, and the ingest makes
  it from the same text, so no score changes.

## Consequences
- Stophammer gets a smaller, more explicit v1 schema that matches current
  product scope.
- Source truth remains preserved for later artist claim workflows or future
  canonicalization.
- The project stops carrying canonical artist/release/recording tables by
  inertia before there is an approved artist-ownership model.
- A future cross-source canonical layer would require a new ADR; it is not
  implied by this decision.

## Guards

Section 11 has these tests:

- A node applies a `FeedUpserted` and a `TrackUpserted` event with no
  `artist_credit_id` (release A).
- After release B, a node applies a log with old `ArtistCreditCreated` and
  `ArtistUpserted` events, and its database has no artist table.
- After release B, the quality score of a feed is the same as before the
  change.
