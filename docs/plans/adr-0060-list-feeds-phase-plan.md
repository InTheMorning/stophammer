# ADR 0060 Phase Plan: List Feeds

Owner: [ADR 0060](../adr/0060-a-list-feed-keeps-its-items.md), Accepted on
2026-09-26. This plan states no rule.

## Goal

A `musicL` playlist keeps the `itemGuid` and the `title` of each entry, and
its value block as source data. Each entry gives the indexed track that it
names. The crawler follows a list to the albums that it names.

## Current State

On 2026-09-26 the index holds 11 `musicL` feeds. 6 are track playlists.
"Best of Lightning Thrashes Playlist" gives 48 `remote_items` entries, with no
`itemGuid`. 2 of the 48 name an album that the index does not hold. The
evidence is in [the musicL research](../reviews/musicl-support-research.md).

## Tasks

| Task | Repository | Change | Depends on |
|---|---|---|---|
| [001](../tasks/adr-0060-task-001-parser-item-guid.md) | `stophammer-parser` | `item_guid` and `item_title` on each remote item | None |
| [002](../tasks/adr-0060-task-002-node-list-items.md) | `stophammer` | Migration, ingest, event, read fields, raw value block | 001 for the end-to-end test only |
| [003](../tasks/adr-0060-task-003-crawler-follows-lists.md) | `stophammer-crawler` | Follow a list, limit of 1,000 | 001, because the crawler builds the parser type |

Task 001 runs first. Tasks 002 and 003 can then run at the same time, because
they change different repositories.

## Deploy Sequence

1. The node, with `./deploy.sh indexer`. It accepts the new optional ingest
   fields before any crawler sends them.
2. The crawler, with `./deploy.sh crawler`. It includes the parser.
3. A `feed` crawl of the 11 `musicL` feed URLs. The crawler follows each list
   to its albums, and the node stores `itemGuid` and `title`.

The order is safe also when it is reversed, because each ingest field is
optional. With the node first, no field is lost.

## Verification After The Deploy

Mechanical, with the live API:

- `GET /v1/feeds/65b0c4fe-e65f-5642-9a28-69355b876112?include=remote_items`
  gives `remote_item_guid` on each entry, and `remote_track_guid` on each
  entry whose album is indexed.
- The album "Disciples by Design" by Prismind
  (`05b75483-9f5b-5236-bd66-69e9d3e1b995`) is indexed after the crawl.
- `include=payment_routes` on a `musicL` feed gives an empty list.

Operator check on the VPS, because no route gives the rows:

- `feed_list_value_raw` holds rows for the playlists that have a value block.
  The research found a value block on 6 `musicL` feeds: 3 playlists, the
  catalog and 2 empty lists.

## Result Of The Deploy Of 2026-09-26

Commit `9b6dc21` of the node and the crawler with the new parser are
deployed. A `feed` crawl of the 11 `musicL` feeds followed 85 URLs in wave 2.

| Playlist | Entries | With `itemGuid` | Track found | Albums missing |
|---|---|---|---|---|
| Best of Lightning Thrashes | 48 | 48 | 48 | 0 |
| Lightning Thrashes 1 - 60 | 383 | 383 | 367 | 10 |
| Boostagram Ball 1 to 25 | 278 | 278 | 268 | 8 |
| The College Years | 10 | 10 | 10 | 0 |
| Death of the Close Minded | 8 | 8 | 8 | 0 |
| Kolomona's Test Playlist | 21 | 21 | 21 | 0 |

- The two large Kolomona playlists give no `feedUrl`. The crawler counted
  278 and 383 entries that it cannot follow.
- 17 album GUIDs were missing. 5 are in the Podcast Index snapshot of
  2026-09-19, and 12 are not.
- A `feed` crawl of the 5 URLs indexed 1. The node rejected 3 by the medium
  check: 2 are `podcast` feeds, and 1 gives no medium. Wavlake answered `404`
  for the last one.
- Thus 16 GUIDs stay missing, and their entries give null. This is correct,
  because the index holds only music feeds.
- The Prismind album `05b75483-9f5b-5236-bd66-69e9d3e1b995` is indexed.
- `include=payment_routes` on a playlist gives an empty list.
- On the VPS, `feed_list_value_raw` holds rows for 6 feeds, 15 rows in
  total. These are the 6 feeds that the research found with a value block.

## Risks

- A playlist names more than 1,000 feeds. The crawler follows the first 1,000
  and reports the rest. On 2026-09-26 the largest names 383 tracks.
- A playlist names an album on a host that sends `429`. The existing host
  delay and retry apply.
- A community node on an older version cannot parse the new event type. On
  2026-09-26 the operator runs no community node.
