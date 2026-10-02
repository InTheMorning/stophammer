# musicindex.org Open Requests To Stophammer

## Status

Open - 2026-09-25. This document consolidates each open request from the
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
| 1 | Summary fields on each `remote_items` entry | API field addition | Wanted |
| 2 | `itemGuid` and `title` on remote items | API field addition | Wanted |
| 3 | Summary fields on search results | API field addition | Wanted |
| 4 | A stated maximum for `limit` | Contract correction | Small |
| 5 | Track summary fields on each list entry | API field addition | Wanted |
| 6 | The deployed revision in release images | Regression | Small |
| 7 | Link and role facts on publisher feed rows | API field addition | Wanted |
| 8 | An index for the route history read | Performance | Wanted |
| 9 | Track titles and an image on each copy row | API field addition | Wanted |
| 10 | Link fact filters on the publisher feed list | API parameter addition | Wanted |

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

## Stophammer Answers - 2026-09-25

Stophammer examined each request against the live API and the source at
commit `64052ea`. The work plan in `docs/plans/client-requests-work-plan.md`
of the Stophammer repository gives the sequence of the work. These answers are
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

Stophammer examined requests 5 to 9 against the live API, the source at
`v0.4.0`, and a copy of the production data of 2026-09-26. These answers are
advisory. The ADR that each answer names is the owner of the rule.

| # | Answer |
|---|---|
| 5 | Complete in release 0.4.0. Each entry with a `remote_track_guid` gives `remote_track_title`, `remote_track_duration_secs` and `remote_track_image_url`, with the names of this request. ADR 0059 §5 owns the rule |
| 6 | Complete in release 0.3.0. `GET /node/info` of the release images gives `git_revision` and `built_at`. On 2026-10-02 it gives `b3bc0b3` |
| 7 | Complete in release 0.4.0 for `/v1/feeds/recent`, with `include=link_facts` (ADR 0068). The search part is declined: ADR 0038 keeps publisher feeds out of the search index, so each search row has the medium `music`. `GET /v1/node/capabilities` lists `link_facts` under `feed_list` |
| 8 | Confirmed. Each read examined all 30,705 `track_upserted` events of the copy and parsed their payload. Two partial indexes on the `json_extract` expressions of the query took the read from 0.095 to 0.014 seconds on that copy, with the same answer. The query and the answer do not change. Release 0.4.1 carries it |
| 9 | Confirmed. `feed_copies` stores `item_guids`, but no item title and no image. `item_titles` and `image_url` need new stored values, so they need an amendment of ADR 0058 §1. Stophammer gives the three fields together, after that amendment. A copy row gets the titles and the image at the next crawl that observes the copy. The publisher of a copy chooses its image, and for an impersonation that is the attacker. Stophammer recommends that the page shows the image of an open copy only after a person asks for it |

## Requests That v4vmm Also Makes

musicindex.org is a second client for these v4vmm requests. This may matter
for their priority.

| v4vmm # | Request | What it gives musicindex.org |
|---|---|---|
| 1 | Album summary fields in each publisher relationship entry | The publisher page reads each album separately: a maximum of 24, six at a time, and a "Show all" control for the remainder (`ALBUM_PAGE_SIZE` and `FEED_FETCH_CONCURRENCY`, lines 1881 and 1882). With the fields, one request per publisher |
| 2 | A deployed revision that a client can read | musicindex regenerates `api.json` after each Stophammer deploy. On 2026-09-25 the deploy was visible only from the new routes in `/openapi.json`, whose `info.version` stays `0.1.0` |
| 4 | Field renames are breaking changes | The deploy of 2026-09-25 removed `/v1/proofs/challenge` and `/v1/proofs/assert` with no version change. musicindex.org used neither route. On 2026-09-25 the published `api.json` still lists both. The next regeneration removes them |

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
