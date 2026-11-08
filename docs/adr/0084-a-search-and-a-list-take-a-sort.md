# ADR 0084: A Search And A List Take A Sort

## Status
Accepted on 2026-11-06 by the operator, for v4vmm request 15. Tasks 001 to 003 give
the build, which is complete. The primary runs this work in the candidate `v0.19.0-rc.6`, and no release carries it.
It adds to ADR 0070 §2 and §3 and to ADR 0080 §6. ADR 0080 §2 does not change.

## Date
2026-11-06

## Context
v4vmm request 15 asks for a sort direction on `GET /v1/feeds/recent`,
`GET /v1/feeds/by-name`, `GET /v1/artist-names` and `GET /v1/search`. Its reason
is musicindex ADR 0043 R-LIST-7. A reverse icon reverses the whole list, and an
app that cannot do this shows no icon.

The three list routes each have one sequence, so a direction is sufficient for
them. Search is different. A search with a word in `q` has rank order, and a
search with no word has newest first (ADR 0080 §6). Neither one has a name order.
Thus a direction alone cannot give what the request asks for. v4vmm ADR 0089 also
gives Newest as the sort of the results, and a search with a word cannot give
that sequence today.

A search does two different tasks. `q` and the filters select the rows. A sort puts
the rows in a sequence. Today the first task also decides the second task.

## Decision

### 1. A filter selects, and a sort orders
`q` and the filters decide which rows match. The new parameters decide only the
sequence of these rows. ADR 0080 §2 stays true: a filter does not change the
rank of a row.

### 2. `GET /v1/search` takes `sort`
| `sort` | Usual sequence | Key |
|---|---|---|
| `relevance` | Best match first | `effective_rank`, as ADR 0022 and ADR 0078 give |
| `name` | A to Z | The sort key of ADR 0070 §1 |
| `date` | Newest first | `newest_item_at` of a feed or a publisher, `pub_date` of a track |

Each type of row uses the key of its own record. A feed and a publisher use
`feeds.title_sort`, and a track uses `tracks.title_sort` (section 8). A request
with no `type` merges feeds and tracks by the key, as ADR 0080 §6 does by date.

`relevance` needs a word in `q`. A value filter has no words, and a text filter
does not change the rank.

### 3. Each of the four routes takes `reverse`
`reverse=true` reverses the usual sequence. It is one parameter on each route, so
it agrees with R-LIST-7 item 1: the order changes, and the sort does not change.

| Route | Usual sequence | With `reverse=true` |
|---|---|---|
| `GET /v1/feeds/recent` | Newest first | Oldest first |
| `GET /v1/feeds/by-name` | A to Z | Z to A |
| `GET /v1/artist-names` | A to Z | Z to A |
| `GET /v1/search`, `sort=date` | Newest first | Oldest first |
| `GET /v1/search`, `sort=name` | A to Z | Z to A |

"Oldest first" uses the same key as Newest. On a feed it is `newest_item_at`, so
the first feed is the feed whose latest item is the oldest.

`sort=relevance` does not reverse, because the worst match would then come first.
A client shows no reverse icon on it (R-LIST-7 item 10).

The filters and the fields of a row do not change with `reverse`. This includes
the scan limit of ADR 0068 §4.

### 4. The defaults do not change
A request with neither parameter gives the sequence and the response of today.

- With a word in `q`, the default sort is `relevance`.
- With no word in `q`, the default sort is `date`.

### 5. Errors
Each search error uses the body of ADR 0080 §4. The checks of that section run
first, so `missing_positive_term` stays the first result of a request with no
word and no positive filter.

| Code | Condition |
|---|---|
| `unsupported_sort` | `sort` has a value that section 2 does not give, or `reverse` is not `true` or `false`. `filter` names the parameter |
| `relevance_needs_word` | `sort=relevance`, and `q` has no word |
| `relevance_cannot_reverse` | `reverse=true`, and the sort is `relevance`, also by default |

`reverse` accepts only `true` and `false`. Any other value gives `400` on each
route.

### 6. A row with no value comes last
A row with no key comes after each row with a key, in the two directions. This
is the rule of ADR 0080 §6, and it now applies to `GET /v1/feeds/recent`.

