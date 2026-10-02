# ADR 0069: An Album Confirms Each Publisher That It Credits

## Status
Proposed on 2026-10-02.

## Date
2026-10-02

## Context

An album can name only one publisher feed. The namespace gives
`<podcast:publisher>` the count "Single", and the element holds exactly one
`<podcast:remoteItem medium="publisher">`. A release often has two parties: the
artist and a label. A collaboration has more than one artist. Each of them can
have a publisher feed that lists the album. Only one of them can be the
publisher that the album names.

The namespace states the rule of truth for a publisher link, in
`docs/examples/publishers/publishers.md`: a publisher feed lists the feed, and
the feed links back. "If a feed links to a publisher feed without the publisher
feed referencing it, that association should be discarded." ADR 0049 §3 and
ADR 0061 apply this rule. Only a two-way link confirms a relationship.

So today a label release can confirm only one party. When the album names the
label, the artist feed that lists the album has a one-way link. Its artist
goes into `unconfirmed_release_artists` (ADR 0061), and a client cannot show
that the artist confirmed the release.

A label cannot state the artist of an album. `<podcast:remoteItem>` has no
attribute for it, and a statement by the label alone is one-way.

The namespace allows `<podcast:remoteItem>` directly in `<channel>`, with the
count "Multiple". A bare channel `remoteItem` with `medium="publisher"` already
has a meaning in two tools:

- The Stophammer parser (`extract_feed_remote_items`) stores it the same as the
  item inside `<podcast:publisher>`.
- MSP 2.0 (`parseRssFeed` in `src/utils/xmlParser.ts`, commit of 2026-10-01)
  reads it as the publisher reference when the album has no
  `<podcast:publisher>`. When the album has one, MSP drops the bare item when
  it writes the feed again.

On 2026-10-02 a test ingested an album that names a label in its
`<podcast:publisher>` and an artist feed with a bare channel `remoteItem`. The
label feed and the artist feed each listed the album. Without any code change,
the node gave two two-way rows on the album read, `role` `label` and `artist`.
The label read and the artist read each gave the album's artist in
`confirmed_release_artists`. The node cannot tell which of the two rows is
the publisher of the namespace.

## Decision

### 1. The album names its publisher and its credits

An album states two kinds of publisher link:

- **The publisher.** The `remoteItem` inside `<podcast:publisher>`. When the
  album has no `<podcast:publisher>`, the first bare channel `remoteItem` with
  `medium="publisher"` is the publisher. That is the older form, and MSP reads
  it the same way.
- **A credit.** Each other bare channel `remoteItem` with
  `medium="publisher"`. An album can have zero or more credits.

`rel` on each item states the role of that publisher feed, as ADR 0049 §6
gives. A credit with no `rel` has no stated role.

### 2. Only a two-way credit is confirmed

A credit follows the same rule as the publisher link. It is confirmed only
when the credited publisher feed also lists the album. A one-way credit is a
claim of the album, and a one-way listing is a claim of the publisher feed.
The node shows each claim, and it counts only the two-way links.

The checks of ADR 0049 §3 and §4, `two_way_validated`, `stated_rels` (ADR
0068) and `confirmed_release_artists` (ADR 0061) apply to a credit without a
change. The test of 2026-10-02 shows that the node does this now.

### 3. Each link says how the album names it

The parser marks each feed-level `remoteItem` that is the publisher of section
1. The node stores the mark in the existing `source` column of
`feed_remote_items_raw`:

| `source` | Meaning |
|---|---|
| `podcast_publisher` | The publisher of section 1 |
| `podcast_remote_item` | Each other feed-level `remoteItem`, a credit included |

The column and the `source` field of the signed remote item already exist, so
no migration and no event change is necessary. An event signed before this
decision gives `podcast_remote_item`.

Each row of the `publisher` view gives a new field, `album_names_as`:

| Value | Meaning |
|---|---|
| `publisher` | The album names this publisher feed as its publisher |
| `credit` | The album names this publisher feed as a credit |
| `null` | The album does not name this publisher feed |

A row of an album read gives the value of its own item. A row of a publisher
read gives the value of the matching item of the album.

### 4. A publisher read gives the feeds that share its albums

