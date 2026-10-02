# ADR 0061: A Publisher Read Counts Its Confirmed Artists

## Status
Accepted on 2026-09-27, with two changes of the operator. A publisher earns
credit only for the albums that confirm it. An album read shows only the
publisher links that the album states. Section 5 makes that a rule of ADR 0049
§4.

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
in a sample of 249 stated `rel`, so ADR 0049 §6 gave each link
`role: "artist"` with `role_source: "default"`. Since 2026-10-02 it gives
`role` null for such a link.

The operator decided on 2026-09-26 that the index does not derive a kind.
Two rules were tested: a rule on the artist names, and a rule on the
direction of a link. The direction rule failed, because 105 of the 231
one-way links are an artist's own album. The index reports only what the
feeds state, and publishers fix their feeds. Two other steps do that fix: a
proposal of `rel` to the Podcast Namespace, and a change to the tools that
write publisher feeds.

## Decision

A publisher feed earns credit only for an album that confirms the link. The
operator decided on 2026-09-27 that a list with no link back is no evidence.
It does not show that the feed is a label or an artist.

### 1. The confirmed and the unconfirmed artists

A feed read of a publisher feed adds four fields:

| Field | Value |
|---|---|
| `confirmed_release_artists` | The distinct `release_artist` values of the listed albums that also name this publisher, raw, in the order of first listing |
| `confirmed_release_artist_count` | The count of `confirmed_release_artists` |
| `unconfirmed_release_artists` | The same for the listed albums that do not name this publisher |
| `unconfirmed_release_artist_count` | The count of `unconfirmed_release_artists` |

A listed album is a row of the direction `publisher_to_music` that does not
resolve to `unresolved` (ADR 0049 §3). It is confirmed when
`music_names_publisher` is true. An album whose `release_artist_source` is
`placeholder` gives no value. Two values are the same when they are equal
after the normalization of ADR 0049 §7. An artist of a confirmed album is not
also in the unconfirmed list.

### 2. The present fields do not change

`distinct_release_artist_count` and `distinct_release_artists` keep their
meaning: the albums that name the publisher. A `v1` field keeps its meaning
(ADR 0044 §4). The API documentation states the difference between the
fields.

### 3. The node derives at each read

The node stores no count. A publisher read already resolves each listed album
and reads its summary (ADR 0059), so the fields add no query.

### 4. No derived kind

The node derives no artist or label kind. `role` and `role_source` of ADR
0049 §6 stay the only role fields. `role_source: "default"` means that no
feed states a role, and `role` is then null.

### 5. An album shows only the publishers that it names

The `publisher` view of an album gives only the publishers that the album
names. A publisher feed that lists the album, when the album does not name it,
is not in the album read. The read of the publisher feed still gives that
link, in `unconfirmed_release_artists` and in its own `publisher` view.

On 2026-09-27 the code already does this: `load_publisher` in `src/query.rs`
builds the rows of a read only from the feed's own `remoteItem` elements. This
section makes the behavior a rule, and a test guards it.

## Alternatives Considered

### Count each listed album, confirmed or not
The proposal of 2026-09-26 gave one count of the listed artists. It gave a
publisher with no confirmed album the same credit as a confirmed one. Rejected
on 2026-09-27.

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

- "Master's Scroll" shows 0 confirmed artists and 33 unconfirmed artists. Its
  81 albums do not show it as their publisher, as before.
- A publisher gains credit when its albums name it. A publisher fixes its
  standing in its own feeds and in the feeds of its albums.
- The index shows the role only when a feed states `rel`.

## Invariants

- The node stores no count.
- A publisher gets no confirmed artist from an album that does not name it.
- An album read never shows a publisher that the album does not name.

## Guards

- A publisher that lists 2 albums by 2 artists, of which no album names it,
  gives `confirmed_release_artist_count` 0 and
  `unconfirmed_release_artist_count` 2.
- A publisher that lists 2 albums by 2 artists, of which one names it, gives 1
  and 1.
- A listed album with a `placeholder` artist is not counted.
- Two listed albums by "Ann Lee" and " ann  lee " count as one artist.
- An unresolved listed album is not counted.
- The read of an album that a publisher lists, and that does not name the
  publisher, gives no `publisher` row for that publisher.
