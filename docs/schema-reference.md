# Schema Reference

Source: `src/schema.sql`

---

## Lookup tables

### `artist_type`
**Purpose:** Enumerates the kinds of artist entity (person, group, orchestra, choir, character, other).
**Key columns:** `id` (integer PK), `name` (unique label such as `person` or `group`).
**Notes:** Seeded at schema creation. Referenced by `artists.type_id`.

### `rel_type`
**Purpose:** Defines every relationship kind in the system (performer, songwriter, member_of, cover_art, label, etc.).
**Key columns:** `id` (integer PK), `name` (unique machine name), `entity_pair` (which entity combination this type applies to, e.g. `artist-track`, `artist-artist`, `artist-feed`).
**Notes:** Seeded with 35 rows. Used as FK by `artist_artist_rel`, `track_rel`, and `feed_rel`.

---

## Core entities

### `artists`
**Purpose:** Canonical record for every artist known to the index.
**Key columns:** `artist_id` (text PK, typically a UUID), `name` / `name_lower` (display and search names), `type_id` (FK to `artist_type`).
**Notes:** `name_lower` is indexed for case-insensitive lookup. Related tables: `artist_aliases`, `artist_credit_name`, `artist_location`, `artist_artist_rel`, `artist_tag`, `external_ids`. See ADR 0014 for alias resolution rules.

### `artist_aliases`
**Purpose:** Alternate names for an artist used during ingest-time resolution.
**Key columns:** `alias_lower` (lowercased alias text), `artist_id` (FK to `artists`).
**Notes:** Composite PK `(alias_lower, artist_id)`. One artist may have many aliases; one alias may map to multiple artists. See ADR 0014.

### `artist_credit`
**Purpose:** A MusicBrainz-style artist credit representing one or more artists combined into a display string (e.g. "Artist A feat. Artist B").
**Key columns:** `id` (integer PK), `display_name` (the fully rendered credit string).
**Notes:** Referenced by `feeds.artist_credit_id` and `tracks.artist_credit_id`. The individual artist contributions are stored in `artist_credit_name`.

### `artist_credit_name`
**Purpose:** Junction table linking individual artists to positions within an artist credit.
**Key columns:** `artist_credit_id` (FK to `artist_credit`), `artist_id` (FK to `artists`), `position` (ordering), `join_phrase` (text like " feat. " that glues names together).
**Notes:** Unique on `(artist_credit_id, position)`. Indexed on both `artist_credit_id` and `artist_id` for reverse lookups.

### `feeds`
**Purpose:** A podcast/music feed (album, EP, single, etc.) identified by its `podcast:guid`.
**Key columns:** `feed_guid` (text PK), `feed_url` (unique RSS URL), `title` / `title_lower`, `artist_credit_id` (FK to `artist_credit`).
**Notes:** `episode_count`, `newest_item_at`, `oldest_item_at` are denormalized counters/dates maintained during ingest. `raw_medium` stores the unprocessed `<podcast:medium>` value from RSS. Indexed on `newest_item_at DESC` for "recently updated" queries.

### `tracks`
**Purpose:** An individual item (song / episode) within a feed.
**Key columns:** `track_guid` (text PK), `feed_guid` (FK to `feeds`), `artist_credit_id` (FK to `artist_credit`), `enclosure_url` (audio file location).
**Notes:** Indexed on `feed_guid`, `pub_date DESC`, and `title_lower`. Payment information lives in `payment_routes` and `value_time_splits`.

### `payment_routes`
**Purpose:** Track-level `<podcast:value>` payment split destinations.
**Key columns:** `track_guid` (FK to `tracks`), `route_type` (e.g. `node`, `lnaddress`), `address` (Lightning address or node pubkey), `split` (proportional share).
**Notes:** Also carries `custom_key` / `custom_value` for TLV record routing. `fee` flag indicates whether the split is a fee deducted before proportional splitting. Indexed on both `track_guid` and `feed_guid`.

### `feed_payment_routes`
**Purpose:** Feed-level payment split destinations, applied when a track has no track-level routes.
**Key columns:** `feed_guid` (FK to `feeds`), `route_type`, `address`, `split`.
**Notes:** Same structure as `payment_routes` but scoped to the feed. Indexed on `feed_guid`.

