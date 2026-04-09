# Minimal v1 Schema Proposal By Justification

This file is a planning proposal for a stripped-down v1 schema after resolver
removal.

The proposal assumes:

- v1 is source-first, not canonical-first
- resolver tables and canonical merge machinery are gone
- the database must still ingest RSS music feeds, preserve source facts, and
  expose a minimal music-oriented read shape
- anything not required for that should be deferred

## 1. Internal DB Usage To Keep

These are the tables that still make sense even in a minimal source-first v1.

### Required

| Table | Keep? | Why |
| --- | --- | --- |
| `schema_migrations` | yes | required by the migration system even though it is not defined in `src/schema.sql` |
| `events` | yes | signed event log is still the system backbone for mutation history and replication |
| `feed_crawl_cache` | yes | cheap operational win for crawler dedupe |
| `node_sync_state` | yes | required if federation/sync remains part of v1 |
| `peer_nodes` | yes | required if federation/sync remains part of v1 |

### Defer

| Table | Keep? | Why not in the minimum |
| --- | --- | --- |
| `search_index`, `search_entities` | no | search can be rebuilt after the v1 schema is fixed; it should not drive source-of-truth design |
| `entity_quality`, `entity_field_status` | no | scoring is secondary to a clean source model |
| `proof_challenges`, `proof_tokens` | no for minimum | re-add only if proof-authenticated mutations are confirmed part of the first v1 cut |
| all wallet tables | no | these are a separate identity/problem space and should not constrain the minimum music schema |
| all resolver/review tables | no | explicitly out of scope after resolver retirement |
| lookup tables used only by deferred derived layers | no | keep out until a later phase proves they are needed |

## 2. Direct RSS / Source Fact Tables To Keep

This is the core of the proposal. Preserve source truth first, and keep the
tables close to what the feed actually said.

Title mapping policy for v1:

- feed title = release title
- item title = track title
- `feeds.title` is therefore the release-title field in practice
- `tracks.title` is therefore the track-title field in practice
- do not add duplicate `release_title` / `track_title` columns in minimum v1

Core field policy for v1:

- `release_date` = feed `pubDate`
- artwork is stored as URL fields at feed and track level
- `publisher` means publisher by default, except for the narrow Wavlake
  compatibility rule already noted
- enclosures are stored as published RSS enclosure data
- value blocks are mirrored from RSS as-is, including route types and
  arguments
- links and external IDs are stored so users can search by those values, such
  as `npub`
- ordering follows RSS ordering or `pubDate`
- `language` and `explicit` are stored per track, inheriting from the feed
  when missing
- unknown values remain `unknown`

### 2.1 Feeds

Keep a slimmed `feeds` table:

| Field | Why keep it |
| --- | --- |
| `feed_guid` | stable primary identity from RSS / Podcast Namespace |
| `feed_url` | fetch target and practical external identifier |
| `title` | source title; in v1 this is the release title |
| `description` | source description |
| `image_url` | source artwork URL |
| `publisher` | source publisher text used for publisher search and display |
| `language` | source language |
| `explicit` | source content flag |
| `raw_medium` | source `podcast:medium` exactly as published |
| `created_at` | local ingest bookkeeping |
| `updated_at` | local ingest bookkeeping |

Drop from the minimum:

- `title_lower`: internal search convenience, not source truth
- `artist_credit_id`: canonical/derived coupling
- `itunes_type`: can live in `source_release_claims`
- `episode_count`, `newest_item_at`, `oldest_item_at`: denormalized cache fields

### 2.2 Tracks

Keep a slimmed `tracks` table:

| Field | Why keep it |
| --- | --- |
| `track_guid` | stable item identity |
| `feed_guid` | parent release/feed link |
| `title` | source item title; in v1 this is the track title |
| `pub_date` | source chronology |
| `duration_secs` | core music field when present |
| `enclosure_url` | primary audio location |
| `enclosure_type` | media type |
| `enclosure_bytes` | byte size when published |
| `track_number` | music sequencing when present |
| `image_url` | source track artwork URL when published |
| `language` | source track language when published, otherwise inherited from feed |
| `explicit` | source content flag |
| `description` | source item description |
| `created_at` | local ingest bookkeeping |
| `updated_at` | local ingest bookkeeping |

Drop from the minimum:

- `title_lower`: search convenience
- `artist_credit_id`: canonical/derived coupling
- `season`: podcast framing, not part of the minimum music-first model

Track-field behavior in v1:

- `pub_date` is both source chronology and a valid ordering field
- track artwork stays as URL metadata when available
- track `language` and `explicit` are stored per track, inheriting from the
  feed when missing

### 2.3 Value-for-value source tables

Keep these as-is or very close to as-is:

| Table | Keep? | Why |
| --- | --- | --- |
| `feed_payment_routes` | yes | feed-level V4V routing is core Podcasting 2.0 music data |
| `payment_routes` | yes | track-level V4V routing is core Podcasting 2.0 music data |
| `value_time_splits` | yes | segment-level V4V routing is distinctive, source-authored music metadata |

