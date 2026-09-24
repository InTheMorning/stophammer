# Schema Reference

Source: [src/schema.sql](../src/schema.sql)

This reference describes the current source-first schema. It does not document
retired canonical release/recording tables.

For the boundary between stored artist text, source evidence, and RSS-only
artist/contributor identity, see
[artist-source-evidence.md](schema/artist-source-evidence.md).

## Lookup Tables

### `artist_type`
Purpose: enumerates artist kinds used by the internal compatibility artist layer.

### `rel_type`
Purpose: enumerates relationship kinds still used by internal metadata workflows.

## Source-First Feed and Track Tables

### `feeds`
Purpose: source-first release-shaped rows keyed by `feed_guid`.
Key columns:
- `feed_guid`
- `feed_url`
- `title`
- `raw_medium`
- `release_artist`
- `release_artist_source`
- `release_date`
- `release_kind`
- `publisher`
- `image_url`
- `language`
- `explicit`

Notes:
- `title` is the release title in v1.
- `publisher` means publisher by default. It has no host exception. ADR 0049
  section 5.
- `release_artist_source` is nullable. It names the value that
  `release_artist` used: `itunes_author`, `itunes_owner`, or `placeholder`. A
  feed with no ingest since migration 0037 has a null value. ADR 0049 section
  5.
- `episode_count`, `newest_item_at`, and `oldest_item_at` are ingest-maintained
  convenience fields.

### `tracks`
Purpose: source-first track-shaped rows keyed by `track_guid`.
Key columns:
- `track_guid`
- `feed_guid`
- `title`
- `publisher`
- `pub_date`
- `track_artist`
- `image_url`
- `language`
- `enclosure_url`
- `enclosure_type`
- `enclosure_bytes`
- `track_number`
- `explicit`

Notes:
- `title` is the track title in v1.
- `publisher` is source-first publisher text inherited from the resolved feed
  publisher in v1.
- `track_artist` is stored separately from `feeds.release_artist`.
- when track language is missing, ingest may inherit from the feed.

### `payment_routes`
Purpose: track-level `podcast:value` routes.

### `feed_payment_routes`
Purpose: feed-level `podcast:value` routes used when a track has no track-specific routes.

### `value_time_splits`
Purpose: `podcast:valueTimeSplit` rows for time-ranged payment overrides.

## Preserved Source Evidence

### `feed_remote_items_raw`
Purpose: raw feed-level `podcast:remoteItem` declarations.
Notes:
- preserves position, `medium`, target GUID, target URL, and `rel`, all
  optional except position, medium, and target GUID
- `rel` is nullable. The Podcast Namespace does not define `rel` on
  `podcast:remoteItem`, so the value is non-standard. Migration 0035.
- powers the derived `publisher` include in the read API

### `track_remote_items_raw`
Purpose: raw track-level `podcast:remoteItem` declarations.
Notes:
- preserves position, `medium`, target GUID, target URL, and `rel`, the same
  as `feed_remote_items_raw`
- powers the derived `publisher` include for tracks in the read API

### `live_events`
Purpose: current `pending` and `live` `podcast:liveItem` rows.
Notes:
- replaced on each ingest
- ended live items with enclosures are promoted into normal tracks

### `source_contributor_claims`
Purpose: preserved contributor evidence such as `podcast:person` and other contributor claims.
Notes:
- stores contributor `name`, `role`, normalized role, group, `href`, `img`,
  and row-scoped `npub`
- evidence is attached to a feed or track, not to a canonical contributor
  profile

### `source_entity_ids`
Purpose: preserved source-level IDs such as `npub`, MusicBrainz IDs, ISRCs, and platform-native IDs.
Notes:
- `podcast:txt purpose="npub"` is stored as `scheme = "nostr_npub"`
- IDs are entity-level source evidence; they are not promoted into canonical
  artist or contributor identity

### `source_entity_links`
Purpose: preserved typed links such as websites, self-feed links, and release pages.

### `source_release_claims`
Purpose: preserved release-like claims from feeds, tracks, and live items.

### `source_item_enclosures`
Purpose: preserved primary and alternate enclosure variants for tracks and live items.

### `source_item_transcripts`
Purpose: preserved transcript URLs and metadata for tracks and live items from `podcast:transcript` tags.

### `source_platform_claims`
Purpose: preserved feed-level platform evidence such as `wavlake`, `fountain`, or `rss_blue`.
Notes:
- evidence-oriented, not artist identity
- may include URL and owner-name evidence

### `feed_url_observations`
Purpose: records which URL gave which `podcast:guid` on an accepted ingest.
Key columns:
- `url` (primary key)
- `feed_guid`
- `observed_at`

Notes:
- a later accepted ingest of the same URL replaces its row
- `db::resolve_listed_feed` reads this table to resolve a publisher link by
  URL. ADR 0049 section 1 and section 3.
- migration 0036 seeded one row for each feed from `feeds.feed_url` and
  `feeds.created_at`. The seed is not an event, so a community node that
  started empty holds no seeded row. `resolve_listed_feed` also reads
  `feeds.feed_url` directly, so a node without the seed still resolves the
  same link. Task 004b.

## Internal Compatibility and Search Tables

### `artists`
Purpose: internal compatibility artist rows still used by `artist_credit` and some transitional workflows.
Notes:
- this is not the public API model
- v1 public reads use `release_artist` and `track_artist` text directly
- ingest-created source-text artist rows do not receive RSS `npub`,
  `podcast:person img`, or feed artwork as canonical artist-profile fields

### `artist_aliases`
Purpose: stored aliases attached to internal artist rows.

### `artist_credit`
Purpose: internal compatibility credit rows referenced by `feeds` and `tracks`.

### `artist_credit_name`
Purpose: names within an internal compatibility artist credit.

### `external_ids`
Purpose: promoted external IDs that the system has chosen to store outside raw source evidence.

### `entity_source`
Purpose: provenance/trust records for entities managed by internal layers.

### `entity_quality`
Purpose: cached quality scores for internal entity/search workflows.

### `search_index`
Purpose: FTS5 search index backing `/v1/search`.

### `search_entities`
Purpose: companion lookup table for `search_index` row-to-entity resolution.

## Events and Replication

### `events`
Purpose: append-only signed event log for all replicated mutations.
Key columns:
- `event_id`
- `event_type`
- `payload_json`
- `subject_guid`
- `signed_by`
- `signature`
- `seq`

`feed_url_observed` is one `event_type` value. Its payload carries `url`,
`feed_guid`, and `observed_at`. The primary node signs and sends it on each
new URL-to-GUID observation. A community node applies it to
`feed_url_observations`. Deploy each community node before the primary node
sends this type. ADR 0049 section 1.

### `feed_crawl_cache`
Purpose: content-hash deduplication cache for crawler submissions.

### `node_sync_state`
Purpose: per-peer replication cursor state.

### `peer_nodes`
Purpose: known community-node registry for push replication.

## Proof-of-Possession

### `proof_challenges`
Purpose: feed-scoped proof challenges.

### `proof_tokens`
Purpose: short-lived proof tokens issued after successful proof completion.
