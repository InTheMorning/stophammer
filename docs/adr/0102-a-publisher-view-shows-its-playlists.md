# ADR 0102: A Publisher View Shows Its Playlists

## Status
Accepted on 2026-11-17. Release 0.21.0 of 2026-11-17 holds it with §5a.

- Revised: 2026-11-17, musicindex.org request 32 and HitIt request 17. §5a
  adds `unconfirmed_publisher` to a playlist row. The revision is Accepted on
  2026-11-17.

- Bar: crosses repositories. musicindex.org and HitIt read the new fields, and
  a shared musicindex ADR for the display uses their names.
- Changes: [ADR 0101](0101-a-publisher-feed-can-list-playlists.md) §5, the
  first item.
- Changes: [ADR 0049](archive/0049-publisher-relationships-are-rss-facts.md)
  §3 and §4
- Changes: [ADR 0099](0099-the-lists-and-their-rows.md) items 14 and 51
- Uses: ADR 0049 §6 and §6a for the role and the confirmed link.
- Uses: ADR 0069 §1a for the publisher reference, and ADR 0044 §4 for a `v1`
  addition.

## Date
2026-11-16

## Context

ADR 0101 lets a publisher feed list `musicL` feeds. The index holds such a
link, but no read shows it. ADR 0101 §5 kept the read view out of scope until
a client asked for it.

On 2026-11-16 the orchestrator session sent a request of musicindex.org and
HitIt for the link. The operator made these choices on 2026-11-16:

- The suite shows a curator relation in full.
- The Playlists section of the website and the Public playlists section of
  HitIt show each curator with its confirmed playlists, then "Other
  playlists".
- Each confirmed publisher link of a playlist counts, not only
  `rel="curator"`. The `rel` value sets only the label.

The request has six items. Items 1 to 6 of the Decision answer them in the
same sequence.

Facts from the code on 2026-11-16:

- `publisher_direction` (`src/query.rs`) gives a row only for an item with the
  medium `music` or `publisher`.
- `music_to_publisher_facts` reads only the `medium="music"` items of the
  publisher feed for the back-link. So a playlist row never has
  `two_way_validated` true.
- `link_facts` counts only `publisher_to_music` rows.
- HitIt reads `direction` as text (`HitIt/src/api.rs`). Each music read of
  HitIt filters `direction = 'music_to_publisher'`. HitIt stores a row with a
  new direction value and does not use it.

Facts from the production copy `stophammer-20261115T115439Z.db`:

- 1 of 1,811 publisher feeds has a `newest_item_at`.
- The 15 `musicL` feeds have no `newest_item_at`. 13 have a `release_date`,
  and 11 have a `last_build_date`.

So a sort by `newest_item_at` cannot order the curators.

## Decision

### 1. A publisher feed gives a row for each playlist

This section changes ADR 0049 §4 and ADR 0101 §5.

On a feed with the medium `publisher`, `include=publisher` gives one row for
each channel item with the medium `musicL`. The medium comparison ignores ASCII
case. The row has the direction `publisher_to_playlist`. A `musicL` item on a
feed of another medium gives no row.

The row has each field of a `publisher_to_music` row, with these values:

| Field | Value |
|---|---|
| `publisher_feed_guid`, `publisher_feed_url` | The publisher feed that is read |
| `music_feed_guid`, `music_feed_url` | The playlist feed |
| `remote_feed_medium` | The stored medium of the playlist feed, or null |
| `remote_feed_title`, `remote_feed_image_url` | The title and the image of the playlist feed (ADR 0059) |
| `publisher_lists_music` | true |
| `music_names_publisher` | true when the publisher reference of the playlist resolves to this publisher feed |
| `publisher_rel` | The raw `rel` of the `musicL` item |
| `music_rel` | The raw `rel` of the publisher reference of the playlist |
| `two_way_validated` | true when both sides name each other |
| `role`, `role_source`, `role_agreement` | As ADR 0049 §6 and §6a give them |

The publisher reference of a playlist is its item with the source
`podcast_publisher` (ADR 0069 §1a). `album_names_as` and `album_names_rank`
read that item, as on a `publisher_to_music` row.

### 2. A playlist confirms its publisher reference

This section changes ADR 0049 §3.

