# ADR 0064: A Live Item Is An RSS Fact

## Status
Accepted on 2026-09-26

Supersedes [ADR 0021](archive/0021-live-events.md).

Amended on 2026-09-26: the sections "Invariants", "Non-Goals" and
"Alternatives Considered" are added for the implementation plan. They change
no decision.

Amended on 2026-09-27: section 4 also drops an item with an unknown status.
The review of task 002b found that such an item made the whole ingest of its
feed fail.

Amended on 2026-10-01, accepted the same day: section 6 gives each row two derived
fields, `in_now_view` and `in_upcoming_view`. v4vmm reported two rows of the
"100% Retro" feeds with the status `live` and an end in December 2022. `view=now`
leaves them out, but `view=all` gives them with no sign of that.

## Date
2026-09-26

## Context
ADR 0021 makes the index find the start of a live stream "within seconds". It
has two parts for this. A podping starts a crawl. A crawl scheduler polls a
feed with a `pending` live item at an interval of 60 seconds. On 2026-09-26
the code and the deployment give these facts:

1. The parser reads each `<podcast:liveItem>`. The node stores each `pending`
   and `live` item in `live_events`, and replicates the rows with the signed
   event `LiveEventsReplaced`. An `ended` item with an enclosure becomes a
   track.
2. No crawler mode has a poll interval. The `gossip` mode crawls a URL when a
   podping arrives. [ADR 0062](0062-a-podping-is-never-dropped.md) does not
   delay a podping with the reason `live` or `liveEnd`.
3. The live metadata relay, `musicindex-live-relay` in the `splitkit`
   repository, serves `/v1/liveitems/*`. It holds the real-time state of a
   stream and its payment routes. A feed names its relay event with
   `<podcast:liveValue uri="EVENT_ID" protocol="socket.io"/>`. The parser does
   not read that tag.
4. No read route gives a `live_events` row. The node makes the SSE messages
   `live_event_started` and `live_event_ended`, but
   [ADR 0037](0037-defer-public-sse-route.md) keeps `GET /v1/events`
   unregistered. Thus a client sees a live item only after it becomes a track.
5. ADR 0021 removes a live item that never ends after 7 days. No code does
   this.
6. The Podcast Namespace makes `start` required and `end` recommended on a
   live item. `end` is the time when the stream is intended to end. A stream
   can continue past it.
7. The backup of 2026-09-26 at 13:16 UTC holds 10 `live_events` rows from two
   feeds. Each row has an `end` in 2022 or 2023. The two feeds give a station
   that runs 24 hours a day as a schedule of shows, one item for each show.
   Six tracks came from `ended` items.
8. A station that runs 24 hours a day usually has `medium` `podcast`. The
   medium check admits only `music`, `publisher` and `musicL`, so the index
   does not see such a station.

The operator decided these points on 2026-09-26:

- Real-time live state is outside the index. The index makes a live item
  available to its clients and nodes, and a podping starts the crawl that
  finds it.
- A recording is outside the index. An artist publishes a recorded show as a
  normal item, in a feed of their choice.
- A `live` item with no relay link is not a V4V stream when it has no `end`
  or runs longer than 6 hours. The index does not keep it. An artist who plays
  longer, or a festival that pays its performers, uses a relay.
- A `pending` item often gets its relay link only when the show starts. The
  index looks only at the `start` and `end` of a `pending` item.
- The index keeps an `ended` item as a record of when a stream was live.
- The index keeps the RSS truth. The default read aligns with what happens,
  also when a feed is wrong.
- Only a relay that is compatible with `musicindex-live-relay` can confirm a
  stream. The relays of thesplitkit.com and of Kolomona give no such function.
- A `live` row with no confirming relay stays in the default view until 1
  hour after its end.
- A `pending` row with no `end` leaves the view of coming items 1 hour after
  its `start`.

[The case table](../plans/adr-0064-live-item-cases.md) lists each type of live
item and how the index serves it. [The security
review](../reviews/adr-0064-live-items-security-review.md) gives the reason for
each limit.

## Decision

### 1. The index keeps a live item as an RSS fact

The parser, the `live_events` table and `LiveEventsReplaced` stay. A row gives
what the last crawl of its feed read. The index keeps `pending`, `live` and
`ended` items. An `ended` row records when a stream was live. The index makes
no track from a live item.

### 2. The relay owns the real-time path

The relay owns the real-time state of a stream and its payment routes. The
index does not poll a feed for a live item, and it does not call a relay. A
podping starts the crawl, and ADR 0062 owns the rule that a `live` or
`liveEnd` podping is not delayed.

### 3. The index gives the relay link

The parser reads `<podcast:liveValue>` on a live item. The node stores its
`uri` and `protocol` on the row, replicates them in `LiveEventsReplaced`, and
gives them in each read.

