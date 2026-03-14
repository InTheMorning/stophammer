# ADR 0020: SSE Push Notifications for Artist Follow

## Status
Accepted (updated: id field semantics, mutation path coverage, community node SSE)

## Context

A client application (mobile or desktop) that lets users follow artists needs a way to
learn about new content without polling. The two things a user wants to know:

1. **New recording published** — a new track or feed update has been indexed.
2. **Live event started** — an artist is streaming right now; the client needs to know
   within seconds.

### Why polling is not the answer

Polling is the status quo — it is what every client already does in the absence of any
server-side feature. It is wasteful:

- Each client independently re-fetches the same RSS feeds on independent schedules.
- For live events, acceptable latency (< 30 s) requires polling every 10–15 s across
  every followed feed. At any meaningful scale this is a denial-of-service pattern
  against RSS hosts.
- Stophammer already receives events from the gossip network in real time. That signal
  exists and should be surfaced rather than discarded.

Polling is not an alternative we consider. It is the floor we are improving on.

### Why WebSocket is the wrong primitive

WebSocket is bidirectional. A client that only needs to receive notifications has no use
for the uplink. SSE (Server-Sent Events, WHATWG EventSource) is:

- Unidirectional (server → client): matches the use case exactly.
- HTTP/1.1 and HTTP/2 compatible: no upgrade negotiation, works through most proxies.
- Auto-reconnect: specified in the EventSource API — clients reconnect on drop with
  `Last-Event-ID` resumption.
- Battery-friendly: the connection is idle the vast majority of the time; no
  per-message round-trip overhead.

### Privacy problem

Surfacing the SSE endpoint naively creates a behavioral profiling risk:

- The query parameter carries the full set of followed artist IDs.
- Any node operator who logs requests learns the user's complete follow list, their IP
  address, and the timing of their listening sessions.
- In a decentralized network where a client queries arbitrary nodes, this profile is
  scattered across operators the user has no relationship with.

This is not a reason to abandon SSE. It is a risk that must be documented, understood,
and handled at the client layer. Stophammer defines the protocol contract and states the
privacy obligation clearly. Enforcement is a client implementation responsibility —
analogous to how HTTP specifies cookie semantics without mandating how browsers enforce
privacy policy.

## Decision

### Endpoint

```
GET /v1/events?artists=<id1>,<id2>,...
```

The server emits newline-delimited SSE frames. Each frame corresponds to one event from
the gossip log whose `subject_guid` matches one of the requested artist IDs, or whose
payload references a feed or track belonging to that artist.

Event types emitted on this stream:

| SSE event name | Trigger |
|----------------|---------|
| `track_upserted` | New track indexed or existing track metadata changed |
| `feed_upserted` | Feed metadata changed (title, image, description, etc.) |
| `live_event_started` | A `podcast:liveItem` transitioned to `status="live"` |
| `live_event_ended` | A `podcast:liveItem` transitioned to `status="ended"` |

`live_event_started` and `live_event_ended` depend on live event support (not yet
specified). They are listed here to establish that the SSE stream is the intended
delivery channel for live event notifications.

### Frame `id:` field

Each SSE frame carries an `id:` field set to the event's `seq` value — the monotonically
increasing integer primary key assigned by the `events` table at commit time. This is the
same `seq` described in ADR 0004 as delivery-ordering metadata excluded from the event
signature.

An earlier implementation used `subject_guid` (the feed/track/artist GUID) as the `id:`
field. This was incorrect: `subject_guid` is not unique across events (multiple events
can share the same subject), so `Last-Event-ID` replay could not distinguish which events
had already been delivered. Using `seq` provides an unambiguous, monotonically increasing
cursor that supports gap-free replay.

### `Last-Event-ID` replay

Clients that reconnect with a `Last-Event-ID` header receive any frames they missed
during the gap, up to a configurable replay window (default: 5 minutes / 1024 frames per
artist).

The server parses `Last-Event-ID` as an integer (`i64`). On reconnect, the server scans
the per-artist ring buffers and replays all frames where `frame.seq > last_seq`. Replayed
frames are sorted by `seq` across all subscribed artists so events arrive in commit order.
If `Last-Event-ID` is absent or not a valid integer, replay starts from the current
position (no backfill).

### Mutation path coverage

`publish_events_to_sse()` must be called from every code path that commits new events to
the database. This includes:

- `POST /ingest/feed` — primary ingest of crawled feeds
- `PATCH /feeds/{guid}` — feed URL relocation
- `PATCH /tracks/{guid}` — track enclosure relocation
- `DELETE /feeds/{guid}` — feed retirement
- `DELETE /feeds/{guid}/tracks/{track_guid}` — track removal
- `POST /admin/artists/merge` — artist merge
- `apply_events()` — community node event application (push and poll paths)

A mutation path that commits events without calling `publish_events_to_sse()` is a bug.
The invariant is: if an event row is inserted into the `events` table, the SSE registry
must be notified in the same request cycle.

### Community node SSE

Community nodes serve `GET /v1/events` on the same read-only router as the primary. The
`SseRegistry` is instantiated at startup and shared with both the SSE endpoint handler
and the event application path. When `apply_events()` processes events received via push
or poll-loop fallback, it publishes each applied event to the registry so that SSE
clients connected to the community node receive updates without depending on the primary.

The `SseRegistry` is passed as `Option<&Arc<SseRegistry>>` to `apply_events()`. On a
community node this is `Some`; the option exists to allow test harnesses to omit the
registry.

### What the node does not do

- The node does not store follow lists. The `?artists=` parameter is held in memory for
  the duration of the SSE connection only and is never written to the database.
- The node does not forward follow lists to peers.
- The node does not authenticate SSE clients. The endpoint is read-only and
  unauthenticated.

### Privacy obligation — client layer

**Stophammer does not enforce how clients use this endpoint. That is a client
implementation responsibility.**

The risk is:

> A client that sends a user's full follow list to a node the user has no relationship
> with leaks that user's listening profile to an unknown operator.

The intended pattern is that a client connects to a node the user has chosen and trusts
— the same node they use for search and discovery. This is a recommendation, not a
protocol requirement enforced by the node.

Client implementers are expected to:

1. Connect SSE subscriptions only to nodes the user has explicitly configured or
   consented to share data with.
2. Disclose in their privacy documentation that followed artist IDs are transmitted to
   the connected node.

A client that sends follow lists to arbitrary nodes discovered via the tracker or peer
list, without user awareness, is acting against the spirit of this design. Stophammer
cannot prevent this — the obligation is on the client, in the same way cookie misuse is
on the application, not the HTTP spec.

## Alternatives considered

**WebSocket**
Bidirectional, heavier than needed. Rejected in favour of SSE.

**Polling**
The status quo. Not an alternative — the reason this ADR exists.

**Push via APNs / FCM**
Battery-optimal for infrequent mobile notifications. Does not replace SSE for live
event delivery where latency < 30 s is required. A future ADR may describe how a node
can relay `LiveEventStarted` to APNs/FCM on behalf of opted-in users. Out of scope
here.

**WebSub (W3C)**
Server-to-server push. Requires the subscriber to expose a public callback URL — not
available on mobile clients. Stophammer nodes already consume WebSub as subscribers
(receiving feed updates from hubs); this ADR is about the node-to-client direction,
which WebSub does not address.

**Node registration / service registry**
Requiring streaming services to register with stophammer in order to make SSE requests
— allowing banning of non-compliant clients. Rejected: makes stophammer a registrar and
app-store for music clients. That is out of scope and operationally unsustainable.

## Consequences

- A new route `GET /v1/events` is added to both the primary and read-only (community)
  routers. Community nodes serve SSE to their own connected clients.
- The node gains an in-memory SSE fanout registry: a map from artist ID to connected
  SSE response senders. This registry is ephemeral — it does not survive restarts.
- Every mutation path that commits events to the database calls
  `publish_events_to_sse()` after the transaction commits. This covers ingest, patch,
  delete, merge, and community-node apply. Missing a call path is a correctness bug that
  causes SSE clients to miss events silently.
- The SSE `id:` field is the event's `seq` (integer primary key from the `events` table),
  not the `subject_guid`. This is consistent with ADR 0004's treatment of `seq` as
  delivery-ordering metadata.
- `Last-Event-ID` is parsed as an integer. The ring buffer replay uses `frame.seq >
  last_seq` to filter, providing gap-free ordered replay across artists.
- `live_event_started` and `live_event_ended` are first-class event types on this
  stream; their full specification is deferred to the live events ADR (ADR 0021).
- APNs/FCM relay is deferred to a future ADR.
- Privacy enforcement is a client obligation. The protocol documents the risk; it does
  not enforce compliance.
