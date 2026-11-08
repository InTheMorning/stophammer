# ADR 0091: An Artist List Takes The Role Filters

## Status
Accepted on 2026-11-08 by the operator, for musicindex.org request 28. The build
is complete, and no candidate carries it.

It adds to ADR 0089 §2, ADR 0071 §2 and ADR 0059 §1. ADR 0068 §4 and §5 do not
change.

## Date
2026-11-08

## Context
musicindex.org request 28 asks for a Newest sort on the Artists, Labels and No
role pages of the site. It also asks for a Newest sort on the releases of an
artist or a label feed.

The site reads these pages from `GET /v1/feeds/recent?medium=publisher` with
`stated_rel` or `two_way_links`. That list sorts by the `newest_item_at` of the
publisher feed. On 2026-11-08, only 1 of the first 200 rows had a date. So
Newest gave the sequence of the tie key.

ADR 0089 §2 gives `newest_release_at` on each row of `GET /v1/artist-names`.
For a `publisher` row, it is the newest date of the music releases that name
the feed. That route has no role filter.

The rows of `include=publisher` on `GET /v1/feeds/{guid}` give no date.

## Decision

### 1. `GET /v1/artist-names` takes `stated_rel` and `two_way_links`
The two parameters take the values of `GET /v1/feeds/recent` (ADR 0068 §4 and
§5):

- `stated_rel` keeps the rows with that role token in `stated_rels`.
- `two_way_links=none` keeps the rows with `two_way_link_count` 0.
- An empty `stated_rel`, two role tokens, or another `two_way_links` value
  gives `400`.

With a filter, the list gives only the `publisher` rows that match. A `name`
row has no link facts, so it never matches. The set of the `publisher` rows is
the set of `GET /v1/feeds/recent?medium=publisher` with the same filter.

The filters work with `sort=name`, `sort=date`, `reverse`, `from` and the
cursor. The cursor format does not change. The copy in memory holds the link
facts (ADR 0071 §2), so a page reads no database row. A filtered page reads the
rows of the sequence and skips each row that does not match. The list has
approximately 2,100 rows.

### 2. Each `include=publisher` row gives `newest_item_at`
The value is the `newest_item_at` of the named feed, from the same read as
`remote_feed_title` (ADR 0059 §1). It is null when the node holds no feed for
the entry, or when the feed has no dated item.

- On a `publisher_to_music` row, the named feed is the album, so the value is
  the date of the release.
- On a `music_to_publisher` row, the named feed is the publisher feed. Most
  publisher feeds have no dated item, so the value is mostly null.

### 3. Two capabilities name the change
`GET /v1/node/capabilities` lists `artist_role_filter` for §1 and
`publisher_row_date` for §2. A node with no `artist_role_filter` ignores the two
parameters on `GET /v1/artist-names` and gives each row. So a client sends them
only when the flag is listed.

### What this ADR does not decide
- A date sort on `GET /v1/feeds/recent` by the dates of the releases.
- A sort of the `include=publisher` rows. The client sorts them.

## Alternatives Considered

### A release date on `GET /v1/feeds/recent`
The list could give `newest_release_at` on each publisher row, and sort by it.
That list sorts in SQL by a stored column, and its filter examines at most
1,000 rows (ADR 0068 §4). A sort by the release date then needs a stored
column and a migration. It also needs a write to the publisher row at each
ingest of a music feed that names it.

The copy in memory already holds the date and the link facts.

### The date only on a `publisher_to_music` row
The field is then absent on some rows of one array. One rule for each row is
easier to read, and the null value tells the same fact.

## Consequences
- The site can give Newest on the Artists, Labels and No role pages, and on the
  releases of an artist or a label feed.
- A publisher row of `GET /v1/artist-names` gives `name`, `image_url`,
  `feed_guid`, `newest_release_at` and, with `include=link_facts`, the link
  facts. It does not give the other fields of a feed row.
- Each change is an addition. No event, no table and no dependency changes, so
  it belongs in a MINOR release (ADR 0066).

## Invariants
- With a filter, `GET /v1/artist-names` gives no `name` row.
- The filtered set of `GET /v1/artist-names` equals the set of
  `GET /v1/feeds/recent?medium=publisher` with the same filter.
- A page of `GET /v1/artist-names` reads no database row.

## Acceptance Criteria

### Mechanical
Tests in `tests/adr0091_role_filters_and_row_date_tests.rs` prove each of these:

1. §1: for `stated_rel=artist`, `stated_rel=label`, `two_way_links=none` and no
   filter, the set of the `publisher` rows of `GET /v1/artist-names` equals the
   set of `GET /v1/feeds/recent?medium=publisher` with the same filter.
2. §1: a filter gives no `name` row.
3. §1: `sort=date` with a filter gives the newest first, and the oldest first
   with `reverse=true`.
4. §1: pages of 1 row give each matching row one time, in each direction and
   with each `sort`.
5. §1: `from` with a filter starts at the first matching row at or after the
   text.
6. §1: an empty `stated_rel`, two role tokens and another `two_way_links` value
   each give `400`.
7. §2: a `publisher_to_music` row gives the date of the album, and an entry
   that the node does not hold gives null.
8. §3: the capabilities list `artist_role_filter` and `publisher_row_date`.

The guards of ADR 0044 cover the two parameters and the field `newest_item_at`
of `PublisherResponse`.

### Visual
None in this repository. The site owns its sort controls.
