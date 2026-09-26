# Client Requests Work Plan

Status: Approved - 2026-09-25. The operator approved this sequence: wave 1 is
items 1 to 4 and item 6. Item 5 follows, after its ADR. Then ADR 0044 tasks 002
and 003, then ADR 0057.

Wave 1 is complete and deployed on 2026-09-26, commit `607bb3a`. Item 5 is
complete and deployed on 2026-09-26, commit `264706e`. On 2026-09-25 the operator decided item 7: ADR 0044
stays as it is. The operator also put ADR 0060 after item 5 and before ADR
0044 task 002. ADR 0060 answers musicindex request 2.

This plan states no rule. Each item names the ADR that owns its rule. An item
without an owner needs a new ADR before the work starts, and the item says so.

## Sources

- [The v4vmm open requests](v4vmm-open-requests.md), with the Stophammer
  answers of 2026-09-25.
- [The musicindex.org open requests](musicindex-open-requests.md), with the
  Stophammer answers of 2026-09-25.
- The live API at `https://api.musicindex.org` and its `/openapi.json`, read on
  2026-09-25.
- The source at commit `64052ea`.

Each of the two request documents has a copy in the client repository. When
an answer changes, change the two copies of that document in the same
session. Each copy needs a commit in its own repository.

## Goal

Each client can show a page from fewer calls, and it can trust the contract
and the capabilities route. Each change in this plan adds a field or corrects
a defect. No change renames or removes a field.

## Non-Goals

- A version change of the API. ADR 0044 §4 classifies an added field as not a
  breaking change.
- A reverse list of the albums that name a publisher, or of the `musicL`
  feeds that name a feed. The clients did not request them now.
- Summary fields in `/v1/feeds/{guid}/copies` and
  `/v1/feeds/{guid}/route-history`. No client reads these routes.

## Priority

| Priority | Item | Requests | Owner | Size |
|---|---|---|---|---|
| 1 | The capabilities route gives the includes that each route accepts | v4vmm 3 | Defect. No ADR | Small |
| 2 | The contract gives the maximum of each `limit`, and `/v1/publishers` gives a correct `has_more` | musicindex 4 | ADR 0044 | Small |
| 3 | A search result gives the artist, the track count and the duration | musicindex 3 | ADR 0042 | Small |
| 4 | `/node/info` gives the revision and the build time | v4vmm 2 | Added field. No ADR | Small |
| 5 | An entry that names a feed gives the summary of that feed | v4vmm 1, musicindex 1 | New ADR | Medium |
| 6 | ADR 0049 states that a publisher resolution is not kept | v4vmm open question | ADR 0049 | Document |
| 7 | The operator decides if a route removal needs a version | v4vmm 4 | ADR 0044 | Decided |
| 8 | A list feed keeps its items, and each entry gives its track | musicindex 2 | ADR 0060, Proposed | Large |

Items 1 to 3 correct a condition that a client measured on the live API.
Items 4 and 5 add a feature. Item 5 is last of these because it needs a new
ADR. Items 6 and 7 change no code, and the operator can do them at any time.

## 1. The Capabilities Route Gives The Accepted Includes

**Current state.** `handle_capabilities` in `src/query.rs` lists the
includes by hand. For tracks, it does not list `remote_items` and
`publisher`. `GET /v1/tracks/{guid}` accepts both. The example response in
`src/openapi.rs` has the same omission. The track route ignores an include
name that it does not know, and gives no error.

**Change.**

- Declare one list of include names for feeds and one for tracks in
  `src/query.rs`. `handle_capabilities` gives these lists.
- Correct the example in `src/openapi.rs`.
- Keep the behavior for an unknown include name. A `400` for an unknown name
  can break a client that works today. The contract can state that the route
  ignores an unknown name.

**Mechanical criteria.**

- A test reads `/v1/node/capabilities`. For each track include in the
  response, a track read with that include gives the key of that include.
  The same test does this for feeds.
- The failure message names v4vmm request 3 and the include list in
  `src/query.rs`.

This rule broke on the live node on 2026-09-25. That incident is the reason
for the test.

## 2. The Contract Gives The Maximum Of Each `limit`

**Current state.** The code limits `limit` to these maximums. The contract
states only the last two.

