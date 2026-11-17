# ADR 0101: A Publisher Feed Can List Playlists

## Status
Accepted on 2026-11-16. No release holds it. The operator selected option A
of the investigation of 2026-11-16: the index ingests the link, and the read
view does not change.

- Bar: crosses repositories. The node verifier, the crawler follow rules and
  the crawler skip store change together.
- Changes: [ADR 0049](archive/0049-publisher-relationships-are-rss-facts.md)
  §2 steps 2 and 3
- Changes: [ADR 0060](archive/0060-a-list-feed-keeps-its-items.md) §5 item 1
- Changed by: [ADR 0102](0102-a-publisher-view-shows-its-playlists.md) §1,
  for the first item of §5. Accepted on 2026-11-17.
- Changed by: [ADR 0103](0103-a-marked-publisher-feed-is-a-person.md) §3,
  for §1. Proposed on 2026-11-16.

## Date
2026-11-16

## Context

A curator can publish a `publisher` feed that lists `musicL` feeds. Each
playlist then names that publisher feed. On 2026-11-16 the main session read
one real example:

- `https://raw.githubusercontent.com/ChadFarrow/chadf-musicl-playlists/refs/heads/main/docs/chadf-musicl-publisher.xml`,
  `podcast:guid` `a778e463-ecc3-5219-8f05-f33b3983e173`, `pubDate`
  2026-11-16. It has `podcast:medium` `publisher` and 14 channel
  `podcast:remoteItem` elements with `medium="musicL"` and `rel="curator"`.
  It has no item with `medium="music"`.
- Three of its playlists (IAM, MMM and LT) name it in `<podcast:publisher>`
  with `medium="publisher"` and `rel="curator"`. Two playlists of Boost Me
  Buddy name a different publisher feed. A comment in the publisher feed says
  that it lists those two in one direction only.

The production copy `stophammer-20261115T115439Z.db` holds 1,811 publisher
feeds and 15 `musicL` feeds. No stored publisher feed lists a `musicL` item.
The copy holds 3 of the 14 playlists and does not hold the publisher feed.

Four parts of the code stop this link:

1. `MediumMusicVerifier` (`src/verifiers/medium_music.rs`) rejects a
   publisher feed with no `medium="music"` item. The reason is
   `[medium_music] publisher feed has no remoteItem with medium='music'`.
2. A publisher feed follows only its `medium="music"` items
   (`all_follow_urls` in `stophammer-crawler/src/follow.rs`, ADR 0049 §2
   step 2).
3. A `musicL` feed follows no `medium="publisher"` item (ADR 0060 §5 item 1).
   The test `a_musicl_feed_with_a_publisher_item_gives_nothing_for_it`
   enforces that rule.
4. The skip store (`stophammer-crawler/src/feed_skip.rs`) skips a URL after an
   earlier `[medium_music]` rejection. After a change of item 1, a publisher
   feed that the node rejected before stays skipped.

The parser does not stop the link. It keeps `medium`, `rel`, `itemGuid` and
the `<podcast:publisher>` wrapper of each channel item.

The read side also does not show the link. `publisher_direction`
(`src/query.rs`) gives a `publisher` view row only for a `music` or a
`publisher` item. The back-link check and the link facts of ADR 0099 read only
`medium="music"` items. This decision does not change the read side.

## Decision

### 1. The node accepts a publisher feed that lists playlists

`MediumMusicVerifier` accepts a publisher feed that has a minimum of one channel
`podcast:remoteItem` with the medium `music` or `musicL`. The medium
comparison ignores ASCII case. A publisher feed with neither medium stays
rejected. The reason names the two mediums.

### 2. A publisher feed follows its playlists

This section changes ADR 0049 §2 step 2.

A publisher feed gives the `feedUrl` of each channel item with the medium
`music` or `musicL`. The comparison ignores ASCII case. The follow limit of
200 URLs for one publisher feed (ADR 0054 §3) does not change.

ADR 0049 §2 step 3 does not change. A playlist that the crawler fetched only
because a publisher feed listed it gives no follow URL in that wave.

