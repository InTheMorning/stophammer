# ADR 0068: A Publisher Row Gives Its Link Facts

## Status
Accepted on 2026-10-01, in the opt-in form of section 1. Release 0.4.0
carries it. Amended on 2026-10-01 before the build: the decision applies to
the list route only. The search part is removed. See "Search is not in
scope". Amended on 2026-10-02 with section 4, the two filters of
musicindex.org request 10. Amended on 2026-10-02 with section 5: the link
facts are role tokens, musicindex.org request 11.

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

### 4. The list can keep only the rows with a link fact

Amended on 2026-10-02, musicindex.org request 10. On 2026-10-02 the list held
1,772 publisher feeds. Only 4 rows stated a `rel`, and only 15 rows had no
two-way link. To show these rows, the page read the full list.

With `medium=publisher`, `GET /v1/feeds/recent` takes two filters:

| Parameter | Rows that it keeps |
|---|---|
| `stated_rel=<value>` | Rows with `<value>` in `stated_rels`. The node compares the raw value, as section 1 gives it |
| `two_way_links=none` | Rows with `two_way_link_count` 0 |

Each filter gives the link facts on each row, as `include=link_facts` does.
The two filters together keep the rows that agree with both. A filter with
another medium, an empty `stated_rel`, or a `two_way_links` value that is not
`none` gives `400`.

The node computes the facts for each row that it examines, in the sequence of
the route. It examines at most 1,000 rows for one request. When it reaches
that limit, it gives the rows that it kept, and a cursor at the last row that
it examined. So a page can hold fewer rows than `limit`, or none, and
give `has_more` true. A client follows the cursor until `has_more` is false.

The filter compares facts that a row gives. It does not derive a type
(section 2). On a copy of the production data of 2026-09-26, a pass over all
publisher rows with the facts took 801 ms of node time.

### 5. The link facts are role tokens

Amended on 2026-10-02, musicindex.org request 11. A `rel` value is a set of
role tokens (ADR 0049 §6, podcast-namespace PR #793). A feed that states
`rel="artist host author label producer"` has five roles. A raw value cannot
be filtered by one of them.

The facts of section 1 change, and one is added. Each counts only confirmed
links (ADR 0049 §6a):

| Field | Value |
|---|---|
| `two_way_link_count` | The count of confirmed links |
| `stated_rels` | The different role tokens of `publisher_rel` over the confirmed links, normalized as ADR 0049 §6 gives, and sorted |
| `agreed_roles` | The different role tokens of the confirmed links with `role_agreement` `both`, sorted. Added |
| `confirmed_release_artists` | As before, over the confirmed links |

`stated_rel=<value>` of section 4 keeps a row when the normalized `<value>` is
one of its `stated_rels` tokens. A raw value with a comma or with white space
no longer matches as one string.

This changes the meaning of `stated_rels`. Section 1 said that the node does
not split a `rel` value. That sentence no longer holds.

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
- A filtered request examines at most 1,000 rows. A client that shows the
  rows of one type reads one or two requests, not the full list.

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
- Section 4: with `stated_rel=label`, the list gives only the publisher rows
  that state `label`, with their link facts. With `two_way_links=none`, it
  gives only the rows with no two-way link.
- Section 4: a filtered request that reaches the limit of examined rows gives
  a cursor and `has_more` true, and the next request continues after the last
  examined row.
- Section 4: a filter with `medium=music`, or `two_way_links=some`, gives
  `400`.
