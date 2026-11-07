# ADR 0085: A Name List Takes The Role Filters, And A Track Title Filter Reads Its Feed

## Status
Accepted on 2026-11-06 by the operator, for musicindex.org request 21 and v4vmm
request 14. Tasks 001 and 002 give the build, which is complete and not deployed.
It adds to ADR 0068 §4 and ADR 0070 §2. It amends ADR 0080 §2 for `title` on a track.
It keeps the sentence "The node infers no kind" of ADR 0080 §2.

## Date
2026-11-06

## Context

### Request 21
The website gives its Artists and Labels pages the sort A to Z (musicindex ADR 0047
R-SITE-4). These pages read `GET /v1/feeds/recent` with `stated_rel`. That route has
the filter and no name order. `GET /v1/feeds/by-name` has the name order, `from`, the
cursor and `reverse`, and it has no such filter. No route gives "the labels, A to Z".

The request asks for a name order on `GET /v1/feeds/recent`. Two routes would then
give the same list, and the name "recent" would no longer describe the route.

### Request 14, part 1
The v4vmm Library matches `title:` against the track title, the album title and the
feed title (v4vmm ADR 0087, operator answer 2). The Index `title` filter on a track
reads the track title only. On 2026-11-06, `title=assorted&type=track` gave no row, and
`release=assorted&type=track` gave 7 rows of the feed "Various & Assorted".

The Library tests each term against each column separately. So one word of a title can
be in the track title and the other word in the album title.

### Request 14, part 2
The request asks that `kind=single` also match a feed with `release_kind` `unknown` and
one track. ADR 0080 §2 says that the node infers no kind. Musicindex ADR 0020 R-WORD-1
says that each app does this inference until the Podcasting 2.0 namespace gives guidance.

## Decision

### 1. `GET /v1/feeds/by-name` takes `stated_rel` and `two_way_links`
The two filters have the values, the checks and the errors of ADR 0068 §4 and §5. They
need `medium=publisher`. They give the link facts, as they do on `GET /v1/feeds/recent`.

The route keeps its sequence, `from`, its cursor and `reverse`. `GET /v1/feeds/recent`
takes no name order.

### 2. A filtered name list has the scan limit of ADR 0068 §4
One request reads at most the same count of rows as `GET /v1/feeds/recent`. So a page
can be short or empty with `has_more` true. A client follows the cursor until
`has_more` is false.

### 3. The capability `sort` also marks these two filters
A list route ignores a parameter that it does not know. So a node with no such change
gives each publisher to a `stated_rel` request, with no error. These filters ship in
the same release as ADR 0084. A client sends them on this route only when
`GET /v1/node/capabilities` lists `sort` (ADR 0084 §11). This ADR adds no token.

### 4. `title` on a track also reads the title of its feed
This changes one row of the table of ADR 0080 §2:

| Parameter | `type=feed` | `type=track` | `type=publisher` |
|---|---|---|---|
| `title` | the feed title | the track title or the title of the feed of the track | the publisher title |

A search with no `type` applies the `type=feed` rule to a feed row and the `type=track`
rule to a track row.

Each word and each phrase of the value can match either title. `title=love song` matches
a track "Love" on the album "Song". A phrase in double quotes must match in one title.

`release` does not change, so it still reads only the title of the feed. No filter now
reads only the title of the track.

### 5. `not_title` follows `title`
ADR 0080 §3 says that a `not_` parameter applies to the same field as its positive
parameter. So `not_title=love` removes a track "Love", and each track on an album "Love".

### 6. The node still infers no kind
The rule of a kind stays with the apps (R-WORD-1). An app can send R-WORD-1 as one
request for each type today, because each feed of the backup of 2026-11-05 has
`release_kind` `unknown`:

| The person types | The app sends | Music feeds in the backup |
|---|---|---|
| `kind:single` | `kind=unknown&tracks=1` | 6,228 |
| `kind:album` | `kind=unknown&min_tracks=2` | 2,617 |
| `kind:ep` | No Index request, because a count never gives EP | 0 |

Musicindex owns this mapping. The code can live in `musicindex-lib` when an ADR accepts
that subject. This ADR gives the facts, and it states no rule for the apps.

### What this ADR does not decide
- How a client finds a stated kind if the namespace gives guidance. A feed that states a
  kind does not match `kind=unknown`, so this mapping must then change. That decision
  comes with the change of the ingest.
- A name order for `GET /v1/feeds/recent`.

