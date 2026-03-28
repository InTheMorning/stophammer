# ADR 0031: Archive-Backed Gossip with Durable Feed Memory

## Status
Accepted

Date: 2026-03-27

Supersedes the SSE-only gossip design described in
[ADR 0012: Podping Listener](0012-podping-listener.md) (already marked
superseded by gossip-listener).

## Context

Gossip mode consumed podping notifications over SSE with only a
`last_seen_timestamp` cursor and no per-feed memory. Restarts lost in-flight
URLs, every re-announcement of a non-music feed caused redundant work, and the
replay-to-live handoff could silently drop notifications. The gossip path needed
the same durability and skip logic that import mode already had.

## Decision

### 1. gossip-listener's archive.db is the durable backlog

SSE is a fast-path notification channel, not a durability layer. When
`--archive-db` is provided, the crawler treats gossip-listener's `archive.db`
as the authoritative backlog. The archive cursor in `gossip_state.db` is a
compound `(created_at, hash)` pair — the hash tiebreaker gives stable ordering
when multiple messages share the same timestamp.

### 2. Batched replay with backpressure

Archive replay processes notifications in batches of ~500. Between batches the
crawler drains all in-flight tasks, then persists the cursor. This bounds
memory, gives the ingest server breathing room, and lets an interrupted replay
resume from the last completed batch.

Before replay begins, the crawler captures the archive high-water mark. Replay
stops at this mark, giving a defined endpoint before SSE takes over.

### 3. Periodic archive reconciliation

After SSE connects, a background task periodically queries the archive for
rows beyond the stored cursor. This closes two gaps:

- Notifications that arrived in the archive during replay but after the
  high-water mark was captured.
- Notifications that SSE missed due to transient disconnects.

Reconciliation timing:

- First reconciliation: 10 seconds after SSE connects (closes the
  replay-to-live gap quickly).
- Steady-state: every 60 seconds.
- Backoff: doubles the interval up to 5 minutes when no new rows are found;
  resets to 60 seconds when rows appear.

The durable archive cursor is advanced only by archive-backed processing
(replay and reconciliation), never by SSE-only observations.

### 4. Fail-closed on archive gap

If the stored cursor's `created_at` is older than the oldest retained archive
row, notifications between those two points are unrecoverable. Rather than
silently proceeding with a gap, the crawler exits with a fatal error and a
clear message directing the operator to reset `gossip_state.db`.

### 5. Durable gossip feed memory

`gossip_state.db` contains a `gossip_feed_memory` table recording the latest
crawl result per feed URL (HTTP status, outcome, medium, attempt count),
mirroring the importer's `import_feed_memory` pattern.

With `--skip-known-non-music`, feeds proven irrelevant (non-music medium at
HTTP 200, or medium-gate rejection) are skipped on future notifications.
`--skip-ttl-days` expires skip decisions so feeds are periodically re-evaluated.

Feed memory is latest-known-state, not append-only history. Prior successful
feeds are never skipped — only known-irrelevant feeds are.

### 6. Live-only mode remains available

Without `--archive-db`, gossip mode operates in SSE-only mode. This is
explicitly marked as best-effort and not restart-safe. The
`last_seen_timestamp` cursor is kept for observability but does not enable
replay.

## Consequences

- **Restart safety**: archive cursor is the authoritative resume point;
  survives restarts and SSE disconnects without losing notifications.
- **Reduced redundant work**: skip logic eliminates ~95% of re-fetches for
  non-music feeds.
- **Operator visibility**: batch progress, reconciliation, and skip counts are
  logged. Feed memory is queryable via SQLite.
- **Archive dependency**: requires gossip-listener to maintain `archive.db`.
  Refuses to start if the archive is unavailable or empty.
- **No notification-level dedup**: in-memory cooldown (5 min / 30 min spam)
  covers the hot path; feed memory covers cold restarts. Re-crawling is cheap
  — the ingest server returns `no_change` on content-hash match.
