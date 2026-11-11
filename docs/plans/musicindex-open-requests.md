# musicindex.org Open Requests To Stophammer

## Status

Open - 2026-09-25. Updated on 2026-10-21: requests 1 to 15, 19 and 20 are
closed. Updated on 2026-10-25: release 0.16.0 deployed requests 17 and 18 (ADR
0059 §6 and §7). Updated on 2026-11-06: requests 21 and 22 are open. Updated on 2026-11-07: Stophammer answers request 21, release 0.19.0 gives it, and requests 23 to 25 are open. Requests 26 and 27 are open. Updated on 2026-11-08: Stophammer answers requests 27 to 30. On 2026-11-08 the live node gives request 30, and request 30 is closed. Stophammer answers request 22. Updated on 2026-11-10: Stophammer answers requests 23, 24 and 26, and request 24 is closed. This document consolidates each open request from the
musicindex.org web client: the search site (`search.html`) and the API console
(`api.html`).

The Stophammer repository and the musicindex repository each hold a copy at
`docs/plans/musicindex-open-requests.md`. Update both copies together.

This document binds nothing in Stophammer. Stophammer records each decision in
its own ADR. The v4vmm client keeps its own list in
`docs/plans/v4vmm-open-requests.md`. Section "Requests That v4vmm Also Makes"
names the requests that the two clients share.

## Evidence Base

- Live API: read-only GET requests to `https://api.musicindex.org` on
  2026-09-24 and 2026-09-25, and the contract at `/openapi.json` on 2026-09-25.
  On 2026-09-25 the contract lists `/v1/copies`, `/v1/feeds/{guid}/copies`
  and `/v1/feeds/{guid}/route-history`, and no proof routes. Feed reads give
  `copy_count`. So the deploy of those changes is complete.
- Stophammer source: the local checkout, read-only, at commit `6e4c5de` on
  2026-09-25. No build was run.
- musicindex source: `search.html` at commit `967ae3d`. Line numbers refer to
  that commit.

## Requests

| # | Request | Kind | Priority |
|---|---|---|---|
| 1 | Summary fields on each `remote_items` entry | API field addition | Deployed on 2026-09-26 (ADR 0059) |
| 2 | `itemGuid` and `title` on remote items | API field addition | Deployed on 2026-09-26 (ADR 0060) |
| 3 | Summary fields on search results | API field addition | Deployed on 2026-09-26 |
| 4 | A stated maximum for `limit` | Contract correction | Deployed on 2026-09-26 |
| 5 | Track summary fields on each list entry | API field addition | Deployed in release 0.4.0 |
| 6 | The deployed revision in release images | Regression | Deployed in release 0.3.0 |
| 7 | Link and role facts on publisher feed rows | API field addition | Deployed in release 0.4.0 for the list, and in release 0.13.0 for search (ADR 0076 §3) |
| 8 | An index for the route history read | Performance | Deployed in release 0.4.1 |
| 9 | Track titles and an image on each copy row | API field addition | Deployed by release 0.7.0 (ADR 0058 §1c) |
| 10 | Link fact filters on the publisher feed list | API parameter addition | Deployed in release 0.5.0 |
| 11 | Publisher link rules of namespace PR #793 | Contract change | Deployed in release 0.7.0 on 2026-10-02 |
| 12 | The feed list of a copy of a publisher feed or a playlist | API field addition | Deployed in release 0.8.0 on 2026-10-08 |
| 13 | Item facts on each copy row | API field addition | Deployed in release 0.10.0 on 2026-10-18 |
| 14 | The advertised page of each live item | API field addition | Deployed in release 0.9.0 on 2026-10-18 |
| 15 | A website link that has no scheme | Contract change | Deployed in release 0.9.0 on 2026-10-18 |
| 16 | One Nostr row for each `npub` of a feed | API field addition | Declined on 2026-10-17 |
| 17 | The image of the indexed feed on each copies row and live row | API field addition | Deployed in release 0.16.0 on 2026-10-25 (ADR 0059 §6) |
| 18 | Summary fields on each remote value time split | API field addition | Deployed in release 0.16.0 on 2026-10-25 (ADR 0059 §7) |
| 19 | The value time splits of a feed that did not change after ADR 0073 | Data refill | Refilled on 2026-10-21 for the four feeds in the fetch cache |
| 20 | No copy of two feeds whose old album files stay public | Data question | Copies recorded on 2026-10-22 |
| 21 | A name order for each list of feeds | API parameter addition | Live in release 0.19.0 (ADR 0085) |
| 22 | HTML entities in a description | Data correction | Answered on 2026-11-08. The next crawler image decodes them (ADR 0017) |
| 23 | A search finds a playlist | Search behavior | Open |
| 24 | A faster role filter on the publisher lists | Performance | Open |
| 25 | A title match before a description match | Search behavior | Open |

### 1. Summary Fields On Each `remote_items` Entry

**What happens.** A feed read with `include=remote_items` gives one entry for
each `podcast:remoteItem`. On 2026-09-25 each entry gives `position`,
`medium`, `remote_feed_guid`, `remote_feed_url`, `rel` and `source`. No entry
gives a title or an image.

**What it costs.** The feed page shows the remote items as a "Related feeds"
rail of covers with titles. `search.html` reads each named feed separately
(`fetchFeedSummaries`, line 1886, called at line 2454): a maximum of 24 feeds,
six at a time. The musicL feed "Local Theory — Full Catalog" names 13
feeds, so its page sends 14 requests.

**Request.** Add these fields to each `remote_items` entry, for the feed that
`remote_feed_guid` names:

| Field | Value |
|---|---|
| `remote_feed_title` | The `<title>` of the named feed. Null when the node has not indexed it |
| `remote_feed_image_url` | The channel image URL of the named feed. Null when it states none |
| `remote_release_artist` | The `release_artist` of the named feed |
| `remote_release_artist_source` | The `release_artist_source` of the named feed |

Each value is a value that the named channel states, or its existing source
label. None of them is new derived data.

This is the same rule as v4vmm request 1, which asks for these fields on the
`publisher` entries. One rule can close both: each entry that names a feed
carries the title, the image and the artist of that feed.

### 2. `itemGuid` And `title` On Remote Items

**What happens.** `podcast:remoteItem` has two optional attributes that point
to one item and name it: `itemGuid` and `title`. The Podcast Namespace defines
both. Stophammer does not store them for feed-level remote items, and the API
does not return them. The only stored remote item GUID is in
`value_time_splits.remote_item_guid` (`src/schema.sql`).

**What it costs.** A `musicL` feed is a playlist. A playlist of tracks names
each track with `feedGuid` and `itemGuid`. Without `itemGuid`, a client can
show only the feeds that a playlist names, not its tracks. The musicindex.org
playlist page is designed for a track list, and it waits for this request.

**Request.** Store and return these fields on each `remote_items` entry:

| Field | Value |
|---|---|
| `remote_item_guid` | The `itemGuid` attribute. Null when the element states none |
| `remote_item_title` | The `title` attribute. Null when the element states none |
| `remote_track_guid` | The `track_guid` of the indexed item that `remote_feed_guid` and `remote_item_guid` name. Null when the node has not indexed it |

The first two are stated values. The third is a lookup, as
`publisher_link_resolution` is for publishers.

### 3. Summary Fields On Search Results

**What happens.** On 2026-09-25 a `/v1/search` track result gives
`entity_type`, `entity_id`, `rank`, `quality_score`, `feed_guid`, `href`,
`title`, `feed_title`, `track_image_url`, `feed_image_url` and `pub_date`. A
feed result gives `entity_type`, `entity_id`, `rank`, `quality_score`,
`title` and `feed_image_url`.

**What it costs.** Each result row shows the artist. A feed row also shows the
track count, and a track row the duration (`resultLines`, line 2710). To get
these values, `search.html` reads the full record of each result (line 2956).
A page of 20 results costs 21 requests, and each automatic "load more" costs
21 more.

**Request.** Add these fields to each search result:

| Result | Field | Value |
|---|---|---|
| feed | `release_artist` | The stored `release_artist` of the feed |
| feed | `release_artist_source` | Its source label |
| feed | `episode_count` | The stored track count of the feed |
| track | `track_artist` | The stored artist of the track |
| track | `duration_secs` | The stored duration of the track |

Each value is already stored for the entity.

### 4. A Stated Maximum For `limit`

**What happens.** On 2026-09-25, `/v1/feeds/recent?medium=all` gave 150 rows
for `limit=150`, and 200 rows for `limit=200`, `201`, `500` and `1000`. Each
response had `has_more: true` and status 200. The contract describes `limit`
only as "Maximum rows to return.", with no maximum.

**What it costs.** A client that asks for more than 200 rows gets 200, with
no error and no signal. `has_more` is also true for a full page, so a client
cannot tell a cap from an exact page.

**Request.** State the maximum of `limit` for each list route in the contract,
for example as `maximum: 200` in the parameter schema. Alternatively, answer
400 above the maximum.

### 5. Track Summary Fields On Each List Entry

Added on 2026-10-01. Evidence: read-only GET requests to the live API on
2026-10-01.

**What happens.** Since ADR 0060, each `remote_items` entry of a `musicL`
feed gives `remote_track_guid` when the index holds the track. It gives no
title, duration or image of that track. `remote_item_title` is the optional
`title` attribute of the element, and the indexed playlists do not state it.
On 2026-10-01:

| Playlist | Entries | With `remote_track_guid` | With `remote_item_title` |
|---|---|---|---|
| "Boostagram Ball Playlist 1 to 25" | 278 | 269 | 0 |
| "Lightning Thrashes Playlist episodes 1 - 60" | 383 | 367 | 0 |

**What it costs.** The playlist page shows each entry as a row. Without the
track title, a row can show only the feed of the track. To show the track
title and the duration, the page must read each track: 269 or 367 requests
for one playlist. The page also cannot give the total time of a playlist.

**Request.** Add these fields to each `remote_items` entry that has a
`remote_track_guid`, for that track:

| Field | Value |
|---|---|
| `remote_track_title` | The `<title>` of the item |
| `remote_track_duration_secs` | The stored duration of the track. Null when the item states none |
| `remote_track_image_url` | The item image URL. Null when the item states none |

Each value is null when `remote_track_guid` is null. This is the rule of
ADR 0059, applied to the track that an entry names. Each entry that names an
entity carries the summary of that entity.

### 6. The Deployed Revision In Release Images

