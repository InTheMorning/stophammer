# ADR 0021: Live Event Support

## Status
Accepted (parser/storage replication implemented; scheduler and SSE follow-up remain)

## Context

The Podcast Namespace defines `<podcast:liveItem>` — a feed element structurally
identical to `<item>` but with an additional `status` attribute that cycles through
`pending → live → ended`. It is used by podcasters and music artists to announce
scheduled streams, signal when they go live, and then record the event.

Stophammer no longer ignores `<podcast:liveItem>` entirely. The parser extracts it,
the ingest pipeline stores `pending` and `live` entries in `live_events`, and an
`ended` live item with an enclosure is promoted into the normal `tracks` table.

The current implementation is intentionally simpler than the original proposal:

- replication uses per-feed snapshot events (`LiveEventsReplaced`) instead of
  per-transition `LiveEventStarted` / `LiveEventEnded` events
- feed-level `podcast:remoteItem` references are also staged in
  `feed_remote_items_raw` via `FeedRemoteItemsReplaced`
- aggressive scheduler polling and SSE fanout for live events are still follow-up work

This creates a specific gap: the two stages of a live event have fundamentally
different time constraints.

### Stage 1: Live (time-critical)

A user who follows an artist needs to know the stream started **within seconds**, not
minutes. Polling RSS every few minutes is insufficient. This is the hardest part of the
problem — it requires near-real-time detection and delivery.

Detection paths available:

- **Podping**: Live transitions are typically announced via Podping. The podping
  listener (ADR 0012) already receives these pings and triggers a crawl. A crawl
  triggered by a podping event is the primary detection mechanism.
- **Scheduled polling**: For feeds that do not use Podping, a short-interval polling
  schedule (60–120 s) for feeds with a known `pending` liveItem is acceptable. This is
  a narrow case — only feeds already known to have a pending live event need aggressive
  polling. All other feeds use the normal crawl schedule.
- **No push from the RSS host**: `<podcast:liveItem>` has no built-in push mechanism.
  Stophammer cannot be told directly by the RSS host that a transition happened.

### Stage 2: Recording (not time-critical)

When `status` transitions to `ended`, the live stream becomes a recording. This is
indexed like any other track — the enclosure URL points to the recording file. Standard
`TrackUpserted` event handling covers this case once the parser understands
`<podcast:liveItem>`.

### Ephemeral vs. persistent storage

Live events in `pending` or `live` state are ephemeral — they may never result in a
recording. Storing them in the main track table (which is replicated to community nodes
via the event log) is premature. A live item that never transitions to `ended` should
not persist in the index.

When a live item transitions to `ended` and gains a valid enclosure URL, it becomes a
permanent track — it should be indexed via `TrackUpserted` exactly like any other item.

## Decision

### Parser changes

`stophammer-parser` gains `<podcast:liveItem>` extraction alongside `<item>`. The
extracted fields mirror those of a regular item:

- `status`: `pending | live | ended` (required)
- `start`: ISO 8601 datetime of scheduled start (optional)
- `end`: ISO 8601 datetime of scheduled end (optional)
- `content_link`: URL of the live stream (optional; absent on `ended` items)
- All standard item fields: `guid`, `title`, `enclosure`, `duration`, payment routes,
  value time splits.

The parsed output is a `LiveItemData` struct alongside `IngestTrackData` in the
`IngestFeedData` payload. The ingest endpoint receives both.

### Event types

The current implementation adds one snapshot event type for live state and stores the
current feed-scoped view in the community node:

| Event type | When emitted |
|------------|-------------|
| `LiveEventsReplaced` | The set of `pending` / `live` `<podcast:liveItem>` rows for a feed changed |

When a `podcast:liveItem` transitions to `ended` and has a valid enclosure, the
recording is indexed as a normal `TrackUpserted` event in the same ingest pass. The
ended item is omitted from the replacement snapshot, which removes it from
`live_events` on replicas.

### Storage: ephemeral `live_events` table

A `live_events` table stores the current state of in-progress live items:

```sql
CREATE TABLE IF NOT EXISTS live_events (
    live_item_guid  TEXT PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    title           TEXT NOT NULL,
    content_link    TEXT,
    status          TEXT NOT NULL CHECK(status IN ('pending', 'live', 'ended')),
    scheduled_start INTEGER,
    scheduled_end   INTEGER,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);
```

Rows are replaced on each crawl with the current set of `pending` or `live` items for
that feed. When a live item transitions to `ended`, it is omitted from the replacement
set and therefore deleted from `live_events` (the recording is promoted to a permanent
track via `TrackUpserted`).

### Current ingest behavior

On each ingest of a feed containing `<podcast:liveItem>` elements, the pipeline:

- normalizes `status` to lowercase during parsing
- stores `pending` and `live` items in `live_events`
- promotes `ended` items with an enclosure into normal `tracks`
- emits `LiveEventsReplaced` when the feed's live-event snapshot changes

### Aggressive polling for pending live items

When a feed has one or more rows in `live_events` with `status = 'pending'` and
`scheduled_start` within the next 2 hours, the crawl scheduler reduces the poll
interval for that feed to 60 seconds. This interval returns to normal once the item
transitions out of `pending`.

This is a narrow carve-out — it applies only to feeds known to have an imminent live
event, not to all feeds.

### SSE delivery (ADR 0020)

Not implemented yet for live-event-specific payloads. The current work focuses on
parsing, storage, and replication so primary and community nodes share the same live
snapshot state.

### What is not in scope

- **Live stream proxying or relay**: stophammer indexes the live event; it does not
  proxy the stream. `content_link` points to the stream at the original host.
- **Chat or interaction**: out of scope.
- **Live items that never end**: cleaned up after 7 days. No permanent record.
- **Payment streaming during live events**: `<podcast:value>` inside a `liveItem` is
  parsed and stored in the same way as track-level routes. No new payment logic needed.

## Alternatives considered

**Treat live items as regular tracks immediately on `pending`**
Pollutes the track table with items that may never conclude. Community nodes would
replicate speculative data. Rejected.

**Only index live items after `ended`**
Misses the Stage 1 requirement entirely — clients would never receive real-time
notification that a stream started. Rejected.

**Separate live event service**
Adds operational complexity. The detection and fanout logic belongs in the primary
node where the gossip event stream already flows. Rejected.

## Consequences

- `stophammer-parser` gains `<podcast:liveItem>` extraction. The `IngestFeedData`
  struct gains a `live_items: Vec<LiveItemData>` field.
- `stophammer-parser` now extracts `<podcast:liveItem>` and feed-level
  `<podcast:remoteItem>`.
- `event.rs` adds snapshot events for `LiveEventsReplaced` and
  `FeedRemoteItemsReplaced`.
- `live_events` and `feed_remote_items_raw` are added to `schema.sql`.
- `apply.rs` and the ingest pipeline now replace those feed-scoped snapshots on each
  successful ingest.
- Live items that transition to `ended` with a valid enclosure are indexed as permanent
  tracks — no change to the existing track indexing path.
- Fast-poll scheduling and live-specific SSE delivery remain follow-up work.