| Route group | Maximum | Code |
|---|---|---|
| Routes that read `ListQuery`, `PublisherDetailQuery` or `ArtistTracksQuery` | 200 | `capped_limit` in `src/query.rs` |
| `/v1/search` and `/v1/publishers` | 100 | `handle_search`, `handle_publisher_search` |
| `/v1/copies` and `/v1/guid-changes` | 100 | Stated in the contract |

`/v1/publishers` has no paging. On 2026-09-25, `limit=20` gave 20 rows with
`has_more: false`, and `limit=150` gave 100 rows with `has_more: false`. A
client cannot know that the route cut the list.

**Change.**

- Add `minimum: 1` and `maximum` to the schema of each `limit` parameter in
  `spec_value()` in `src/openapi.rs`.
- `/v1/publishers` reads one row more than `limit`, and sets `has_more` when
  that row exists. Paging with a cursor is not in this item.
- Keep the behavior above the maximum. Do not answer `400`.

This item can go with task 002 of ADR 0044, which changes the same document.
It does not need to wait for that task.

**Mechanical criteria.**

- A test reads the generated document. Each `limit` parameter gives a
  `maximum`, and the value is equal to the limit in the code for that route.
- A test puts more publishers in the database than `limit`. The response of
  `/v1/publishers` gives `has_more: true`.

## 3. A Search Result Gives The Row Fields

**Owner.** ADR 0042 states: "A search result holds the fields that a client
needs to show a row." musicindex.org shows the artist in each row, the track
count in a feed row and the duration in a track row. The search response does
not give these values, so the rule is not met.

**Change.** Add these fields to `SearchResponseItem` in `src/query.rs`.

| Result | Field | Stored value |
|---|---|---|
| feed | `release_artist` | `feeds.release_artist` |
| feed | `release_artist_source` | `feeds.release_artist_source` |
| feed | `episode_count` | `feeds.episode_count` |
| track | `track_artist` | `tracks.track_artist` |
| track | `duration_secs` | `tracks.duration_secs` |

Each name is the name that the feed and track reads use now. Use the same
serde rule as the other summary fields of `SearchResponseItem`. Add the
fields to the schema of the result and to `docs/API.md`.

**Mechanical criteria.**

- A test searches for an indexed feed. The feed result gives
  `release_artist`, `release_artist_source` and `episode_count`, with the
  stored values.
- A test searches for an indexed track. The track result gives
  `track_artist` and `duration_secs`, with the stored values.

## 4. `/node/info` Gives The Revision And The Build Time

**Current state.** `NodeInfoResponse` in `src/api.rs` gives only
`node_pubkey`. `info.version` of the contract is `CARGO_PKG_VERSION`, which
stays `0.1.0`.

**Change.**

- Add `git_revision` and `built_at` to `NodeInfoResponse`. Read each value at
  compile time with `option_env!`, from `STOPHAMMER_GIT_REVISION` and
  `STOPHAMMER_BUILT_AT`. Each value is null when the build did not set it.
- `deploy.sh` builds the node binary on the operator machine, where `.git`
  is present. It exports `STOPHAMMER_GIT_REVISION` from
  `git rev-parse --short HEAD`, with the suffix `-dirty` when the tree has
  changes, and `STOPHAMMER_BUILT_AT` in UTC.
- A `build.rs` emits `cargo:rerun-if-env-changed` for the two variables.
  Without it, Cargo keeps an old value in a binary that it does not build
  again.
- A build outside `deploy.sh` sets no value, and the two fields are null.
  Record the step in `docs/operations.md`.
- `/v1/node/capabilities` does not change. `/node/info` is the one owner of
  these values.

**Mechanical criteria.**

- A test reads `/node/info`. The response gives the keys `git_revision` and
  `built_at`.

**Visual criterion.**

- After the next deploy, the operator reads
  `https://api.musicindex.org/node/info`. `git_revision` is equal to the
  deployed commit. No test can do this check, because it needs the deployed
  node.

## 5. An Entry That Names A Feed Gives Its Summary

**Owner.** None. Write an ADR before the work starts. The next number is
0059. The rule to decide: each response entry that names a feed gives the
title, the image and the artist of that feed, or null when the node has not
indexed it.