Added on 2026-10-01. Evidence: `GET /node/info` on 2026-09-26 and on
2026-10-01.

**What happens.** On 2026-09-26, `GET /node/info` gave `git_revision`
(`607bb3a`, then `264706e` and `9b6dc21`) and `built_at`. On 2026-10-01, with
release `0.2.0`, it gives `"git_revision": null` and `"built_at": null`.

**What it costs.** A client cannot tell which build serves a response. v4vmm
request 2 asked for these fields, and its answer added them. The release
image does not fill them.

**Request.** Fill `git_revision` and `built_at` in the release image. If the
release version replaces them, state that in the contract, and give the
version in `/node/info`.

### 7. Link And Role Facts On Publisher Feed Rows

Added on 2026-10-01. Evidence: read-only GET requests for all 1,772 publisher
feeds on 2026-10-01. The page rule is musicindex ADR 0007.

**What happens.** The search site shows a publisher feed as an artist, a
label, a publisher or an unverified link. It gets this type from the
`publisher_to_music` rows of a full feed read with `include=publisher`:
`two_way_validated`, `publisher_rel` and `confirmed_release_artists`. A row of
`/v1/feeds/recent?medium=publisher` gives none of these fields. A search row
does not give `raw_medium`, so the page cannot identify a publisher feed in
the search results.

**What it costs.** The Browse list "Artists & Labels" shows each row as
"Publisher feed". To show the type of each row, the page must read each feed:
20 requests for each page of the list. On 2026-10-01, 15 of 1,772 publisher
feeds had no two-way link, and 1 stated `rel="label"`. The list cannot show
these feeds differently.

**Request.** Add these fields to each row with medium `publisher`, in
`/v1/feeds/recent` and in `/v1/search`:

| Field | Value |
|---|---|
| `raw_medium` | On search rows. The list rows give it at this time |
| `two_way_link_count` | The count of `publisher_to_music` rows with `two_way_validated` |
| `stated_rels` | The different `publisher_rel` values of the two-way rows, such as `["label"]`. Empty when the feed states none |
| `confirmed_release_artists` | The value of the feed read (ADR 0061) |

These are RSS facts, as ADR 0049 requires. The page derives the type, with
the rule of its ADR 0007. Stophammer derives no type.

### 8. An Index For The Route History Read

Added on 2026-10-02. Evidence: timed read-only GET requests to the live API on
2026-10-02, and the Stophammer source, read-only, at commit `de3a7ee`.

**What happens.** `GET /v1/feeds/{guid}/route-history` takes 0.22 to 0.36
seconds for each of 20 feeds. A feed read with tracks, `/copies` and
`/v1/live-items` each take about 0.05 seconds from the same client. An answer
of 2,683 bytes and an answer of 86,969 bytes take almost the same time.

`get_route_history_events_for_feed` in `src/db.rs` selects from `events`
with this condition:

```sql
WHERE (event_type = 'feed_routes_replaced' AND subject_guid = ?1)
   OR (event_type = 'routes_replaced' AND json_extract(payload_json, '$.feed_guid') = ?1)
   OR (event_type = 'track_upserted' AND json_extract(payload_json, '$.track.feed_guid') = ?1)
```

SQLite cannot use an index for `json_extract` on `payload_json`. Thus each read
examines each row of the `events` table that has the two event types.

**What it costs.** The search site reads the route history when a viewer
pushes "Show route changes" (musicindex ADR 0008). The operator found the
answer slow on 2026-10-02. The time increases with the number of events in
the log, not with the number of tracks in the feed.

**Request.** Make this read use an index. One method is a stored column, or a
generated column, for the feed GUID of each `routes_replaced` and
`track_upserted` event. Add an index on that column, and select on it. The answer stays
the same. Only the plan of the query changes.

### 9. Track Titles And An Image On Each Copy Row

Added on 2026-10-02. Evidence: read-only GET requests to the live API on
2026-10-02, and Stophammer ADR 0058. The page rule is musicindex ADR 0009.

**What happens.** A row of `GET /v1/feeds/{guid}/copies` gives the title, the
URL, the two differences, `feed_recipients` and `track_recipients`. The node
stores the ordered item GUIDs of each copy (ADR 0058 §1), but the row does not
give them. The only item GUIDs in the row are the keys of
`track_recipients`, and these name only the items with their own recipients.
The row gives no item title and no image.

On 2026-10-02, the copy of "THERAPY IN SESSION"
(`190dd27e-02b3-440d-9d1c-38e2304d93b3`) at
`https://headstarts.uk/msp/nat-hills-music/Nat_Hills_Music.xml` has one item
that the indexed record does not have:
`a6e153ad-6433-4d67-a7de-f7b276bf19aa`. The search site can show only its
GUID.

**What it costs.** The search site compares a copy with the indexed feed. For
an item that only the copy has, the page shows a GUID and no title. For an
item with no own recipients, the page cannot tell if the copy has it. The
copy view has no cover, because the row gives no image.

**Request.** Add these fields to each copy row:

| Field | Value |
|---|---|
| `item_guids` | The ordered item GUIDs of the copy, as the node stores them |
| `item_titles` | The `<title>` of each item, in the same sequence |
| `image_url` | The channel image URL of the copy. Null when the copy states none |

These are values from the parsed body of the copy, as ADR 0058 §1 reads it.
The node keeps no other part of the body.

### 10. Link Fact Filters On The Publisher Feed List

Added on 2026-10-02. Evidence: read-only GET requests for all 1,772 publisher
feeds with `include=link_facts` on 2026-10-02. The page rule is musicindex
ADR 0007.

**What happens.** The search site gives artists, labels and other publisher
feeds each a Browse item of its own. It gets the type of a row from
`two_way_link_count`, `stated_rels` and `confirmed_release_artists`.
`/v1/feeds/recent` cannot filter on these values. On 2026-10-02:

| Type | Feeds | Position in the list |
|---|---|---|
| Artist | 1,751 | All through the list |
| Label | 1 | Row 1,372 |
| Unverified link (no two-way link) | 15 | All through the list |
| Publisher (two-way links, no role) | 5 | All through the list |

**What it costs.** To show the one label, the page must read the full list:
9 requests of 200 rows, 4.6 seconds from a fast client. The cost increases
with the number of publisher feeds.

**Request.** Add two filters to `/v1/feeds/recent` with
`medium=publisher`. Each one keeps the cursor paging of the route:

| Parameter | Rows that it keeps |
|---|---|
| `stated_rel=<value>` | Rows with that value in `stated_rels` |
| `two_way_links=none` | Rows with `two_way_link_count` 0 |

These are filters on RSS facts that the list gives at this time. The page
keeps its own rule for the artist type, which uses the names. Thus the five
feeds with two-way links and no role continue to need a full read of the
list.

### 11. Publisher Link Rules Of Namespace PR #793

Added on 2026-10-02. Evidence: Podcast Namespace pull request
<https://github.com/Podcastindex-org/podcast-namespace/pull/793>, open and not
merged on 2026-10-02. The Stophammer source, read-only, at `cf4659e`:
`stophammer-parser/src/engine.rs` (`extract_feed_remote_items`) and
`src/query.rs` (`normalize_rel`, `resolve_role`). The live API at release
0.6.0, and the RSS of the HeyCitizen feeds on 2026-10-02.

**What PR #793 states.** `<podcast:publisher>` holds one `remoteItem` for each
party. `rel` is a set of tokens separated by white space. A link counts only
when the two feeds name each other. Then:

- When the two sides give `rel` and the token sets are equal, the link has
  those roles.
- When the two sides give `rel` and the sets are different, the app
  discards the link.
- When one side does not give `rel`, the link keeps its first meaning: the
  entity publishes the feed. It has no role.
- An app ignores `rel` on a `remoteItem` that is not in a publisher link.

**What Stophammer does.** The parser keeps each `remoteItem` in
`<podcast:publisher>`, in sequence. `normalize_rel` compares token sets. These
agree with the PR. These parts do not agree:

| PR #793 | Stophammer 0.6.0 |
|---|---|
| Different sets: the app discards the link | `two_way_validated` stays `true`. `role` is null and `role_source` is `conflict`. The link counts in `two_way_link_count`, `confirmed_release_artists` and `co_credited_feeds` |
| One side gives `rel`: no role | `role` is the value of that side, with `role_source` `publisher_rel` or `music_rel` |
| `rel` only on publisher links | ADR 0069 reads a bare `medium="publisher"` item that is not in `<podcast:publisher>` as a credit, and confirms and counts it |
| A set of tokens | `stated_rels` (ADR 0068 §1) holds raw values, and `stated_rel=` (§4) compares the raw value |

On 2026-10-02, 4 links of the index had different sets: three of Sir Libre
Records (`label` and `recordLabel`) and one of Crash Landing (`artist` and
`wrongRel`). At 20:55 UTC the three HeyCitizen feeds changed to
`rel="artist host author label producer"` on each side. The node read them
last at 20:02 UTC. After the next read, the raw `stated_rels` value is
`artist host author label producer`, so `stated_rel=artist` and
`stated_rel=label` do not find the feed.

**What it costs.** The search site follows the PR (musicindex ADR 0011). A
list row gives only the `rel` of the publisher side, so the page cannot know
from the row if the two sides agree. It reads each publisher feed that
gives a `rel` in full. It cannot use `stated_rel=` for a set of tokens.

**Request.** If Stophammer follows PR #793:

1. Give `stated_rels` as tokens, and make `stated_rel=` match one token.
2. Tell a role that the two sides give from a role that one side gives.
   For example, a field `role_agreement` with `both`, `one_side`,
   `conflict` or null. Or give `role` only when the two sides agree.
3. Show a link with different sets apart from the confirmed links, or leave
   it out of `two_way_link_count`, `confirmed_release_artists` and
   `co_credited_feeds`.
4. Record in ADR 0069 that a credit that is not in `<podcast:publisher>`
   is an extension of the PR, or remove it.
5. Give the agreed roles of each two-way link in the list facts. A list can
   then show the role without a full read.

### 12. The Feed List Of A Copy Of A Publisher Feed Or A Playlist

Added on 2026-10-02. Evidence: read-only GET requests to the live API at
release 0.7.0, and the RSS of the two URLs of one GUID on 2026-10-02.

**What happens.** A copy row gives `item_guids` and `item_titles` (ADR 0058
§1c). A publisher feed and a `musicL` feed list feeds as channel-level
`podcast:remoteItem` elements, not as items. So for these feeds the row
gives no content to compare.

