# ADR 0021: Live Event Support

## Status
Proposed

## Context

The Podcast Namespace defines `<podcast:liveItem>` — a feed element structurally
identical to `<item>` but with an additional `status` attribute that cycles through
`pending → live → ended`. It is used by podcasters and music artists to announce
scheduled streams, signal when they go live, and then record the event.

Stophammer currently ignores `<podcast:liveItem>` entirely. The parser does not extract
it; the event log has no live event types; the ingest pipeline has no transition
detection.

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

Two new event types are added to `EventType` and `EventPayload`:

| Event type | When emitted |
|------------|-------------|
| `LiveEventStarted` | A `podcast:liveItem` transitions to `status="live"` |
| `LiveEventEnded` | A `podcast:liveItem` transitions to `status="ended"` with a valid enclosure |

`LiveEventStarted` carries: `feed_guid`, `live_item_guid`, `title`, `content_link`,
`start`, `artist_credit`.

`LiveEventEnded` carries: `feed_guid`, `live_item_guid`. The recording itself is
indexed as a separate `TrackUpserted` event emitted in the same ingest pass.

These events replicate to community nodes via the normal gossip push. Community nodes
apply them by updating the ephemeral live event table (see below).

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

Rows are upserted on each crawl when `status` is `pending` or `live`. When `status`
transitions to `ended` the row is deleted (the recording has been promoted to a
permanent track via `TrackUpserted`). Rows older than 7 days with no transition to
`ended` are purged by a background cleanup task — they represent streams that started
but never concluded with a recording.

### Transition detection in the ingest pipeline

On each ingest of a feed containing `<podcast:liveItem>` elements, the ingest pipeline
compares the incoming `status` against the stored row in `live_events`:

| Previous state | Incoming state | Action |
|---------------|---------------|--------|
| absent / `pending` | `live` | Emit `LiveEventStarted`, upsert row |
| `pending` | `pending` | Upsert row (update scheduled times if changed) |
| `live` | `live` | No-op (already started) |
| `pending` or `live` | `ended` + enclosure | Emit `LiveEventEnded`, emit `TrackUpserted`, delete live_events row |
| `live` | absent (item removed) | Delete row, no event (stream abandoned) |

### Aggressive polling for pending live items

When a feed has one or more rows in `live_events` with `status = 'pending'` and
`scheduled_start` within the next 2 hours, the crawl scheduler reduces the poll
interval for that feed to 60 seconds. This interval returns to normal once the item
transitions out of `pending`.

This is a narrow carve-out — it applies only to feeds known to have an imminent live
event, not to all feeds.

### SSE delivery (ADR 0020)

`LiveEventStarted` and `LiveEventEnded` are dispatched to the SSE fanout registry after
apply. Connected clients subscribed to the relevant artist ID receive the event within
the normal gossip propagation window (typically < 5 s end-to-end).

This is the primary real-time delivery path. No separate push mechanism is needed for
clients with an active SSE connection.

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
- Two new event types (`LiveEventStarted`, `LiveEventEnded`) are added to `event.rs`.
- A new `live_events` table is added to `schema.sql`.
- `apply.rs` gains two new match arms.
- The ingest pipeline gains transition detection logic (compare incoming vs. stored
  status).
- The crawl scheduler gains a fast-poll path for feeds with imminent pending live items.
- `LiveEventStarted` and `LiveEventEnded` are the SSE delivery mechanism for live
  events (ADR 0020).
- Live items that transition to `ended` with a valid enclosure are indexed as permanent
  tracks — no change to the existing track indexing path.
