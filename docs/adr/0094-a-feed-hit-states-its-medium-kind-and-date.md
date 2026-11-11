# ADR 0094: A Feed Hit States Its Medium, Kind And Date

## Status
Accepted on 2026-11-10, for musicindex.org requests 23 and 26 and item 1 of
v4vmm request 11. The operator approved the research message of the
orchestrator, and that message gives each decision to Stophammer. Stophammer
selected the form of §1. The candidate `v0.19.0-rc.8` holds it.

It amends ADR 0071 §5 and the search rule of ADR 0038: a `musicL` feed is in the
search index. It adds to ADR 0080 §2.

## Date
2026-11-10

## Context

### A search does not find a playlist
musicindex.org request 23 reports that `GET /v1/search?q=Deck%20Darlings` gives
no row, also with `type=feed`. On 2026-11-10 the live node gives the same
result.

`sync_source_read_models_for_feed` in `src/db.rs` writes a search row only for a
feed with the medium `music` or `publisher`. ADR 0071 §5 states this rule: "A
`musicL` feed and a feed with no medium stay out of the index." So the cause is
the rule. The prefix match of ADR 0086 and the order of ADR 0087 are not the
cause.

The backup of 2026-11-09 01:08 UTC holds 13 feeds with the medium `musicL`, 8,889
with `music` and 1,810 with `publisher`. Each `musicL` feed has `episode_count`
0, no `newest_item_at` and the `release_kind` `unknown`. A playlist holds remote
items, not tracks.

The same section of ADR 0071 keeps a promise to each present client. A search
with `type=feed`, with `type=track` or with no `type` gives the same results as
before. Its invariants give the same promise. One of them is "A response with
no new include, filter or search type does not change". ADR 0038 gives the same rule for the medium of a
list: another medium is an explicit request of the client. v4vmm reads
`type=feed` hits as releases, so a playlist row in that scope is a new row kind
for v4vmm.

### A feed hit has no kind and no date
musicindex.org request 26 asks for `release_kind` and `newest_item_at` on a feed
hit, as a row of `GET /v1/feeds/recent` gives them. Item 1 of v4vmm request 11
asks for the same `newest_item_at`. `feed_search_summary` in `src/query.rs`
reads neither column. The two values are columns of `feeds`, so no new
computation is necessary.

## Decision

### 1. A search finds a playlist when the client names its medium
`GET /v1/search` takes `medium`. Its value is a comma list of `music` and
`musicL`. With no `medium`, the value is `music`, so each present search gives
the same rows.

- `medium=music,musicL` gives the music feeds and the playlists. The site sends
  this value for its Releases scope.
- With `type=feed`, `medium=musicL` gives only the playlists.
- `medium` applies to the feed rows of `type=feed` and of a search with no
  `type`. With no `type`, the track rows do not change, so `medium=musicL`
  gives the playlists and the tracks. With `type=track` or `type=publisher`, `medium` gives `400` with
  `unsupported_filter`.
- Each other value, `publisher` included, gives `400` with `invalid_filter`.
  `type=publisher` finds a publisher feed (ADR 0071 §5).
- The other filters of ADR 0080 apply to a playlist and to a music feed in the
  same way. A playlist has
  `episode_count` 0, so `min_tracks=1` removes it.

A playlist hit gives the `entity_type` `feed`, and a client reads it at
`GET /v1/feeds/{guid}`.

### 1a. The index holds a playlist as its own row type
The search index holds each `musicL` feed as an entity of the type `playlist`.
A search reads that row type only when `medium` names `musicL`. So a search
with no `medium` cannot read a playlist row.

A feed is at most one of the row types `feed`, `publisher` and `playlist`. A
change of the medium deletes the row of the type that the feed left, as ADR
0071 §5 gives for `feed` and `publisher`. A feed with no medium, or with an
unknown medium, stays out of the index.

On the first start of a release with this decision, the node adds each `musicL`
feed that the index does not hold. This is the step that ADR 0071 §5 gives for
the publisher feeds. It reads only the stored rows, and no crawl is necessary.

