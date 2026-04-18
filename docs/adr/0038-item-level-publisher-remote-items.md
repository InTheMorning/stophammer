# 0038: Publisher-Feed Handling — Item-Level `remoteItem` Extraction and Non-Music Listing Filter

- Status: Proposed
- Date: 2026-04-18

## Context

Stophammer ingests three `podcast:medium` values that show up in real feeds:
`music`, `publisher`, and `musicL`. Only `music` feeds carry tracks that
clients want for music discovery. `publisher` feeds are container/aggregator
feeds whose job is to declare cross-feed relationships via
`podcast:remoteItem`; `musicL` feeds are playlist containers. Both are kept
in the store because the verifier chain and the derived publisher view need
them to build the cross-feed graph, but they are not music feeds.

Two gaps in current behavior motivate this ADR:

### Gap 1 — item-level `remoteItem` is dropped

`stophammer-parser::engine::extract_feed_remote_items` walks direct children
of `<channel>` and persists them into `feed_remote_items_raw` with
`position`, `medium`, `remote_feed_guid`, and `remote_feed_url`. The derived
`publisher` view on `GET /v1/feeds/{guid}` reads that table to report
`music_to_publisher` / `publisher_to_music` direction and
`reciprocal_declared` / `two_way_validated` flags.

The Podcast Namespace spec also allows `podcast:remoteItem` at the `<item>`
level. Today, Stophammer only parses item-level `<podcast:remoteItem>` when
it appears as a child of `<podcast:valueTimeSplit>` (see
`extract_value_time_splits` in `stophammer-parser/src/engine.rs:594`).
Direct item-level `<podcast:remoteItem medium="publisher">` declarations —
used by some publishers to attach per-track publisher context distinct from
the channel-level declaration — are silently dropped by the parser. That
means per-track publisher links never reach the ingest transaction and
cannot participate in reciprocal validation.

### Gap 2 — publisher and musicL feeds leak into public listings

The read layer does not filter by medium. Publisher feed
`4dfd6ec2-2849-568f-ba60-c5f67417afbf` was observed being returned as a
regular feed. Confirmed leak points:

| Endpoint | Current filter | Problem |
|---|---|---|
| `GET /v1/feeds/{guid}` (`src/query.rs:~478`) | none | acceptable — direct lookup should stay permissive |
| `GET /v1/feeds/recent` (`src/query.rs:~1232`) | `lower(raw_medium) = lower(?1)`, defaults to `music` | overridable via `?medium=` query param |
| `GET /v1/search` (`src/search.rs:~329`) | `entity_type='feed'` only | publisher / musicL / null mediums appear in results |
| `GET /v1/publishers/{publisher}` (`src/query.rs:~1788`, `~1889`) | `publisher_text` only | non-music feeds counted and listed |
| `populate_search_index` in `sync_source_read_models_for_feed` (`src/db.rs:~1190`) | indexes every feed | indexes non-music feeds |

## Decisions

### Decision A — extract item-level `podcast:remoteItem`

Extract item-level `podcast:remoteItem` declarations that appear as direct
children of `<item>` (not inside `valueTimeSplit`) and persist them as
source-truth evidence attached to the track.

#### Parser

`stophammer-parser`:

- Add `remote_items: Vec<IngestRemoteFeedRef>` to `IngestTrackData`
  (`stophammer-parser/src/types.rs`).
- Add `extract_item_remote_items(item: &roxmltree::Node)` in
  `stophammer-parser/src/engine.rs`. It mirrors
  `extract_feed_remote_items`, skipping any `remoteItem` whose ancestor is a
  `valueTimeSplit` so the existing VTS path remains the single source for
  value-split remote references.
- Wire the new extractor into the track-building loop next to `persons`,
  `entity_ids`, `links`.

#### Wire types

`stophammer/src/ingest.rs`:

- Add `#[serde(default)] pub remote_items: Vec<IngestRemoteFeedRef>` to
  `IngestTrackData` so older crawlers that omit the field continue to
  validate.

#### Storage

Add `track_remote_items_raw`, parallel to `feed_remote_items_raw`:

```sql
CREATE TABLE IF NOT EXISTS track_remote_items_raw (
    id               INTEGER PRIMARY KEY,
    track_guid       TEXT NOT NULL REFERENCES tracks(track_guid),
    position         INTEGER NOT NULL,
    medium           TEXT,
    remote_feed_guid TEXT NOT NULL,
    remote_feed_url  TEXT,
    source           TEXT NOT NULL DEFAULT 'podcast_remote_item',
    UNIQUE(track_guid, position)
) STRICT;

CREATE INDEX idx_track_remote_items_track ON track_remote_items_raw(track_guid);
CREATE INDEX idx_track_remote_items_guid  ON track_remote_items_raw(remote_feed_guid);
```

Ingest replaces the set atomically per track, matching the
`*_replaced` snapshot pattern used elsewhere (see
`feed_remote_items_replaced` event). Introduce a new event type
`track_remote_items_replaced` with `subject_guid = track_guid`.

#### API surface

`GET /v1/tracks/{guid}`:

- add `remote_items` as a new opt-in value for the `include` parameter
- include the derived `publisher` view (same shape as the feed-level variant)
  when the include list contains `publisher`, fed by the track's own
  declarations

The feed-level `publisher` view is unchanged. An item-level publisher link
supplements, never overrides, the feed-level one in v1.

#### Publisher text inheritance (ADR 0035 interaction)

The stored `tracks.publisher` text still inherits from the parent feed's
resolved publisher text. Item-level remote items are recorded as *evidence*;
they do not replace the stored string. A future ADR may use item-level
reciprocal declarations to override `tracks.publisher`, but that decision is
deferred so item-level extraction can ship without re-opening the v1
publisher-text policy (Wavlake exception, strict reciprocal rule for
non-Wavlake feeds).