So Oldest is not an exact mirror of Newest. A client that sorts its own rows into
such a list uses the same rule.

The tie key follows the direction: `feed_guid` on the two feed lists, `sort_name`,
`kind` and `key` on the artist list, and `rowid` on search.

### 7. `from` in Z to A
With `reverse=true`, `from` starts the list at the last row whose sort key starts
with the sort key of `from`. When no key has that start, the list starts at the
last row before that position.

A plain reverse of the compare is not correct. `from=M` would start below each
title that starts with "m", because "madonna" sorts after "m".

### 8. A track stores its sort key
`tracks.title_sort` holds the sort key of the track title. The same function,
`model::sort_key`, makes it. The field `title_lower` is not this key, because it
does not collapse white space.

A new migration adds the column. As with `feeds.title_sort`, a Rust fill in
`open_db` sets each null value, and each write of a track derives it. The column
holds no new value from a feed body, so ADR 0077 needs no refill pass.

The column has no index at the start. The current search query starts from the
search index rows and sorts after a join, so an index cannot serve that query form.
A query with only `type=track` could be written to use an index. An index does not
change the API, so a later change can add one for a query that is slow.

### 9. A cursor holds its sort and its direction
A cursor names the sort, the direction, the key and the tie key of its last row.
A cursor with a different sort or direction than the request gives `400`. A
cursor from before this change has the default sort and the usual direction, so
it stays valid.

ADR 0080 §6 already states that a cursor is valid only with the parameters of the
request that gave it. This section makes the node check it for these parameters.

### 10. A row gives the value of its sort
A client merges its Library rows with Index rows, so it needs the key of each row
(ADR 0070 §1 gives this reason for a list).

- With `sort=name`, each search row gives `sort_name`.
- With `sort=date`, each search row gives `sort_date`. The value is absent on a
  row with no date.

A request with no `sort` does not give these fields.

### 11. The node states the sort in its capabilities
`GET /v1/node/capabilities` adds `sort` to the list `capabilities`, as ADR 0070 §6
adds `name_lists`.

A list route ignores an unknown parameter. So a node with no such change gives the
usual sequence to a `reverse=true` request, with no error. A client shows a reverse
icon on an Index list only when the node states `sort`.

### What this ADR does not decide
- An "exact match first" rule for a text filter.
- A total for a list (v4vmm draft request 13).
- The sort Newest of Artists. `GET /v1/artist-names` has no date, and v4vmm ADR 0089
  states that a name has no source for it.
- The prefix match of [the research request](../plans/research-search-index-request.md).
- The `title` and `kind` changes of v4vmm draft request 14.

## Evidence
The backup of 2026-11-05 holds 10,688 feeds, of which 8,869 are music feeds, and
28,555 tracks. The search index has 39,231 rows.

| Fact | Count |
|---|---|
| Music feeds with no `newest_item_at` | 30 |
| Feeds of each medium with no `newest_item_at` | 1,848 |
| Publisher feeds with no `newest_item_at` | 1,807 of 1,808 |
| Tracks with no `pub_date` | 32 |
| Music titles that start with "m" | 498 |
| Music titles that are exactly "m" | 0 |
| Music titles that sort after "z" | 109 |

The query plans of the backup show these results:

- `GET /v1/feeds/by-name` scans `idx_feeds_title_sort_guid` in each direction.
- `GET /v1/feeds/recent` sorts each row in a temporary B-tree in each direction. A
  page took approximately 6 ms in each direction.

These are one run of each SQL form, on a local copy, with `title_lower` as the
track key. They include no summary of a row, and they are not a load test.

| SQL form | Matching rows | Time of one page |
|---|---|---|
| All feed and track rows, by name, reversed | 37,423 | 0.55 s, the first read |
| Tracks that match `love`, by name | Part of 1,179 | 0.18 s |
| Rows that match `the`, by date | 9,273 | 0.15 s |

ADR 0080 §6 also records that the first track search with no `q` after a restart
took 14.5 s, and approximately 195 ms after that. A sort of this ADR can have the
same cold cost.

## Alternatives Considered