### 2. Each feed hit states its medium
Each hit with the `entity_type` `feed` or `publisher` gives `medium`: the stored
`raw_medium` of the feed. So the site can give the type of a row. A track hit
does not give the field.

### 3. Each feed hit gives its kind and its date
Each hit with the `entity_type` `feed` or `publisher` gives `release_kind` and
`newest_item_at`. Each value is the value that `GET /v1/feeds/recent` gives for
the same feed. Each can be null, and the key is then present with a null value.

`pub_date` stays empty on a feed hit. `newest_item_at` is the date of a feed hit,
with the name and the meaning of the list rows. This answers item 1 of v4vmm
request 11.

A playlist hit gives `newest_item_at` null today, because a playlist holds no
tracks.

### 4. The capabilities name the change
`GET /v1/node/capabilities` lists `search_medium` for §1 and §2, and
`search_feed_summary` for §3. A client sends `medium` only when the node lists
`search_medium`. A node with no such flag gives `400` for the unknown parameter
(ADR 0080).

### What this ADR does not decide
- `include=named_feeds` on a search, item 2 of v4vmm request 11. It stays a
  draft until the v4vmm operator walks ADR 0084 packet 012 and ADR 0086 packet
  005 of v4vmm.
- A count of the items of a playlist. No request asks for it.

## Alternatives Considered

### Each playlist in `type=feed` with no parameter
The orchestrator recommended this form. It gives the site the playlists with no
new parameter. But each present client of `type=feed` gets a new row kind, and
ADR 0071 §5 and its invariants forbid that. v4vmm would show a playlist as a
release with zero tracks. Rejected.

### A new search type `playlist`
A search takes one `type`. The Releases scope of the site would then need two
searches and a merge of two pages. Rejected.

## Consequences
- The site finds a playlist with `type=feed&medium=music,musicL`, and gives its
  type from `medium`. v4vmm sees no change until it sends `medium`.
- The site can show the kind and the date on a search row. v4vmm can date an
  Index single of a saved search with no detail read.
- The index grows by 13 rows. Each feed hit is larger by three fields.
- The change has no migration, no event and no dependency, so it belongs in a
  MINOR release (ADR 0066).

## Invariants
- A search with no `medium` gives the rows that it gave before this decision.
- Each value of §2 and §3 is the value of a read of the same feed at the same
  time.

## Acceptance Criteria

### Mechanical
Tests in `tests/adr0094_feed_hit_facts_tests.rs` prove each of these:

1. §1: a `musicL` feed is not a hit of `type=feed` or of no `type` with no
   `medium`.
2. §1: the same feed is a hit of `type=feed&medium=music,musicL` and of
   `medium=musicL`, by a word of its title and by a prefix. A music feed is a
   hit of the first and not of the second.
3. §1: `medium` with `type=track` gives `400` with `unsupported_filter`.
   `medium=publisher` gives `400` with `invalid_filter`.
4. §1a: a change of the medium from `musicL` to `music` and back leaves one row
   of the correct type.
5. §1a: the first open of a database with a `musicL` feed and no `playlist` row
   adds the row.
6. §2 and §3: a feed hit and a publisher hit give `medium`, `release_kind` and
   `newest_item_at` with the values of `GET /v1/feeds/recent`. A null value
   gives a present key.
7. §4: the capabilities list `search_medium` and `search_feed_summary`.

The guards of ADR 0044 cover the new fields on `SearchResponseItem` and the
parameter in the OpenAPI document.

### Visual
None in this repository. The site owns the words of a playlist row.

## Follow-Up
- [Task 001](../tasks/adr-0094-task-001-playlist-index-rows.md) adds the
  `playlist` rows to the index.
- [Task 002](../tasks/adr-0094-task-002-search-medium.md) adds `medium` to the
  search. It needs tasks 001 and 003.
- [Task 003](../tasks/adr-0094-task-003-feed-hit-summary.md) adds the three
  fields to a feed hit.
