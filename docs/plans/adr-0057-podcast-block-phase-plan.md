# ADR 0057 Phase Plan: A Feed Can Block This Index

Owner: [ADR 0057](../adr/0057-a-feed-can-block-this-index.md), Accepted on
2026-09-25. This plan states no rule.

## Goal

A `podcast:block` tag at the source URL removes the feed from this index, and
the publisher can reverse it.

## Current State

- The parser reads no `podcast:block` tag.
- The ingest contract has no field for it.
- The crawler submits the parser type `IngestFeedData` as it is. So a new
  parser field reaches the node when the crawler is built with the new
  parser.
- The crawler does not follow the links of a rejected feed.
- `db::delete_feed_with_event` retires a record with a signed `FeedRetired`
  event, and writes the blocks that it receives. ADR 0057 needs it with no
  block.

## Tasks

| Task | Repository | Change | Depends on |
|---|---|---|---|
| [001](../tasks/adr-0057-task-001-parser-reads-block.md) | `stophammer-parser` | A typed list of the channel `podcast:block` tags | None |
| [002](../tasks/adr-0057-task-002-node-applies-block.md) | `stophammer` | The ingest field, the rule, the retirement, the reason `source_blocked` | ADR 0044 task 003 |
| [003](../tasks/adr-0057-task-003-crawler-and-deploy.md) | `stophammer-crawler` | Build with the new parser, one test, and the deploy | 001 and 002 |

## Deploy Sequence

1. The node. It accepts the optional field and applies the rule.
2. The crawler, built with the new parser. It sends the field.

With the other sequence, the node ignores the unknown field, because serde
ignores it. No feed is blocked until both are deployed.

## Verification After The Deploy

- Mechanical: `GET /openapi.json` gives the reason `source_blocked`.
- Operator check: a test feed on a host that the operator controls gets
  `<podcast:block>yes</podcast:block>`. A `feed` crawl of it answers
  `source_blocked`, and `GET /v1/feeds/{guid}` answers `404`. The tag is then
  removed, and the next crawl admits the feed again. No test can do this
  check, because it needs the deployed node and a public feed.

## Result Of The Deploy Of 2026-09-26

The node commit `76487f7` and the crawler are deployed. `/openapi.json` gives
the reason `source_blocked`. The operator test passed on a feed on a host of
the operator:

1. The index admitted the feed.
2. A `yes` block removed it. The answer was `source_blocked`, and a read gave
   `404`.
3. The removal of the tag admitted it again.

## Risks

- A wrong direction of a rule removes a feed or keeps one that asked to leave.
  The guards of ADR 0057 test each direction.
- On 2026-09-26 no indexed feed has a `yes` block. The deploy removes no feed
  that the operator can see in advance. A measurement before the deploy can
  confirm this from the fetch cache.
