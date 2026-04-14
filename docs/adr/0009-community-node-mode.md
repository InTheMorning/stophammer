# ADR 0009: Community Node Mode

## Status
Accepted; sequence-signing consequence superseded by [ADR 0036](0036-sign-event-sequence-numbers.md)

Historical note: The authenticated sync-read requirement described by current
runtime behavior was decided later in ADR 0027. References below to open
`GET /sync/events` / `GET /sync/peers` reflect the original decision context
for community-node mode.

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
- `tracker_url` — base URL of the Cloudflare tracker (default: `https://stophammer-tracker.workers.dev`). Optional — set to an empty string or a local URL to remove the external dependency. Tracker registration is best-effort and not required for sync
- `node_address` — this node's public address registered with the tracker
- `poll_interval_secs` — seconds between polls (default: 30)

### Sync loop
`run_community_sync` is spawned as a `tokio::task` alongside the Axum server. It:
1. Fires a `POST {tracker_url}/nodes/register` request on startup (fire-and-forget).
2. Reads `last_seq` from `node_sync_state` table, keyed by this node's pubkey.
3. Polls `GET {primary_url}/sync/events?after_seq={last_seq}&limit=500` every `poll_interval_secs`.
4. For each received event: verifies the ed25519 signature via `signing::verify_event_signature`, then calls `apply_single_event`.
5. `apply_single_event` opens a single transaction and inserts the event row via `INSERT OR IGNORE` as the **first** operation (dedup guard). If the event already exists, the transaction commits (no-op) and returns `ApplyOutcome::Duplicate` immediately -- no entity mutations are executed. This dedup-first invariant guarantees that a duplicate event can never produce partial side-effects.
6. For new events, `apply_single_event` re-derives the `EventPayload` from the signed `payload_json` bytes (closing a MITM vector where the deserialized struct could differ from the signed content), then dispatches on the payload variant: upsert artist, feed, track/routes/splits, replace routes, feed retire (cascade delete), track remove (cascade delete), or artist merge.
7. After all mutations succeed, advances the cursor via `upsert_node_sync_state` and commits the transaction.

### Read-only API
`api::build_readonly_router` exposes the sync endpoints (`GET /sync/events`,
`GET /sync/peers`), the current `v1` query API (`/v1/search`,
`/v1/feeds/{guid}`, `/v1/tracks/{guid}`, `/v1/wallets/{id}`, publisher reads,
and related read routes), `GET /node/info`, and `GET /health`. The
ingest, reconcile, and admin write-paths are absent by construction, not by runtime
guard. Community mode passes a dummy `VerifierChain` (empty crawl token) because the
ingest handler is never reachable.

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
- Current implementations verify the primary-signed `seq` before advancing the
  sync cursor. See [ADR 0036](0036-sign-event-sequence-numbers.md).
- Duplicate events (received via both push and fallback poll, or replayed during recovery) are detected at the top of the transaction via the `INSERT OR IGNORE` dedup guard and returned as `ApplyOutcome::Duplicate`. No entity mutations are attempted for duplicates, eliminating a class of partial-write bugs where the event row insert could succeed but entity mutations had already been applied.
- `FeedRetired` and `TrackRemoved` events are fully implemented with cascade hard-deletes. `FeedRetired` removes the feed, all child tracks, payment routes, and search index entries. `TrackRemoved` removes the track, its child rows, and search index entry. Both use `INSERT OR IGNORE` / idempotent semantics consistent with the rest of `apply_single_event`.
- The read-only router eliminates the ingest surface area on community nodes without any runtime flag checks — the routes simply do not exist.
- A community node holds its own signing key (loaded from `KEY_PATH`) even though it never signs. This simplifies `AppState` reuse; the key is present but dormant.
