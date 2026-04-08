# Current Schema Inventory By Justification

This file is a planning artifact for the v1 schema review.

It does not propose changes. It groups the current schema by why each part
exists today, because the current database mixes three different concerns:

1. internal database/runtime operations
2. direct RSS/source-fact preservation
3. derived music/canonical representation

Source of truth for table definitions: `src/schema.sql`

## 1. Internal DB Usage

These tables primarily exist so the node can run, replicate, authorize, score,
or search data. They are not themselves the user-facing music model.

### Runtime, replication, and crawler state

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `events` | signed mutation log and sync backbone | `event_id`, `event_type`, `payload_json`, `subject_guid`, `signed_by`, `signature`, `seq`, `created_at`, `warnings_json` |
| `feed_crawl_cache` | crawler dedupe cache | `feed_url`, `content_hash`, `crawled_at` |
| `node_sync_state` | per-peer sync cursor memory | `node_pubkey`, `last_seq`, `last_seen_at` |
| `peer_nodes` | known peer registry | `node_pubkey`, `node_url`, `discovered_at`, `last_push_at`, `consecutive_failures` |

### Search and scoring internals

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `search_index` | FTS5 search index | `entity_type`, `entity_id`, `name`, `title`, `description`, `tags` |
| `search_entities` | rowid mapping for contentless FTS | `rowid`, `entity_type`, `entity_id` |
| `entity_quality` | computed completeness score | `entity_type`, `entity_id`, `score`, `computed_at` |
| `entity_field_status` | per-field completeness breakdown | `entity_type`, `entity_id`, `field_name`, `status` |

### Proof / mutation authorization

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `proof_challenges` | proof-of-possession challenge state | `challenge_id`, `feed_guid`, `scope`, `token_binding`, `state`, `expires_at`, `created_at` |
| `proof_tokens` | short-lived mutation tokens | `access_token`, `scope`, `subject_feed_guid`, `expires_at`, `created_at` |

### Resolver and review internals

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `resolver_queue` | background resolver work queue | `feed_guid`, `dirty_mask`, `first_marked_at`, `last_marked_at`, `locked_at`, `locked_by`, `attempt_count`, `last_error` |
| `resolver_state` | resolver worker state flags | `key`, `value` |
| `artist_identity_override` | manual merge/do-not-merge rules | `source`, `name_key`, `evidence_key`, `override_type`, `target_artist_id`, `note`, `created_at`, `updated_at` |
| `artist_identity_review` | pending/reviewed artist identity conflicts | `review_id`, `feed_guid`, `source`, `name_key`, `evidence_key`, `status`, `artist_ids_json`, `artist_names_json`, `created_at`, `updated_at` |
| `resolved_external_ids_by_feed` | feed-scoped resolved external ID cache | `feed_guid`, `entity_type`, `entity_id`, `scheme`, `value`, `created_at` |
| `resolved_entity_sources_by_feed` | feed-scoped resolved provenance cache | `feed_guid`, `entity_type`, `entity_id`, `source_type`, `source_url`, `trust_level`, `created_at` |

### Wallet / V4V identity internals

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `wallets` | normalized wallet identity rows | `wallet_id`, `display_name`, `display_name_lower`, `wallet_class`, `class_confidence`, `created_at`, `updated_at` |
| `wallet_endpoints` | normalized payment destinations | `id`, `route_type`, `normalized_address`, `custom_key`, `custom_value`, `wallet_id`, `created_at` |
| `wallet_aliases` | wallet alias history | `id`, `endpoint_id`, `alias`, `alias_lower`, `first_seen_at`, `last_seen_at` |
| `wallet_track_route_map` | payment route to wallet endpoint join | `route_id`, `endpoint_id`, `created_at` |
| `wallet_feed_route_map` | feed route to wallet endpoint join | `route_id`, `endpoint_id`, `created_at` |
| `wallet_id_redirect` | merged wallet redirect map | `old_wallet_id`, `new_wallet_id`, `created_at` |
| `wallet_artist_links` | wallet-to-artist link layer | `wallet_id`, `artist_id`, `evidence_entity_type`, `evidence_entity_id`, `confidence`, `created_at` |
| `wallet_identity_review` | wallet conflict review queue | `id`, `wallet_id`, `source`, `evidence_key`, `wallet_ids_json`, `endpoint_summary_json`, `status`, `created_at`, `updated_at` |
| `wallet_identity_override` | wallet merge/block overrides | `id`, `override_type`, `wallet_id`, `target_id`, `value`, `created_at` |

### Lookup tables

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `artist_type` | artist classification dictionary | `id`, `name` |
| `rel_type` | relationship-type dictionary | `id`, `name`, `entity_pair`, `description` |

## 2. Direct RSS Namespace / Source Fact Preservation

These tables are the closest thing to RSS truth in the current schema. Some are
direct field storage, others are normalized source-fact snapshots extracted
from Podcasting 2.0 tags and related feed metadata.

### Feed and track rows

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `feeds` | top-level feed record | `feed_guid`, `feed_url`, `title`, `title_lower`, `artist_credit_id`, `description`, `image_url`, `language`, `explicit`, `itunes_type`, `episode_count`, `newest_item_at`, `oldest_item_at`, `created_at`, `updated_at`, `raw_medium` |
| `tracks` | top-level item/track record | `track_guid`, `feed_guid`, `artist_credit_id`, `title`, `title_lower`, `pub_date`, `duration_secs`, `enclosure_url`, `enclosure_type`, `enclosure_bytes`, `track_number`, `season`, `explicit`, `description`, `created_at`, `updated_at` |

