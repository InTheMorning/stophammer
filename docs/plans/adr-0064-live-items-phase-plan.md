# ADR 0064 Phase Plan: Live Items

Date: 2026-09-26. This plan states no rule.
[ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) owns each rule. [The case
table](adr-0064-live-item-cases.md) gives the expected result of each type of
live item.

## Tasks

| Task | Repository | Goal | Needs |
|---|---|---|---|
| [001](../tasks/adr-0064-task-001-parser-live-value.md) | `stophammer-parser` | Read `podcast:liveValue` on a live item | None |
| [002](../tasks/adr-0064-task-002-node-ingest.md) | `stophammer` | Store, limit and replicate live rows. No track from a live item | 001 |
| [003](../tasks/adr-0064-task-003-node-reads.md) | `stophammer` | The two reads, the views and `CONFIRMING_RELAY_HOSTS` | 002 |
| [004](../tasks/adr-0064-task-004-deploy.md) | All three | Build the crawler with the new parser, deploy, check production | 003 |

Task 001 and the first half of task 002 can run at the same time. Task 002
reads the new parser fields only through the ingest DTO.

## Order Of Deploy

1. The node, with tasks 002 and 003. The node accepts the new DTO fields as
   optional, so an old crawler still works.
2. The crawler, built with the parser of task 001.
3. The community nodes. Until they update, they store no relay link. The
   payload fields are optional, so an old community node still applies the
   event.

## Checks After The Deploy

- The 6 tracks that came from `ended` items leave the index at the next crawl
  of their feeds, with a signed `TrackRemoved` each. Their items stay as
  `ended` rows.
- The two feeds of "100% Retro" stop emitting `LiveEventsReplaced` at each
  crawl.
- `GET /v1/live-items` gives no row. `GET /v1/live-items?view=all` gives the
  rows of production.

## Outside This Plan

- `CONFIRMING_RELAY_HOSTS` stays empty until the relay gives its lease. The
  proposal is `splitkit/docs/plans/live-lease-heartbeat-proposal.md`.
- `v4vmm` writes a full URL in `liveValue`
  (`v4vmm/docs/plans/livevalue-uri-is-a-url.md`).
- Items 10 and 11 of the remaining work plan.