## Evidence
The backup of 2026-11-05 holds 10,688 feeds, and each one has `release_kind` `unknown`.
It holds 8,869 music feeds: 6,228 with one track, 2,617 with two or more, and 24 with no
track. R-WORD-1 gives "Album" to a feed with no track, and `min_tracks=2` does not find
these 24 feeds.

`search.html` sends `stated_rel=label` and `stated_rel=artist` on its list groups.

The backup holds 1,808 publisher feeds, so a filter that keeps few rows reads to the scan
limit on most requests. These are measurements of one local copy with a release build, at
`limit=50`. They are not a load test:

| Request | Time |
|---|---|
| `GET /v1/feeds/by-name`, each filter, each direction, first page | 0.43 to 0.52 s |
| `GET /v1/feeds/by-name`, each filter, each direction, second page | 0.34 to 0.39 s |
| `GET /v1/feeds/recent`, the same filters, first page | 0.51 to 0.54 s |
| `GET /v1/feeds/by-name` with no filter | 0.003 s |

The first page of `stated_rel=label` holds no row, and the first reversed page holds 2. So
a client follows the cursor to fill a list. The cost comes from the shared scan loop, which
reads the link facts of each row. This ADR accepts that cost, because it is the cost of the
filters that the site uses today. The node keeps these facts in the memory copy of the
artist list (ADR 0071 §2), so a later change can use that copy.

## Alternatives Considered

### A name order on `GET /v1/feeds/recent`
This is the form of the request. Two routes then give one list, each with its own
`from` and cursor rules to keep the same.

### A new parameter for the two titles
`title` keeps its meaning, and a new parameter reads the two titles. The filter would
exist only for the meaning that each app wants. The website does not offer the fields
yet (`research_notes/search/shared-query-text.md`), and v4vmm asks for this meaning.

### Two requests for the title
An app sends `title` and `release`, and merges the rows. The two sets can overlap, so
the app must remove repeats across pages. An "either title" match belongs in one query.

### `kind` with the count rule in the node
This is the form of request 14, part 2. The node then states a kind that no feed states,
and ADR 0080 §2 rejects that. The stated value and the inferred value also cannot be
told apart when a real stated kind arrives.

### A new parameter for the count rule
This keeps `kind` as the stated value, but the node still holds the rule of R-WORD-1.
The mapping of section 6 needs no change to the node.

## Consequences
- The website can show A to Z on Artists and Labels with `GET /v1/feeds/by-name`.
- A `stated_rel` or `two_way_links` request on this route with a `medium` other than
  `publisher` now gives `400`. Before this ADR the route ignored the two parameters.
- A `title` search on a track can give more rows than before. It can give each track of
  an album whose title matches. A person who wants one track can add a word of `q`.
- v4vmm Known Difference 1 of ADR 0087 closes. Its Known Difference 2 closes when v4vmm
  sends the mapping of section 6.
- `min_tracks=2` leaves out 24 music feeds with no track. Musicindex decides whether to
  record this as a known difference.
- A filter still does not change `rank` (ADR 0080 §2).

## Invariants
- `kind` matches the stated `release_kind` only.
- `release` reads only the title of the feed.
- A row of a filtered name list has the same link facts as the same row of
  `GET /v1/feeds/recent` with the same filter.
- The sequence of a filtered name list is the sequence of the same list with no filter.

## Acceptance Criteria

### Mechanical
Tests in `tests/adr0085_name_filter_tests.rs` prove each of these:

1. `GET /v1/feeds/by-name?medium=publisher&stated_rel=label` keeps only those rows, in
   name order, over all pages, in each direction, and with `from`.
2. `two_way_links=none` keeps only the rows with `two_way_link_count` 0.
3. Each error of ADR 0068 §4 has its condition on this route.
4. A filtered request stops at the scan limit, with `has_more` true and a cursor.
5. `title` on `type=track` finds a track by its own title and by the title of its feed,
   with `q`, with no `q`, and with no `type`.
6. A two-word `title` can have one word in each title. A phrase in double quotes cannot.
7. `not_title` removes a track by its own title and by the title of its feed.
8. `release` on a track still reads only the title of the feed. `title` on `type=feed` and
   `type=publisher` does not change. `rank` does not change with a `title` filter.
9. `kind=single` matches no feed with `release_kind` `unknown`.

### Visual
None in this repository.

## Follow-Up
- [Task 001](../tasks/adr-0085-task-001-by-name-filters.md) adds the two filters.
- [Task 002](../tasks/adr-0085-task-002-track-title-filter.md) changes `title` on a track.
- Stophammer answers musicindex.org request 21 and v4vmm request 14.
