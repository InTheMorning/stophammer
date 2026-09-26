# musicL Support Research

Date: 2026-09-25. This record states no rule. It is the evidence for a
decision on `musicL` feeds.

## The Namespace

A list medium ends in `L`. The namespace says that a list feed "is intended to
exclusively contain one or more `<podcast:remoteItem>`'s", and has no
`<item>`. A `podcast:remoteItem` has `feedGuid`, which is required, and
`feedUrl`, `itemGuid`, `medium` and `title`, which are optional. The `musicL`
example of the namespace is a track playlist: each `remoteItem` names one
track with `feedGuid` and `itemGuid`. The `mixed` medium is a list of more
than one medium.

Sources: `docs/tags/medium.md` and `docs/tags/remote-item.md` of
`Podcastindex-org/podcast-namespace`, read on 2026-09-25.

## Stophammer Today

| Part | Behavior | Where |
|---|---|---|
| Medium check | Accepts `musicL`. Rejects `mixed` | `src/verifiers/medium_music.rs` |
| Payment check | Exempts `musicL` | ADR 0048 |
| Parser | Keeps `feedGuid`, `feedUrl`, `medium` and `rel` of a channel `remoteItem`. Drops `itemGuid` and `title` | `extract_feed_remote_items` in `stophammer-parser/src/engine.rs` |
| Node ingest | Stores no track and no live item. Also stores no feed payment route | `src/api.rs`, `is_musicl` |
| Storage | `feed_remote_items_raw` has no column for an item GUID or a title | `src/schema.sql` |
| Read API | `include=remote_items` gives each entry. Search and `/v1/feeds/recent` leave `musicL` out. `?medium=musicL` lists it | ADR 0038 |
| Crawler | Follows no link of a `musicL` feed | `a_medium_l_feed_gives_nothing` in `stophammer-crawler/src/follow.rs` |

## The Index

On 2026-09-25 the index held one `musicL` feed, "Local Theory — Full Catalog".
Its 13 channel `remoteItem` elements name full albums, with no `itemGuid`.
It has one value block.

## Podcast Index

The snapshot of 2026-09-19 holds 4,728,321 feeds and has no medium column. A
list feed has no item. Thus the search took the live feeds with an item count
of 0. Of those, it kept the feeds on the hosts of V4V music, and the feeds with
the generator of Local Theory: 291 feeds. Each was fetched one time with
`curl`. 285 answered.

11 declared `musicL`:

| Kind | Feeds | `remoteItem` elements | With `itemGuid` | Value block |
|---|---|---|---|---|
| Track playlist | 6 | 8 to 383 each | All, or all but one | 3 of 6 |
| Album catalog | 1 | 13 | 0 | Yes |
| Empty | 4 | 0 | 0 | 2 of 4 |

The largest is "Lightning Thrashes Playlist episodes 1 - 6", with 383 tracks.
"The College Years Playlist" also gives `title` on each `remoteItem`. The
Kolomona feeds give a non-standard `feedImg` attribute.

The Podcast Index IDs of the 11 are 6612768, 6926780, 6926781, 7072339,
7074166, 7102691, 7177724, 7223260, 7373979, 7410299 and 7922584.

## A Local Test

On 2026-09-25 a local node with an empty database received two of the
playlists from the crawler. The node accepted both. For "Best of Lightning
Thrashes Playlist" it stored 48 `remote_items` entries, no `itemGuid`, and no
payment route, although the feed has a value block. So the node does not
reject a playlist. The 10 missing feeds are missing because no crawl reached
them.

## Gaps

1. The parser drops `itemGuid` and `title`, so a track playlist keeps only
   its albums.
2. ADR 0040 makes a track identity from the feed GUID and the item GUID, so a
   stored `itemGuid` gives the `track_guid` with no search.
3. The node drops the value block of a list feed. Mandate 3 of `AGENTS.md`
   keeps source data.
4. The crawler follows no `remoteItem` of a list feed, so a playlist does not
   lead to its albums.
5. No route gives the playlists that name a feed or a track.
6. `mixed` is rejected.
7. A playlist can name more than 200 feeds. The follow limit of ADR 0054 for
   one feed is 200.
