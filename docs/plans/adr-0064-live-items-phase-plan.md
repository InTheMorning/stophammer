# ADR 0064 Phase Plan: Live Items

Date: 2026-09-26. This plan states no rule.
[ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) owns each rule. [The case
table](adr-0064-live-item-cases.md) gives the expected result of each type of
live item.

## Goal

The index keeps each `pending`, `live` and `ended` item with its relay link.
It applies the limits of ADR 0064 §4. It gives the rows in two reads with
three views.

## Non-Goals

The non-goals of ADR 0064. Also: no change in `stophammer-crawler` code, no
change to the relay, and no change to `CONFIRMING_RELAY_HOSTS` in production.

## Assumptions

- The crawler sends the `IngestFeedData` of the parser without a change, so a
  new parser field reaches the node when the crawler is built again.
- A missing `Option` field of `LiveEvent` deserializes as `None`. An old event
  in the log and an old community node stay valid.
- A community node verifies the signature over the stored payload text, not
  over a new serialization.

## Affected Modules

| Repository | Module | Change |
|---|---|---|
| `stophammer-parser` | `engine`, `types` | Read `podcast:liveValue` |
| `stophammer` | `live`, new | The rules of §4 and §6 as pure functions |
| `stophammer` | `ingest`, `model`, `event`, `db`, `apply` | The relay link, the compare |
| `stophammer` | `api` | The ingest rules, no track from a live item, the SSE message |
| `stophammer` | `query`, `openapi` | The two reads |
| `stophammer` | `main` | Read `CONFIRMING_RELAY_HOSTS` at start |

## Sequence

| Task | Repository | Goal | Needs |
|---|---|---|---|
| [001](../tasks/adr-0064-task-001-parser-live-value.md) | `stophammer-parser` | Read `podcast:liveValue` on a live item | None |
| [002](../tasks/adr-0064-task-002-node-storage.md) | `stophammer` | Store and replicate the relay link. Compare a sorted, deduplicated set | None |
| [002b](../tasks/adr-0064-task-002b-node-ingest-rules.md) | `stophammer` | `src/live.rs` with the ingest rules. Keep `ended` rows. No track from a live item | 002 |
| [003](../tasks/adr-0064-task-003-node-feed-read.md) | `stophammer` | `CONFIRMING_RELAY_HOSTS`, the row response, `live_items` on the feed | 002b |
| [003b](../tasks/adr-0064-task-003b-node-list-route.md) | `stophammer` | `GET /v1/live-items` with the views and filters | 003 |
| [004](../tasks/adr-0064-task-004-deploy.md) | All three | Build, deploy, check production | 001, 003b |

Task 001 and task 002 can run at the same time. They change different
repositories.

## Schema And API

- Migration `0044_live_item_relay_link.sql` adds `live_value_uri` and
  `live_value_protocol` to `live_events`. The `status` check already accepts
  `ended`.
- `LiveEventsReplacedPayload` gains the two fields in each `LiveEvent`, as
  optional fields.
- `GET /v1/feeds/{guid}` gains `live_items`. `GET /v1/live-items` is new. The
  guards of ADR 0044 cover both.

## Risk Areas

- **The removal of the track promotion** in `src/api.rs` touches the ingest
  handler, which is long. A wrong cut can change the track diff of a normal
  item. Task 002b needs a test that a normal item still becomes a track.
- **The 6 tracks in production** leave at the next crawl of their feeds. This
  is intended, and task 004 checks it.
- **`live_event_ended`** depends on the promotion today. Task 002b changes its
  trigger.
- **The ingest handler and the event compare** run on each crawl. A defect in
  the compare can emit an event at each crawl. Task 002 tests it.

## Test Strategy

- Unit tests in `src/live.rs` for each rule, with no database.
- Integration tests in `tests/adr0064_*.rs` for storage, ingest, replication,
  and the two reads. The view tests set the clock and the relay host list.
- The migration test.
- The guards of ADR 0044.

## Rollback

- Before the deploy: revert the commits of each task. Each task is one commit
  in one repository.
- After the deploy: the node can go back to the previous image. Migration 0044
  only adds columns, so the previous code still runs on the new database. The
  previous code again makes a track from an `ended` item. The 6 tracks come
  back at the next crawl of their feeds.

## Review

[The review checklist](../reviews/adr-0064-review-checklist.md) applies to each
task.

## Measurement Before The Deploy

On 2026-09-27, before the deploy of tasks 002 to 003b, the primary gave these
counts:

| Count | Value |
|---|---|
| `live_events` rows | 10 |
| `track_removed` events | 410 |
| `live_events_replaced` events | 14 |
| Tracks that came from a live item | 6 |

After the next crawl of the feeds with live items, the expected values are:

- `track_removed`: 416. The 6 tracks from live items leave.
- Tracks that came from a live item: 0.
- `live_events_replaced`: 14, plus one event for each feed that gains `ended`
  rows. The three feeds of the 6 tracks are such feeds. The two "100% Retro"
  feeds emit no event, because their rows do not change.

## Measurement After The Deploy

On 2026-09-27 a forced `feed` crawl of the three feeds with recordings and the
two "100% Retro" feeds gave these counts:

| Count | Before | After |
|---|---|---|
| `live_events` rows | 10 | 16 |
| `track_removed` events | 410 | 416 |
| `live_events_replaced` events | 14 | 17 |
| Tracks that came from a live item | 6 | 0 |

Each value agrees with the expected value. The 6 recordings are now `ended`
rows. The two "100% Retro" feeds emitted no event, so the compare of task 002
works in production.

The mechanical checks of task 004 are thus met. On the same day the operator
did the visual check of `/api`, and it passed. ADR 0064 is complete.

## Outside This Plan

- `CONFIRMING_RELAY_HOSTS` stays empty until the relay gives its lease
  (`splitkit/docs/plans/live-lease-heartbeat-proposal.md`).
- `v4vmm` writes a full URL in `liveValue`
  (`v4vmm/docs/plans/livevalue-uri-is-a-url.md`).
- Items 10 and 11 of the remaining work plan.