**Current state.** A `publisher` entry and a `remote_items` entry each give a
GUID, a URL and the link state. A client reads each named feed with one more
call. On 2026-09-25 the publisher "James Goulding" lists 81 albums, and the
`musicL` feed "Local Theory — Full Catalog" names 13 feeds.

**Change.**

| Entry | Fields | Feed that gives the values |
|---|---|---|
| `publisher` entry | `music_feed_title`, `music_feed_image_url`, `music_release_artist`, `music_release_artist_source` | The feed that `music_feed_guid` names |
| `remote_items` entry | `remote_feed_title`, `remote_feed_image_url`, `remote_release_artist`, `remote_release_artist_source` | The feed that `remote_feed_guid` names |

The ADR decides these points:

- A `remote_items` entry resolves its feed the same way a publisher link does:
  by GUID, and then by `remote_feed_url` through `resolve_listed_feed`.
- The image is the channel image of the named feed. It is not a resolved
  value, so the name obeys ADR 0042.
- The track reads with `include=remote_items` and `include=publisher` get
  the same fields.

Each entry costs one point lookup in the node. A publisher with 131 albums
costs 131 lookups in one request, not 132 calls from the client.

**Mechanical criteria.**

- A test reads a publisher feed with `include=publisher`. Each entry for an
  indexed album gives the title, the image and the artist of that album.
- A test reads a feed with `include=remote_items`. An entry for an indexed
  feed gives its summary. An entry for a feed that is not indexed gives null
  in each summary field.

## 6. ADR 0049 States That A Resolution Is Not Kept

`music_to_publisher_facts` in `src/query.rs` calculates
`publisher_link_resolution` and `publisher_link_observed_at` at each read.
ADR 0049 section 3 already states that the node does not store a resolution.
On 2026-09-25 section 3 also names the deletion case and
`publisher_link_observed_at`. When an album feed changes or
is deleted, the next read gives the new resolution. Stophammer keeps no
earlier resolution.

**Mechanical criterion.** A test deletes an album feed that a publisher lists.
The next publisher read gives that entry as `unresolved`. If a test already
does this, the amendment names it.

## 7. The Operator Decides On Route Removal

ADR 0044 §4 answers v4vmm request 4 for fields. It gives no rule for the
removal of a route. The deploy of 2026-09-25 removed
`/v1/proofs/challenge` and `/v1/proofs/assert` in `v1`. Both routes needed
a credential, and ADR 0044 §3 does not apply to them.

Decided on 2026-09-25: keep ADR 0044 as it is. ADR 0044 §4 closes v4vmm
request 4. The operator examined these answers:

- **Keep ADR 0044 as it is.** Tell v4vmm that ADR 0044 §4 closes request 4.
  This is the recommendation. No client used the removed routes.
- **Amend ADR 0044.** A removal of a route that a client reads without a
  credential needs a new path version.

## 8. A List Feed Keeps Its Items

Owner: [ADR 0060](../adr/0060-a-list-feed-keeps-its-items.md), Proposed.

**Current state.** On 2026-09-25, Podcast Index holds 6 `musicL` track
playlists that the index does not hold. The node accepts a playlist. It drops
`itemGuid`, `title` and the value block. The evidence is in
[the musicL research](../reviews/musicl-support-research.md).

**Change.** The parser, a migration, the node read and the crawler follow
rule. ADR 0060 gives each part. The work needs its own phase plan.

**Before the build.** The operator crawls 8 of the missing `musicL` feeds
with the `feed` mode. The two test playlists of Kolomona are left out. The
node then keeps their albums, and a crawl after the build adds the tracks.

## Deferred

| Request | Reason | Condition to start |
|---|---|---|
| A reverse list of `musicL` feeds | Needs item 8 | Item 8 is complete |
| A reverse list of albums that name a publisher | v4vmm did not request it now. The query exists: `get_publisher_album_release_artists` in `src/db.rs` | v4vmm requests it |

## Risks

- Item 5 makes a publisher read slower. Measure the read of the largest
  publisher before and after the change.
- Item 2 changes `has_more` on `/v1/publishers`. A client that ignored
  `has_more` sees no change. A client that read it gets a correct value.

## Test Strategy

Each item adds the tests that its criteria name, in `tests/`. Each commit
passes the gate in `AGENTS.md`. After each deploy, the operator repeats the
live measurement that the client document gives for that request.
