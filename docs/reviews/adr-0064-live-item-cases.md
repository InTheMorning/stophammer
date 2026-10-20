# ADR 0064 Live Item Cases

Date: 2026-09-26. This plan states no rule. It lists each type of live item
that the index can receive, and shows how the index and a client serve it. It
is the input for the final text of
[ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md).

## The Signals

| Signal | Source | Meaning |
|---|---|---|
| `status` | RSS, edited by the publisher | The state that the publisher gives now |
| `start`, `end` | RSS | The plan. `start` is required, `end` is recommended |
| `podcast:liveValue` | RSS | The relay event of the stream. The index does not read it today |
| Podping `live`, `liveEnd` | The software of the publisher | "Read my feed now". The crawler reads at once (ADR 0062 §4) |
| Relay `on_air` | The relay, renewed by the broadcaster | The stream sends now. This is the lease of the relay proposal, not a present feature |

## The Client Views

ADR 0064 §6 gives three views. A confirming relay is a relay on a host of
`CONFIRMING_RELAY_HOSTS`. A relay link is any `podcast:liveValue`.

| View | Gives |
|---|---|
| Q1: `now`, the default | Each `live` row with a confirming relay or with no `end`. Each other `live` row until 1 hour after its `end` |
| Q2: `upcoming` | Each `pending` row until its `end`. With no `end`, until 1 hour after its `start` |
| Q3: `all` | Each row, with the raw filters |

In the tables below, "our relay" means a confirming relay, and "no relay"
means no confirming relay. A relay of thesplitkit.com, and a `uri` that is only
an identifier, count as no relay.

## Pending Items

| # | The feed gives | Q2 | The client shows | Result |
|---|---|---|---|---|
| P1 | `pending`, `start` in the future | Yes | "Coming up" at `start` | Correct |
| P2 | `pending`, `start` in the past, `end` in the future | Yes | "Starts soon". With a relay that is on air: LIVE | Correct. The relay also catches a publisher that forgot to set `live` |
| P3 | `pending`, `end` in the past | No | Nothing | Correct. 8 of the 10 rows in production are this case |
| P4 | `pending`, no `end`, `start` more than 1 hour in the past | No | Nothing | Correct. A `pending` item needs no relay link yet |

## Live Items

| # | The feed gives | Query | The client shows | Result |
|---|---|---|---|---|
| L1 | `live`, our relay, relay on air | Q1 | LIVE, also past `end` and with no `end` | Correct. This covers overtime and stations that run 24 hours a day |
| L2 | `live`, our relay, relay off air or `404` | Q1 | Not live | Correct after the relay lease exists. Before that, as L5 |
| L3 | `live`, a relay link that is not confirming, or a `uri` with no host | Q1 until 1 hour after `end`. With no `end`, while the feed says `live` | LIVE | Correct as far as the feed is correct. The index cannot check that relay |
| L4 | `live`, no relay, now before `end` | Q1 | LIVE | Correct as far as the feed is correct |
| L5 | `live`, no relay, `end` passed less than 1 hour ago | Q1 | LIVE, "past its scheduled end" | Overtime or a forgotten `ended`. The feed cannot tell which |
| L6 | `live`, no relay, `end` passed more than 1 hour ago | No | Nothing | Correct. 2 of the 10 rows in production are this case |
| L7 | `live`, no relay link, no `end`, or an `end` more than 6 hours after `start` | Not kept | Nothing | Not a V4V stream. The ingest drops the item and warns |

## Ended Items

| # | The feed gives | The index | The client shows |
|---|---|---|---|
| E1 | `ended` with an enclosure | Keeps an `ended` row. It makes no track | "Last live". The artist publishes a recording as a normal item |
| E2 | `ended` with no enclosure | Keeps an `ended` row | "Last live" |

## Changes In The Feed

| # | What the publisher does | The index | Result |
|---|---|---|---|
| T1 | Changes `status` and sends a `live` or `liveEnd` podping | Reads at once | Correct in seconds |
| T2 | Changes `status` and sends no podping | Reads at the next podping or `refresh` pass | Late. A confirming relay covers it in Q1 |
| T3 | Sends the podping before the host serves the new feed | Reads the old feed, and no second podping comes | Gap G3 |
| T4 | Moves `end` in overtime and sends a `live` podping | Stores the new `end` | Correct |

## Feed Types

| # | Feed | The index | Result |
|---|---|---|---|
| K1 | `medium` `music` | All cases above | Served |
| K2 | `medium` `podcast`, for example a radio station | The medium check rejects the feed | Not served. Item 10 of the remaining work plan |
| K3 | `medium` `musicL` | The ingest ignores its live items | Correct for a list feed |
| K4 | `medium` `publisher` | Stores its live items, as for `music` | Served. A label can announce a live showcase |
| K5 | A blocked or deleted feed | No row | Correct. A test checks it (R4) |

## Shapes Of A Stream

| # | Shape | The feed gives | Result |
|---|---|---|---|
| S1 | One concert | One item with `start` and `end` | P1, then L1 or L4, then E1 |
| S2 | A weekly show on a reserved relay event | A new item each week, the same `liveValue` | Each week as S1. The relay is on air only during the show |
| S3 | A station as a schedule of shows | One item for each show, each with `end` | The current show is live. When the feed stops, its rows leave Q1 and Q2 |
| S4 | A station as one item with no `end` | One `live` item | With a relay link, in `now` while the feed says `live`. With no relay link, L7 |

## Faults In The Feed

| # | Fault | The index |
|---|---|---|
| A1 | One GUID on two items | Keeps the first (F3) |
| A2 | More than 10 `pending` and `live` items | Keeps the first 10, and warns (F2) |
| A3 | A `content_link` that is not `http` or `https` | Warns, and a read gives no value (F4) |

## Decisions Of 2026-09-26

- **P4.** `upcoming` shows a `pending` row with no `end` until 1 hour after
  its `start`. A `pending` item needs no relay link, because a publisher often
  makes the relay event when the show starts.
- **L7 and S4.** A `live` item with no relay link is not a V4V stream when it
  has no `end` or runs longer than 6 hours. The ingest drops it. An artist who
  plays longer, or a festival that pays its performers, uses a relay.
- **Relays.** Any `podcast:liveValue` is a relay link. Only a relay on a host
  of `CONFIRMING_RELAY_HOSTS` can confirm "on air". thesplitkit.com and
  Kolomona give no such function.
- **T3.** A separate crawler item, outside ADR 0064. It is item 11 of the
  remaining work plan.
- **E1, E2 and K4.** A recording is outside the index. No `ended` item becomes
  a track. The index keeps `ended` rows as a record of "last live", at most 10
  for each feed.
