# Gossip Archive Catch-Up Plan

This document translates the current gossip-listener redesign into a concrete
implementation plan for `stophammer-crawler`.

The target model is:

- `gossip-listener`'s local `archive.db` is the durable podping backlog
- SSE is only the low-latency live tail
- `gossip_state.db` stores both replay progress and per-feed attempt memory
- gossip metadata alone is never trusted to decide feed relevance

## Prerequisite: Legacy Podping Mode Removal

The legacy `podping` crawler mode (WebSocket listener via Livewire/Hive) is
removed in favor of `podping.alpha/gossip-listener`. The gossip mode is the sole
real-time discovery path going forward.

Removal scope:

- Delete `stophammer-crawler/src/modes/podping.rs`
- Remove the `Podping` CLI variant and dispatch from `main.rs`
- Remove the `tokio-tungstenite` dependency from `Cargo.toml`
- Remove `PODPING_WS_URL` and `HIVE_API_URL` environment variables
- Remove all podping references from `README.md`
- Remove `podping_state.db` from `.gitignore`
- Mark [ADR 0012](/home/citizen/build/stophammer/docs/adr/0012-podping-listener.md)
  as superseded by gossip-listener

Shared code that stays: `dedup.rs` (used by gossip mode), `futures-util` and
`chrono` dependencies (used by gossip and import modes respectively).

## Why

The current gossip path is not equivalent to import mode:

- `gossip_state.db` currently stores only `gossip_progress.last_seen_timestamp`
- it does not persist per-feed attempt memory
- it does not store `fetch_http_status`
- it does not store `raw_medium`
- it cannot skip already-proven irrelevant feeds on future notifications

This is not sufficient for a long-running crawler that must survive restarts
and avoid repeatedly fetching feeds already proven non-music or non-publisher.

There is also a correctness gap in the current replay model:

- archive replay is based on `messages.created_at`
- crawler progress is based on podping payload `timestamp`
- replay finishes before SSE starts, which leaves a replay-to-live handoff gap

The redesign must make archive replay authoritative and align gossip-state
durability with the importer's rigor.

## Invariants

- Feed fetch and parse results remain the source of truth for relevance.
- Gossip metadata is only a crawl trigger, not a relevance oracle.
- `archive.db` is the authoritative backlog in archive-backed gossip mode.
- SSE is a fast-path, not the durability layer.
- `gossip_state.db` remains the crawler-owned state file.
- Gossip feed memory is latest-known-state, not append-only history.
- Feed memory is keyed by announced `feed_url`.
- Known irrelevant feeds are skipped by default in gossip mode after they have
  been proven irrelevant by a real fetch/parse result.
- Prior successful feeds are not skipped by default in gossip mode.
- Archive-backed gossip mode fails closed if correct catch-up is unavailable.

## Code Touchpoints

Primary implementation files:

- [stophammer-crawler/src/modes/gossip.rs](/home/citizen/build/stophammer/stophammer-crawler/src/modes/gossip.rs)
- [stophammer-crawler/src/crawl.rs](/home/citizen/build/stophammer/stophammer-crawler/src/crawl.rs)
- [stophammer-crawler/src/main.rs](/home/citizen/build/stophammer/stophammer-crawler/src/main.rs)
- [stophammer-crawler/src/dedup.rs](/home/citizen/build/stophammer/stophammer-crawler/src/dedup.rs)
- [stophammer-crawler/README.md](/home/citizen/build/stophammer/stophammer-crawler/README.md)

External dependency contract to validate against:

- [podping.alpha/gossip-listener/src/archive.rs](/home/citizen/build/podping.alpha/gossip-listener/src/archive.rs)
- [podping.alpha/gossip-listener/src/sse.rs](/home/citizen/build/podping.alpha/gossip-listener/src/sse.rs)
- [podping.alpha/gossip-listener/README.md](/home/citizen/build/podping.alpha/gossip-listener/README.md)