On 2026-10-02 the GUID `4d25f0dd-9270-4fb7-8aeb-ddd4a5213585` had two
publisher feeds:

| URL | `remoteItem` feeds |
|---|---|
| `https://headstarts.uk/msp/publisher-feeds/longy-everything-publisher-feed.xml` (indexed) | 24 |
| `https://wavlake.com/feed/artist/4d25f0dd-9270-4fb7-8aeb-ddd4a5213585` (copy) | 18 |

No feed GUID was in the two lists. The copy row gives `item_guids: []` and
`differs_tracks: false`.

**What it costs.** The copy view of the search site (musicindex ADR 0009 and
0013) cannot show that the two versions list different albums. It tells the
person to open the RSS of the copy. `differs_tracks` is `false`, so the copy
facts also do not show the difference.

**Request.** Keep the channel-level `remoteItem` feed GUIDs of a copy in
sequence when the medium is `publisher` or `musicL`. Give them on the copy
row, for example as `remote_feed_guids`. Compare them as ADR 0058 §2 compares
the items, so that a different list sets a difference flag.
The page gets the title of each indexed feed with one read.

### 13. Item Facts On Each Copy Row

Added on 2026-10-03. Changed on 2026-10-18 to the scope that Stophammer
accepted. Evidence: read-only GET requests to the live API at release 0.7.0
on 2026-10-03.

**What happens.** The search site compares a copy with the indexed feed in
two columns (musicindex ADR 0014). For each item of the indexed feed it shows
the title, the duration and the recipients. For each item of the copy, the row
gives the GUID, the title (`item_titles`) and the recipients
(`track_recipients`). It gives no duration, no audio URL and no publication
date. For the channel, it gives no author and no description.

On 2026-10-03 the copy of "THERAPY IN SESSION"
(`190dd27e-02b3-440d-9d1c-38e2304d93b3`) had the item "Exist", which the
indexed feed does not have. The page can show its title and its recipients,
but not its length or its audio.

**What it costs.** A person cannot see if a track of the two versions is the
same recording. The audio URL and the duration are the facts that show it.

**Request.** Keep these values from the parsed body of a copy, as ADR 0058
§1c keeps the titles. Keep them out of the summary digest, as §1c does:

| Field | Value |
|---|---|
| `item_durations` | The duration of each item in seconds, in the sequence of `item_guids`. Null when an item gives none |
| `item_enclosure_urls` | The enclosure URL of each item, in the same sequence |
| `item_pub_dates` | The publication date of each item, in the same sequence |
| `author` | The channel `itunes:author` of the copy |

A different enclosure URL for the same item GUID can show a different
recording. The client compares the URLs and shows the difference. The index
does not count it as a difference of ADR 0058 §2, because a mirror can serve
the same recording from a different URL.

### 14. The Advertised Page Of Each Live Item

Added on 2026-10-17. Changed on 2026-10-17 to the scope that Stophammer
accepted. Evidence: read-only GET requests to the live API and the RSS file of
"Pixel Palace" (`d1dca4bb-1966-59d7-9aff-c1eda9f31fb7`) on 2026-10-17.

**What happens.** The `podcast:liveItem` "Jorfest 2026" gives
`<link>https://www.pixelpalace.party/</link>`. That is the page that the show
advertises. A live item row gives `content_link` (here an HLS stream) and
`live_value_uri` (here a curiohoster.com event URL with the protocol
`socket.io`). It does not give the `<link>`.

**What it costs.** The search site cannot send a viewer to the show
(musicindex ADR 0016). It must use the website of the feed, or no link.

**Request.** Give the RSS `<link>` of each live item as `link` on each row of
`/v1/live-items` and of `live_items` in the feed read. Null when the item gives
none. Only the RSS `<link>` element gives the value, and the web URL rules of
Stophammer ADR 0054 §4 and §4a apply. The node already stores the value, so
this changes only the read.

### 15. A Website Link That Has No Scheme

Added on 2026-10-17. Changed on 2026-10-17 to the scope that Stophammer
accepted. Evidence: the same reads as request 14, and the Stophammer backup
of 2026-09-26.

**What happens.** On 2026-10-17 the channel of "Pixel Palace" gave
`<link>www.pixelpalace.party</link>`. The ADR 0054 §4 rule gave `url: null`
for this `website` row, because the value had no scheme. At 03:00 UTC on
2026-10-18 the feed gave `https://www.pixelpalace.party`, so this feed is
correct now. Other feeds can give a value with no scheme. In the backup, 13 of
9,956 `website` values had no scheme. One of them, `The Fraglebay`, is not a
host name.

**What it costs.** The feed page shows no website for the feed, although the
feed advertises one.

**Request.** When a `website` or `web_page` link, or the live item `link` of
request 14, has no scheme and starts with a strict host name, read it as
`https://` and the value. Mark the row with a flag, so that a client knows
that the index added the scheme. Each other value with no scheme stays null.
The stored raw value does not change. Stophammer ADR 0054 §4a gives the host
name rule.

### 16. One Nostr Row For Each `npub` Of A Feed

Added on 2026-10-17. Declined by Stophammer on 2026-10-17. Evidence: the feed
read of "The Heycitizen Experience" (`a2d2e313-9cbd-5169-b89c-ab07b33ecc33`)
with `include=source_contributors` on 2026-10-17.

**What happens.** The feed gives the same `npub` on seven contributor rows,
one for each role. The page must merge the rows to show one Nostr handle
(musicindex ADR 0016).

**Decision.** The client groups the contributor rows by `npub`. The rows
already give each fact that a merged row gives. A second shape of the same
facts adds work to the index and gives no new fact.

### 17. The Image Of The Indexed Feed On Each Copies Row And Live Row

Added on 2026-10-21. Evidence: read-only GET requests to the live API at
revision `71aa360` (release 0.13.2) on 2026-10-21.

**What happens.** A row of `GET /v1/copies` gives `feed_guid`, `feed_url`,
`title`, `copy_count`, `copies_over_limit` and `newest_first_seen`. A row of
`GET /v1/live-items` gives `feed_guid` and the facts of the live item. Neither
row gives the image of the indexed feed that `feed_guid` names. A live row also
gives no title of that feed.

**What it costs.** The Browse items Copies and Live show a cover for each row
(musicindex ADR 0015). The search site reads each named feed only to get its
image (`indexedFeedImage`). A page of 20 rows thus sends 20 more requests.

**Request.** Add these fields to each row of the two lists, for the indexed
feed that `feed_guid` names:

| Route | Field | Value |
|---|---|---|
| `/v1/copies` | `feed_image_url` | The channel image URL of the indexed feed. Null when it states none |
| `/v1/live-items` | `feed_title` | The `<title>` of the feed |
| `/v1/live-items` | `feed_image_url` | The channel image URL of the feed. Null when it states none |

This is the rule of ADR 0059 §1: an entry that names a feed gives the title
and the image of that feed. The image is the image of the indexed feed, not
the image of a copy. So the request does not change the advice of request 9
about the image of a copy.

### 18. Summary Fields On Each Remote Value Time Split

Added on 2026-10-21. Evidence: the contract at `/openapi.json` of the live API
at revision `71aa360` on 2026-10-21, and Stophammer ADR 0073.

**What happens.** With `include=value_time_splits`, a track read gives each
split of the remote form. Each row gives `remote_feed_guid`,
`remote_item_guid`, the times and the percentage. It gives no title, no artist
and no image of the named feed or the named track.

**What it costs.** The track page shows each split as a row that opens the
named track (musicindex ADR 0019). To show the title, the page reads each
named track: at most 12 requests for one track page.

**Request.** Add the summary fields of ADR 0059 §1 and §5 to each remote split
row:

| Field | Value |
|---|---|
| `remote_feed_title`, `remote_feed_image_url`, `remote_release_artist`, `remote_release_artist_source` | The values of the feed that `remote_feed_guid` names. Null when the node does not hold it |
| `remote_track_guid` | The `track_guid` of the indexed item that `remote_feed_guid` and `remote_item_guid` name. Null when the node does not hold it, or when `remote_item_guid` is empty |
| `remote_track_title`, `remote_track_duration_secs`, `remote_track_image_url` | The values of that track. Null when `remote_track_guid` is null |

These are the names that a `remote_items` entry already uses. Each value is a
value that the named feed or track states.

### 19. The Value Time Splits Of A Feed That Did Not Change After ADR 0073

Added on 2026-10-21. Evidence: read-only GET requests to the live API at
revision `71aa360` and the RSS file of "Autumn Rust"
(`9e72a512-c678-5621-8fd6-dfc7cd11a846`) on 2026-10-21.

**What happens.** The RSS file at
`https://serve.podhome.fm/rss/9e72a512-c678-5621-8fd6-dfc7cd11a846` has one
item, "Autumn Rust, Boost Recap", with 8 `podcast:valueTimeSplit` elements.
The first one is:

```xml
<podcast:valueTimeSplit startTime="1" duration="86399" remotePercentage="95">
  <podcast:remoteItem feedGuid="9e72a512-c678-5621-8fd6-dfc7cd11a846" itemGuid="efe2226c-5390-4178-8950-8eab2d76bfbf" />
</podcast:valueTimeSplit>
```

The track read of `f0e12a75-5ec9-4abb-81d9-f918eeb474bc` with
`include=value_time_splits,local_value_time_splits` gives an empty list for
each include. The feed gives `updated_at` 1790971365, 2026-10-02 20:02:45 UTC.
That is before the release that carries ADR 0073.

Stophammer ADR 0073, "Consequences", says that a crawl of the Satellite
Skirmish feed after the deploy writes three remote-form rows. The archived
ADR 0072 says that the node checks the content hash of each body. We do not
know if the node skips a body with the same hash. If it does, a feed that
does not change never gets its splits.

**What it costs.** The track page shows the splits of a track (musicindex ADR
0019). For this feed it shows none, and the operator cannot check the view.

**Request.** Examine why this feed gives no split. If the node skips an
unchanged body, read again each feed with a `podcast:valueTimeSplit` in its
stored body. Use the parser of ADR 0073. Alternatively, record
in ADR 0073 when such a feed gets its splits.

### 20. No Copy Of Two Feeds Whose Old Album Files Stay Public

Added on 2026-10-21. Evidence: read-only GET requests on 2026-10-21 to the
live API at revision `71aa360` and to the RSS files below. Also, the public
pages of Podcast Index feeds 7026851 and 7689614.

