# ADR 0059: An Entry That Names A Feed Gives Its Summary

## Status
Accepted on 2026-09-25

Amended on 2026-10-01, accepted the same day, for release 0.4.0: section 5 applies the same rule to an entry
that names a track. musicindex.org request 5 reported that a playlist page
needs one request for each entry to show the title of the track.

Amended on 2026-10-18, v4vmm request 7: section 1 adds
`remote_episode_count`.

Amendment proposed on 2026-10-25, musicindex.org requests 17 and 18. Section 6
adds the image and the title of the indexed feed to each row of
`GET /v1/copies` and `GET /v1/live-items`. Section 7 adds the summary of
sections 1 and 5 to each remote value time split. The amendment is not
accepted. "Open Questions" gives the decisions that the operator makes.

## Date
2026-09-25

## Context
Two read views give entries that name a different feed:

- The `publisher` view of ADR 0049. Each entry has `remote_feed_guid`, the feed
  on the other side. On a publisher feed, the direction is
  `publisher_to_music`, and the other side is an album. On an album, the
  direction is `music_to_publisher`, and the other side is the publisher.
- The `remote_items` view. Each entry has `remote_feed_guid` and
  `remote_feed_url` of one `podcast:remoteItem`.

No entry gives the title, the image or the artist of the named feed. A client
reads each named feed with one more request. On 2026-09-25 the publisher
"James Goulding" lists 81 albums, and a publisher in an earlier database
listed 131. The `musicL` feed "Local Theory — Full Catalog" names 13 feeds.

v4vmm request 1 asks for these values on each `publisher` entry, with the
names `music_feed_title`, `music_feed_image_url`, `music_release_artist` and
`music_release_artist_source`. musicindex.org request 1 asks for the same
values on each `remote_items` entry, with the names `remote_feed_title`,
`remote_feed_image_url`, `remote_release_artist` and
`remote_release_artist_source`. The requests are in
[the v4vmm requests](../plans/v4vmm-open-requests.md) and
[the musicindex.org requests](../plans/musicindex-open-requests.md).

ADR 0042 requires that a field name says which record gives the value. ADR
0044 section 4 makes an added field a change inside `v1`.

On 2026-10-21, musicindex.org asked for the same rule on three more rows.
Request 17: a row of `GET /v1/copies` and a row of `GET /v1/live-items` give
`feed_guid`, but not the image of that feed. A live row also gives no title of
that feed. The search site reads each feed to show its cover, so a page of 20
rows sends 20 more requests. Request 18: a remote value time split of ADR 0073
gives `remote_feed_guid` and `remote_item_guid`, but no title. The track page
reads each named track, with at most 12 requests for one track page.

## Decision

### 1. The summary fields

Each `publisher` entry and each `remote_items` entry, on a feed read and on a
track read, gets these fields:

| Field | Value |
|---|---|
| `remote_feed_title` | The `title` of the named feed |
| `remote_feed_image_url` | The channel image URL of the named feed |
| `remote_release_artist` | The `release_artist` of the named feed |
| `remote_release_artist_source` | The `release_artist_source` of the named feed |
| `remote_episode_count` | The `episode_count` of the named feed. Added on 2026-10-18 |

Each field is null when the node holds no feed for the entry. Each value is a
stored value of the named feed. The node derives none.

The prefix `remote_` names the feed of `remote_feed_guid`, in both views and
in both directions. The prefix `music_` of v4vmm fits only the direction
`publisher_to_music`, so this ADR does not use it.

### 2. Which feed the entry names

- A `publisher` entry names the feed that its resolution of ADR 0049 section
  3 gives: by GUID, then by URL. An `unresolved` entry gives null.
- A `remote_items` entry resolves its feed in the same way, with
  `resolve_listed_feed`: by `remote_feed_guid`, then by `remote_feed_url`.

The node resolves at each read, as ADR 0049 section 3 requires. It stores no
summary.

### 3. The image follows ADR 0042 and ADR 0054

`remote_feed_image_url` is the channel image of the named feed, not a resolved
image. The read route passes it through `web_url_or_none` of ADR 0054 section
4.

### 4. The cost stays on the node

Each entry costs one point lookup by primary key. A publisher with 131 albums
costs 131 lookups in one request, not 132 requests from a client. The read of
the largest publisher is measured before and after the change.

### 5. An entry that names a track gives its summary

Since ADR 0060, a `remote_items` entry gives `remote_track_guid` when the
index holds the track that the entry names. Such an entry also gives:

| Field | Value |
|---|---|
| `remote_track_title` | The stored `title` of the track |
| `remote_track_duration_secs` | The stored duration of the track, or null |
| `remote_track_image_url` | The stored image of the item, through `web_url_or_none`, or null |

Each field is null when `remote_track_guid` is null. The image is the image of
the item, not a resolved image, as section 3 gives for a feed. The values come
from the same lookup that gives `remote_track_guid`, so section 4 adds no
lookup.

### 6. A list row gives the image of its feed

Proposed on 2026-10-25, musicindex.org request 17. Each row of these two lists
gets these fields, for the feed that `feed_guid` of the row names:

