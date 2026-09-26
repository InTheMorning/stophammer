# ADR 0061: A Publisher Read Counts Its Listed Artists

## Status
Proposed

## Date
2026-09-26

## Context
ADR 0049 §7 gives `distinct_release_artist_count`, the count of the distinct
`itunes:author` values of the albums that **name** the publisher. An album
that the publisher lists, but that does not name the publisher, is not in the
count.

On 2026-09-26 the 1,772 publisher feeds of the index listed 8,249 albums:

| Link | Count |
|---|---|
| Two-way: the album also names the publisher | 7,862 |
| One-way: the album resolves and does not name the publisher | 231 |
| The album does not resolve | 156 |

15 publishers have no two-way link. For them the count is 0. The publisher
"Master's Scroll" lists 81 albums by 33 different artists, and no album names
it. musicindex.org shows "0 artists" for it.

ADR 0059 gives each `publisher` entry `remote_release_artist` and
`remote_release_artist_source`, from the album that the entry resolves to. So
the node can count the artists of each **listed** album at read time.

A client also wants to show a publisher as an artist or as a label. No link
in a sample of 249 stated `rel`, so ADR 0049 §6 gives each link
`role: "artist"` with `role_source: "default"`.

The operator decided on 2026-09-26 that the index does not derive a kind.
Two rules were tested: a rule on the artist names, and a rule on the
direction of a link. The direction rule failed, because 105 of the 231
one-way links are an artist's own album. The index reports only what the
feeds state, and publishers fix their feeds. Two other steps do that fix: a
proposal of `rel` to the Podcast Namespace, and a change to the tools that
write publisher feeds.

## Decision

### 1. The listed artists

A feed read of a publisher feed adds two fields:

| Field | Value |
|---|---|
| `listed_release_artists` | The distinct `release_artist` values of the listed albums that resolve, raw, in the order of first listing |
| `listed_release_artist_count` | The count of `listed_release_artists` |

A listed album is a row of the direction `publisher_to_music` that does not
resolve to `unresolved` (ADR 0049 §3). An album whose
`release_artist_source` is `placeholder` gives no value. Two values are the
same when they are equal after the normalization of ADR 0049 §7.

### 2. The present fields do not change

`distinct_release_artist_count` and `distinct_release_artists` keep their
meaning: the albums that name the publisher. A `v1` field keeps its meaning
(ADR 0044 §4). The API documentation states the difference between the two
counts.

### 3. The node derives at each read

The node stores no count. A publisher read already resolves each listed album
and reads its summary (ADR 0059), so the fields add no query.

### 4. No derived kind

The node derives no artist or label kind. `role` and `role_source` of ADR
0049 §6 stay the only role fields. A client that shows a role shows
`role_source: "default"` as not stated by the feed.

## Alternatives Considered

### Change `distinct_release_artist_count` to count the listed albums
A client that reads the field would receive a different meaning with no
signal.
ADR 0044 §4 forbids it in `v1`. Rejected.

### Derive `publisher_kind` from the album artist names
A rule was tested on 240 publishers and gave no false label. It is still an
assumption that the index would show beside the facts. The operator prefers
to make the feeds state the role. Rejected on 2026-09-26.

### Take the kind from the direction of the link
A two-way link as an artist and a one-way link as a label. 105 of 231 one-way
links are an artist's own album, and Sir Libre Records links both ways as a
label. Rejected.

## Consequences

- "Master's Scroll" shows 33 listed artists.
- A publisher with only one-way links shows its artists.
- The index shows the role only when a feed states `rel`.

## Invariants

- The node stores no count.
- `listed_release_artist_count` counts only resolved listed albums.

## Guards

- A publisher that lists 2 albums by 2 artists, of which no album names it,
  gives `listed_release_artist_count` 2 and `distinct_release_artist_count` 0.
- A listed album with a `placeholder` artist is not counted.
- Two listed albums by "Ann Lee" and " ann  lee " count as one artist.
- An unresolved listed album is not counted.