**What happens.** Two feed GUIDs each have a Podhome file and an older album
file on `music.behindthesch3m3s.com`. Each pair gives the same
`podcast:guid` and `podcast:medium` `music`:

| GUID | URL | Items | `lastBuildDate` |
|---|---|---|---|
| `9e72a512-c678-5621-8fd6-dfc7cd11a846` "Autumn Rust" | `https://serve.podhome.fm/rss/9e72a512-c678-5621-8fd6-dfc7cd11a846` (source) | 1 | 6 Feb 2026 |
| same | `https://music.behindthesch3m3s.com/wp-content/uploads/Sat_Skirmish/autumnrust/mp3s/album_feed/feed.xml` | 75 | 18 Jan 2025 |
| `b39fc3b2-2742-5944-a800-e87c42a093a3` "The Satellite Skirmish" | `https://serve.podhome.fm/rss/b39fc3b2-2742-5944-a800-e87c42a093a3` (source) | 1 | 6 Feb 2026 |
| same | `https://music.behindthesch3m3s.com/wp-content/uploads/Sat_Skirmish/the_satellite_skirmish_album.xml` | 31 | none |
| same | `https://music.behindthesch3m3s.com/wp-content/uploads/Sat_Skirmish/oldfeed/the_satellite_skirmish_album.xml` | 29 | none |

Each Podhome file declares `itunes:new-feed-url` and `atom:link rel="self"`
with its own URL. The album files declare neither. Thus, by ADR 0052, the
Podhome file is the source and each album file is a mirror. We agree with
that result.

`GET /v1/feeds/{guid}/copies` gives no row for either GUID, and
`GET /v1/guid-changes` does not name them. Podcast Index has two feeds with the
title "Autumn Rust": 7689614 with Podhome art, and 7026851 with art on
`music.behindthesch3m3s.com`. We could not read the feed URL of 7026851,
because the public page needs a proof-of-work header.

**What it costs.** The boost recap of each Podhome file has value time splits
that name album tracks by `itemGuid`. Only the album files hold those tracks:

| Split of track `f0e12a75-5ec9-4abb-81d9-f918eeb474bc` | Named item | Only in |
|---|---|---|
| 0:01 to the end, 95% | `efe2226c-5390-4178-8950-8eab2d76bfbf` "The Doerfels - Islamorada" | Autumn Rust album file |
| 47:39 to the end, 95% | `9db6d76b-fe9e-4914-bcc4-c429e0d75d6b` "The HeyCitizen Experience - Love Song 4 U" | Satellite Skirmish album files |
| 1:55:54 to the end, 95% | `79901278-8f46-4efa-a602-98627b8f6949` Matt the Tall, "Wayfaring Stranger" | Autumn Rust album file |

The search site shows each of these rows as "Track not indexed". Without a copy
row, the page also cannot show the copy warning of musicindex ADR 0009. The
copy view of musicindex ADR 0013 cannot show the 75 and 31 album tracks.

**Request.** Examine why the node has no copy row for these URLs. For example,
the crawler may not list them, or they may answer `304`. If the rules of ADR
0058 apply to them, record each album file as a copy of its GUID.

This request does not ask the node to change the source or to index the album
tracks as tracks. The publisher decides which file is the feed.

### 21. A Name Order For Each List Of Feeds

The site gives Artists and Labels the sort A to Z, and each list page can have it (ADR 0047
R-SITE-4). `GET /v1/feeds/recent` has no name order. Stophammer ADR 0084 adds `reverse` to that
route, and it adds `sort=name` only to `GET /v1/search`. That route refuses a request with no
positive term.

The request: `GET /v1/feeds/recent` takes a name order with `medium` and `stated_rel`, with the
cursor and the `from` start of `GET /v1/artist-names`. It uses the sort key of ADR 0070 §1.

Until Stophammer answers, the site shows no A to Z sort, and "A to Z" stays a destination.

### 22. HTML Entities In A Description

`GET /v1/tracks/ac06ffb0-8425-4d00-b94f-580d41c2a18d` gives the description of "Jorfest 2026" with HTML
entities as text, for example `Jorbear&rsquo;s` and `&middot;`. The site shows these as text. On
2026-11-06 the live API gave this value, and the capture of 2026-10-24 in the research repository has it.
Stophammer ADR 0017 lists "decode entities" for the parser.

The request: the API gives each description as decoded text. Each client then shows the same text. The
site does not parse the value (the site rule is that Stophammer gives the data).

No person examined the source feed or the parser for this request, so it does not state the cause.

### 23. A Search Finds A Playlist

On 2026-11-07, `GET /v1/feeds/recent?medium=musicL` lists the playlist "Deck Darlings". The search
`GET /v1/search?q=Deck%20Darlings` gives no row, also with `type=feed`. The search
`q=Local%20Theory%20Full%20Catalog&type=feed` also gives no row.

ADR 0023 R-SCOPE-1 item 2 puts playlists in the Releases scope, and the site sends that scope as
`type=feed`. A person can thus not find a playlist by its title.

The prefix match of Stophammer ADR 0086 does not change this. On 2026-11-07 after that change, `q=deck dar`
also gives no row, with no type and with `type=feed`.

The request: a search finds a feed with medium `musicL`. A row states that medium, so that the site can
give its type.

No person examined the search index for this request, so it does not state the cause.

### 24. A Faster Role Filter On The Publisher Lists

The site reads Labels and the stated artists with `stated_rel` (Stophammer ADR 0085). These are three
times of each request on the live node on 2026-11-07, with `include=link_facts` and `limit=200`. They
are not a load test:

| Request | Time |
| --- | --- |
| `GET /v1/feeds/by-name?medium=publisher` | 0.22 to 0.26 s |
| The same, `stated_rel=artist`, first page | 0.75 to 0.85 s |
| The same, `stated_rel=artist`, second page | 0.57 to 0.66 s |
| `GET /v1/artist-names` | 0.08 to 0.16 s |

The list of Artists thus shows, and its stated artists arrive about 1.1 s later and push the rows down.

The request: the filter reads the link facts from the memory copy of the artist list. ADR 0085 names
this as a possible later change. The site asks for no change to the contract.

### 25. A Title Match Before A Description Match

On 2026-11-07, `q=love&type=publisher&limit=5` gives Strange Love, Love United, I love dafneestre, Callum
and DJ Yo!. The word is in the description of Callum ("I love music."), and not in its title. The
search `title=love&type=publisher` gives "Love Ethic" and "Strange Love (Wavlake)", and these two are not
in the five rows.

The site shows no description in a row, so the match of Callum looks random to a person.

Evidence after the prefix match of Stophammer ADR 0086, on the live node on 2026-11-07. The node gives no
version. The commit `f610586` holds that change, and `q=jsmo` now gives rows.

- `q=love&type=publisher` gives the same five rows and the same `rank` values. "Love Ethic" is row 6, and
  "Strange Love (Wavlake)" is row 8.
- `q=lov&type=publisher` gives Strange Love, Love United, I love dafneestre, Loveska and JACK_1986, then
  Callum. "Love Ethic" is row 8, and "Strange Love (Wavlake)" is row 16. The word of JACK_1986 is in its
  description ("Music Lover"). So a hit by prefix has the same result as a whole word.
- `q=the l&type=feed` starts with "THE FLOOD" (`rank` -9.608), and `q=the l` with no type starts with
  "THE FLOOD" and "THE CREEK". The artist of these rows is `T.E.A.L`. The title has "the" and no word
  that starts with "l". The text filter `artist=l` finds "THE FLOOD", the filter `title=l` does not, and
  the phrase `"t e a l"` finds it. So the node reads `T.E.A.L` as the words t, e, a and l. The description
  and the publisher title of the feed have the same text, so these results do not show which field gave
  the hit.
- A letter that is a whole word thus comes before each hit by prefix (ADR 0086 §2). With `type=feed`, 800
  rows of `q=the l` held 17 hits with whole words and 783 hits by prefix. The first hit by prefix was
  "On The Low". A person who types "the love" sees these rows one letter before the word. `q=the lov` and
  `q=the love` give titles with "love" first.

The request: with the sort `relevance`, a title match comes before a match in other text. Stophammer
decides how, and whether the same rule applies to each type. No person examined how the node gives
this order, so the request does not state the cause.

### 26. The Stated Kind And A Date On A Feed Row Of A Search

On 2026-11-07 a feed row of `/v1/search` gives `entity_id`, `title`, `feed_image_url`, `release_artist`,
`release_artist_source`, `episode_count`, `rank` and `quality_score`. It gives no `release_kind` and no date.
A row of `/v1/feeds/recent` gives `release_kind` and `newest_item_at`.

The site shows the kind of a release (musicindex ADR 0020 R-WORD-1) on each row. A search row has only the
track count, so a feed that states a kind would get the kind of its count. Each sampled feed has `unknown`
now, so the label is correct today. A list row shows the kind, then the date. A search row has no date, so it
shows the kind and the count.

The request: a feed row of `/v1/search` gives `release_kind` and `newest_item_at`, as a row of
`/v1/feeds/recent` does. The site works without them, so this request blocks nothing.

### 27. A Feed By Its URL

On 2026-11-07 the operator asked that the search field of the site find a feed by its RSS URL. No route of
Stophammer gives a feed for a URL. The routes are `/v1/feeds/{guid}`, `/v1/feeds/recent`, `/v1/feeds/by-name` and
the routes under one GUID. A search with the full URL in `q` gives no row: on 2026-11-07,
`q=https://feeds.wavlake.com/feed/music/0d1aa48a-3e7b-4d0a-ad8c-7d47e57b1a6f` gave none. A search for `wavlake.com`
gives rows, because the full text index holds the words of the host.

The site already opens a feed by its GUID: it reads `/v1/feeds/{guid}`. A URL can name a feed that is a copy of an
indexed feed (ADR 0008 of this site), or a feed that the index does not hold.

The request: a route gives the feed of one URL, with the same row as `/v1/feeds/{guid}`. Stophammer decides how a URL
compares, for example the scheme, a trailing slash and the case of the host. Stophammer also decides what the route
gives for the URL of a copy.

Until the route exists, the search field of the site does not promise a search by URL.

### 28. A Release Date For Each Artist And Label