### `value_time_splits`
**Purpose:** Time-ranged payment splits that override the default route during a segment of a track (the `<podcast:valueTimeSplit>` tag).
**Key columns:** `source_track_guid` (FK to `tracks`), `start_time_secs` / `duration_secs` (the segment window), `remote_feed_guid` / `remote_item_guid` (the destination item receiving the split).
**Notes:** Indexed on `source_track_guid`. Used to credit sampled or guest content within a track.

---

## Events & sync

### `events`
**Purpose:** Append-only signed event log that records every mutation and replicates across nodes.
**Key columns:** `event_id` (text PK), `event_type` (e.g. `FeedUpserted`, `TrackRemoved`), `payload_json` (full event payload), `seq` (monotonically increasing sequence number for sync cursors).
**Notes:** `signed_by` and `signature` authenticate the originating node. `warnings_json` captures non-fatal issues encountered during ingest. Indexed on `seq`, `subject_guid`, `event_type`, and `created_at DESC`. Central to ADR 0004 (signed event log) and ADR 0016 (push-gossip replication).

### `feed_crawl_cache`
**Purpose:** Deduplication cache for the RSS crawler -- avoids re-processing a feed whose content has not changed.
**Key columns:** `feed_url` (text PK), `content_hash` (hash of the last-seen feed body), `crawled_at` (epoch timestamp).
**Notes:** See ADR 0011 (RSS crawler).

### `feed_remote_items_raw`
**Purpose:** Raw feed-level `podcast:remoteItem` references discovered during ingest.
**Key columns:** `feed_guid` (FK to `feeds`), `position` (stable order within the feed), `remote_feed_guid`.
**Notes:** This is staged source data, not yet a canonical relationship table. `remote_feed_url` preserves the optional source URL, and `medium` preserves the publisher-provided medium hint.

### `live_events`
**Purpose:** Current `pending` / `live` snapshot of `<podcast:liveItem>` rows for each feed.
**Key columns:** `live_item_guid` (text PK), `feed_guid` (FK to `feeds`), `status`, `scheduled_start`, `scheduled_end`.
**Notes:** Replaced on each ingest via `LiveEventsReplaced`. When a live item transitions to `ended` and has an enclosure, it is removed from this table and promoted into the permanent `tracks` table.

### `source_contributor_claims`
**Purpose:** Staged source-level contributor claims, intended for raw `podcast:person`-style data and other contributor evidence before canonical normalization.
**Key columns:** `feed_guid` (owning feed snapshot), `entity_type` / `entity_id` (which feed, track, or live item the claim belongs to), `position`, `name`.
**Notes:** This is a replicated source-claim table, not a canonical credit table. `role` preserves the published text verbatim, while `role_norm` stores a lowercase, whitespace-normalized copy for grouping and analysis. `group_name`, `href`, `img`, `source`, and `extraction_path` preserve source provenance.

### `source_entity_ids`
**Purpose:** Staged source-level identity claims such as Nostr IDs, MusicBrainz IDs, ISRCs, and platform-native identifiers.
**Key columns:** `feed_guid` (owning feed snapshot), `entity_type` / `entity_id`, `scheme`, `value`.
**Notes:** This is distinct from `external_ids`: `source_entity_ids` stores raw evidence from source feeds, while `external_ids` remains canonical-facing.

### `source_entity_links`
**Purpose:** Staged source-level links such as artist websites, release pages, self-feed URLs, and live content stream links.
**Key columns:** `feed_guid` (owning feed snapshot), `entity_type` / `entity_id`, `position`, `link_type`, `url`.
**Notes:** Replaced as a feed-scoped snapshot via `SourceEntityLinksReplaced`. This table stores typed source evidence, not canonical external links.

### `source_release_claims`
**Purpose:** Staged source-level release facts derived from feed, track, and live-item metadata before canonical release rows exist.
**Key columns:** `feed_guid` (owning feed snapshot), `entity_type` / `entity_id`, `claim_type`, `claim_value`.
**Notes:** Replaced as a feed-scoped snapshot via `SourceReleaseClaimsReplaced`. Current claim families include release-date-like timestamps plus descriptive metadata such as description, language, image URL, medium, and iTunes type where available.