| Route | Field | Value |
|---|---|---|
| `/v1/copies` | `feed_image_url` | The channel image URL of the feed |
| `/v1/live-items` | `feed_title` | The `title` of the feed |
| `/v1/live-items` | `feed_image_url` | The channel image URL of the feed |

- Each field is null when the node holds no feed for `feed_guid`, as section 1
  gives. Each image passes through `web_url_or_none`, as section 3 gives.
- The prefix `feed_` names the feed of `feed_guid`. The row of `/v1/copies`
  already gives `title`, the title of that feed.
- The image of a copy row is the image of the indexed feed, not the image of
  a copy. A copy gives its own image only on `GET /v1/feeds/{guid}/copies`
  (the archived ADR 0058 §1c).
- The handler of `/v1/copies` already reads the feed of each row, so the image
  adds no query. A live row adds one point read by primary key.

### 7. A remote value time split gives its summary

Proposed on 2026-10-25, musicindex.org request 18. Each row of
`include=value_time_splits` on a track read gets the fields of section 1 and
section 5:

- `remote_feed_title`, `remote_feed_image_url`, `remote_release_artist`,
  `remote_release_artist_source` and `remote_episode_count`.
- `remote_track_guid`, `remote_track_title`, `remote_track_duration_secs` and
  `remote_track_image_url`.

Rules:

- A split names its feed by `remote_feed_guid` and gives no URL. Thus the node
  resolves the feed by GUID only, as the first step of `resolve_listed_feed`.
- `remote_track_guid` is the `track_guid` of the track of that feed whose
  `track_guid` is `remote_item_guid`. It is null when the feed does not
  resolve, when `remote_item_guid` is `""` (ADR 0073 §4), or when the node
  holds no such track.
- Each value follows sections 1, 3 and 5. The fields of ADR 0073 §4 do not
  change.
- A local split names no feed, so `include=local_value_time_splits` does not
  change.

Each remote split costs at most two point reads by primary key. The backup of
2026-10-25 holds 18 remote splits on 4 tracks, with at most 8 on one track.
Each of the 18 names an indexed feed, and 4 name an indexed track. Thus the
largest track read does at most 16 more reads, in place of 8 more requests from
a client.

## Alternatives Considered

### The names of each request
Two sets of names for one value makes a client learn both. The `music_` names
are incorrect for an entry of the direction `music_to_publisher`. Rejected.

### A batch route for feed summaries
A route that takes a list of GUIDs gives the same values in two requests, not
one. It is a new route for one purpose. Rejected.

### Store the summary with the entry
The summary would become stale when the named feed changes. ADR 0049 section 3
rejects a stored resolution for the same reason. Rejected.

### An include for each new summary (sections 6 and 7)
Sections 1 and 5 give the summary with no include. A new include name gives
two forms of one row, and a client must know both. Rejected.

## Consequences

- A publisher page and a `musicL` page need one request.
- A read with `include=publisher` or `include=remote_items` does one more
  lookup for each entry.
- v4vmm gets the values with different names than it asked for. The answer in
  its request file says so.
- With sections 6 and 7, the Copies and Live pages of musicindex.org need one
  request for each page. A track page needs one request for its splits.
- A read of `/v1/live-items` does one more point read for each row. A track
  read with `include=value_time_splits` does at most two more for each remote
  split.

## Invariants

- A summary field holds a stored value of the feed that the entry resolves
  to, or null.
- The node stores no summary.

## Guards

- A publisher read with `include=publisher` gives the title, the image and
  the artist of each indexed album.
- An album read with `include=publisher` gives the summary of the publisher.
- A `remote_items` entry for an indexed feed gives its summary. An entry for
  a feed that is not indexed gives null in each summary field.
- An entry that resolves by URL gives the summary of the feed at that URL.
- A named feed with a `javascript:` image gives a null image.
- A `musicL` entry that names an indexed track gives its title, duration and
  item image. An entry for a track that is not indexed gives null in each of
  the three fields.
- A row of `/v1/copies` gives the image of its feed. A feed with a
  `javascript:` image gives a null image.
- A row of `/v1/live-items` gives the title and the image of its feed.
- A remote split that names an indexed feed and an indexed track gives the
  summary of each. A split with `remote_item_guid` `""` gives the feed summary
  and null in each track field. A split that names a feed that is not indexed
  gives null in each summary field.

## Open Questions

The operator answers each question before the amendment is accepted.

1. Does a split give `remote_episode_count`? Request 18 does not ask for it.
   Proposed: yes, so that a split and a `remote_items` entry give one set of
   names.
2. Is `feed_image_url` the name on a copy row? Proposed: yes. The request asks
   for it, and a feed hit of `GET /v1/search` gives the channel image with the
   same name.
3. Does this ADR own sections 6 and 7, or a new ADR? Proposed: this ADR,
   because sections 1 and 5 state the rule, and the two requests only add rows
   to it. ADR 0064 §6, ADR 0073 §4 and the archived ADR 0058 §3 keep the other
   fields of each row.
