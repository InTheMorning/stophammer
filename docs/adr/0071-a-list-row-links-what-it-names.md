# ADR 0071: A List Row Links What It Names

## Status
Accepted on 2026-10-18, for v4vmm request 8. It amends ADR 0038 Decision B and
the section "Search is not in scope" of ADR 0068 (section 5). Tasks 001 to 005
give the build. Amended on 2026-10-21 by ADR 0076: a `named_feeds` entry
also gives `two_way_validated` (§3).

## Date
2026-10-18

## Context

v4vmm request 8 of 2026-10-18 asks for one rule. One read of a list row or a
search row gives the facts that a client needs to show, classify and link the
row. Three decisions already apply a part of this rule:

- ADR 0042, Invariants: a search result holds the fields that a client needs to
  show a row.
- ADR 0059 §1: an entry that names a feed gives the title, the image and the
  artist of that feed.
- ADR 0068: a batch route for the publisher facts is rejected, because the row
  is the place a client already reads.

The two name lists of ADR 0070 and the search route do not apply it. The
Stophammer review of 2026-10-18 found each of the four gaps of the request on
the live node, revision `99e90f7`:

| Gap | What the node gives | What a client cannot do |
|---|---|---|
| A | A `publisher` row of `GET /v1/artist-names` gives no link facts | Tell an artist feed from a label feed in the list |
| B | A release row gives `publisher_feed_title`, and no GUID | Open the page of that feed, or group releases by it |
| C | A `name` row gives `release_count`, and no route lists those releases | Show the releases of the name level |
| D | Search indexes only music feeds and tracks (ADR 0038 Decision B) | Find a publisher feed by a word of its title |

The review also found a contract defect. `GET /v1/feeds/by-name` accepts
`include=link_facts` through the code of `GET /v1/feeds/recent`, and gives the
four fields of ADR 0068 on a publisher row. The OpenAPI document declares no
`include` parameter on that route.

On 2026-10-18, 7 albums named two publisher feeds. For each of the 7 albums, the
v4vmm rule for the name level (v4vmm ADR 0085 Decision 6) and ADR 0070 §4
gave the same result.

The request states that musicindex.org reads `GET /v1/feeds/{guid}` for each
publisher row with a stated role. The review found that `confirmPublisherRows`
in the site reads only a row with no `agreed_roles` list. Since release 0.7.0,
each such row has the list. On 2026-10-18, 0 of the first 200 rows had no list.
So gap A is a v4vmm need. The site sends no read for each row.

The operator decided the scope on 2026-10-18. The node uses `rel` facts first.
It gives an inference input only as a fallback, until more feeds state `rel`.

## Decision

### 1. A row links what it names

These three rules apply to each `v1` response. A new route follows them in its
design.

**1a. A feed title has its GUID.** A field that gives the title of another feed
has a field in the same object that gives the GUID of that feed. A field
`<prefix>feed_title` has the field `<prefix>feed_guid`. The guard of this rule
holds the list of each pair with other names, for example `remote_feed_title`
and `resolved_feed_guid` of a copy entry.

**1b. A count names its list.** A field that counts items has a route or a
filter that lists the same items. The decision that adds the count names that
list. A review checks this rule, because the contract does not record which
route lists a count.

**1c. A route declares each include.** The OpenAPI document declares each
`include` name that a route acts on. The node ignores an unknown name, as each
route does today, so a client that sends an old name gets no error.

The rules give facts. They do not let the node derive a kind. ADR 0061 §4 and
ADR 0070 §5 stay true.

### 2. An artist list row gives its link facts

`GET /v1/artist-names` takes `include=link_facts`. With it, each `publisher` row
gives:

| Field | Value |
|---|---|
| `two_way_link_count`, `stated_rels`, `agreed_roles`, `confirmed_release_artists` | The values of ADR 0068 §1 and §5 for the same feed |
| `first_named_count` | The count of the confirmed links of `two_way_link_count` whose `album_names_rank` is 1 (ADR 0069 §3a) |

A `name` row gives none of these fields. Without the include, no row gives
them.

`first_named_count` is a fallback. A client uses it only for a feed whose
`agreed_roles` is empty. It counts confirmed links only, so an album that
states a link with no answer from the feed does not change the count. The node
derives no artist from it, as ADR 0069 §3a gives.

The node calculates the fields in the build of the list in memory (ADR 0070
§4a), with the functions of the `publisher` view.

### 3. A release row names each feed of its album

Each row of `GET /v1/feeds/recent` and `GET /v1/feeds/by-name` gives
`publisher_feed_guid` beside `publisher_feed_title` (section 1a). It is the GUID
of the feed that gives the title, and it is null when the title is null.

Both routes take `include=named_feeds`. With it, each row of a music feed gives
`named_feeds`. It holds one entry for each `music_to_publisher` row of the
`publisher` view of that feed, in the sequence of `album_names_rank`. Each entry gives the
values of that row:

| Field | Value |
|---|---|
| `publisher_feed_guid` | The GUID that the item names |
| `remote_feed_title` | The title of that feed. Null when the node does not hold it |
| `album_names_rank` | ADR 0069 §3a |
| `role` | ADR 0049 §6 |
| `role_agreement` | ADR 0049 §6a |
| `two_way_validated` | ADR 0049. Added by ADR 0076 §1 |

The list is empty when the album names no publisher feed. A row of another
medium does not give `named_feeds`.

### 4. A name row lists its releases

`GET /v1/feeds/by-name` takes `artist_key`. With it, the route gives only the
music releases that the `name` row with that `key` counts in `release_count`
(ADR 0070 §3 and §4). The order, `limit` and `cursor` do not change. A key with
no `name` row gives an empty list.

The route reads the set of releases from the list in memory of ADR 0070 §4a.
The count and the list thus come from one build, and the node has no second rule
for the name level.

A request with `artist_key` and a `medium` other than `music` gives `400`.

### 5. Search finds a publisher feed on request

`GET /v1/search` takes `type=publisher`. It gives the feeds with the medium
`publisher` whose text matches `q`, in the row shape of a feed result. Each row
gives the `entity_type` `publisher`.

A search with `type=feed`, with `type=track` or with no `type` gives the same
results as before. Thus no present client gets a new row kind.

The search index holds each publisher feed as an entity of the type
`publisher`. A search with `type=feed` or with no `type` reads only the types
`feed` and `track`, so it does not change. On the first start of a release with
this decision, the node adds each publisher feed that the index does not hold.

This changes ADR 0038 Decision B for search: a publisher feed is in the index,
and only `type=publisher` gives it. A `musicL` feed and a feed with no medium
stay out of the index.

### 6. The name level does not change

ADR 0070 §4 stays. A release with a link that is not confirmed stays out of the
name level. This is also true when its first named feed has an agreed role
without `artist`.
The rule thus uses a one-side `rel` fact as its fallback, not the sequence of
the named feeds.

### 7. The changes are additions

Each field, include, filter and search type is a `v1` addition (ADR 0044 §4).
`GET /v1/node/capabilities` lists the new include names and the search type.

## Alternatives Considered

### The rule as the request states it
"The facts that a client needs" has no test. A client can always need one more
fact, and "classify" asks the node for a kind. Rejected for the testable rules
of section 1.

### Only `publisher_feed_guid` for gap B
It names only the first feed, with no `rel` facts. The client then reads each
release to find the artist feed. Rejected as the only change. Section 3 gives it
for rule 1a, and gives the full list on request.

### `first_named_count` over each link
An album that states a link with no answer from the feed would then raise the
count of a label feed. Rejected for the count over confirmed links.

### The v4vmm rule for the name level
It selects the first named feed when the links are not confirmed. The sequence
of the items is not a `rel` fact. Rejected. No album gave a different result on
2026-10-18.

### Publisher feeds in each search
Each present client would get a new row kind. Rejected for `type=publisher`.

## Consequences

- v4vmm ADR 0084 packets 016 and 017 can read the two scopes and the name level
  page with no read for each row. Packet 010 can find a label by a word of its
  title.
- The build of the artist list also reads the link facts of 1,772 publisher
  feeds. Task 001 measures the build time on the backup before and after.
- A list page with `named_feeds` reads the `publisher` view of each music row.
  A page without it costs the same as before.
- The search index grows by the publisher feeds.
- The guard of rule 1a finds `publisher_feed_title` with no GUID today. Section 3
  corrects it.

## Invariants

- Each value on a list row is the value that a read of the same feed gives at
  the same time. The node computes it with the same functions.
- The node derives no artist and no label kind.
- A response with no new include, filter or search type does not change.

## Acceptance Criteria

Mechanical, each one checked by a test:

- A guard over the OpenAPI document fails when an object has a feed title with
  no GUID field. Its message names this ADR §1a and the fix.
- A guard fails when a route acts on an `include` name that the document does
  not declare for it (§1c). It also fails when the document declares a name
  that the route ignores. `GET /v1/feeds/by-name` declares `link_facts`.
- With `include=link_facts`, a `publisher` row of `GET /v1/artist-names` gives
  the values of a read of that feed. `first_named_count` counts only the
  confirmed links with rank 1.
- A row of the two release lists gives `publisher_feed_guid`, and with
  `include=named_feeds` gives one entry for each `music_to_publisher` row, in
  rank order.
- `artist_key` gives the releases that `release_count` counts, with no gap and
  no repeat over a cursor.
- `type=publisher` finds a publisher feed by a word of its title. `type=feed`
  and no `type` give no publisher feed.

Visual, checked by the v4vmm operator:

- The v4vmm "Everything" scope shows a label feed apart from the artist feeds.
- The name level page of v4vmm shows its Index releases.