One simplification:

- `payment_routes.feed_guid` can be dropped because it is derivable through
  `tracks.feed_guid`
- value rows should mirror RSS tag structure and arguments rather than
  reinterpreting them into a different ontology

### 2.4 Source evidence tables to keep

Keep these tables because they preserve facts without forcing canonical
conclusions:

| Table | Keep? | Why |
| --- | --- | --- |
| `feed_remote_items_raw` | yes | direct publication-time cross-feed evidence |
| `source_contributor_claims` | yes | raw people/role evidence must survive intact |
| `source_entity_ids` | yes | raw external identifiers must survive intact |
| `source_entity_links` | yes | raw URLs and typed links must survive intact |
| `source_release_claims` | yes | flexible home for release-ish claims without schema churn |
| `source_item_enclosures` | yes | preserves alternate enclosure variants without flattening |

Keep these columns in source evidence tables because they are provenance, not
noise:

- `source`
- `extraction_path`
- `observed_at`

Simplify where possible:

- `source_contributor_claims.role_norm` should be removed from source-of-truth
  storage; if useful later, it should be a derived cache
- `feed_remote_items_raw.source` can be implicit if the table exists only for
  remote-item extraction

Search-facing evidence rule:

- source links and external IDs should be stored in a way that allows direct
  search by claimed values such as `npub`

### 2.5 Source tables to defer from the minimum

| Table | Keep? | Why not in the minimum |
| --- | --- | --- |
| `live_events` | no for minimum | important, but not required for the first music-first schema cut |
| `source_platform_claims` | no for minimum | derived from URLs/owner patterns, not primary source truth |

## 3. Proposed Minimal Music-First Representation

The minimum should not reintroduce a full canonical graph. Instead, it should
offer a tiny music-facing layer that is easy to explain and easy to rebuild
from preserved source facts.

## 3.1 What to remove entirely

Remove these tables from the minimum v1 plan:

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
- all resolver/review tables
- all resolved-by-feed cache tables

Reason:

- all of these encode a stronger opinionated graph than the minimum v1 needs
- most of them exist to support canonicalization, merge review, or enrichment
- the source tables already preserve the evidence needed to revisit them later

## 3.2 What to add for a minimal music-first read shape

Instead of a canonical artist/release/recording graph, add a few explicit
display fields that support a usable music UI without claiming identity merges.

Current planning assumption:

- `podcast:person` is a rich contributor list, not the strict release-artist
  field for v1
- if the RSS / Podcasting 2.0 ecosystem wants a native artist-truth field in
  the future, it should allow an explicit `artist_credit` tag rather than
  overloading contributor metadata

Artist-field policy for v1:

- `release_artist` and `track_artist` are separate fields
- `release_artist` belongs on the feed/release-shaped row
- `track_artist` belongs on the track/item-shaped row
- feed-level `itunes:author` = `release_artist`
- `podcast:person` is preserved as contributor evidence and must not override
  `release_artist`
- `track_artist` may default to `release_artist` when no strict item-level
  artist field exists, but it remains a separate schema field
- artist sort order is separate optional metadata, not something to infer
  aggressively as truth
- if no reliable published sort form exists, artist sort fields should remain
  null
- any internal derived sort key used for indexing or UI ordering must remain a
  non-authoritative helper, separate from published metadata

Publisher policy for v1:

- `publisher` means publisher by default everywhere in the schema
- do not globally reinterpret publisher metadata as artist identity
- `tracks.publisher` exists for item-level publisher search/display and
  inherits the resolved feed publisher in v1
- Wavlake is a narrow platform-specific exception: current Wavlake feed-level
  and track-level publisher data may carry artist text useful for
  `release_artist` / `track_artist`
- even in that Wavlake exception, the publisher-derived artist string is still
  display text / source evidence, not a stable unique artist identifier
- if a stable platform-native Wavlake identifier exists, preserve it as source
  evidence separately from the artist text

Artist-entity policy for v1:

- minimum v1 does not include an `artists` table or `artist_id`
- artist text is stored on feed/track rows plus source evidence tables, not as
  a first-class resolved entity
- a future `artists` table should only be introduced when StopHammer has a
  direct artist claim / submission workflow
- feed ownership proof is the prerequisite building block for that later
  workflow
- future `artist_id` values must be created through explicit claim/link flows,
  not inferred from artist-name text

### Add to `feeds`

| Proposed field | Why |
| --- | --- |
| `release_artist` | explicit release-artist text for the feed/release-shaped row; feed-level `itunes:author` maps here |
| `release_artist_sort` | optional published sort form for the release artist; null when not explicitly available |
| `publisher` | explicit publisher text kept searchable as publisher, not overloaded as artist identity outside the narrow Wavlake compatibility rule |
| `release_date` | feed `pubDate` used as the release date in v1 |
| `release_kind` | optional strict release classification; should remain `unknown` unless a future namespace field or other explicitly approved source publishes it |

### Add to `tracks`

