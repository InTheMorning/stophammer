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

## Requests That v4vmm Also Makes

musicindex.org is a second client for these v4vmm requests. This may matter
for their priority.

| v4vmm # | Request | What it gives musicindex.org |
|---|---|---|
| 1 | Album summary fields in each publisher relationship entry | The publisher page reads each album separately: a maximum of 24, six at a time, and a "Show all" control for the remainder (`ALBUM_PAGE_SIZE` and `FEED_FETCH_CONCURRENCY`, lines 1881 and 1882). With the fields, one request per publisher |
| 2 | A deployed revision that a client can read | musicindex regenerates `api.json` after each Stophammer deploy. On 2026-09-25 the deploy was visible only from the new routes in `/openapi.json`, whose `info.version` stays `0.1.0` |
| 4 | Field renames are breaking changes | The deploy of 2026-09-25 removed `/v1/proofs/challenge` and `/v1/proofs/assert` with no version change. musicindex.org used neither route. On 2026-09-25 the published `api.json` still lists both. The next regeneration removes them |

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
feed or a track would support an "On playlists" rail. It needs request 2
first. On 2026-09-25 the node indexes one `musicL` feed.

**Summary fields in the new copy and route-history routes.** musicindex.org
does not use `/v1/feeds/{guid}/copies` or `/v1/feeds/{guid}/route-history`
yet. It can show a copy warning from `copy_count` alone.

## Answered, No Action

| Item | Answer | Evidence |
|---|---|---|
| Can a client list `musicL` feeds? | Yes. `/v1/feeds/recent?medium=musicL` | Live API, 2026-09-24 |
| Do feed and track reads accept several includes in one request? | Yes. For example `include=tracks,source_contributors,remote_items,payment_routes,publisher` | Live API, 2026-09-24 |
| Does the publisher of an album come from `publisher_text`? | No. `publisher_text` is the `itunes:owner` name. The publisher is the feed that `include=publisher` names | Stophammer ADR 0049 §5 |
| Do publisher and `musicL` feeds appear in search? | No, by design | Stophammer ADR 0038 |
