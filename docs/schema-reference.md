# Schema Reference

Source: [src/schema.sql](/home/citizen/build/stophammer/src/schema.sql)

This reference describes the current source-first schema. It does not document
retired canonical release/recording tables.

## Lookup Tables

### `artist_type`
Purpose: enumerates artist kinds used by the internal compatibility artist layer.

### `rel_type`
Purpose: enumerates relationship kinds still used by internal metadata and wallet link workflows.

## Source-First Feed and Track Tables

### `feeds`
Purpose: source-first release-shaped rows keyed by `feed_guid`.
Key columns:
- `feed_guid`
- `feed_url`
- `title`
- `raw_medium`
- `release_artist`
- `release_date`
- `release_kind`
- `publisher`
- `image_url`
- `language`
- `explicit`

Notes:
- `title` is the release title in v1.
- `publisher` means publisher by default.
- Wavlake is the narrow compatibility exception where linked publisher data may
  also supply artist text while stored `publisher` remains `"Wavlake"`.
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
- preserves position, `medium`, target GUID, and optional target URL
- powers the derived `publisher` include in the read API
- non-Wavlake publisher text is only promoted after reciprocal validation

### `live_events`
Purpose: current `pending` and `live` `podcast:liveItem` rows.
Notes:
- replaced on each ingest
- ended live items with enclosures are promoted into normal tracks

### `source_contributor_claims`
Purpose: preserved contributor evidence such as `podcast:person` and other contributor claims.

### `source_entity_ids`
Purpose: preserved source-level IDs such as `npub`, MusicBrainz IDs, ISRCs, and platform-native IDs.

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

## Internal Compatibility and Search Tables

### `artists`
Purpose: internal compatibility artist rows still used by `artist_credit` and some transitional workflows.
Notes:
- this is not the public API model
- v1 public reads use `release_artist` and `track_artist` text directly

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

## Wallet Review Layer

### `wallets`
Purpose: normalized wallet entities used by wallet review and linking flows.

### `wallet_endpoints`
Purpose: normalized route endpoints behind feed and track payment routes.

### `wallet_aliases`
Purpose: observed aliases for wallet endpoints.

### `wallet_track_route_map`
Purpose: links `payment_routes` rows to normalized wallet endpoints.

### `wallet_feed_route_map`
Purpose: links `feed_payment_routes` rows to normalized wallet endpoints.

### `wallet_id_redirect`
Purpose: redirect table for merged wallet identities.

### `wallet_artist_links`
Purpose: reviewed or provisional links between wallet identities and internal artist rows.

### `wallet_identity_review`
Purpose: queued or resolved wallet identity review records.

### `wallet_identity_override`
Purpose: operator overrides for wallet review decisions.