On 2026-11-08 the live node ran v0.19.0-rc.5. Only 1 of the first 200 rows of `/v1/feeds/recent?medium=publisher`
had `newest_item_at`. With `stated_rel=artist` 0 of 3 rows had it, and with `stated_rel=label` 0 of 1 had it. The
Artists, Labels and No role pages of the site read these lists, so their sort Newest gave the order of the tie key.
The site removed Newest from these pages (musicindex ADR 0050 R-SITE-9 item 5).

Stophammer ADR 0089 gives `newest_release_at` on each row of `/v1/artist-names?sort=date`. That route has no filter for
a stated role, so it cannot give the Artists list or the Labels list of the site.

The rows of `include=publisher` on `/v1/feeds/{guid}` give no date. The site lists the releases of an artist or a label
feed from these rows, so those sections have Feed Order and A to Z, and no Newest (musicindex ADR 0053).

The request has two parts:

1. A sort by release date of the publisher feeds, with the filters of `/v1/feeds/recent` that the site sends now
   (`stated_rel`, `two_way_links`, `include=link_facts`). One form: `newest_release_at` on each publisher row of
   `/v1/feeds/recent`, with a sort by it. Another form: `stated_rel` on `/v1/artist-names?sort=date`. Stophammer
   selects the form.
2. `newest_item_at` on each row of `include=publisher` of `/v1/feeds/{guid}`, for the release of that row.

The site offers Newest again only when the node lists a capability for each part.

### 29. The Artist Name Of A Publisher Row Of The Artist List

On 2026-11-08 a `publisher` row of `/v1/artist-names` with `include=link_facts` gave `name`, `sort_name`, `key`,
`feed_guid`, `image_url`, `newest_release_at`, `release_count`, `kind` and the link facts. It gave no `release_artist`
and no `release_artist_source`. A row of `/v1/feeds/recent?medium=publisher` gives both.

The site infers an artist from a feed with two-way links and no agreed role: an album artist of a confirmed link must
match the title of the feed or its `release_artist` (musicindex ADR 0007). The inferred artists and the No role page
have no node filter, so with Newest the site reads the full `/v1/artist-names?sort=date&include=link_facts` (Stophammer
ADR 0089) and groups each row itself. On the live node, 1,773 publisher feeds were inferred artists with the two names,
and 1,768 with the title only. Five feeds need `release_artist`, for example "Kolomona Myer - AKA Sir Libre" with the
artist "Kolomona Myer", and "DJ Cricket- G-Funk Summer Mixes" with "DJ Cricket".

The request: a `publisher` row of `/v1/artist-names` with `include=link_facts` gives `release_artist` and
`release_artist_source`, with the values of `/v1/feeds/recent`, behind a capability. Until then the site groups these
rows with the title only, and the five feeds are not in the Newest order of the inferred artists.


### 30. The Releases Of An Artist Row By Track Count

On 2026-11-08 the search row of the publisher feed jSmovie (`44f2785c-6a55-4f59-b47f-4a7fcedc7d01`) gave
`two_way_link_count` 17. The site showed "Inferred artist · 17 albums", but each of the 17 releases has one track and
no stated kind, so each one is a single (musicindex ADR 0020 R-WORD-1). The site now writes "17 releases" (musicindex
ADR 0023 R-SCOPE-1 item 2). The operator asked on 2026-11-08 for the exact count of each kind, for example "17
singles".

A row gives only a total now: `two_way_link_count` on each row with `include=link_facts`, and `release_count` on a
`name` row of `/v1/artist-names`. On 2026-11-08 each of the 8,887 music feeds of the node had `release_kind`
`unknown`, so a count of each stated kind gives nothing. musicindex ADR 0020 R-WORD-1 finds the kind from the track
count: one track gives "Single", and each other track count, also zero, gives "Album".

The request:

1. Next to each total, `release_track_counts`: a map from a track count to the number of releases with that track
   count. For example `"release_track_counts": {"1": 5, "9": 1, "12": 1}` next to a total of 7. The sum of the map is
   the total. A key is the track count as text. A track count with no release is absent.
2. The rows: each row with `include=link_facts` (`/v1/feeds/recent`, `/v1/feeds/by-name`, the publisher rows of
   `/v1/artist-names` and `/v1/search?type=publisher`), for the releases that `two_way_link_count` counts. Also the
   `name` rows of `/v1/artist-names`, for the releases that `release_count` counts.
3. A capability flag in `/v1/node/capabilities`, so that a client sends no request for the map to a node without it.
   A client with no flag keeps the word "releases".

The rows of `include=publisher` on `/v1/feeds/{guid}` already give `remote_episode_count`, so a publisher page needs
nothing more.

The test case: jSmovie gives `{"1": 17}`. The artist feed OLAMIDE (`ff6c8ef7-e3d5-47bf-9cfb-e50c51597424`, 52
confirmed releases) is a case with more than one track count.

## Stophammer Answers - 2026-09-25

Stophammer examined each request against the live API and the source at
commit `64052ea`. These answers are
advisory. The ADR that each answer names is the owner of the rule. The
Stophammer operator approved the recommended work on 2026-09-25.

| # | Answer | Work plan item |
|---|---|---|
| 1 | Confirmed and recommended. One rule covers this request and v4vmm request 1. Each entry that names a feed carries the title, the image and the artist of that feed. The fields are added fields, so they stay in `v1` (ADR 0044 §4). ADR 0059, Accepted on 2026-09-25, owns the rule, with the names of this request | 5 |
| 2 | Confirmed, and deferred. `feed_remote_items_raw` has no column for `itemGuid` or `title`. The change needs an ADR, a migration, and a change to the parser and the ingest contract. Update of 2026-09-25: Podcast Index holds 6 `musicL` track playlists with `itemGuid`, and the index did not hold them. ADR 0060, Proposed, gives the three fields of this request, after ADR 0059 | 8 |
| 3 | Confirmed and recommended. ADR 0042 states: "A search result holds the fields that a client needs to show a row." The response does not obey that rule, so the work needs no new ADR. Each field is stored: `feeds.episode_count`, `tracks.track_artist` and `tracks.duration_secs` | 3 |
| 4 | Confirmed and recommended. The list routes give at most 200 rows. `/v1/search`, `/v1/publishers`, `/v1/copies` and `/v1/guid-changes` give at most 100, and the contract states only the last two. Also, `/v1/publishers` has no paging, and `has_more` is always `false`, also when rows are cut. Stophammer recommends `maximum` in the parameter schema, not a `400` above the maximum. A `400` can break a client that works today | 2 |

### Deploy Of 2026-09-26

Stophammer deployed commit `607bb3a` on 2026-09-26 at 03:29 UTC.
`GET /node/info` gives the revision of each deploy from now on.

- Request 3 is complete. A feed result gives `release_artist`,
  `release_artist_source` and `episode_count`. A track result gives
  `track_artist` and `duration_secs`.
- Request 4 is complete. Each `limit` parameter in `/openapi.json` gives
  `minimum: 1` and its `maximum`: 200 for the list routes, and 100 for
  `/v1/search`, `/v1/publishers`, `/v1/copies` and `/v1/guid-changes`.
  `/v1/publishers` now gives a correct `has_more`. It still gives no cursor.
- The capabilities route now lists each include of the track routes.

### Deploy Of 2026-09-26, ADR 0059

Deploy of commit `264706e` on 2026-09-26 at 04:05 UTC: ADR 0059 is
complete. Each `publisher` and `remote_items` entry gives
`remote_feed_title`, `remote_feed_image_url`, `remote_release_artist` and
`remote_release_artist_source`. A track read gives them too. Each value is
null when the index holds no feed for the entry.

This closes request 1. The names are the names of this request.

### Deploy Of 2026-09-26, ADR 0060

Deploy of commit `9b6dc21` on 2026-09-26 at 04:59 UTC. Each `remote_items`
entry gives `remote_item_guid`, `remote_item_title` and `remote_track_guid`.
`remote_track_guid` is null when the index holds no such track. This closes
request 2. Stophammer then indexed 10 more `musicL` feeds. For example,
"Lightning Thrashes Playlist episodes 1 - 60"
(`287e27fa-adc5-4762-956f-0282bba5ed77`) gives a track for 367 of its 383
entries.

## Stophammer Answers - 2026-10-02

Stophammer examined requests 5 to 12 against the live API, the source at
`v0.4.0`, and a copy of the production data of 2026-09-26. These answers are
advisory. The ADR that each answer names is the owner of the rule.

| # | Answer |
|---|---|
| 5 | Complete in release 0.4.0. Each entry with a `remote_track_guid` gives `remote_track_title`, `remote_track_duration_secs` and `remote_track_image_url`, with the names of this request. ADR 0059 §5 owns the rule |
| 6 | Complete in release 0.3.0. `GET /node/info` of the release images gives `git_revision` and `built_at`. On 2026-10-02 it gives `b3bc0b3` |
| 7 | Complete in release 0.4.0 for `/v1/feeds/recent`, with `include=link_facts` (ADR 0068). The search part is declined: ADR 0038 keeps publisher feeds out of the search index, so each search row has the medium `music`. `GET /v1/node/capabilities` lists `link_facts` under `feed_list` |
| 8 | Confirmed. Each read examined all 30,705 `track_upserted` events of the copy and parsed their payload. Two partial indexes on the `json_extract` expressions of the query took the read from 0.095 to 0.014 seconds on that copy, with the same answer. The query and the answer do not change. Release 0.4.1 carries it |
| 9 | Confirmed. `feed_copies` stores `item_guids`, but no item title and no image. `item_titles` and `image_url` need new stored values, so they need an amendment of ADR 0058 §1. Stophammer gives the three fields together, after that amendment. A copy row gets the titles and the image at the next crawl that observes the copy. The publisher of a copy chooses its image, and for an impersonation that is the attacker. Stophammer recommends that the page shows the image of an open copy only after a person asks for it |
| 10 | Built for release 0.5.0, as ADR 0068 §4. `stated_rel` and `two_way_links=none` take the names of this request, and each one gives the link facts on each row. On the copy of 2026-09-26, a pass over all 1,772 publisher rows with the facts took 801 ms of node time. Most of the 4.6 seconds of the request is transfer. So one request examines at most 1,000 rows, and then gives a cursor. A page can hold fewer rows than `limit`, or no row, with `has_more` true. The page follows the cursor until `has_more` is false. On that copy, 4 rows stated a `rel` and 15 rows had no two-way link |
| 11 | Deployed in release 0.7.0 on 2026-10-02, with task 014 of ADR 0049. (1) `stated_rels` gives normalized role tokens, and `stated_rel=` matches one token (ADR 0068 §5). (2) Each `publisher` row gives `role_agreement`: `both`, `one_side`, `conflict` or null (ADR 0049 §6a). `role` keeps its meaning, so show it only when `role_agreement` is `both`. (3) A link with `conflict` is not a confirmed link. It stays in the view, and no count uses it. Sir Libre Records loses three links from its counts until a feed changes. (4) The bare-item credit of ADR 0069 is removed (§1a). Only an item inside `<podcast:publisher>`, or the first bare item of an album with no wrapper, is a link. (5) A list row with `include=link_facts` gives `agreed_roles`. The HeyCitizen value `artist host author label producer` is five real roles, and after 0.7.0 each one finds the feed |
| 12 | Deployed in release 0.8.0 on 2026-10-08, as ADR 0058 §1d with task 007. The evidence is correct, but a comparison of the raw `feedGuid` values gives a false answer. The Wavlake artist feed gives the Wavlake album ID as `feedGuid`, and the headstarts.uk feed gives the `podcast:guid` of each album. By the album URL, 11 albums are on both lists, 13 are only on the indexed feed, and 7 are only on the copy. So the node resolves each entry, as it resolves a publisher link, and compares the resolved feeds. (1) Each copy row gives `remote_items`: each channel-level entry in sequence, with `medium`, `feed_guid`, `feed_url`, `item_guid`, `resolved_feed_guid`, and the ADR 0059 summary of the resolved feed. The page needs no other read. A `musicL` entry keeps its `item_guid`. (2) A new flag, `differs_remote_items`, is true when the two lists name different feeds. `differs_tracks` keeps its meaning. (3) The rule applies to each medium. An album copy that names another publisher is also a copy, so `copy_count` can increase. (4) A row gets its list at the next body from its URL. After the deploy, a replay of the cached copy bodies fills the old rows |

