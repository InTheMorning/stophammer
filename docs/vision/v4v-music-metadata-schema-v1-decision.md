# Definitive v1 Schema Decision

This file is the Phase 3 decision artifact for the v4v music metadata schema.

It replaces open-ended planning with an implementation-ready answer for the
first source-first music schema cut.

## 1. Rollout Decision

The v1 music schema is rebuild-first.

That means:

- rebuild the music metadata database around the approved v1 table set
- do not attempt to preserve every resolver-era or canonical table in place
- keep non-database persistent assets such as signing keys out of scope

The rebuild should preserve only the source-truth model and the operational
tables still required by the runtime.

## 2. Approved Table Buckets

### Preserve

| Table | Decision | Justification |
| --- | --- | --- |
| `schema_migrations` | keep | required migration bookkeeping |
| `events` | keep | signed mutation log remains the replication backbone |
| `feed_crawl_cache` | keep | operational crawler dedupe |
| `node_sync_state` | keep | sync cursor memory |
| `peer_nodes` | keep | peer registry for federation/sync |
| `feeds` | keep and reshape | release-shaped source object for v1 reads |
| `tracks` | keep and reshape | track-shaped source object for v1 reads |
| `feed_payment_routes` | keep | feed-level V4V routes are core source data |
| `payment_routes` | keep and simplify | track-level V4V routes are core source data |
| `value_time_splits` | keep | source-authored time-split value data |
| `feed_remote_items_raw` | keep | raw remote item evidence |
| `source_contributor_claims` | keep and simplify | contributor/source-credit evidence |
| `source_entity_ids` | keep | direct search by claimed external ID |
| `source_entity_links` | keep | direct search by claimed URL/link |
| `source_release_claims` | keep | flexible home for release-ish source facts |
| `source_item_enclosures` | keep | preserves primary and alternate media facts |

### Drop in v1 Rebuild

| Table | Decision | Why |
| --- | --- | --- |
| `resolver_queue` | drop | resolver runtime is retired |
| `resolver_state` | drop | resolver runtime is retired |
| `artist_identity_override` | drop | manual canonical merge review is out of v1 scope |
| `artist_identity_review` | drop | manual canonical merge review is out of v1 scope |
| `resolved_external_ids_by_feed` | drop | cache tied to canonical resolver behavior |
| `resolved_entity_sources_by_feed` | drop | cache tied to canonical resolver behavior |
| `artists` | drop | no first-class artist entity in minimum v1 |
| `artist_aliases` | drop | depends on canonical artist entity |
| `artist_credit` | drop | canonical credit layer is out of v1 |
| `artist_credit_name` | drop | canonical credit layer is out of v1 |
| `releases` | drop | feeds are the release-shaped rows in v1 |
| `recordings` | drop | tracks are the track-shaped rows in v1 |
| `release_recordings` | drop | depends on canonical release/recording layer |
| `source_feed_release_map` | drop | depends on canonical release layer |
| `source_item_recording_map` | drop | depends on canonical recording layer |
| `artist_artist_rel` | drop | canonical artist graph is deferred |
| `artist_id_redirect` | drop | canonical artist graph is deferred |
| `track_rel` | drop | derived graph layer, not source truth |
| `feed_rel` | drop | derived graph layer, not source truth |
| `tags` | drop | normalized tag graph is out of minimum v1 |
| `artist_tag` | drop | normalized tag graph is out of minimum v1 |
| `feed_tag` | drop | normalized tag graph is out of minimum v1 |
| `track_tag` | drop | normalized tag graph is out of minimum v1 |
| `external_ids` | drop | promoted canonical IDs are out of v1 |
| `entity_source` | drop | promoted canonical provenance is out of v1 |
| `search_index` | drop | search must be rebuilt for the new schema later |
| `search_entities` | drop | search must be rebuilt for the new schema later |
| `entity_quality` | drop | quality scoring should not constrain source truth |
| `entity_field_status` | drop | quality scoring should not constrain source truth |

### Defer

| Table | Decision | Why |
| --- | --- | --- |
| `proof_challenges` | defer | not part of the minimum music schema |
| `proof_tokens` | defer | not part of the minimum music schema |
| `live_events` | defer | useful but not required for the first music-first cut |
| `source_platform_claims` | defer | derived platform cache, not primary source truth |
| all wallet tables | defer | separate identity/payment normalization problem |
| `artist_type`, `rel_type` | defer | only needed by deferred canonical layers |

## 3. Column Decisions

### `feeds`