### 3. A playlist follows its publisher reference

This section changes ADR 0060 §5 item 1.

A `musicL` feed also gives the `feedUrl` of its publisher reference. The
publisher reference is the item that the parser marks with
`publisher_reference`: the item in `<podcast:publisher>`, or else the first
bare item with `medium="publisher"`. A `musicL` feed gives at most one such
URL. Each other item with `medium="publisher"` gives no fetch.

The crawler fetches that publisher feed at the publisher level of ADR 0049 §2.
When it parses as a publisher feed, it gives its `music` and `musicL` URLs
under §2. The crawler fetches those feeds at the listed level, and the walk
stops there.

### 4. A rejection that this decision removes does not skip a URL

The skip store does not skip a URL for an earlier `[medium_music]` rejection
when the stored medium of that URL is `publisher`. The next fetch sends the
body to the node again. The node then applies §1.

### 5. Not in scope

- A `publisher` view row, a back-link check or a link fact for a `musicL`
  item. A client request starts that decision. It changes the `v1` contract
  (ADR 0044).
- The `rel` value `curator`. The node keeps it as a raw role token, as it
  keeps each other `rel` value (ADR 0049 §6).
- A `musicL` item at the item level of a feed.

## Alternatives Considered

### Ingest the playlists only

The crawler can submit each playlist URL as an input today, and the node
accepts it. The index then holds the playlists but not the publisher feed of
the curator. The next playlist of the curator needs a new manual step.
Rejected.

### Follow each `medium="publisher"` item of a playlist

A playlist can list many items with `medium="publisher"`. Each one can lead to
200 more fetches. The publisher reference is one item, and it is the item that
names the owner of the playlist. Rejected.

### Let a playlist at the publisher level follow its albums

The walk from a publisher feed would then reach the albums of each playlist in
one wave. 200 playlists of 1,000 entries each can name 200,000 URLs, and only
the pass limit of 50,000 stops them. The `refresh` pass reads each stored
medium (ADR 0047) at the input level, so the albums arrive at the next
`refresh` pass. Rejected.

### Show the playlist link in the `publisher` view now

No client has asked for it. A new direction value changes the `v1` contract.
Rejected until a client request.

## Consequences

- The publisher feed of a curator enters the index. `include=remote_items`
  on `GET /v1/feeds/{guid}` gives each playlist entry with `medium` `musicL`.
  `type=publisher` search gives the publisher feed (ADR 0100). Its link facts
  count only music links, so they stay 0 for a feed that lists only playlists.
- `include=publisher` on a playlist gives one `music_to_publisher` row for its
  publisher reference. The publisher side lists the playlist with `musicL`, not
  with `music`, so the row does not confirm the back-link. §5 keeps this.
- A gossip notification for a playlist can cause one publisher fetch and at
  most 200 more fetches.
- The crawler and the node change together. The crawler commit is in the
  `stophammer-crawler` repository and names this ADR.
- The tests below replace the test of ADR 0060 §5 item 1 that this decision
  reverses.

## Acceptance

Each criterion is mechanical.

1. A unit test of `MediumMusicVerifier`: a publisher feed with only
   `medium="musicL"` items passes, and a publisher feed with only
   `medium="podcast"` items fails.
2. A unit test of `follow_urls`: a publisher feed with one `music` item and
   one `musicL` item gives both URLs.
3. A unit test of `follow_urls`: a `musicL` feed with a publisher reference
   and a second `medium="publisher"` item gives only the URL of the reference.
   This test replaces `a_musicl_feed_with_a_publisher_item_gives_nothing_for_it`.
4. A unit test of `follow_urls_at_level`: a `musicL` feed at the publisher
   level gives nothing.
5. A unit test of the skip store: a `[medium_music]` rejection with the stored
   medium `publisher` does not skip, and the same rejection with the stored
   medium `podcast` skips.
6. A node test: an ingest of a body like the ChadF publisher feed gives `200`,
   and `include=remote_items` gives each `musicL` entry.