A relay that gives a lease for "on air" is a confirming relay. The node
setting `CONFIRMING_RELAY_HOSTS` lists the hosts of such relays. It is empty by
default, and the operator adds a host when its relay gives the lease. A row
has a confirming relay when its `uri` is an `https` URL on a listed host. A
`uri` that is only an event identifier names no host, so its row has no
confirming relay. Each read gives `confirming_relay: true` or `false`.

### 4. The limits of the ingest

- The ingest does not keep an item whose `status` is not `pending`, `live`
  or `ended`, and gives a warning for it.
- The ingest does not keep a `live` item with no relay link in these
  conditions, and gives a warning for it:
  - It has no `start` or no `end`.
  - Its `end` is more than 6 hours after its `start`.

  This rule does not apply to a `pending` or `ended` item.
- A feed has at most 10 `pending` and `live` rows. The ingest keeps the first
  10 in RSS order and gives a warning for the others.
- A feed has at most 10 `ended` rows. The ingest keeps the 10 with the newest
  `start`.
- The ingest keeps the first item of each `live_item_guid` and ignores a
  second item with the same GUID.
- The node compares the stored rows and the new rows in the order of
  `live_item_guid`. An unchanged set of rows emits no `LiveEventsReplaced`.
- `content_link` and a `uri` that is a URL go through the web URL rule of
  [ADR 0054](0054-a-fetch-reaches-only-public-feed-hosts.md) §4. The ingest
  warns about a value that is not `http` or `https`, and a read gives no such
  value.
- A `musicL` feed gives no live row. A `publisher` feed gives live rows as a
  `music` feed does.

### 5. A row has no age limit

A row stays while its feed gives the item. The next crawl of the feed replaces
the rows of that feed. The index removes no row because of its age.

### 6. The reads

- `GET /v1/feeds/{guid}` gives `live_items`: each row of the feed, with each
  status. A client reads "last live" from the `ended` rows.
- `GET /v1/live-items` gives the rows of all feeds, with `feed_guid`, in the
  `cursor` and `limit` paging of the other list routes.
- Each row gives `live_item_guid`, `title`, `status`, `content_link`,
  `scheduled_start`, `scheduled_end`, `live_value_uri`, `live_value_protocol`
  and `confirming_relay`.
- Each row also gives `in_now_view` and `in_upcoming_view`. Each is true when
  the view of the same name gives the row at the time of the read. The node
  computes them with the same rule as the view. `status` stays the value of
  the feed. A client that reads `view=all` or `live_items` thus needs no rule
  of its own. It knows from the row that a `live` row with an end in 2022 is
  not live now.
- The list takes `view`. The default is `now`:

  | View | Gives |
  |---|---|
  | `now` | Each `live` row with a confirming relay or with no `end`. Each other `live` row until 1 hour after its `end` |
  | `upcoming` | Each `pending` row until its `end`. A `pending` row with no `end` until 1 hour after its `start` |
  | `all` | Each row. The filters below apply |

  Section 4 makes sure that a `live` row with no `end` has a relay link.
- With `view=all`, the list takes these filters. Each one compares a stored
  value:

  | Filter | Values | Selects |
  |---|---|---|
  | `status` | `pending`, `live`, `ended` | The rows with that status |
  | `live_value` | `set`, `none` | The rows with or without a relay link |
  | `ends_after` | Unix seconds | The rows whose `scheduled_end` is after the time |
  | `starts_after` | Unix seconds | The rows whose `scheduled_start` is after the time |

- The list pages in the order of `feed_guid`, then `live_item_guid`. It does
  not sort on a value that the feed controls. A client sorts a page by its own
  rule.
- A read gives no row of a feed that is not public.

[ADR 0044](0044-api-contract-declares-its-fields.md) owns the contract of the
two responses.

### 7. The views are the only rule that the index adds

The two views apply a margin of 1 hour to the times that the feed gives. The
rows and their fields stay as the feed gives them. `in_now_view` and
`in_upcoming_view` state the result of the same rule for each row. They add no
rule.

### 8. The index derives no stream type

The index does not label a live item as a stream that runs 24 hours a day or
as a concert. When a feed gives a tag for the type, a new decision reads it as
a fact.

## Invariants

- A read gives each stored field as the feed gave it. Only `confirming_relay`
  and the views are derived, and section 6 defines each one.
- The index never makes a track, a payment route or a value time split from a
  live item.
- `src/live.rs` holds each rule of sections 4 and 6. The ingest and the reads
  call the same functions.
- An unchanged set of live items emits no `LiveEventsReplaced`.
- The index never calls a relay.
- The list order does not depend on a value that the feed controls.

## Non-Goals