On a feed with the medium `musicL`, the `music_to_publisher` row of its
publisher reference reads the `musicL` items of the publisher feed for the
back-link. On a feed of each other medium, the row reads the `music` items, as
before. Thus an album is never confirmed by a `musicL` item, and a playlist is
never confirmed by a `music` item.

When the back-link matches, `publisher_lists_music`, `reciprocal_declared` and
`two_way_validated` are true. `reciprocal_medium` is `musicL`. `publisher_rel`
is the `rel` of the matched `musicL` item. The direction of the row stays
`music_to_publisher`.

A confirmed playlist link is a row of §1 or §2 with `two_way_validated` true
and a `role_agreement` that is not `conflict` (ADR 0049 §6a). Each `rel` value
counts.

### 3. The playlist count is separate

This section changes ADR 0099 item 14.

A new field, `two_way_playlist_link_count`, gives the count of the confirmed
`publisher_to_playlist` rows of a publisher feed. It is on these rows:

- Each row that gives `two_way_link_count`, with the same include. These are
  the `publisher` rows of `GET /v1/feeds/recent`, `GET /v1/feeds/by-name`,
  `GET /v1/artist-names` and `GET /v1/search`.
- `GET /v1/feeds/{guid}` of a feed with the medium `publisher`, with no
  include. A feed of each other medium does not give the field.

A playlist is not a release. A `publisher_to_playlist` row does not change
`two_way_link_count`, `stated_rels`, `agreed_roles`,
`confirmed_release_artists`, `first_named_count`, `release_track_counts`,
`release_count`, `newest_release_at` or `GET /v1/publisher-links/stats`.

### 4. The change only adds to `v1`

Each change of this ADR adds a field, a value or a route to `v1` (ADR 0044 §4).
The new direction value does not change a row of the directions
`music_to_publisher` and `publisher_to_music`. A client that filters on these
two values reads the same rows as before. The values of §2 change only on a
row of a `musicL` feed.

### 5. A playlist row names its confirmed publisher

Each row with the medium `musicL` of `GET /v1/feeds/recent` and
`GET /v1/feeds/by-name` gives `confirmed_publisher`. The field needs no
include. A row of each other medium does not give it.

| Field | Value |
|---|---|
| `feed_guid` | The GUID of the publisher feed |
| `title` | The title of the publisher feed, or null |
| `role` | The `role` of the link (ADR 0049 §6), or null |
| `role_agreement` | `both` or `one_side` |

The value is the confirmed link of §2 of the publisher reference. It is null
when the playlist has no confirmed link. A playlist has at most one publisher
reference, so it has at most one confirmed publisher.

### 5a. A playlist row names its unconfirmed publisher

Each row that gives `confirmed_publisher` (§5) also gives
`unconfirmed_publisher`. The field needs no include.

| Field | Value |
|---|---|
| `feed_guid` | The `publisher_feed_guid` of the `music_to_publisher` row of the publisher reference |
| `title` | The title of that publisher feed, or null when the node does not hold it |

The value is the publisher reference of the playlist when its
`music_to_publisher` row is not a confirmed link (ADR 0049 §6a). This occurs
when the publisher feed does not list the playlist, or when the two sides
state different roles. The value is null when the playlist has no publisher
reference, or when the link is confirmed. Thus a row gives at most one
non-null value of `confirmed_publisher` and `unconfirmed_publisher`.

### 6. A list of the publisher feeds with playlists

A new route, `GET /v1/feeds/playlist-publishers`, gives each publisher feed with
a `two_way_playlist_link_count` of 1 or more.

- Each row is the `publisher` row of `GET /v1/feeds/recent` with
  `include=link_facts`, with `two_way_playlist_link_count` and
  `newest_playlist_at`.
- `newest_playlist_at` is the newest date of the confirmed playlists of the
  row, in Unix seconds. The date of one playlist is its `release_date`, else
  its `last_build_date`. The value is null when no confirmed playlist has a
  date.
- The rows sort by `newest_playlist_at`, newest first. A row with null comes
  last. `feed_guid` sorts rows with the same value.
- The route takes `limit` and `cursor`, as `GET /v1/feeds/recent` does. The
  maximum of `limit` is the list maximum of 200.

