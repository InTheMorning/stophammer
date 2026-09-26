# ADR 0059: An Entry That Names A Feed Gives Its Summary

## Status
Accepted on 2026-09-25

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

## Consequences

- A publisher page and a `musicL` page need one request.
- A read with `include=publisher` or `include=remote_items` does one more
  lookup for each entry.
- v4vmm gets the values with different names than it asked for. The answer in
  its request file says so.

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