| Column | Decision | Reason |
| --- | --- | --- |
| `feed_guid` | keep | source publication identity |
| `feed_url` | keep | fetch target and practical external reference |
| `title` | keep | release title in v1 semantics |
| `description` | keep | source description |
| `image_url` | keep | release artwork URL |
| `publisher` | add | publisher search/display without overloading artist identity |
| `language` | keep | feed language |
| `explicit` | keep | feed explicit flag |
| `raw_medium` | keep | exact source `podcast:medium` |
| `release_artist` | add | strict release-artist text |
| `release_artist_sort` | add | optional published sort form |
| `release_date` | add | feed `pubDate` in v1 semantics |
| `release_kind` | add | strict release type, default `unknown` |
| `created_at` | keep | operational bookkeeping |
| `updated_at` | keep | operational bookkeeping |
| `title_lower` | drop | search convenience, not source truth |
| `artist_credit_id` | drop | canonical credit coupling |
| `itunes_type` | drop | can stay as raw source claim if needed |
| `episode_count` | drop | denormalized cache field |
| `newest_item_at` | drop | denormalized cache field |
| `oldest_item_at` | drop | denormalized cache field |

### `tracks`

| Column | Decision | Reason |
| --- | --- | --- |
| `track_guid` | keep | source publication identity |
| `feed_guid` | keep | parent release/feed link |
| `title` | keep | track title in v1 semantics |
| `pub_date` | keep | chronology and fallback ordering |
| `duration_secs` | keep | core track field when published |
| `enclosure_url` | keep | primary media URL |
| `enclosure_type` | keep | primary media type |
| `enclosure_bytes` | keep | primary media size when published |
| `track_number` | keep | published sequencing when present |
| `image_url` | add | track artwork URL |
| `publisher` | add | item-level publisher search/display using source-first truth |
| `language` | add | track language, inheriting from feed when missing |
| `explicit` | keep | track explicit flag, inheriting from feed when missing |
| `description` | keep | source item description |
| `track_artist` | add | strict track-artist text |
| `track_artist_sort` | add | optional published sort form |
| `created_at` | keep | operational bookkeeping |
| `updated_at` | keep | operational bookkeeping |
| `artist_credit_id` | drop | canonical credit coupling |
| `title_lower` | drop | search convenience |
| `season` | drop | podcast framing outside minimum music schema |

### Source-fact and V4V simplifications

| Table/Column | Decision | Reason |
| --- | --- | --- |
| `payment_routes.feed_guid` | drop | derivable through `tracks.feed_guid` |
| `source_contributor_claims.role_norm` | drop | derived normalization, not source truth |
| integer surrogate IDs on source-fact child tables | optional/drop from logical model | composite natural keys are the approved identity model |
| `source`, `extraction_path`, `observed_at` on source-fact tables | keep | provenance is core v1 behavior |

## 4. Semantic Rules

- `feeds.title` = release title
- `tracks.title` = track title
- feed-level `itunes:author` = `feeds.release_artist`
- `tracks.track_artist` stays separate even when it defaults from
  `feeds.release_artist`
- `podcast:person` remains contributor evidence in
  `source_contributor_claims`
- `publisher` means publisher everywhere by default
- `tracks.publisher` exists for item-level publisher search/display and
  inherits the resolved feed publisher in v1
- Wavlake is the only approved narrow exception where current publisher data
  may also supply artist text
- artist strings are display metadata, not identity
- artist sort fields stay null unless explicitly published
- `feeds.release_date` = feed `pubDate`
- artwork is stored as feed/item URL metadata
- links and external IDs remain searchable by claimed values such as `npub`
- ordering follows RSS order or `pub_date`
- track `language` and `explicit` inherit from feed values when missing
- unknown values stay `null` or `unknown`

## 5. Identity Policy

- `feed_guid` and `track_guid` are source publication identities
- `events.event_id` is operational and not music identity
- no `artist_id`, `release_id`, `recording_id`, `work_id`, or
  `artist_credit_id` in minimum v1
- a future `artists` table is blocked on direct artist claim / feed-link
  workflow
- any future cross-source canonical identity layer requires a new ADR

## 6. API and Docs Follow-Up

The schema decision implies these follow-up changes:

- keep source-first feed and track read APIs
- keep source evidence APIs for contributors, links, IDs, release claims, and
  enclosures
- remove or defer canonical artist/release/recording routes and their docs
- remove canonical includes from feed/track responses
- rebuild search only after the new source-first schema is implemented
- document proof/auth separately if it survives outside the minimum music
  schema

## 7. Implementation Consequence

Phase 4 can now write the actual schema migration / rebuild plan against this
decision without reopening entity design.
