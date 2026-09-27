# ADR 0064 Live Items: Security Review

Date: 2026-09-26. This record states no rule. It gives the findings that
[ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) must answer before the
operator accepts it.

## Scope

The path of a live item, from the RSS source to a client:

1. The parser reads `<podcast:liveItem>` (`stophammer-parser/src/engine.rs`).
2. The crawler fetches the feed. A `live` or `liveEnd` podping starts a crawl
   at once (ADR 0062 §4).
3. The node stores `pending` and `live` rows in `live_events`, and promotes an
   `ended` item with an enclosure to a track (`src/api.rs`, `src/db.rs`).
4. The node signs `LiveEventsReplaced`, and each community node applies it.
5. ADR 0064 adds two read paths: `live_items` on the feed, and
   `GET /v1/live-items` across feeds.

The attacker is any person who can publish an RSS feed that passes the
verifier chain, or who can send a podping. The index is open, so both are
easy.

## Findings In The Present Code

Each finding comes from a reading of the code. No test reproduces it yet.

### F1. An `ended` live item avoids the track limit

`MAX_TRACKS_PER_INGEST` is 500. The check at step 3b of the ingest handler
counts `tracks` only. The loop that promotes each `ended` live item to a track
has no limit. Only `MAX_BODY_BYTES`, 2 MiB, limits the count. A feed can thus
add some thousands of tracks in one ingest.

### F2. A feed can give any number of live rows

No limit applies to `pending` and `live` rows. With `GET /v1/live-items`, one
feed can fill the list with rows that never end. ADR 0064 §3 removes the age
limit, so these rows stay.

### F3. An unchanged snapshot can emit a signed event at each crawl

`live_events_changed` in `src/db.rs` compares the two lists position by
position. `get_live_events_for_feed` gives the stored rows in the order of
`COALESCE(scheduled_start, created_at), live_item_guid`. The new rows keep the
RSS order. When a feed gives two or more live items in a different order, each
crawl emits a new `LiveEventsReplaced`. Each community node receives and
stores it. A `live` podping starts a crawl at once, so a person can repeat
this at the rate of the host throttle.

### F4. `content_link` has no web URL check

ADR 0054 §4 makes the ingest warn about a URL field that is not `http` or
`https`, and the read routes hide it with `model::web_url_or_none`.
`non_web_url_warnings` does not check `content_link`. No read route gives it
today. The new read paths would give a `javascript:` or `data:` value to a
client.

### F5. A `live` podping has no minimum interval

A `live` or `liveEnd` podping for a URL starts a crawl at once, at each
podping. The host throttle limits the fetch rate for each host. No limit
applies to one URL. The podping archive gave 82 such messages in the 7 days
before 2026-09-26. Thus a limit of one crawl each 60 seconds for each URL
stops no real notification.

### Production Evidence For F3 And R2

The backup `stophammer-20260926T131633Z.db` of 2026-09-26 at 13:16 UTC gives
these facts:

- `live_events` holds 10 rows: 2 `live` and 8 `pending`. They come from two
  feeds on `feeds.podcastindex.org`: `100retro.xml` and `100retro_test.xml`.
  Their titles say "Live 24/7".
- Each row has a `start` and an `end`, from 1 to 3 hours apart. Each `end` is
  in December 2022 or December 2023. A row with `pending` has a `start` that is
  some years in the past.
- The two feeds give a station that runs 24 hours a day as a schedule of
  shows. Each show is one `liveItem` with its own `start` and `end`.
- Each feed has 7 `LiveEventsReplaced` events from 2026-04-19 to 2026-09-25.
  The 7 payloads of a feed hold the same rows in the same order. Each payload
  holds 6 rows, and one `live_item_guid` is in it two times. The table key
  `(feed_guid, live_item_guid)` stores 5 rows. The count compare of
  `live_events_changed` thus gives a change at each accepted ingest.

So the cause of F3 in production is a duplicate GUID, not the order. The
correction removes duplicate GUIDs, keeps the first, and sorts by
`live_item_guid` before the compare.

## Risks Of The New Read Paths

### R1. Spam in the list across feeds

The list is a new place to be seen. The cost to be listed is one feed with
`medium` `music` and a value block. Without limits, a small number of feeds can
fill each page with rows that never end.

### R2. Old rows

A feed that stops, or that the crawler cannot fetch, keeps its last live rows.
The node does not record when a crawl last confirmed a row. `updated_at`
changes only when the snapshot changes. A client cannot tell a stream that
runs 24 hours a day from a feed that stopped a year ago.

### R3. Attacker-controlled order

`scheduled_start` and `title` come from the feed. An order on one of them lets
a feed put its rows first, for example with a start date in the future.

### R4. Blocked and retired feeds

A block of ADR 0053 or ADR 0057 removes the feed record, and the delete
trigger removes its `live_events` rows. The new route must still give no row
of a feed that is not public. A test must check it.

## Proposed Answers

| # | Answer | Where |
|---|---|---|
| F1 | Count the promoted `ended` items in `MAX_TRACKS_PER_INGEST` | Node ingest |
| F2, R1 | At most 10 `pending` and `live` rows for each feed. The ingest keeps the first 10 in RSS order and warns | Node ingest |
| F3 | Sort the two lists by `live_item_guid` before the compare | `src/db.rs` |
| F4 | Add `content_link` to `non_web_url_warnings`, and give it through `web_url_or_none` | Node ingest and read |
| F5 | At most one crawl each 60 seconds for each URL from a `live` or `liveEnd` podping | Crawler, amends ADR 0062 §4 |
| R2 | Record `confirmed_at` on each accepted crawl that gives the row, replicated in `LiveEventsReplaced`. The list takes `confirmed_within` | Node, event payload |
| R3 | The list orders by `feed_guid, live_item_guid` for the cursor. A client sorts a page by its own rule | Read route |
| R4 | A test that a deleted or blocked feed gives no row | Read route |

R2 has a cost. `confirmed_at` changes at each crawl, so each crawl of a feed
with a live row emits a signed event. With F5 and the limit of 10 rows, the
rate stays small: 82 live podpings in 7 days. The other choice is a local
column on the primary only, as `last_seen` of ADR 0058. Community nodes then
cannot give it.

## Operator Decisions

On 2026-09-26 the operator decided these items:

- F1, F3, F4 and R4 go into ADR 0064. Each one corrects a defect.
- F2 and R3 go into ADR 0064.
- F5 does not go into ADR 0064. ADR 0062 §4 stays as it is.
- R2 closes with the views `now` and `upcoming`, the filters `ends_after` and
  `starts_after`, and the relay link `podcast:liveValue`. The ingest drops a
  `live` item with no relay link that has no `end` or runs longer than 6
  hours.
- F1 closes because no `ended` item becomes a track. A recording is outside
  the index.