### Decision B — whitelist music-only for public listings and search

Public list and search endpoints return only feeds with `raw_medium = 'music'`.
Publisher, musicL, null, and any unknown medium are excluded. Direct GUID
lookup (`GET /v1/feeds/{guid}`) continues to return any feed regardless of
medium so callers can still inspect containers and act on the `raw_medium`
field in the response.

No new "list publisher feeds" endpoint is added in this ADR. Publisher feeds
remain reachable via direct GUID lookup and via the `publisher` derived view
on music feeds. A future ADR can introduce a dedicated container listing if
a real consumer appears.

#### Read-layer changes

- `GET /v1/feeds/recent` (`src/query.rs`): drop the `medium` query param from
  `RecentFeedsParams`; hard-code `WHERE raw_medium = 'music'`. Update
  `src/openapi.rs` to remove the parameter.
- `GET /v1/search` (`src/search.rs`): add an `INNER JOIN feeds` with
  `feeds.raw_medium = 'music'` on rows where `entity_type = 'feed'`. Track
  search rows are unaffected; they already only exist for music tracks.
- `GET /v1/publishers/{publisher}` (`src/query.rs`): add
  `AND raw_medium = 'music'` to the feeds sub-query and to the aggregate
  `feed_count` / `track_count` query on `GET /v1/publishers` so the
  aggregate matches the detail listing.
- `GET /v1/feeds/{guid}`: **no change**. Permissive direct lookup is the
  documented escape hatch for inspecting non-music feeds.

#### Search-index population

`sync_source_read_models_for_feed` in `src/db.rs` (the `populate_search_index`
call on the feed-entity branch): skip insertion when `raw_medium != 'music'`,
and delete any pre-existing `entity_type='feed'` row for that GUID so the
index stays coherent if a feed's medium changes between ingests. Reuse
`medium::is_music` from `src/medium.rs` rather than inline string matching.

#### Tests

- **Search**: ingest one music feed + one publisher feed with matching
  title; assert `GET /v1/search?q=<term>` returns only the music feed.
- **Recent**: ingest both mediums; assert `GET /v1/feeds/recent` returns
  only the music feed.
- **Publisher detail**: ingest two feeds with the same `publisher_text` —
  one music, one publisher — assert `GET /v1/publishers/{publisher}` lists
  only the music feed.
- **Direct lookup preserved**: `GET /v1/feeds/{publisher_guid}` still
  returns 200 with `raw_medium='publisher'`.
- **Re-index coherence**: ingest as music, re-ingest as publisher, call
  `sync_source_read_models_for_feed`, assert the feed-entity row is gone
  from `search_index`.

#### Docs

- `docs/API.md`: note on `/v1/feeds/recent`, `/v1/search`, and
  `/v1/publishers/{publisher}` that results are whitelisted to
  `raw_medium='music'`. Remove the `medium` query param from the
  `/v1/feeds/recent` entry. Leave the `/v1/feeds/{guid}` note about
  `raw_medium` being returned as-is.
- `docs/schema-reference.md`: record the policy so future query work
  defaults to the same whitelist.
- Regenerate static OpenAPI previews via `cargo run --bin gen_openapi` if
  they are checked in.

## Consequences

- Item-level publisher declarations are preserved as source-truth and become
  queryable and replicable.
- Reciprocal validation becomes available per-track, which is the minimum
  requirement for future per-track publisher confirmation.
- Replication gains one new event type (`track_remote_items_replaced`) and
  one new table; peers must accept the new event before running the
  migration or they will log unknown-event warnings.
- Publisher and musicL feeds no longer appear in recent, search, or
  publisher-detail listings. Callers that need them use the direct GUID
  endpoint or the `publisher` view on a music feed.
- The `medium` query param on `/v1/feeds/recent` is removed. No known
  caller relied on it.
- No change to feed-level publisher behavior, `feed_remote_items_raw`, or
  the existing `publisher` view on feed reads.
- `tracks.publisher_text` semantics from ADR 0035 are unchanged; the "can
  item-level override feed-level" question is explicitly deferred.

## Out of scope

- Overriding `tracks.publisher_text` from item-level declarations.
- A dedicated listing endpoint for publisher / musicL container feeds.
- Canonical artist/publisher identity graphs.
- Admin mutation of item-level remote items (they are ingest-only, like
  feed-level remote items today).
- Changes to ingest policy or the verifier chain — non-music feeds are
  still stored; only their visibility in list/search paths is restricted.
- Re-ingest tooling; operators who want backfill run a normal crawler
  replay with `--force`, which repopulates the new table and re-syncs the
  search index on the next ingest cycle.

## Implementation order

1. Parser: add `remote_items` field, `extract_item_remote_items`, tests
   against a fixture that has both channel-level and item-level
   `remoteItem medium="publisher"`.
2. Wire type: add field to `IngestTrackData` in `stophammer/src/ingest.rs`.
3. Schema + event: add `track_remote_items_raw`, add the event variant, add
   `replace_track_remote_items` DB helper, wire into ingest transaction.
4. Query layer: extend track reads with `include=remote_items` and
   `include=publisher`.
5. Listing filter: apply `raw_medium='music'` whitelist to
   `/v1/feeds/recent`, `/v1/search`, `/v1/publishers/{publisher}`, and the
   feed-entity branch of `sync_source_read_models_for_feed`. Drop the
   `medium` param from `/v1/feeds/recent` and its OpenAPI entry.
6. Docs: update `docs/API.md` (track include options, listing whitelist
   note, drop the `medium` param), `docs/schema-reference.md` (new table,
   event, policy), and the feed ingest example in the API doc to
   illustrate item-level `remote_items`.