A read of a publisher feed with `include=publisher` gives
`co_credited_feeds`. It holds each other publisher feed that a confirmed album
of this feed also confirms, with:

- `feed_guid` and `title` of that publisher feed,
- `roles`: the different raw `rel` values that those albums give it, sorted,
- `album_count`: the number of shared confirmed albums.

The list is computed at read time and is not stored, as ADR 0061 computes the
artists. For a label, the list holds the artist feeds of its confirmed
releases. For an artist, it holds its labels. Each entry rests on two two-way
links through one album. Thus a label cannot add an artist, and an artist
cannot add a label.

### 5. The node gives facts and derives no type

As ADR 0061 §4 and ADR 0068 §2 give, the node does not decide that a
publisher is an artist or a label. `album_names_as`, `rel` and the two-way
facts are RSS facts. The client derives the type.

## Writing Rules For A Feed Tool

These rules are advisory. They tell a tool how to write a feed that this
decision reads correctly. The guide `docs/publisher-links-guide.md` gives them
with examples.

1. Put the release owner in `<podcast:publisher>`: the label for a label
   release, the artist for a release by the artist.
2. Put each other party as a bare channel `remoteItem` with
   `medium="publisher"` and a `rel`.
3. Write the same `rel` value on the matching item of each publisher feed.
4. Keep each bare `medium="publisher"` item when the feed is written again.
   MSP 2.0 drops such an item when the album has `<podcast:publisher>`. That
   needs a change in MSP before MSP can write a credit.

## Alternatives Considered

### More than one `remoteItem` inside `<podcast:publisher>`
The specification says that the element contains exactly one. A tool that
reads only the first item would show the wrong publisher. Rejected.

### The label states the artist
`<podcast:remoteItem>` has no attribute for the artist, and a statement of
the label alone is one-way. Rejected.

### A direct link between a label feed and an artist feed
The label lists the artist feed, and the artist feed names the label in its
own `<podcast:publisher>`. An artist feed can name only one publisher, so an
artist with releases on two labels cannot confirm both. The link also does not
say which albums it covers. Rejected.

### `podcast:person` with a link to the artist feed
`podcast:person` has `href`, a web page, and no feed GUID. The publisher feed
cannot link back to a person tag, so the link cannot be two-way. Rejected.

### Treat each bare item as the publisher
That is what the parser does now. The node then cannot tell the publisher of
the namespace from a credit, and a client cannot show "label, with artist".
Rejected.

## Consequences

- A label release can confirm the label and each artist.
- A client can show the publisher and the credits separately.
- A label read lists the artist feeds of its confirmed releases, and an artist
  read lists its labels.
- The parser, the crawler and the node change: one parser field, one ingest
  field, the `source` value, two API fields. No migration and no new event.
- A record keeps `podcast_remote_item` until its next submission. Most feeds
  do not change, so the content-hash check gives `no_change`. One `refresh`
  pass with `--force` after the deploy writes the new `source` values.
- A tool that drops bare `medium="publisher"` items, as MSP 2.0 does now,
  loses the credits when it writes the feed again.

## Invariants

- A credit counts only when the credited publisher feed lists the album.
- An album has at most one publisher of section 1.
- A label cannot confirm an artist, and an artist cannot confirm a label. Only
  an album that names both confirms the two.

## Non-Goals

- A credit for one song. A credit is a channel element, so it applies to the
  whole album. ADR 0038 stores an item-level `remoteItem` with
  `medium="publisher"` as evidence for one track. The namespace does not list
  `<item>` as a parent of `remoteItem`, and this decision does not change
  ADR 0038.
- A type for a publisher feed (section 5).

## Guards

- An album with a label in `<podcast:publisher>` and an artist as a bare item,
  each listing the album back: two two-way rows, `album_names_as` `publisher`
  and `credit`.
- An album with only bare items and no `<podcast:publisher>`: the first is the
  publisher, the others are credits.
- A credit that the publisher feed does not list: one-way, and its artist is
  in `unconfirmed_release_artists`.
- A label read gives the artist feed in `co_credited_feeds` with `roles`
  `["artist"]` and `album_count` 1. A publisher feed that only lists the album
  is not in the list.
- The parser marks the item inside `<podcast:publisher>`, also when a bare
  `medium="publisher"` item comes first in the channel.