## Stophammer Answers - 2026-10-17

Stophammer examined requests 14 to 16 against the live API at revision
`c1734b3`, the RSS file of "Pixel Palace", the source, and the backup of
2026-09-26. Each report is correct. The Stophammer operator decided on
2026-10-17. The text of each request above now gives the accepted scope.

| # | Answer |
|---|---|
| 14 | Accepted as an amendment of ADR 0064 §6. Each live row gives `link` and `link_scheme_added`. The node already stores the RSS `<link>` of a live item: the backup holds 16 such rows. An Atom link does not give the value |
| 15 | Accepted as ADR 0054 §4a. A `website` or `web_page` link row, and a live item `link`, read a value with no scheme as `https://` and the value, when the value starts with a strict host name. A link row gives `url_scheme_added`, and a live row gives `link_scheme_added`. `The Fraglebay` stays null |
| 16 | Declined. A client groups the contributor rows by `npub`. ADR 0041 does not change |

A release record gives the release that carries requests 14 and 15.

## Stophammer Answers - 2026-10-18

The Stophammer operator decided request 13 on 2026-10-18. The text of the request above now gives the accepted scope.

| # | Answer |
|---|---|
| 13 | Accepted as ADR 0058 §1e. Each copy row gives `item_durations`, `item_enclosure_urls`, `item_pub_dates` and `author`. They follow §1c: display values, out of the summary digest, and an event when one changes. Each enclosure URL passes the web URL rule of ADR 0054 §4, because the publisher of a copy can be an attacker. A different enclosure URL is not a difference of §2. In the backup of 2026-09-26, 225 of 2,022 indexed tracks with more than one event had changed their URL on their own feed, for example with an analytics prefix. After the deploy, a replay of the cached copy bodies fills the rows from before the release. A release record gives the release |

## Stophammer Answers - 2026-10-21

Stophammer examined requests 19 and 20 against the live API at revision
`71aa360`, the RSS files that each request names, the source, and the backup of
2026-10-21. Each report is correct. These answers are advisory. ADR 0073 owns
the split rule. The archived ADR 0072 §4 owns the content hash, and the
archived ADR 0058 owns the copy rows.

| # | Answer |
|---|---|
| 19 | Confirmed. The node skips a body with the same hash. The `content_hash` verifier returns `NO_CHANGE`, and the node then writes no track and no split. The SHA-256 of the "Autumn Rust" body on 2026-10-21 is the hash that the node recorded on 2026-10-02. Also, the feed server answers the conditional GET with a `304`, so a usual crawl sends no body. The parser of ADR 0073 reads all 8 splits of the body. A forced pass of the crawler on 2026-10-21 at 23:54 UTC read the two feeds again. The track `f0e12a75-5ec9-4abb-81d9-f918eeb474bc` now gives 8 remote-form splits, and the track `f475f98b-8ed4-4b06-8038-53727b9100d6` of "The Satellite Skirmish" gives 3. Then a search of the bodies in the crawler fetch cache found two more feeds with splits: "The Satellite Spotlight" (`9e3cea98-d04d-5190-88b3-46ee6030d4ea`) with 14 and "Polar Embrace" (`2a3a5f81-8325-521b-b168-07ff09b15435`) with 13. A second forced pass read them again. On 2026-10-22, the live API gives each of the 38 splits of the four feeds. Five splits of "Autumn Rust" name a feed and no item, so `remote_item_guid` is `""` (ADR 0073 §4). The index holds none of the three named items. A change to the parser does not cause a new read of an unchanged feed. The fetch cache does not hold a feed that only an `ndjson` replay gave to the index. Such a feed can have splits in an unchanged body. A forced `refresh` pass of all feeds finds it. It has not run |
| 20 | Confirmed. No crawler sent the album files after 2026-04-20. On 2026-04-19 and 2026-04-20, two passes sent each album file and then each Podhome file, and the Podhome file stayed the source. ADR 0058 came later, and it records a copy only when a crawler sends a body from the copy URL. `refresh` reads only the URL that the node holds for each GUID, and no link in the Podhome files names an album file. The album host sends no podping, and the cursor of `import` does not go back. The `oldfeed/` URL never reached the node. A `feed` pass of the three album URLs on 2026-10-22 at 00:35 UTC recorded each one as a copy. "Autumn Rust" gives one copy row with 73 items: the album file has 73 items on 2026-10-22, not 75. "The Satellite Skirmish" gives two rows, with 31 and 29 items. Each row gives `differs_tracks` true, and the `oldfeed/` row also gives `differs_recipients` true. The Podhome files stay the source. No crawler reads these album files again, so a later change to them makes no event. This is a limit of ADR 0058, and a change to it needs a new decision |

## Stophammer Answers - 2026-10-25

The Stophammer operator accepted requests 17 and 18 on 2026-10-25. ADR 0059
owns the rule, and these answers are advisory.

| # | Answer |
|---|---|
| 17 | Accepted as ADR 0059 §6, with the names of the request. A row of `/v1/copies` gives `feed_image_url`. A row of `/v1/live-items` gives `feed_title` and `feed_image_url`. Each value is of the indexed feed that `feed_guid` names, not of a copy. Each field is null when the node holds no such feed, and each image passes the web URL rule of ADR 0054 §4. A copy gives its own image only on `GET /v1/feeds/{guid}/copies` |
| 18 | Accepted as ADR 0059 §7. Each row of `include=value_time_splits` gives the feed fields and the track fields that a `remote_items` entry gives, with the same names. This includes `remote_episode_count`, which the request does not ask for. A split gives no URL, so the node finds its feed by `remote_feed_guid` only. When `remote_item_guid` is `""`, each track field is null. `include=local_value_time_splits` does not change |

## Stophammer Answers - 2026-11-07

