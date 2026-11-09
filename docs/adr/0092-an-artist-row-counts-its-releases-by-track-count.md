# ADR 0092: An Artist Row Counts Its Releases By Track Count

## Status
Accepted on 2026-11-08 by the operator, for musicindex.org request 30. The
orchestrator selected the form. On 2026-11-08 the primary runs it in the
candidate `v0.19.0-rc.7`.

It adds to ADR 0068 §5, ADR 0071 §2, ADR 0076 §3 and ADR 0070 §3. ADR 0080 §2 and
ADR 0085 §6 do not change: the node infers no release kind.

## Date
2026-11-08

## Context
A row of an artist or a label gives only a total: `two_way_link_count` with
`include=link_facts`, and `release_count` on a `name` row of
`GET /v1/artist-names`. The site showed "17 albums" for jSmovie, but each of the
17 releases has one track. musicindex.org request 30 asks for the count of each
kind, for example "17 singles".

The node has no stated kind to count. In the backup of 2026-11-08 05:27 UTC,
each of the 8,887 music feeds has `release_kind` `unknown`. ADR 0080 §2 and
ADR 0085 §6 decide that the node infers no kind. musicindex ADR 0020 R-WORD-1
gives the kind from the track count, and the apps own that rule.

The music feeds of that backup have these track counts:

| Tracks | Feeds |
|---|---|
| 0 | 25 |
| 1 | 6,241 |
| 2 or 3 | 678 |
| 4 to 6 | 649 |
| 7 or more | 1,294 |

## Decision

### 1. A row gives `release_track_counts`
`release_track_counts` is an object. Its key is a track count, as text. Its
value is the number of releases with that track count. A track count with no
release is absent. The node gives facts and no kind, and the client applies its
own rule.

| Row | The releases | The sum of the values |
|---|---|---|
| Each row with `two_way_link_count` | The confirmed links of `two_way_link_count` | `two_way_link_count` |
| A `name` row of `GET /v1/artist-names` | The releases of `release_count`, the set of `artist_key` | `release_count` |

- A row with `two_way_link_count` gives the field with `include=link_facts`
  only: `GET /v1/feeds/recent`, `GET /v1/feeds/by-name`, the `publisher` rows of
  `GET /v1/artist-names` and of `GET /v1/search`.
- A `name` row gives the field with no include, as it gives `release_count`.
- A row with no confirmed link gives `{}`.

### 1a. The track count is `episode_count`
The track count of a release is `feeds.episode_count`. The filters `tracks`,
`min_tracks` and `max_tracks` of ADR 0080 read the same column, so a map agrees
with these filters. On a confirmed link, the value is the
`remote_episode_count` of the row (ADR 0059 §1), which reads the same column.

In the backup, `episode_count` equals the count of the stored tracks for 8,886
of the 8,887 music feeds. The other feed is
`87750227-32d0-5154-8944-a6c65cf57157`, "How Will I Know What I Like? (Single)":
`episode_count` 1, and no stored track.

A confirmed link reads the items of its album, so the node holds the album and
`remote_episode_count` is not null. So the sum of a map is the total.

### 1b. The source of the map
The map of a row with `two_way_link_count` comes from the same rows as that
count (`link_facts`). So each route that gives the count gives the map with the
same set, and no route reads the copy in memory for it.

The orchestrator first approved a read of the copy in memory of
`GET /v1/artist-names` by `feed_guid`. The request then named the set of
`two_way_link_count`. That set is not the set of `named_feed_guid` that the copy
holds for a publisher row. The rows of `link_facts` give the set of the request,
with no wait for a build of the copy.

### 2. A capability names the field
`GET /v1/node/capabilities` lists `release_track_counts`. A client with no flag
keeps the word "releases".

### What this ADR does not decide
- A release kind on the node. ADR 0085 §6 keeps the rule with the apps.
- A count of each kind for a stated `release_kind`. No feed states one today.

## Evidence
The release binary ran on a disk copy of the backup of 2026-11-08 05:27 UTC. It
is not a load test.

| Fact | Value |
|---|---|
| jSmovie (`44f2785c-6a55-4f59-b47f-4a7fcedc7d01`) | `{"1": 17}`, with `two_way_link_count` 17 |
| OLAMIDE (`ff6c8ef7-e3d5-47bf-9cfb-e50c51597424`) | 12 track counts, with a sum of 52 |
| Rows of `GET /v1/artist-names` with a sum equal to their total | 2,090 of 2,090 |
| `GET /v1/feeds/recent` and `by-name`, `medium=publisher&include=link_facts&limit=50` | 21 to 33 ms |

The build of the copy in memory reads `episode_count` in the query that already
reads `release_artist`, so it has no new query.

## Consequences
- The site can write "17 singles" or "2 albums · 5 singles" with its own rule.
- Each row with the field is larger. The change is an addition, with no
  migration, no event and no dependency, so it belongs in a MINOR release
  (ADR 0066).

## Invariants
- The sum of a map is the total of its row.
- The node gives no release kind.

## Acceptance Criteria

### Mechanical
Tests in `tests/adr0092_release_track_counts_tests.rs` prove each of these:

1. §1: confirmed links to albums of 1, 1, 1, 9 and 12 tracks give
   `{"1": 3, "9": 1, "12": 1}` on each of the four routes. The sum is
   `two_way_link_count`.
2. §1: a one-way link is not counted.
3. §1: a publisher with no confirmed link gives `{}`.
4. §1: without `include=link_facts`, a publisher row has no key.
5. §1: a `name` row gives its map with and without the include, with a sum equal
   to `release_count`.
6. §2: the capabilities list `release_track_counts`.

The guards of ADR 0044 cover the field on `FeedResponse`, `SearchResponseItem`
and `ArtistNameRow`.

### Visual
None in this repository. The site owns the words of a count.