The node selects the candidates with one SQL query: each publisher feed with a
channel item with the medium `musicL`. It computes the confirmed links of each
candidate at read time with the functions of §1. It stores no result.

### 7. One capability names the addition

`GET /v1/node/capabilities` names `playlist_links` in `capabilities`. The name
covers §1 to §6. A client calls the route of §6 only when the node lists the
name (ADR 0099 item 52).

## Alternatives Considered

### Keep the direction `publisher_to_music` for a playlist row

A client could then read a playlist as an album. The website filters on the
direction and would show the playlist in the release list. Rejected.

### A new direction on the playlist side too

A new direction such as `playlist_to_publisher` is possible on the playlist row.
HitIt and the website read the `music_to_publisher` row of a feed as the
publisher that the feed names. That read is also correct for a playlist. A new
value would hide the curator from them. Rejected.

### Count a playlist in `two_way_link_count`

The release count, the artist count and the role filters of ADR 0099 would
then count playlists as releases. Rejected by the operator choice of
2026-11-16.

### A filter on `GET /v1/feeds/recent`

`playlist_links=some` with `medium=publisher` could give item 6. The route
sorts by `newest_item_at`, and a publisher feed has none. A filtered request
examines at most 1,000 rows (ADR 0099 item 25), and the index holds 1,811
publisher feeds. So the filter can miss a curator. Rejected.

### A filter on `type=publisher` search

The search sorts by `newest_item_at` with `sort=date` (ADR 0100). The same
fault applies. Rejected.

### Count only `rel="curator"`

The operator chose on 2026-11-16 that each `rel` value counts. Rejected.

## Consequences

- A curator page needs one read of `include=publisher` on the publisher feed.
- A playlist page gets its curator from the playlist row of a list, with no
  second read.
- The Playlists section of the website needs one read of the route of §6 and
  one read of the playlist list.
- A read of `include=publisher` on a publisher feed reads one more feed for
  each `musicL` item.
- The route of §6 computes the links of each candidate at each read. The index
  holds no such candidate on 2026-11-16, so the cost is small. A later
  decision can add a copy in memory, as for `GET /v1/artist-names`.
- The website and HitIt start their display work with the field names of this
  ADR.

## Acceptance

Each criterion is mechanical. No criterion is visual, because the display is
the work of the clients.

1. A router test: a publisher feed with one `musicL` item, and a playlist that
   names that publisher feed. `include=publisher` on the publisher feed gives
   one `publisher_to_playlist` row with `two_way_validated` true.
2. A router test: the same playlist gives one `music_to_publisher` row with
   `two_way_validated` true and `reciprocal_medium` `musicL`.
3. A router test: an album that a publisher feed lists only with `musicL` gives
   a `music_to_publisher` row with `two_way_validated` false. A playlist that a
   publisher feed lists only with `music` gives `two_way_validated` false.
4. A router test: a publisher feed with one confirmed album and one confirmed
   playlist gives `two_way_link_count` 1 and `two_way_playlist_link_count` 1
   with `include=link_facts` on `GET /v1/feeds/recent`, `GET /v1/feeds/by-name`,
   `GET /v1/artist-names` and `GET /v1/search`. `GET /v1/feeds/{guid}` gives
   `two_way_playlist_link_count` 1.
5. A router test: a playlist with a confirmed link gives `confirmed_publisher`
   with the GUID, the title and the role of the publisher feed on
   `GET /v1/feeds/recent?medium=musicL`. A playlist with a one-way link gives
   null.
5a. A router test: a playlist with a one-way link gives
   `unconfirmed_publisher` with the GUID and the title of the named publisher
   feed, and `confirmed_publisher` null. A playlist with a confirmed link gives
   `unconfirmed_publisher` null. A playlist with no publisher reference gives
   null in the two fields.
6. A router test: `GET /v1/feeds/playlist-publishers` gives the two publisher
   feeds with confirmed playlists, newest `newest_playlist_at` first. A
   publisher feed with only one-way playlist links is not in the list.
7. A unit test: `GET /v1/node/capabilities` names `playlist_links`.
8. The OpenAPI document and `tests/fixtures/v1-schema-fields.json` name each
   new field and the new route.