Architecture follow-up:

- [docs/adr/0012-podping-listener.md](/home/citizen/build/stophammer/docs/adr/0012-podping-listener.md)

## Phase Plan

### Phase 1: Correct Archive Cursor and State Schema

Scope:

- Replace the current timestamp-only progress model with an archive cursor
  stored in `gossip_state.db`.
- Add archive cursor keys for:
  - `archive_cursor_created_at`
  - `archive_cursor_hash`
- Add a new gossip feed memory table keyed by `feed_url`.

Implementation notes:

- Archive replay must query:
  - `SELECT hash, payload, created_at FROM messages`
  - `WHERE created_at > ? OR (created_at = ? AND hash > ?)`
  - `ORDER BY created_at ASC, hash ASC`
- The crawler must validate that `archive.db` exposes the expected
  `messages(hash, payload, created_at)` schema before starting replay, using
  `PRAGMA table_info(messages)` to confirm the required columns exist and
  verifying at least one row is present. Query failures must surface as explicit
  schema-mismatch errors rather than silently returning empty results.
- If a legacy `last_seen_timestamp` key exists and no archive cursor is present,
  auto-migrate on first startup: query `archive.db` for the first row with
  `created_at >= last_seen_timestamp`, use that row's `(created_at, hash)` as
  the initial archive cursor, delete the legacy key, and log the migration.
  Worst case this replays a few already-seen notifications, which feed memory
  and in-memory dedup handle gracefully.
- Feed memory should persist at minimum:
  - `feed_url`
  - `fetch_http_status`
  - `fetch_outcome`
  - `outcome_reason`
  - `raw_medium`
  - `parsed_feed_guid`
  - `attempt_duration_ms`
  - `first_attempted_at`
  - `last_attempted_at`
  - `attempt_count`

Acceptance:

- Unit tests cover stable replay ordering for equal `created_at` values.
- Resume after a partial replay continues from the correct
  `(created_at, hash)` pair.
- `gossip_state.db` migrations create the new tables without breaking an
  existing timestamp-only DB.

### Phase 2: Unified Notification Pipeline and Feed Memory Skip

Scope:

- Route both archive replay and live SSE notifications through one shared
  notification handler.
- Populate gossip feed memory after every crawl result.
- Add `--skip-known-non-music` flag to skip known-irrelevant feeds, matching
  the importer's existing pattern.
- Keep existing `reason != newValueBlock` filtering behavior.

Implementation notes:

- No durable notification-level dedupe table. The existing in-memory `Dedup`
  cooldown (5min normal, 30min spam) covers the hot path where SSE and archive
  deliver the same URL within minutes. For cold restarts, feed memory handles
  the skip decisions. Re-crawling a feed that was already crawled is cheap — the
  ingest server returns `no_change` on content-hash match. A canonical
  notification hash would also be fragile to serialization drift in
  gossip-listener.
- The shared handler should:
  1. filter by `reason != newValueBlock`
  2. iterate the announced URLs
  3. check in-memory cooldown dedup
  4. consult gossip feed memory (skip if known irrelevant and
     `--skip-known-non-music` is set)
  5. crawl the feed
  6. update gossip feed memory with the crawl result
- A feed is eligible for skip when `--skip-known-non-music` is set and prior
  memory shows `fetch_http_status = 200` and either `raw_medium` is neither
  `music` nor `publisher`, or a prior medium-gate rejection including absent
  `podcast:medium`. Skip outcomes are recorded as `skipped_known_irrelevant`.
- Optional skip TTL via `--skip-ttl-days <n>` (default: off). When set,
  known-irrelevant skip decisions expire after N days: if `last_attempted_at` is
  older than the TTL, the feed is re-evaluated on next notification regardless
  of prior outcome. Feeds evolve and a podcast-only feed may later add
  `podcast:medium = music`. Recommended production value: 30.