These answers are advisory. Stophammer [ADR 0085](https://github.com/InTheMorning/stophammer/blob/main/docs/adr/0085-a-name-list-takes-the-role-filters.md) owns the rule. Its build is complete, and no release carries it yet. A release record names the release that carries it.

**Request 21, a name order.** Accepted in a different form. `GET /v1/feeds/recent` takes no name order. `GET /v1/feeds/by-name` already has the name order, `from`, the cursor and the sort key of ADR 0070 §1, and it now takes the two filters:

```text
GET /v1/feeds/by-name?medium=publisher&stated_rel=label
GET /v1/feeds/by-name?medium=publisher&stated_rel=artist
GET /v1/feeds/by-name?medium=publisher&two_way_links=none
```

- The filters have the values and the checks of ADR 0068 §4 and §5. They give the link facts, and each row keeps `sort_name`.
- `reverse=true` gives Z to A (ADR 0084).
- Send these filters on this route only when `GET /v1/node/capabilities` lists `sort`. A node with no such change ignores them and gives each publisher.
- One request examines at most 1,000 rows, as on `GET /v1/feeds/recent`. So a page can be short or empty with `has_more` true. On the backup of 2026-11-05, the first page of `stated_rel=label` holds no row. The site must follow the cursor until `has_more` is false.
- On a local copy, each filtered page took 0.34 to 0.52 s. The same filters on `GET /v1/feeds/recent` took 0.51 to 0.54 s.


**Request 25, a title match before a description match.** Accepted in a different form. Stophammer [ADR 0087](https://github.com/InTheMorning/stophammer/blob/main/docs/adr/0087-a-search-orders-its-hits-by-field.md) owns the rule. Its build is complete, and no release carries it yet. A release record names the release that carries it.

With `sort=relevance`, the node puts each hit in the first group that matches. The rule is the same for each type:

1. All words of `q` are in the title, or all words of `q` are in the artist name.
2. All words of `q` are in the title and the artist name together.
3. Each other hit. This includes a match in the description.

- In each group, a hit with the whole words comes before a hit by prefix. The score and the quality weight then give the sequence.
- This differs from the request in one item: an artist name has the group of a title. A title-first order moved the rows of an artist down. In 33 of 285 sampled artist searches, these rows lost at least half of their places in the first 10.
- A title prefix hit can come before a whole word in other text. `q=the l&type=feed` starts with "LVX3 l Into The Unknown l", "LVX1 l Answering the initial call l" and "On The Low". "THE FLOOD" comes later.
- `q=love&type=publisher` gives Strange Love, Love United, I love dafneestre, Love Ethic and Strange Love (Wavlake). Callum is row 10.
- `rank` is the BM25 score, 0 or less, and it does not give the order. The sequence of the rows is the order. The site must not sort by `rank`.
- A site with no commit `4fa24eb` sorts each page by `rank` and loses the group order. On 2026-11-07 the live `search.html` equals commit `1f18039`, which holds that commit, so the site does not have to deploy first.

## Stophammer Answers - 2026-11-08

These answers are advisory. Stophammer [ADR 0088](https://github.com/InTheMorning/stophammer/blob/main/docs/adr/0088-a-feed-is-found-by-its-url.md) owns the rule. On 2026-11-08 the primary runs it, with the short Wavlake form of §1a, in the candidate `v0.19.0-rc.7`. A release record names the release that carries it.

**Request 27, a feed by its URL.** Accepted in a different form. The route gives the GUID of the feed and how the URL matched, and the site reads the feed at `href`:

```text
GET /v1/feeds/by-url?url=<the RSS URL, encoded as a query value>
```

- A `200` answer gives `data` with `feed_guid`, `href` (`/v1/feeds/<guid>`), `match` and `matched_url`. `match` is `feed_url`, `copy` or `observed_url`.
- The node compares these forms in sequence: the URL as given with no white space at each end, the parsed URL with no fragment (lowercase scheme and host, no default port), the other one of `http` and `https`, and the same two with the trailing slash added or removed. The query string and the path keep their text and case. `www.` is a different host. The only rule for one host is the short Wavlake form (ADR 0088 §1a).
- For each form, the `feed_url` of a feed comes first, then a copy URL, then a URL that gave the GUID at an earlier ingest. A copy URL gives the indexed record, not the copy (ADR 0008 of this site). On 2026-11-07 the node had observed the Wavlake short form `wavlake.com/feed/<uuid>` for only 1,405 of the 7,699 Wavlake music feeds. So ADR 0088 §1a adds one last form: for `https://wavlake.com/feed/<uuid>`, the node also compares `https://wavlake.com/feed/music/<uuid>`. An observation of the short form still matches first. Until a candidate carries §1a, send the music form when the site has it.
- With no match, the answer is `404` with `{"error": "feed not found"}`, as on `GET /v1/feeds/{guid}`. It carries `superseded_by` when the URL names a GUID that a GUID change retired. A value that is not an `http` or `https` URL of at most 2,048 bytes gives `400`.
- The node does not fetch the URL. A URL that the index does not hold gives `404`.
- Send the route only when `GET /v1/node/capabilities` lists `feed_by_url`. A node with no such flag answers the path as a feed read of the GUID `by-url`, which also gives `404`.
- The example of the request, `https://feeds.wavlake.com/feed/music/0d1aa48a-3e7b-4d0a-ad8c-7d47e57b1a6f`, gives `404`. On 2026-11-07 that host answered `404`, and the index held no feed with that ID.
- On a copy of the backup of 2026-11-07, a lookup took 0.4 to 1.6 ms.

**Request 28, a release date for each artist and label.** Accepted. Stophammer [ADR 0091](https://github.com/InTheMorning/stophammer/blob/main/docs/adr/0091-an-artist-list-takes-the-role-filters.md) owns the rule. On 2026-11-08 the primary runs it in the candidate `v0.19.0-rc.7`.

- Part 1: `GET /v1/artist-names` takes `stated_rel` and `two_way_links`, with the values and the `400` rules of `GET /v1/feeds/recent`. With a filter, the list gives only the `publisher` rows that match, and no `name` row. A test proves that the set equals the set of `GET /v1/feeds/recent?medium=publisher` with the same filter. Stophammer selected this form because the list in memory already holds the release date and the link facts. A date sort on `GET /v1/feeds/recent` needs a stored column and a migration.
- So the stated artists can come from `GET /v1/artist-names?stated_rel=artist&sort=date&include=link_facts`, and the labels from `stated_rel=label`. `two_way_links=none` gives only the publisher feeds with no two-way link. No filter gives the inferred artists, or the feeds with two-way links and no role. For those groups, read the full list with `sort=date&include=link_facts` and group each row. `sort=date` gives the newest `newest_release_at` first: the newest date of the music releases that name the feed (ADR 0089). A row with no date comes last. `reverse`, `from` and the cursor work as on the unfiltered list.
- A `publisher` row gives `name`, `sort_name`, `key`, `feed_guid`, `image_url`, `newest_release_at` and, with `include=link_facts`, the link facts. It does not give the other fields of a feed row, such as the description.
- Part 2: each row of `include=publisher` on `GET /v1/feeds/{guid}` gives `newest_item_at`, the date of the named feed. On a `publisher_to_music` row, that is the date of the release. On a `music_to_publisher` row, it is the date of the publisher feed, which is null for most feeds. It is null when the node holds no feed for the entry.
- Send the filters only when `GET /v1/node/capabilities` lists `artist_role_filter`. A node with no such flag ignores them and gives each row. Use `newest_item_at` only when the node lists `publisher_row_date`.

**Request 29, the artist name of a publisher row.** Accepted, as ADR 0091 §1a. Request 29 completes request 28. On 2026-11-08 the primary runs it in the candidate `v0.19.0-rc.7`.

- With `include=link_facts`, each `publisher` row of `GET /v1/artist-names` gives `release_artist` and `release_artist_source`, with the values of `GET /v1/feeds/recent` for the same feed. Each can be null, and the key is then present with a null value.
- Without `include=link_facts`, a row gives neither field. A `name` row never gives them.
- Use the two fields only when `GET /v1/node/capabilities` lists `artist_row_release_artist`.

**Request 30, the releases of an artist row by track count.** Accepted. Stophammer [ADR 0092](https://github.com/InTheMorning/stophammer/blob/main/docs/adr/0092-an-artist-row-counts-its-releases-by-track-count.md) owns the rule. On 2026-11-08 the primary runs it in the candidate `v0.19.0-rc.7`.

- Each row with `two_way_link_count` gives `release_track_counts` with `include=link_facts`: on `GET /v1/feeds/recent`, `GET /v1/feeds/by-name`, and the `publisher` rows of `GET /v1/artist-names` and `GET /v1/search`. The map counts the confirmed links of `two_way_link_count`, and its values sum to that count. A row with no confirmed link gives `{}`.
- Each `name` row of `GET /v1/artist-names` gives the map with no include, for the releases of `release_count`. Its values sum to `release_count`.
- The key is the `episode_count` of the release, as text. The `tracks`, `min_tracks` and `max_tracks` filters read the same column, so a map agrees with R-WORD-1 requests. The node gives no kind, and the site applies R-WORD-1.
- On a copy of the backup of 2026-11-08, jSmovie gives `{"1": 17}`, and OLAMIDE gives 12 track counts with a sum of 52.
- One music feed of 8,887 has an `episode_count` that differs from its stored tracks: `87750227-32d0-5154-8944-a6c65cf57157` has `episode_count` 1 and no stored track. Its key is `"1"`.
- Use the field only when `GET /v1/node/capabilities` lists `release_track_counts`.

On 2026-11-08 the operator reports that the candidate `v0.19.0-rc.7` is deployed. `/openapi.json` gives the version `0.19.0`. `GET /v1/node/capabilities` lists `query`, `search`, `sync`, `push`, `name_lists`, `sort`, `feed_by_url`, `artist_newest`, `artist_role_filter`, `publisher_row_date`, `artist_row_release_artist` and `release_track_counts`.

A `type=publisher&include=link_facts` search gives `{"1": 17}` for jSmovie. It gives 12 track counts with a sum of 52 for OLAMIDE, the same as its `two_way_link_count`. The `name` row of `$2 Holla` in `GET /v1/artist-names` gives `{"1": 2, "7": 1}` with `release_count` 3. This closes request 30. musicindex commit `b36d25f` (ADR 0023 R-WORD-9) makes the list rows of the site read the field.

**Request 22, HTML entities in a description.** Accepted. Stophammer [ADR 0017](https://github.com/InTheMorning/stophammer/blob/main/docs/adr/0017-canonical-rss-parser-crate.md) owns the rule: the parser decodes entities. Parser commit `7130556` decodes the 252 named entities of HTML 4.01, and no release carries it.

- Before that commit, the parser decoded only `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;` and numeric entities. It kept each other name, for example `&rsquo;`, as text.

- The crawler decodes a title and a description when it reads a feed. The node stores the decoded text, and the API does not change.
- A crawl of a feed with the same body writes nothing. So a stored row gets the decoded text when its feed changes, or when the operator replays the fetch cache with `export-feed-cache-ndjson.py`.
- On a copy of the backup of 2026-11-09, 91 track descriptions and 1 feed description hold a named entity. The most frequent are `&nbsp;`, `&rsquo;` and `&auml;`. No title holds one.
- The site needs no change.

## Stophammer Answers - 2026-11-10

These answers are advisory. Stophammer [ADR 0094](https://github.com/InTheMorning/stophammer/blob/main/docs/adr/0094-a-feed-hit-states-its-medium-kind-and-date.md) owns the rule for requests 23 and 26. It is Accepted on 2026-11-10. It is not built, and no candidate holds it. A release record names the release that carries it.

**Request 23, a search finds a playlist.** Accepted in a different form. The site names the medium in the search:

```text
GET /v1/search?q=<words>&type=feed&medium=music,musicL
```

- The cause is a rule, not the prefix match. Stophammer ADR 0071 §5 keeps each feed with the medium `musicL` out of the search index. On 2026-11-10 the live node still gives no row for `q=Deck%20Darlings&type=feed`.
- With no `medium`, a search gives the rows that it gives today. ADR 0071 §5 promises this to each present client, and v4vmm reads each `type=feed` hit as a release. So the node adds no playlist to a search that does not name it.
- `medium=music,musicL` gives the music feeds and the playlists. `medium=musicL` gives only the playlists. `medium` applies to `type=feed` and to a search with no `type`. With `type=track` or `type=publisher`, it gives `400` with `unsupported_filter`.
- A playlist hit gives the `entity_type` `feed`, and a read at `GET /v1/feeds/{guid}` gives the playlist.
- Each feed hit and each publisher hit gives `medium`, the stored medium of the feed. So the site can give the type of a row.
- On the first start of the release, the node adds each playlist to the index. The backup of 2026-11-09 holds 13 playlists. Each has `episode_count` 0 and no `newest_item_at`, because a playlist holds remote items, not tracks. So `min_tracks=1` removes a playlist.
- Send `medium` only when `GET /v1/node/capabilities` lists `search_medium`. A node with no such flag gives `400` for the parameter.

**Request 24, a faster role filter.** Met by Stophammer ADR 0091, and closed. No change to `GET /v1/feeds/by-name`.

- The site already reads `GET /v1/artist-names` with `stated_rel` for a filtered list in name order. That route reads the link facts from the copy in memory of the artist list, which is the change that the request asks for.
- On 2026-11-10 on the live node, `GET /v1/artist-names?stated_rel=artist&limit=200&include=link_facts` took 0.12 to 0.14 s. The orchestrator measured `GET /v1/feeds/by-name` with `stated_rel=artist` at 0.85 to 1.42 s on the same day. These times are not a load test.
- The first call of the orchestrator on 2026-11-10 took 6.46 s. This is possibly the build of the copy in memory at the first read after a restart. It is not part of this request. Stophammer examines it if it occurs after each restart.

**Request 26, the kind and the date on a feed row of a search.** Accepted, as ADR 0094 §3.

- Each feed hit and each publisher hit of `GET /v1/search` gives `release_kind` and `newest_item_at`. Each value is the value of `GET /v1/feeds/recent` for the same feed. Each can be null, and the key is then present with a null value.
- `pub_date` stays empty on a feed hit.
- Use the two fields only when `GET /v1/node/capabilities` lists `search_feed_summary`.
- This answer also gives item 1 of v4vmm request 11.

## Release 0.19.0

On 2026-11-07 the live node lists the capability `sort`. `reverse=true` gives 200 on
`GET /v1/feeds/recent`, `GET /v1/feeds/by-name` and `GET /v1/artist-names`, and
`GET /v1/feeds/by-name` takes `stated_rel` with `medium=publisher`. This release gives request 21.
The commit `f610586`, with the tag `v0.19.0-rc.2`, adds the prefix match of ADR 0086.

## Requests That v4vmm Also Makes

musicindex.org is a second client for these v4vmm requests. This may matter
for their priority.

| v4vmm # | Request | What it gives musicindex.org |
|---|---|---|
| 1 | Album summary fields in each publisher relationship entry | The publisher page reads each album separately: a maximum of 24, six at a time, and a "Show all" control for the remainder (`ALBUM_PAGE_SIZE` and `FEED_FETCH_CONCURRENCY`, lines 1881 and 1882). With the fields, one request per publisher |
| 2 | A deployed revision that a client can read | musicindex regenerates `api.json` after each Stophammer deploy. On 2026-09-25 the deploy was visible only from the new routes in `/openapi.json`, whose `info.version` stays `0.1.0` |
| 4 | Field renames are breaking changes | The deploy of 2026-09-25 removed `/v1/proofs/challenge` and `/v1/proofs/assert` with no version change. musicindex.org used neither route. On 2026-09-25 the published `api.json` still lists both. The next regeneration removes them |

## Release 0.16.0 - 2026-10-25

Release 0.16.0 deployed requests 17 and 18. Stophammer read the live API at
revision `1a7d6a1` on 2026-10-25:

- `GET /v1/copies?limit=5` gives `feed_image_url` on each row. "The Satellite
  Skirmish" and "Autumn Rust" give the Podhome image of the indexed feed, not
  the image of an album copy.
- `GET /v1/live-items?view=all&limit=5` gives `feed_title` "100% Retro - Live
  24/7 (MegaCRON!)" and its image on each row. `title` stays the title of the
  live item.
- The track `ec043f9d-1b46-4637-ab60-6528594a43fc` of "Polar Embrace" gives 6
  remote splits. Three give `remote_feed_title` "Polar Embrace", with the title
  and the duration of each named track. The three splits with
  `remote_item_guid` `""` give their feed fields and null track fields.
- The track `f0e12a75-5ec9-4abb-81d9-f918eeb474bc` of "Autumn Rust" gives the
  title of the named feed on each of its 8 splits. The three named items give
  `remote_track_guid` null, because only the album copies hold them (request
  20).

The rows of request 18 also give `remote_episode_count`. musicindex.org can
remove `indexedFeedImage` and the read of each named track.

## Release 0.10.0 - 2026-10-18

Release 0.10.0 delivers request 13 (Stophammer ADR 0058 §1e). `GET /node/info` gives `git_revision` `b45062b`.

- Each copy row gives `item_durations`, `item_enclosure_urls`, `item_pub_dates` and `author`. A replay of the cached copy bodies filled all 81 rows.
- The copy of "THERAPY IN SESSION" at headstarts.uk gives the durations `[183, 226, 219]` and the author "Nat Hills".
- The two copies of the Strange Love album give the same durations in a different sequence. One item gives 323 seconds on one copy and 324 seconds on the other.

## Release 0.9.0 - 2026-10-18

Release 0.9.0 delivers requests 14 and 15. `GET /node/info` gives `git_revision` `3ec1352`.

- Each live row gives `link` and `link_scheme_added` (Stophammer ADR 0064 §6). On 2026-10-18, each of the 17 rows of `GET /v1/live-items?view=all` gave a `link`.
- A `website` or `web_page` link row gives `url_scheme_added` (ADR 0054 §4a). "Good Enough (Single)" (`cfcabfcf-fdf1-556b-8955-94d48dbb6dcb`) stores `nathanabbottmusic.co.uk`, and gives `https://nathanabbottmusic.co.uk` with `url_scheme_added` true.

## Release 0.6.0 - 2026-10-02

**One change of meaning:** when neither side of a publisher link states a
`rel`, `role` is now null. Before, it was the guess `"artist"`.
`role_source` stays `"default"`. The site already shows "Role not stated" for
`default`, so it needs no change. A code path that reads `role` alone must
accept null.

Each `publisher` row gives `album_names_as`: `"publisher"`, `"credit"` or
null (ADR 0069). A publisher read with `include=publisher` gives
`co_credited_feeds`: for a label, the artist feeds of its releases, and for
an artist, its labels. Each entry rests on two two-way links through one
album. The guide `docs/publisher-links-guide.md` tells a feed author how to
write a credit.

## Release 0.5.0 - 2026-10-02

Request 9 is complete. Each row of `GET /v1/feeds/{guid}/copies` gives
`item_guids`, `item_titles` in the same sequence, and `image_url`. All 80
copy rows have their titles. Show the image of an open copy only after a
person asks for it.

Request 10 is complete. With `medium=publisher`, `GET /v1/feeds/recent` takes
`stated_rel` and `two_way_links=none`. One request examines at most 1,000
rows, so follow the cursor while `has_more` is true. On 2026-10-02 the label
filter found Sir Libre Records on its second request.

## Release 0.4.1 - 2026-10-02

`GET /v1/feeds/{guid}/route-history` uses two new indexes (migration 0048).
On 2026-10-02 five reads of "THERAPY IN SESSION" took 0.061 to 0.076 seconds,
and a plain read of the same feed took 0.058 to 0.107 seconds. The answer does
not change. This closes request 8.

## Release 0.4.0 - 2026-10-02

`info.version` of `/openapi.json` gives `0.4.0`.

- Each `remote_items` entry that names an indexed track gives
  `remote_track_title`, `remote_track_duration_secs` and
  `remote_track_image_url` (ADR 0059 §5). This closes request 5.
- `GET /v1/feeds/recent` takes `include=link_facts`. With it, each publisher
  row gives `two_way_link_count`, `stated_rels` and
  `confirmed_release_artists` (ADR 0068). This closes request 7 for the list.
- A `rel` value with no comma is now a list separated by white space, as HTML
  `rel` is (ADR 0049 §6). A value with a comma reads as before. No stored
  value changed its role.

## Release 0.3.0 - 2026-10-01

- Each live row gives `in_now_view` and `in_upcoming_view` (ADR 0064 §6). Each
  one is `true` when that view gives the row at the time of the read.
- A feed whose source URL is gone leaves the index after two gone answers, 24
  hours or more apart (ADR 0067). It is retired with the reason `source_gone`.
- `GET /node/info` gives `git_revision` and `built_at` again. This closes
  request 6.
- The node keeps no artist credit (ADR 0034 §11). `release_artist` and
  `track_artist` do not change.

## Release 0.2.0 - 2026-09-27

`info.version` of `/openapi.json` gives `0.2.0`. A read of a publisher feed,
`GET /v1/feeds/{guid}`, gives four new fields (Stophammer ADR 0061):

- `confirmed_release_artists` and `confirmed_release_artist_count`: the
  artists of the albums that the publisher lists, and that also name the
  publisher.
- `unconfirmed_release_artists` and `unconfirmed_release_artist_count`: the
  artists of the albums that the publisher lists, but that do not name it.

`distinct_release_artists` and `distinct_release_artist_count` do not change.
A music feed read has none of the four fields. A publisher page can show the
unconfirmed artists apart, for example as "listed only".

## Release 0.1.0 - 2026-09-27

Stophammer 0.1.0 is the first release. `info.version` of `/openapi.json`
gives the version of the running node. `GET /node/info` gives the commit in
`git_revision`. The
[GitHub release](https://github.com/InTheMorning/stophammer/releases/tag/v0.1.0)
holds the role tarballs, the Arch packages and the images.

The deploy of 2026-09-27 also adds live items (Stophammer ADR 0064):

- `GET /v1/feeds/{guid}` gives `live_items`.
- `GET /v1/live-items` gives the live items of all feeds, with the views
  `now`, `upcoming` and `all`.

A client that shows a live stream reads `confirming_relay`. When it is `true`,
the client asks the relay of the row if the stream is on air. `docs/API.md`
gives the details.

## Deferred, Not Requested Now

**A reverse playlist list.** A query that gives the `musicL` feeds that name a
feed or a track would support an "On playlists" rail. Request 2 is complete
since 2026-09-26, and the node indexes 11 `musicL` feeds. musicindex.org can
make this a request when it wants the rail.

**Summary fields in the route-history route.** musicindex.org uses both routes
since 2026-10-01. Request 9 covers the copy rows. The route-history entries
have no open request.

## Answered, No Action

| Item | Answer | Evidence |
|---|---|---|
| Can a client list `musicL` feeds? | Yes. `/v1/feeds/recent?medium=musicL` | Live API, 2026-09-24 |
| Do feed and track reads accept several includes in one request? | Yes. For example `include=tracks,source_contributors,remote_items,payment_routes,publisher` | Live API, 2026-09-24 |
| Does the publisher of an album come from `publisher_text`? | No. `publisher_text` is the `itunes:owner` name. The publisher is the feed that `include=publisher` names | Stophammer ADR 0049 §5 |
| Do publisher and `musicL` feeds appear in search? | No, by design | Stophammer ADR 0038 |
