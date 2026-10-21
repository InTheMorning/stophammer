# ADR 0076: A Named Feed Entry States Its Confirmation

## Status
Proposed on 2026-10-21, for v4vmm request 9. It amends ADR 0071 §3 and the
section "Search is not in scope" of ADR 0068 (section 5).

## Date
2026-10-21

## Context

v4vmm request 9 of 2026-10-20 gives three gaps in the routes of ADR 0071. The
Stophammer review of 2026-10-21 found each gap on the live node, revision
`056199b`:

| Gap | What the node gives | What a client cannot do |
|---|---|---|
| A | A `named_feeds` entry gives no `two_way_validated` | Tell a confirmed link from a link that only the album states |
| B | No release list takes a filter for a named feed | List the releases that name one feed without a read of each page |
| C | A `publisher` search row gives no link facts | Tell a label feed from an artist feed in a search result |

For gap A, the node already has the value. `NamedFeedEntry::from_rows` copies
five fields of the `publisher` view row, and does not copy
`two_way_validated`. On 2026-10-21, the first 200 rows of
`GET /v1/feeds/by-name?include=named_feeds` gave 188 entries:

- 185 entries name a held feed through a confirmed link.
- 1 entry names a held feed through a link that only the album states: the
  album "2 Die" names SOD at rank 1.
- 2 entries name a feed that the node does not hold.

Each of the 188 entries gave `role` null and `role_agreement` null. Thus the
five fields do not show the difference between the 185 confirmed links and
the 3 others.

For gap B, the `publisher` view of a feed lists only the albums that the feed
itself lists. An album that names the feed with no listing by the feed is not
in that view. The section "Deferred, Not Requested Now" of the v4vmm request
document gives this list as the reverse album list. Request 9 now asks for it.

For gap C, ADR 0068 §5, as amended by ADR 0071, says that a search row gives
no link facts. A list row with `include=link_facts` gives them through
the same functions as a read of the feed.

The request also states that the filter of gap B can be the list of
`first_named_count` (ADR 0071 §1b). This is not correct. That count includes
only confirmed links with rank 1. A confirmed link is two-way, so the
`publisher` view of the feed already lists each one. The filter gives each
rank and each link, confirmed or not.

## Decision

### 1. A named feed entry gives its confirmation

Each `named_feeds` entry of ADR 0071 §3 also gives `two_way_validated`. The
value is the value of the same `music_to_publisher` row of the `publisher`
view. Its meaning is the meaning of ADR 0049.

The node gives no new state field. A client gets each case from the fields
of the entry:

| Case | Fields |
|---|---|
| The node does not hold the named feed | `remote_feed_title` null |
| Only the album states the link | `two_way_validated` false |
| A confirmed link (ADR 0049 §6a) | `two_way_validated` true, and `role_agreement` is not `conflict` |

### 2. A release list takes a named feed filter

`GET /v1/feeds/by-name` and `GET /v1/feeds/recent` take `named_feed_guid`.
With it, the route gives only the music releases that have a `named_feeds`
entry with that GUID. The rank and the confirmation of the entry have no
effect on the filter. The order, `limit`, `cursor` and `include` do not change.
A GUID that no release names gives an empty list.

A request with `named_feed_guid` and a `medium` other than `music` gives `400`.
A request with `named_feed_guid` and `artist_key` gives `400`.

The filter is not the list of a count, and ADR 0071 §1b does not apply to it.

### 3. A publisher search row gives its link facts on request

`GET /v1/search` takes `include=link_facts`. With it, each `publisher` row
gives `two_way_link_count`, `stated_rels`, `agreed_roles`,
`confirmed_release_artists` and `first_named_count`. The node calculates each
value with the functions of a read of the same feed (ADR 0068 §1 and §5,
ADR 0071 §2). A `feed` row and a `track` row do not change.

The OpenAPI document declares the include on the route (ADR 0071 §1c).
`GET /v1/node/capabilities` gives `include_params.search` with `link_facts`.

This changes ADR 0068 §5: a `publisher` search row gives the link facts on
request.

### 4. The name level does not change

ADR 0070 §4 and ADR 0071 §6 stay. An album that names a held feed first through
a link that only the album states stays out of the name level. The v4vmm
review calls this difference D-2. v4vmm does not ask to change it, and section
1 lets v4vmm count it.

### 5. The changes are additions

Each field, filter and include is a `v1` addition (ADR 0044 §4). A request with
no new filter or include gives the same response as before.

## Alternatives Considered

### A `link_state` field
The request gives it as one form. Its three values come from fields that the
entry already gives, or from `two_way_validated`. A second field for the same
facts can disagree with the first. Rejected for section 1.

### A filter that keeps only confirmed links
A client that groups the singles of one feed also shows a link that only the
album states, with its own label. The client can drop an entry with the
fields of section 1. Rejected for the filter over each entry.

### The filter on `GET /v1/feeds/by-name` only
A group page lists its singles newest first (v4vmm ADR 0084 Decision 6).
The two routes read the same rows, so the second route costs one condition.
Rejected for both routes.

### The intersection of `named_feed_guid` and `artist_key`
A `name` row counts releases with no held confirmed link. A named feed filter
and a name key thus select almost separate sets. No client asks for the
intersection. Rejected for `400`, as ADR 0071 §4 does for a wrong `medium`.

## Consequences

- v4vmm proposed packet 026 can group the Index singles of one feed. Packet 016
  can count each D-2 album.
- v4vmm packet 010 can show a role on a `publisher` search row with no read for
  each row. musicindex.org can do the same.
- The filter makes the reverse album list of the v4vmm request document
  available. That deferred item closes.
- A search page with `include=link_facts` reads the `publisher` view of each
  `publisher` row. A search without it costs the same as before.

## Invariants

- A named feed entry and a search row give the values of a read of the same
  feed. The node calculates them at the same time.
- The node derives no artist and no label kind (ADR 0061 §4).
- A response with no new filter or include does not change.

## Acceptance Criteria

Mechanical, each one checked by a test:

- With `include=named_feeds`, an entry for a link that only the album states
  gives `two_way_validated` false. An entry for a two-way link gives true.
- `named_feed_guid` on each of the two routes gives each music release that
  names the feed, at each rank. A cursor gives no gap and no repeat. The
  result includes a release that the named feed does not list.
- `named_feed_guid` with `medium=publisher`, or with `artist_key`, gives `400`.
- With `include=link_facts`, a `publisher` row of `GET /v1/search` gives the
  values of a read of that feed. A `feed` row and a `track` row give no link
  facts.
- The guard of ADR 0071 §1c passes with `link_facts` declared on
  `GET /v1/search`, and `GET /v1/node/capabilities` gives
  `include_params.search`.

Visual, checked by the v4vmm operator:

- A v4vmm group of Index singles holds each single of the feed on its first
  page.
- The v4vmm search shows a label feed apart from the artist feeds.
