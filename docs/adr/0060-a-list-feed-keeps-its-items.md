# ADR 0060: A List Feed Keeps Its Items

## Status
Proposed

## Date
2026-09-25

## Context
The Podcast Namespace defines a list medium with the suffix `L`. A `musicL`
feed has no `<item>`. Its channel holds `podcast:remoteItem` elements. Each
element gives `feedGuid`, which is required, and `feedUrl`, `itemGuid`,
`medium` and `title`, which are optional. When an element gives `itemGuid`, it
names one track, and the feed is a track playlist.

[The musicL research](../reviews/musicl-support-research.md) found these facts
on 2026-09-25:

- Podcast Index holds 11 `musicL` feeds. 6 are track playlists of 8 to 383
  tracks. Each element of a playlist gives `itemGuid`, or all but one. 4 of
  the 6 have a channel `podcast:value` block.
- The index holds 1 of the 11. The node accepts a playlist. No crawl reached
  the other 10.
- The parser keeps `feedGuid`, `feedUrl`, `medium` and `rel`. It drops
  `itemGuid` and `title`.
- The node stores no payment route for a `musicL` feed (`is_musicl` in
  `src/api.rs`), also when the feed has a value block.
- The crawler follows no link of a `musicL` feed.

Thus a client can show the albums of a playlist, but not its tracks, and the
index loses the value block of the playlist. Mandate 3 of `AGENTS.md` keeps
source data.

musicindex.org request 2 asks for `itemGuid` and `title` on each
`remote_items` entry, and for the indexed track that they name. The request is
in [the musicindex.org requests](../plans/musicindex-open-requests.md).

ADR 0040 makes the track identity the pair of `feed_guid` and the raw
`track_guid`. The raw `track_guid` is the `guid` of the item. Thus the pair of
`feedGuid` and `itemGuid` names one track with no search.

ADR 0049 section 2 lets the crawler follow a link only from a music feed or a
publisher feed. ADR 0054 section 3 limits the crawler to 200 follow URLs from
one source feed in one wave. One playlist names 383 tracks.

## Decision

### 1. The parser reads `itemGuid` and `title`

For each channel `podcast:remoteItem`, the parser gives the `itemGuid` and the
`title` attributes, trimmed. An empty value gives null. The ingest contract
gets two optional fields, `remote_item_guid` and `remote_item_title`. A crawler
that does not send them sends null.

### 2. The node stores them

A migration adds `remote_item_guid` and `remote_item_title` to
`feed_remote_items_raw`. Both are nullable. `src/schema.sql` gets the same
columns. The node stores the values unchanged.

This ADR does not change an item-level `podcast:remoteItem`.

### 3. The read gives the entry and its track

Each `remote_items` entry of a feed read gets three fields:

| Field | Value |
|---|---|
| `remote_item_guid` | The stored `itemGuid`, or null |
| `remote_item_title` | The stored `title`, or null |
| `remote_track_guid` | The `track_guid` of the indexed track that the entry names, or null |

The node finds the track at each read. It resolves the feed of the entry as
ADR 0059 section 2 does: by `remote_feed_guid`, then by `remote_feed_url`.
Then it reads the track with that `feed_guid` and a `track_guid` equal to
`remote_item_guid`. When no track has that pair, the value is null. The node
stores no resolution.

The ADR 0059 summary fields give the feed of the entry, not the track.

### 4. A list feed keeps its value block

The node stores the channel payment routes of a `musicL` feed as the feed
payment routes of that feed. `include=payment_routes` on the list feed gives
them. The route history of ADR 0053 records their changes.

These routes pay the author of the list. They are not a payment route of a
track that the list names. A track read never gives them.

The ADR 0048 exemption for `musicL` does not change. The node
continues to accept a list feed with no value block.

### 5. The crawler follows a list feed

This section amends ADR 0049 section 2.

1. When a `musicL` feed gives a `feedUrl` on a channel `remoteItem`, the
   crawler fetches and submits that feed. The `medium` attribute of the
   element is not necessary. The node medium check decides if the feed is
   music.
2. The crawler does not follow a link from a feed that it fetched only because
   of step 1. The walk stops at one level, as in ADR 0049 section 2 step 3.
3. An element with no `feedUrl` gives no fetch. The crawler reports the count
   of those elements.
4. The crawler follows each URL one time. A playlist with 30 tracks
   from one album gives one fetch.

### 6. The follow limit of a list feed

This section amends ADR 0054 section 3.

The crawler follows at most 1,000 different URLs from one `musicL` feed in one
wave. The limit for each other feed stays 200. The limit of 50,000 URLs for
each pass does not change. The crawler reports the number of URLs that it did
not follow.

## Alternatives Considered

### Keep the playlist as an album list
A client shows the albums of a playlist, not its tracks. The index keeps the
declared values only in part. Mandate 3 rejects the loss. Rejected.

### Store the value block apart from the feed routes
A new table for the routes of a list feed makes one more read path and one
more route history. The routes are the routes of the list feed itself, so
the feed route table has the correct meaning. Rejected.

### `remote_track_guid` as a stored value
A stored value becomes stale when the album changes or is deleted. ADR 0049
section 3 rejects a stored resolution for the same reason. Rejected.

### Keep the follow limit of 200 for a list
The crawler follows the first 200 entries of the playlist with 383 tracks. An
album that only entry 201 or after names is not reached through that list.
Rejected.

### No follow limit for a list
A list feed of 16 MiB can name about 50,000 URLs. One hostile list can then
use all of the limit of a pass. Rejected.

## Consequences

- A playlist page needs one request for its track list. With ADR 0059, it also
  gets the title, the image and the artist of each album.
- A playlist leads the crawler to the albums that it names.
- The list feed shows its own payment routes. A client can pay the author of
  the list.
- The crawler, the parser and the node change together. The ingest fields are
  optional, so the deploys can occur in any sequence. A playlist ingested
  before the parser deploy keeps null values until its next crawl.
- A read with `include=remote_items` does one more lookup for each entry.
- The node continues to reject a `mixed` feed. This ADR makes no decision
  on `mixed`.
- No route gives the playlists that name a feed or a track. That needs
  its own decision.

## Invariants

- The node stores `itemGuid` and `title` of a channel `remoteItem` unchanged.
- `remote_track_guid` is the `track_guid` of an indexed track, or null.
- A payment route of a list feed is never given as a route of a track.
- The crawler follows at most 1,000 URLs from one `musicL` feed in one wave.
- The crawler does not follow a link from a feed that it fetched because of a
  list.

## Guards

- Parser: a channel `remoteItem` with `itemGuid` and `title` gives the two
  values. An element with no `itemGuid` and no `title` gives two nulls.
- Node: a `musicL` feed with a track entry gives `remote_item_guid`,
  `remote_item_title` and the `remote_track_guid` of the indexed track. An
  entry for a track that is not indexed gives null in `remote_track_guid`.
- Node: an entry that resolves its feed by URL gives the track of the feed at
  that URL.
- Node: a `musicL` feed with a value block gives its routes on
  `include=payment_routes`. A track that the list names does not get them.
- Crawler: a `musicL` feed with a `feedUrl` gives a follow URL. The fetched
  feed gives no follow URL of its own.
- Crawler: a `musicL` feed with 1,200 different URLs gives 1,000 follow URLs
  and reports 200 not followed. A music feed with 300 gives 200.