### A direction with no sort
This is the form of the request. It reverses the rank of a search with a word, and
it gives no name order on search. Only a search with no word could then use the
reverse icon.

### A rank from the words of a filter
The filter words go into the bm25 score, so a search with no `q` has a relevance.
ADR 0080 §2 rejects this. The backup gives the 12 most common artist names. For
these names, `name : "<artist>"` had a correlation of 0.82 to 0.98 with the length
of the row text. Golden Bambiha has 8 different scores for 253 rows. The rank then
sorts by short text, not by a better match.

### `order=asc` and `order=desc`
The usual direction of date is descending and that of name is ascending. So one
value would mean the usual sequence on one sort and the reversed sequence on the
other. `reverse` states what R-LIST-7 means.

### An SQL function in place of the track column
A scalar function can call `model::sort_key` for each row, with no migration. Each
reader connection must register it, and each sort calls it once for each matching
row. The column follows `feeds.title_sort`.

### Feeds with no date first in Oldest
This is the exact mirror of Newest. The first page of a music list at the default
limit of 50 would then hold 30 feeds with no date. A list with `medium=all` would
start with 1,848 of them.

## Consequences
- v4vmm can show the reverse icon on the Index lists and on a search with the sort
  `name` or `date`. It keeps no reverse icon on `relevance`.
- `sort=date` with a word gives v4vmm the sequence Newest on its results.
- A client that sorts each page again must not do so with `sort=name` or
  `sort=date`. The research report states that the website sorts each page by
  `rank`.
- The sort `date` on `type=publisher` has little meaning, because almost each
  publisher feed has no date. Most of these rows come in `rowid` order. A client
  can leave this entry out of its menu.
- Z to A starts with titles that sort after "z", such as emoji and other scripts.
  The first music title of the backup is 🖖. This agrees with the code point order
  of ADR 0070 §1.
- The `refresh` and `podcastindex` modes of the crawler send neither parameter, so
  they do not change.
- A primary and a community node can upgrade in each sequence, because a client
  reads section 11.
- The change adds one migration and one fill at the start of the node.
- With no index, each page of `sort=name` or `sort=date` reads and sorts its matching
  rows again. So a client keeps `relevance` for a live search, and sends `sort=name`
  or `sort=date` only when a person selects that sort.

## Invariants
- A request with neither `sort` nor `reverse` gives the sequence and the fields of
  the release before this ADR.
- A filter never changes `rank`.
- A row with no sort key comes last in each direction.
- A cursor continues only the sort and the direction that gave it.
- The sequence of a list is complete over all pages. No row repeats or goes missing
  between two pages when no write occurs.

## Acceptance Criteria

### Mechanical
Tests in a new file, `tests/adr0084_sort_tests.rs`, prove each of these:

1. Each route of section 3 gives the reversed sequence over all pages, with no row
   that repeats or goes missing.
2. Each `sort` of section 2 gives its sequence, with a word and with no word, for
   each `type` and for no `type`.
3. A request with no new parameter gives the same sequence and fields as before.
4. Each code of section 5 has its condition.
5. A row with no key comes last in each direction, on `GET /v1/feeds/recent` and on
   search.
6. `from=M&reverse=true` starts at the last title that starts with "m".
7. A cursor with a different sort or direction gives `400`, and a cursor from before
   this change continues.
8. `tracks.title_sort` equals `model::sort_key(title)` after the fill and after a
   write.
9. `GET /v1/node/capabilities` lists `sort`.
10. The ADR 0044 guards pass with the new parameters and fields in the OpenAPI
    document and in `tests/fixtures/v1-schema-fields.json`.

### Visual
None in this repository. v4vmm and the website own the reverse icon. musicindex
ADR 0043 gives their visual checks.

## Follow-Up
- [Task 001](../tasks/adr-0084-task-001-track-sort-key.md) adds the track sort key.
- [Task 002](../tasks/adr-0084-task-002-list-routes-reverse.md) adds `reverse` to the
  three list routes.
- [Task 003](../tasks/adr-0084-task-003-search-sort.md) adds `sort` and `reverse` to
  search, and the capability.
- v4vmm writes its packet 005 after the release.