- Do not default-skip prior successes, `404`, `429`, transport failures, parse
  failures, or prior successful ingests.
- Listener-added SSE fields such as `sig_status` and `sender_name` should be
  ignored by crawler processing logic.

Acceptance:

- Archive replay and live SSE produce the same URL-processing behavior.
- Existing gossip filtering semantics remain unchanged.
- A previously fetched non-music feed is not re-fetched on a future podping
  notification when `--skip-known-non-music` is set.
- When `--skip-ttl-days` is set, an expired skip decision is re-evaluated on
  next notification. When unset, skip decisions persist indefinitely.
- Prior successful feeds still re-fetch on future podping notifications.

### Phase 3: Archive-Backed Catch-Up and Live Handoff

Scope:

- Make archive-backed mode the correct production path.
- Start live SSE only after replaying up to a captured archive high-water mark.
- Add periodic archive reconciliation after SSE starts so missed live events are
  recovered from the archive.
- Fail closed when archive-backed correctness is unavailable.

Implementation notes:

- Startup sequence in archive-backed mode:
  1. open and validate `archive.db`
  2. capture current archive high-water mark
  3. replay from stored archive cursor to that high-water mark
  4. start live SSE
  5. begin periodic archive reconciliation
- Replay backpressure: process archive replay in batches of ~500 notifications.
  Between batches, wait for in-flight crawl tasks to drain below the concurrency
  limit. Update the archive cursor after each batch — this bounds memory usage,
  gives the ingest server breathing room, and improves resume granularity on
  interrupted replays.
- Advance the durable archive cursor only from archive-backed processing, not
  from SSE-only observations.
- Reconciliation timing after SSE starts:
  - first reconciliation 10 seconds after SSE connects (closes the
    replay-to-live gap quickly)
  - steady-state interval: 60 seconds
  - backoff on empty reconciliation: double the interval up to 5 minutes when
    no new archive rows are found, reset to 60 seconds when rows appear
- Initial bootstrap rules:
  - if archive cursor exists, use it
  - else if legacy `last_seen_timestamp` exists, auto-migrate (see Phase 1)
  - else if `--since-hours` is provided, seed from `now - since_hours`
  - else bootstrap from the oldest available archive row

Acceptance:

- Archive-backed mode survives SSE disconnects without losing notifications.
- Stored archive cursor older than the oldest retained archive row produces a
  clear archive-gap error.
- Live-only SSE mode still works without `--archive-db`, but is explicitly
  best-effort and not restart-safe.

### Phase 4: Operator Docs and ADR

Scope:

- Update operator docs and architecture notes.
- Record the architecture shift in a new ADR instead of silently rewriting
  [ADR 0012](/home/citizen/build/stophammer/docs/adr/0012-podping-listener.md).

Implementation notes:

- Skip behavior is already implemented in Phase 2. This phase documents it.

Acceptance:

- README explains:
  - archive-backed mode versus live-only mode
  - what gossip feed memory stores and the optional `--skip-ttl-days` expiry
  - why known irrelevant feeds are skipped by default
  - replay backpressure and reconciliation behavior
- New ADR records the shift from stateless SSE-only to archive-backed gossip
  with durable feed memory.

## Recommended Delivery Shape

Split the work into small reviewable PRs:

1. Gossip state schema, archive cursor correctness, and legacy auto-migration.
2. Unified notification handler, feed memory population, and skip logic.
3. Replay backpressure, live handoff, and archive reconciliation.
4. Operator docs and ADR.

This keeps the highest-risk correctness work isolated from operator policy and
documentation changes.

## Explicit Non-Goals

- No trust in podping `medium` metadata as a final relevance decision.
- No append-only gossip crawl history in the first pass.
- No requirement for `gossip-listener` to expose replay over SSE.
- No coupling to `gossip-listener` internals beyond the observed archive schema
  and current SSE payload format.
- No default skip policy for prior successful gossip crawls.
