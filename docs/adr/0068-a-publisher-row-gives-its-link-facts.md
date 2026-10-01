# ADR 0068: A Publisher Row Gives Its Link Facts

## Status
Accepted on 2026-10-01, in the opt-in form of section 1. Release 0.4.0
carries it. Amended on 2026-10-01 before the build: the decision applies to
the list route only. The search part is removed. See "Search is not in
scope".

## Date
2026-10-01

## Context
musicindex.org shows a publisher feed as an artist, a label, a publisher or a
link that is not confirmed. The page derives the type with the rule of its
own ADR 0007. It reads three facts from a full feed read. Two
come from each row of the `publisher` view: `two_way_validated` and
`publisher_rel`. The third is `confirmed_release_artists` (ADR 0061).

A row of the list `GET /v1/feeds/recent` gives no such fact. ADR 0049 §7
and ADR 0061 §3 give the derived artist fields only on a read of one feed.
A comment in the list route states this limit.

So the "Artists & Labels" list shows each row as "Publisher feed". To show the
type, the page must read each feed: 20 requests for each page of 20 rows.
musicindex.org request 7 of 2026-10-01 asks for the facts on the row.

On 2026-10-01 a publisher read with 81 links took about 40 ms of node time,
and a read with 1 link took almost none.

## Decision

### 1. A publisher row gives three link facts on request

`GET /v1/feeds/recent` takes the include name
`link_facts`. With it, each row of a feed with the medium `publisher` gives:

| Field | Value |
|---|---|
| `two_way_link_count` | The count of `publisher_to_music` rows of the `publisher` view with `two_way_validated` true |
| `stated_rels` | The different raw `publisher_rel` values of those two-way rows, sorted. Empty when no such row states one |
| `confirmed_release_artists` | The value that a read of that feed gives (ADR 0061 §1) |

Each value is the value that `GET /v1/feeds/{guid}?include=publisher` gives
for the same feed at the same time. The node computes it with the same
functions. A row of another medium gives none of the three fields. Without
`include=link_facts`, no row gives them, and the read costs what it costs
today.

`GET /v1/node/capabilities` lists `link_facts` as an include name of the
list route.

`stated_rels` holds the raw values, as the feed writes them, for example
`label` or `recordLabel`. The node does not split, change or map them. A
client reads the role set of ADR 0049 §6 from a full feed read.

### 2. The node derives no type

The node gives facts, and the client derives the type. The node does not label
a publisher as an artist or as a label (ADR 0061 §4).

### 3. The cost stays bounded

The facts are derived at read time and are not stored. The node computes them
only when a client asks, and only for the publisher rows of the page. Before
the deploy, the time of a page of 200 publisher rows with `link_facts` is
measured on a copy of the production data. When it is more than 1 second, this
decision goes back to the operator before the deploy.

## Alternatives Considered

### The facts on each row, with no request
Each client would pay for them on each page of the two routes that clients
use most, also a client that does not read them. Rejected on 2026-10-01 for
the opt-in form.

### Store the facts on the feed row
A stored count must change with each ingest of each album that names the
publisher. The read-time value cannot go stale. Rejected while the read cost
is low.

### A type field from the node
The node would decide what makes a label. ADR 0061 §4 rejects a derived kind.
Rejected.

### A batch route for publisher facts
The client would make two requests for each page and join them. The row is
the place a client already reads. Rejected.

### Search is not in scope
The first form of this decision also gave the facts on a search row. It also
gave `raw_medium` on each search row. ADR 0038 keeps publisher and `musicL` feeds
out of the search index. So no search row is a publisher feed, and each feed
row has the medium `music`. The operator removed the search part on
2026-10-01, before the build. A client finds publisher feeds with
`GET /v1/feeds/recent?medium=publisher`.

## Consequences

- The "Artists & Labels" list needs one request for each page.
- ADR 0049 §7 and ADR 0061 §3 stay true for the derived artist count and
  list. This decision adds three other facts to the list rows.
- A list page of publishers with `link_facts` costs more node time. A read
  without it costs the same as before.

## Invariants

- A list row gives the same three values as a full read of the same
  feed.
- A row of a feed that is not a publisher feed gives none of the three fields.

## Guards

- For a publisher feed with two two-way links, one with `rel="label"`, and one
  link that is not two-way: a list row gives
  `two_way_link_count` 2 and `stated_rels` `["label"]`, the same as the full
  read.
- A list row gives the same `confirmed_release_artists` as the full read.
- A music feed row gives none of the three fields.
- A list row without `include=link_facts` gives none of the
  three fields.
- `GET /v1/node/capabilities` lists `link_facts`.