- The real-time state of a stream. The relay owns it.
- A recording of a live show. The artist publishes it as a normal item.
- A station on a feed with `medium` `podcast`. Item 10 of the remaining work
  plan holds it.
- A second crawl after a `live` or `liveEnd` podping. Item 11 holds it.
- A public SSE route. ADR 0037 still applies.

## Alternatives Considered

- **A poll of 60 seconds for a `pending` item, as ADR 0021 says.** Rejected. A
  podping already starts a crawl at once, and a poll gives only the RSS, not
  the stream.
- **A time of the last confirming crawl (`confirmed_at`).** Rejected. It
  measures the crawl schedule of the index, not the stream. A station with no
  change sends no podping, so the value would hide it.
- **An assumed end of `start` plus 6 hours for an item with no `end`.**
  Rejected for the ban of section 4. The ban removes the only row that needed
  it.
- **A call from the node to the relay.** Rejected. The index keeps the RSS
  truth, and a community node cannot call the relay of the primary.
- **A promotion of an `ended` item to a track.** Rejected. A recording is
  outside the index.

## Consequences

- The crawler needs no scheduler. Item 9 of the remaining accepted work plan
  closes.
- The six tracks that came from `ended` items leave the index at the next
  crawl of their feeds. Their items stay as `ended` rows. Step 4b of the ingest in `src/db.rs` removes a track
  that the crawl no longer gives, and signs `TrackRemoved`.
- `live_event_ended` needs a track from an `ended` item. The task changes it
  to fire when a row changes to `ended`. The SSE code stays unregistered, as
  ADR 0037 leaves it.
- The parser, the ingest DTO, the `live_events` table and the payload of
  `LiveEventsReplaced` gain the relay link. A community node needs the new
  version to store it.
- A client that uses the default view gets each current live row. It asks
  the relay of each row with `confirming_relay: true`. Before the relay gives
  the lease, `CONFIRMING_RELAY_HOSTS` stays empty, and each row follows the
  time rule.
- A row with no confirming relay can be an overtime concert or a forgotten
  `ended` item. A client sees the two as the same row for 1 hour after its
  end.
- A `liveValue` with only an event identifier, as `v4vmm` writes it today,
  names no confirming relay. It is still a relay link, so section 4 keeps the
  item.
- A 24-hour stream on a relay that is not confirming stays in `now` while its
  feed gives it as `live`. The index cannot check that relay.
- The 8 `pending` rows of production have an `end` in the past. They stay in
  `all` and are not in `upcoming`. The 2 `live` rows have no relay link and an
  `end` at most 3 hours after their `start`, so section 4 keeps them. They are
  not in `now`.
- A station on a feed with `medium` `podcast` stays outside the index. Item 10
  of the remaining work plan holds that question.
- A crawl that starts at once after a podping can read an old copy of the feed
  from a cache. Item 11 of the remaining work plan holds that question.
- The relay plans a lease for "on air"
  (`splitkit/docs/plans/live-lease-heartbeat-proposal.md`). This ADR does not
  depend on it. When the lease is deployed, the operator adds the host of the
  relay to `CONFIRMING_RELAY_HOSTS`.

## Checks

Mechanical. Each check is an integration test in `tests/`:

- The feed response gives `live_items` from `live_events`, with each status
  and the relay link.
- `GET /v1/live-items` pages the rows of two feeds. Each view and each filter
  selects the rows of the case table.
- A `live` item with no relay link is not kept when it has no `end`, no
  `start`, or an `end` more than 6 hours after its `start`. A `pending` item
  with the same times is kept.
- `now` gives a `live` row with no confirming relay until 1 hour after its
  `end`, and not after.
- `now` gives a `live` row with a confirming relay after its `end`, and a
  `live` row with a relay link and no `end`.
- `upcoming` gives a `pending` row with no `end` until 1 hour after its
  `start`.
- A `uri` on a host that `CONFIRMING_RELAY_HOSTS` does not list, and a `uri`
  that is only an identifier, give `confirming_relay: false`.
- A feed with 11 `live` rows stores 10, and a feed with 11 `ended` rows keeps
  the 10 newest.
- A second ingest of the same rows in a different order, or with a duplicate
  GUID, emits no `LiveEventsReplaced`.
- An `ended` item makes an `ended` row and no track.
- A `javascript:` `content_link` gives a warning at ingest, and the two reads
  give no value for it.
- A deleted feed gives no row in `GET /v1/live-items`.
- A `musicL` feed gives no row.
- For each row of the case table, `in_now_view` is true exactly when
  `view=now` gives the row, and `in_upcoming_view` is true exactly when
  `view=upcoming` gives it. The same holds for `live_items` of a feed.
- The guards of ADR 0044 pass with the new route and fields.