### `source_item_enclosures`
**Purpose:** Staged source-level media variants for tracks and live items, including the primary enclosure plus any `podcast:alternateEnclosure` rows.
**Key columns:** `feed_guid` (owning feed snapshot), `entity_type` / `entity_id`, `position`, `url`, `is_primary`.
**Notes:** Replaced as a feed-scoped snapshot via `SourceItemEnclosuresReplaced`. This table is intentionally source-oriented: it preserves per-platform media URLs, MIME types, byte lengths, relation hints, titles, and extraction paths so a future canonical release/recording layer can present all platform media options without flattening them into the single `tracks.enclosure_url` field.

### `source_platform_claims`
**Purpose:** Staged feed-level platform provenance such as `wavlake`, `fountain`, or `rss_blue`, derived from canonical feed URLs, typed source links, and platform-style owner names.
**Key columns:** `feed_guid` (owning feed snapshot), `platform_key`, `url`, `owner_name`.
**Notes:** Replaced as a feed-scoped snapshot via `SourcePlatformClaimsReplaced`. This table is intentionally evidence-oriented: it records why StopHammer believes a feed belongs to a given platform, without conflating that with canonical artist identity.

### `releases`
**Purpose:** Deterministic canonical release layer built above source feeds.
**Key columns:** `release_id` (text PK), `artist_credit_id` (FK to `artist_credit`), `title`, `release_date`.
**Notes:** Current policy is intentionally conservative: one canonical `release` is derived from one source feed. This gives the system a stable canonical anchor without prematurely merging mirrored feeds across platforms.

### `recordings`
**Purpose:** Deterministic canonical recording layer built above source tracks.
**Key columns:** `recording_id` (text PK), `artist_credit_id` (FK to `artist_credit`), `title`, `duration_secs`.
**Notes:** Current policy is also conservative here: one canonical `recording` is derived from one source track. This preserves a clean upgrade path toward cross-feed recording clustering later.

### `release_recordings`
**Purpose:** Ordered tracklist rows connecting canonical `releases` to canonical `recordings`.
**Key columns:** `release_id`, `recording_id`, `position`, `source_track_guid`.
**Notes:** Rebuilt deterministically from the current tracks in a feed. Ordering currently prefers `track_number`, then publication timestamp, then title/GUID as a stable fallback.

### `source_feed_release_map`
**Purpose:** Explicit mapping from a source feed (`feeds`) to its current canonical `release`.
**Key columns:** `feed_guid` (PK/FK to `feeds`), `release_id`, `match_type`, `confidence`.
**Notes:** Today this is always an identity mapping with `match_type = 'feed_guid_identity'` and `confidence = 100`. The table exists so future clustering can replace identity mappings with real multi-platform release resolution.

### `source_item_recording_map`
**Purpose:** Explicit mapping from a source track (`tracks`) to its current canonical `recording`.
**Key columns:** `track_guid` (PK/FK to `tracks`), `recording_id`, `match_type`, `confidence`.
**Notes:** Today this is always an identity mapping with `match_type = 'track_guid_identity'` and `confidence = 100`. It is intentionally additive so later recording dedupe can swap in stronger mappings without losing source rows.