### Value-for-value RSS data

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `payment_routes` | track-level `<podcast:value>` routes | `track_guid`, `feed_guid`, `recipient_name`, `route_type`, `address`, `custom_key`, `custom_value`, `split`, `fee` |
| `feed_payment_routes` | feed-level `<podcast:value>` routes | `feed_guid`, `recipient_name`, `route_type`, `address`, `custom_key`, `custom_value`, `split`, `fee` |
| `value_time_splits` | `<podcast:valueTimeSplit>` rows | `source_track_guid`, `start_time_secs`, `duration_secs`, `remote_feed_guid`, `remote_item_guid`, `split`, `created_at` |

### Raw Podcasting 2.0 objects and source claims

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `feed_remote_items_raw` | raw `<podcast:remoteItem>` feed references | `feed_guid`, `position`, `medium`, `remote_feed_guid`, `remote_feed_url`, `source` |
| `live_events` | current `<podcast:liveItem>` snapshot | `live_item_guid`, `feed_guid`, `title`, `content_link`, `status`, `scheduled_start`, `scheduled_end`, `created_at`, `updated_at` |
| `source_contributor_claims` | raw contributor evidence | `feed_guid`, `entity_type`, `entity_id`, `position`, `name`, `role`, `role_norm`, `group_name`, `href`, `img`, `source`, `extraction_path`, `observed_at` |
| `source_entity_ids` | raw IDs from source feeds | `feed_guid`, `entity_type`, `entity_id`, `position`, `scheme`, `value`, `source`, `extraction_path`, `observed_at` |
| `source_entity_links` | raw typed URLs from source feeds | `feed_guid`, `entity_type`, `entity_id`, `position`, `link_type`, `url`, `source`, `extraction_path`, `observed_at` |
| `source_release_claims` | raw release-ish claims from source feeds | `feed_guid`, `entity_type`, `entity_id`, `position`, `claim_type`, `claim_value`, `source`, `extraction_path`, `observed_at` |
| `source_item_enclosures` | raw primary and alternate media URLs | `feed_guid`, `entity_type`, `entity_id`, `position`, `url`, `mime_type`, `bytes`, `rel`, `title`, `is_primary`, `source`, `extraction_path`, `observed_at` |
| `source_platform_claims` | inferred platform provenance cache | `feed_guid`, `platform_key`, `url`, `owner_name`, `source`, `extraction_path`, `observed_at` |

## 3. Current Derived Music / Canonical Representation

These tables are where the current schema stops being plain RSS preservation
and starts expressing a specific music model.

### Artist and credit layer

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `artists` | canonical artist rows | `artist_id`, `name`, `name_lower`, `sort_name`, `type_id`, `area`, `img_url`, `url`, `begin_year`, `end_year`, `created_at`, `updated_at` |
| `artist_aliases` | artist alias lookup | `alias_lower`, `artist_id`, `created_at` |
| `artist_credit` | normalized display credit rows | `id`, `display_name`, `created_at` |
| `artist_credit_name` | credit member breakdown | `artist_credit_id`, `artist_id`, `position`, `name`, `join_phrase` |

### Canonical release / recording layer

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `releases` | canonical release rows | `release_id`, `title`, `title_lower`, `artist_credit_id`, `description`, `image_url`, `release_date`, `created_at`, `updated_at` |
| `recordings` | canonical recording rows | `recording_id`, `title`, `title_lower`, `artist_credit_id`, `duration_secs`, `created_at`, `updated_at` |
| `release_recordings` | release-to-recording ordered membership | `release_id`, `recording_id`, `position`, `source_track_guid` |
| `source_feed_release_map` | source feed to canonical release map | `feed_guid`, `release_id`, `match_type`, `confidence`, `created_at` |
| `source_item_recording_map` | source track to canonical recording map | `track_guid`, `recording_id`, `match_type`, `confidence`, `created_at` |

### Relationship, tags, IDs, and provenance

| Table | Why it exists now | Key columns |
| --- | --- | --- |
| `artist_artist_rel` | artist-to-artist graph | `artist_id_a`, `artist_id_b`, `rel_type_id`, `begin_year`, `end_year`, `created_at` |
| `artist_id_redirect` | merged artist redirect map | `old_artist_id`, `new_artist_id`, `merged_at` |
| `track_rel` | track-to-track graph | `track_guid_a`, `track_guid_b`, `rel_type_id`, `created_at` |
| `feed_rel` | feed-to-feed graph | `feed_guid_a`, `feed_guid_b`, `rel_type_id`, `created_at` |
| `tags` | normalized tag dictionary | `id`, `name`, `created_at` |
| `artist_tag` | artist-to-tag join | `artist_id`, `tag_id`, `created_at` |
| `feed_tag` | feed-to-tag join | `feed_guid`, `tag_id`, `created_at` |
| `track_tag` | track-to-tag join | `track_guid`, `tag_id`, `created_at` |
| `external_ids` | promoted canonical external IDs | `entity_type`, `entity_id`, `scheme`, `value`, `created_at` |
| `entity_source` | promoted canonical provenance | `entity_type`, `entity_id`, `source_type`, `source_url`, `trust_level`, `created_at` |

## Planning Read

The current schema is not merely "feed storage." It presently contains:

- a source-fact ingest layer
- a canonical artist/release/recording layer
- resolver/review machinery
- wallet identity machinery
- search/quality/auth operational layers

That is exactly why the v1 review needs to separate "must preserve source
truth" from "nice to have later" and from "premature canonical graph design."
