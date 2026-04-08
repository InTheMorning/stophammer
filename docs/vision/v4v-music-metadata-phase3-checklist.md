# Phase 3 Schema Review Checklist

This file is the execution checklist for Phase 3.

It turns the current vision and minimal-v1 proposal into a concrete review
package so the Phase 3 session can produce a definitive v1 schema artifact
without re-opening Phase 1 or Phase 2.

Draft outputs from this checklist now live in:

- `docs/vision/v4v-music-metadata-schema-v1-decision.md`
- `docs/adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md`

## Baseline

- Phase 1 is complete: resolver/runtime/review surfaces are retired
- Phase 2 is complete: importer/crawler behavior is tightened
- Phase 3 is rebuild-first for the music metadata database
- Phase 3 should not promise an in-place migration of every resolver-era table
- non-database persistent assets such as signing keys are out of scope here

## Required Outputs

Phase 3 is complete only when it produces:

- one definitive v1 schema decision artifact
- one preserve/drop/defer/new table decision list
- one column-level decision list for `feeds`, `tracks`, and source-fact tables
- one rebuild policy explaining what is recreated from source facts
- one follow-up list for API, docs, and migration work
- one list of any remaining ADR-gated open questions

## Working Table Buckets

These are the current working buckets derived from the minimal-v1 plan. Phase
3 should confirm them or explicitly override them.

### Preserve as v1 Base Tables

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

### Drop in the Rebuild

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

### Defer Until Post-v1

- `proof_challenges`
- `proof_tokens`
- `live_events`
- `source_platform_claims`
- all wallet tables
- lookup tables used only by deferred canonical layers

### New or Reshaped v1 Fields

- `feeds.release_artist`
- `feeds.release_artist_sort`
- `feeds.publisher`
- `feeds.release_date`
- `feeds.release_kind`
- `tracks.track_artist`
- `tracks.track_artist_sort`
- `tracks.image_url`
- `tracks.language`

## Field Rules To Lock

Phase 3 must explicitly confirm these field rules in the final artifact:

- `feeds.title` = release title
- `tracks.title` = track title
- feed-level `itunes:author` = `feeds.release_artist`
- `tracks.track_artist` stays separate even when it defaults from
  `feeds.release_artist`
- `podcast:person` remains contributor evidence, not release-artist truth
- sort-order metadata is optional and stays null unless explicitly published
- `release_date` = feed `pubDate`
- artwork is URL-based at feed and track level
- `publisher` means publisher except for the narrow Wavlake compatibility rule
- enclosures are preserved as RSS/source facts
- value blocks mirror RSS tags and arguments as published
- links and external IDs remain directly searchable by claimed value
- ordering follows RSS order or `pubDate`
- track `language` and `explicit` inherit from feed values when missing
- unknown values remain `null` or `unknown`

## Identity Rules To Lock

- `feed_guid` and `track_guid` are source publication identities
- source-fact child tables prefer composite natural keys
- `events.event_id` is operational, not music identity
- no `artist_id`, `release_id`, `recording_id`, or `work_id` in minimum v1
- a future `artists` table is blocked on direct artist claim / feed-link
  workflow

## Open Review Edges

These should be answered directly during Phase 3 instead of left implicit:

- whether `payment_routes.feed_guid` is kept for convenience or removed as
  derivable
- whether `live_events` is truly out of minimum v1 or just postponed
- which proof/auth tables are required for the first v1 cut
- which API routes survive once the schema is rebuilt around source-first
  reads

## Exit Condition

The Phase 3 session should end with a final artifact that lets implementation
start without further schema-discovery work.