| Proposed field | Why |
| --- | --- |
| `track_artist` | explicit track-artist text kept separate from `release_artist`, even when it defaults to it in v1 |
| `track_artist_sort` | optional published sort form for the track artist; null when not explicitly available |
| `publisher` | explicit track-level publisher text for item-level publisher search/display; inherits feed publisher in v1 until a strict item-level namespace field exists |
| `image_url` | explicit track artwork URL when the item publishes art distinct from the feed |
| `language` | explicit track language field so item language can differ from and otherwise inherit from the feed |

Everything else needed for a minimal music-first read is already present:

- feed title: `feeds.title`
- feed art: `feeds.image_url`
- medium filter: `feeds.raw_medium`
- track title: `tracks.title`
- duration: `tracks.duration_secs`
- ordering: `tracks.track_number`
- audio URL: `tracks.enclosure_url`

## 3.3 Why this is enough for v1

This minimal representation supports:

- "show me music feeds"
- "show me the tracks in a feed"
- "show me who the publisher claims is the artist"
- "show me payment routes and remote-item links"
- "show me the raw evidence when claims conflict"

It intentionally does not attempt to answer:

- are two feeds the same canonical release?
- are two tracks the same canonical recording?
- should these two artist names be merged?
- what is the global artist graph?

Those are later-phase questions, not minimum-v1 questions.

## 4. Resulting Minimum Table Set

If we apply the proposal strictly, the minimum v1 schema is roughly:

### Internal operation

- `schema_migrations`
- `events`
- `feed_crawl_cache`
- `node_sync_state`
- `peer_nodes`

### Direct RSS / source preservation

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

### Minimal music-first fields

- `feeds.release_artist`
- `feeds.release_artist_sort`
- `feeds.publisher`
- `feeds.release_date`
- `feeds.release_kind`
- `tracks.track_artist`
- `tracks.track_artist_sort`
- `tracks.image_url`
- `tracks.language`

## 5. Identifier Policy

The v1 schema should distinguish source identities from operational identities.

### Source identities to use directly

- `feeds.feed_guid` is the source identity for the published feed
- `tracks.track_guid` is the source identity for the published item/track
- these are publication-object identities, not universal music identities

### Composite natural keys for source-fact child rows

The minimum schema should prefer composite source keys over synthetic IDs for
fact rows that do not have an independent life outside their parent object.

Examples:

- `feed_remote_items_raw`: `feed_guid` + `position`
- `feed_payment_routes`: `feed_guid` + ordinal/position
- `payment_routes`: `track_guid` + ordinal/position
- `value_time_splits`: `track_guid` + ordinal/position
- `source_contributor_claims`: `feed_guid` + `entity_type` + `entity_id` +
  `position` + `source`
- `source_entity_ids`: `feed_guid` + `entity_type` + `entity_id` + `scheme` +
  `value`
- `source_entity_links`: `feed_guid` + `entity_type` + `entity_id` +
  `link_type` + `url`
- `source_release_claims`: `feed_guid` + `entity_type` + `entity_id` +
  `claim_type` + `position`
- `source_item_enclosures`: `feed_guid` + `entity_type` + `entity_id` +
  `position` + `url`

### Operational identities

- `events.event_id` is an operational mutation-log identifier
- `event_id` is not music metadata and does not identify a feed, track,
  release, artist, or work
- keep operational IDs only where the system needs them for replication,
  mutation tracking, or authorization

### IDs to defer until those concepts actually exist

Do not introduce these IDs in minimum v1 unless their entities become
first-class schema concepts:

- `release_id`
- `recording_id`
- `work_id`
- `artist_id`
- `artist_credit_id`
- `audio_asset_id`

## Planning Stance

The minimum v1 should be:

- source-preserving
- music-readable
- non-canonical
- easy to migrate forward later

That means fewer tables, fewer hidden opinions, and a hard separation between
"what the feed said" and "what we might infer later."

Rollout assumption for Phase 3:

- the v1 schema should be finalized with a rebuild-first rollout in mind
- the plan should not promise an in-place transformation of every
  resolver-era or canonical table
- preserved source facts should define what gets rebuilt into the new schema
- resolver-era derived tables should be treated as disposable unless the Phase
  3 decision artifact explicitly keeps them

## Namespace Note

The current plan should explicitly record this namespace gap:

- release-artist truth should not be forced onto `podcast:person`
- `podcast:person` remains contributor metadata
- a future RSS / Podcasting 2.0 extension should allow an `artist_credit` tag
  for strict music-first artist attribution
- the current namespace has `medium=music` but no formal `kind` /
  `release_kind` field for album/EP/single classification
- v1 should therefore keep `release_kind` conservative and push for a future
  namespace field rather than inventing aggressive heuristics
- `publisher` should retain its normal meaning in the namespace; platform
  behaviors like Wavlake's current publisher-as-artist text should be treated
  as narrow compatibility exceptions, not generalized namespace semantics

Unknown/default rule:

- when a value is not explicitly present or safely derivable under the v1
  rules, keep it `null` or `unknown` rather than inventing a guess
