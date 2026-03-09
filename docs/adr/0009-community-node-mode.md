# ADR 0009: Community Node Mode

## Status
Accepted

## Context
Stophammer's primary node accepts crawler ingest, signs events, and serves sync. Community nodes replicate the dataset for redundancy and client proximity but must not be trusted to ingest or sign data. They need to:

1. Pull new events from the primary on a timer.
2. Verify each event's ed25519 signature before writing to local DB.
3. Register with the Cloudflare tracker so clients can discover them.
4. Serve the same `GET /sync/events` and `GET /health` read API so they are useful to downstream clients and cascading community nodes.
5. Persist a sync cursor so restarts resume where they left off without re-applying the full history.

Design constraints:
- A community node must never sign events — only verify.
- A failed sync poll must not crash the process; errors are logged and the loop continues.
- The tracker registration is best-effort — an unreachable tracker does not prevent the node from syncing or serving data.

## Decision

### Module structure
A new `src/community.rs` module owns the sync logic. `src/main.rs` branches on `NODE_MODE` env var (`primary` or `community`).

### `CommunityConfig`
All community-specific parameters are grouped in `CommunityConfig`:
- `primary_url` — base URL of the primary node
- `tracker_url` — base URL of the Cloudflare tracker (default: `https://stophammer-tracker.workers.dev`)
- `node_address` — this node's public address registered with the tracker
- `poll_interval_secs` — seconds between polls (default: 30)

### Sync loop
`run_community_sync` is spawned as a `tokio::task` alongside the Axum server. It:
1. Fires a `POST {tracker_url}/nodes/register` request on startup (fire-and-forget).
2. Reads `last_seq` from `node_sync_state` table, keyed by this node's pubkey.
3. Polls `GET {primary_url}/sync/events?after_seq={last_seq}&limit=500` every `poll_interval_secs`.
4. For each received event: verifies the ed25519 signature via `signing::verify_event_signature`, then calls `apply_single_event`.
5. `apply_single_event` dispatches on `EventPayload` variant: upsert artist, feed, track/routes/splits, or replace routes. `FeedRetired` and `TrackRemoved` are logged and skipped (not yet implemented).
6. After applying, inserts the event row into the local `events` table (so the community node can serve it) and advances the cursor via `upsert_node_sync_state`.

### Read-only API
`api::build_readonly_router` exposes only `GET /sync/events` and `GET /health`. The ingest and reconcile write-paths are absent by construction, not by runtime guard. Community mode passes a dummy `VerifierChain` (empty crawl token) because the ingest handler is never reachable.

### Cursor identity
The community node's own ed25519 pubkey is used as the `node_pubkey` key in `node_sync_state`. This means the same key file that identifies the node to the tracker also identifies its sync position in the DB — no additional identity concept is needed.

### `db::upsert_artist_if_absent`
A new DB helper uses `INSERT OR IGNORE` rather than a full upsert. This preserves the local `created_at` if the artist was resolved earlier from a different event ordering, avoiding a spurious update to a timestamp that has no meaning on a community node.

### `db::get_node_sync_cursor`
A new read helper returns `last_seq` for the node's pubkey, or 0 if no cursor exists. Used at startup to resume without re-applying history.

### `SyncEventsResponse` deserialization
`Deserialize` was added to `SyncEventsResponse` in `sync.rs` so `reqwest` can parse the primary's response body. The type was previously serialize-only because only the primary needed to produce it; community nodes are the first consumers.

## Consequences
- A community node can be bootstrapped against any primary URL and will self-heal across network interruptions.
- Event ordering on the community node may differ from the primary (different `seq` values) because `seq` is assigned by the local DB at insert time. This is consistent with ADR 0004 — `seq` is a delivery-ordering field excluded from the signature.
- `FeedRetired` and `TrackRemoved` events are silently skipped. A future ADR should implement soft-delete semantics before these event types are used in production.
- The read-only router eliminates the ingest surface area on community nodes without any runtime flag checks — the routes simply do not exist.
- A community node holds its own signing key (loaded from `KEY_PATH`) even though it never signs. This simplifies `AppState` reuse; the key is present but dormant.