### `node_sync_state`
**Purpose:** Tracks the last event sequence number received from each peer node.
**Key columns:** `node_pubkey` (text PK, the peer's identity), `last_seq` (highest seq received from that peer).
**Notes:** Used by the push-gossip protocol (ADR 0016) to request only new events from each peer.

### `peer_nodes`
**Purpose:** Registry of known peer nodes in the gossip network.
**Key columns:** `node_pubkey` (text PK), `node_url` (reachable endpoint), `consecutive_failures` (tracks unreachable peers for back-off).
**Notes:** `last_push_at` records the last successful push to that peer. See ADR 0016 and ADR 0009 (community node mode).

---

## Relationships & metadata

### `artist_artist_rel`
**Purpose:** Directed relationships between two artists (member_of, collaboration, booking, management).
**Key columns:** `artist_id_a` / `artist_id_b` (FKs to `artists`), `rel_type_id` (FK to `rel_type`).
**Notes:** `begin_year` / `end_year` scope the relationship in time. Indexed on both artist columns.

### `artist_id_redirect`
**Purpose:** Redirect table for merged artists -- maps a retired artist ID to its canonical replacement.
**Key columns:** `old_artist_id` (text PK), `new_artist_id` (FK to `artists`).
**Notes:** `merged_at` records when the merge happened. Clients and internal lookups should follow redirects.

### `track_rel`
**Purpose:** Relationships between two tracks (e.g. remix_of, cover_of, featuring).
**Key columns:** `track_guid_a` / `track_guid_b` (FKs to `tracks`), `rel_type_id` (FK to `rel_type`).
**Notes:** Indexed on both track columns.

### `feed_rel`
**Purpose:** Relationships between two feeds (e.g. companion releases, deluxe editions).
**Key columns:** `feed_guid_a` / `feed_guid_b` (FKs to `feeds`), `rel_type_id` (FK to `rel_type`).
**Notes:** Indexed on both feed columns.

---

## Tags

### `tags`
**Purpose:** Normalized tag/genre dictionary.
**Key columns:** `id` (integer PK), `name` (unique tag string).
**Notes:** Created as tags are encountered during ingest.

### `artist_tag`
**Purpose:** Many-to-many join between artists and tags.
**Key columns:** `artist_id` (FK to `artists`), `tag_id` (FK to `tags`).
**Notes:** Composite PK `(artist_id, tag_id)`.

### `feed_tag`
**Purpose:** Many-to-many join between feeds and tags.
**Key columns:** `feed_guid` (FK to `feeds`), `tag_id` (FK to `tags`).
**Notes:** Composite PK `(feed_guid, tag_id)`.

### `track_tag`
**Purpose:** Many-to-many join between tracks and tags.
**Key columns:** `track_guid` (FK to `tracks`), `tag_id` (FK to `tags`).
**Notes:** Composite PK `(track_guid, tag_id)`.

---

## External IDs & provenance

### `external_ids`
**Purpose:** Maps entities to identifiers in external systems (MusicBrainz, ISRC, UPC, Spotify, etc.).
**Key columns:** `entity_type` / `entity_id` (polymorphic reference to any entity), `scheme` (identifier namespace, e.g. `musicbrainz`, `isrc`), `value` (the external identifier).
**Notes:** Unique on `(entity_type, entity_id, scheme)`. Indexed for both entity lookup and reverse lookup by scheme + value.

### `entity_source`
**Purpose:** Records where an entity's data came from and how much to trust it.
**Key columns:** `entity_type` / `entity_id` (polymorphic reference), `source_type` (e.g. `rss_crawl`, `bulk_import`, `manual`), `trust_level` (integer ranking).
**Notes:** See ADR 0006 (crawlers as untrusted clients) and ADR 0005 (pluggable verifier chain) for how trust levels influence acceptance.

---

## Quality scoring

### `entity_quality`
**Purpose:** Stores a computed completeness/quality score for any entity.
**Key columns:** `entity_type` / `entity_id` (composite PK), `score` (integer 0-100), `computed_at` (epoch timestamp of last computation).
**Notes:** Scores are recomputed periodically and used for ranking in search results and API responses.

### `entity_field_status`
**Purpose:** Per-field status tracking for entity completeness (which fields are present, missing, or invalid).
**Key columns:** `entity_type` / `entity_id` / `field_name` (composite PK), `status` (default `present`).
**Notes:** Companion to `entity_quality` -- provides the breakdown behind the aggregate score.

---

## Search

### `search_index`
**Purpose:** Contentless FTS5 index storing inverted search terms across all entity types.
**Key columns:** `entity_type`, `entity_id`, `name`, `title`, `description`, `tags`.
**Notes:** Because the virtual table is contentless (`content=''`), column values are not read back directly from it.

### `search_entities`
**Purpose:** Rowid-to-entity mapping companion for the contentless `search_index` table.
**Key columns:** `rowid` (matches the FTS5 rowid), `entity_type`, `entity_id`.
**Notes:** Query code joins `search_index` to `search_entities` to recover the concrete entity behind each FTS match.

---

## Proof of Possession

### `proof_challenges`
**Purpose:** Pending and resolved proof-of-possession challenges used to authorize feed-scoped mutations without an account system.
**Key columns:** `challenge_id` (text PK), `feed_guid`, `scope`, `token_binding`, `state`, `expires_at`.
**Notes:** `state` is one of `pending`, `valid`, or `invalid`. Indexed by `(feed_guid, state)` and `expires_at` for rate limiting and cleanup.

### `proof_tokens`
**Purpose:** Short-lived bearer tokens issued after a successful proof assertion.
**Key columns:** `access_token` (text PK), `scope`, `subject_feed_guid`, `expires_at`.
**Notes:** Used by the proof-authenticated mutation path documented in ADR 0018 and `docs/API.md`.
