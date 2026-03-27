# Gossip Archive Catch-Up Plan

This document translates the current gossip-listener redesign into a concrete
implementation plan for `stophammer-crawler`.

The target model is:

- `gossip-listener`'s local `archive.db` is the durable podping backlog
- SSE is only the low-latency live tail
- `gossip_state.db` stores both replay progress and per-feed attempt memory
- gossip metadata alone is never trusted to decide feed relevance

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
- Add a new processed-notification table keyed by a crawler-owned canonical
  notification hash.

Implementation notes:

- Archive replay must query:
  - `SELECT hash, payload, created_at FROM messages`
  - `WHERE created_at > ? OR (created_at = ? AND hash > ?)`
  - `ORDER BY created_at ASC, hash ASC`
- The crawler must validate that `archive.db` exposes the expected
  `messages(hash, payload, created_at)` schema before starting replay.
- The old `last_seen_timestamp` key should not remain the authoritative replay
  cursor in archive-backed mode.
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

### Phase 2: Unified Notification Pipeline

Scope:

- Route both archive replay and live SSE notifications through one shared
  notification handler.
- Add durable notification-level dedupe so the same podping is not processed
  twice if it arrives from both SSE and archive reconciliation.
- Keep existing `reason != newValueBlock` filtering behavior.

Implementation notes:

- Canonical notification identity should hash:
  - `version`
  - `sender`
  - `timestamp`
  - `medium`
  - `reason`
  - ordered `iris`
- The shared handler should:
  1. canonicalize the notification
  2. check durable notification dedupe
  3. iterate the announced URLs
  4. consult gossip feed memory before crawling
  5. update gossip feed memory after each crawl completes
- Listener-added SSE fields such as `sig_status` and `sender_name` should be
  ignored by crawler identity and replay logic.

Acceptance:

- A notification first seen over SSE and later seen in archive replay is only
  crawled once.
- Archive replay and live SSE produce the same URL-processing behavior.
- Existing gossip filtering semantics remain unchanged.

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
  5. keep polling archive for anything beyond the cursor
- Advance the durable archive cursor only from archive-backed processing, not
  from SSE-only observations.
- Initial bootstrap rules:
  - if archive cursor exists, use it
  - else if `--since-hours` is provided, seed from `now - since_hours`
  - else bootstrap from the oldest available archive row
- If only legacy `last_seen_timestamp` exists, archive-backed mode should fail
  with an actionable migration error instead of guessing.

Acceptance:

- Archive-backed mode survives SSE disconnects without losing notifications.
- Stored archive cursor older than the oldest retained archive row produces a
  clear archive-gap error.
- Live-only SSE mode still works without `--archive-db`, but is explicitly
  best-effort and not restart-safe.

### Phase 4: Gossip Feed Memory Policy and Operator Docs

Scope:

- Add importer-like skip behavior for known irrelevant feeds in gossip mode.
- Persist skip outcomes in gossip feed memory.
- Update operator docs and architecture notes.
- Record the architecture shift in a new ADR instead of silently rewriting
  [ADR 0012](/home/citizen/build/stophammer/docs/adr/0012-podping-listener.md).

Implementation notes:

- A feed is eligible for default skip only when prior memory shows:
  - `fetch_http_status = 200`
  - and either:
    - `raw_medium` exists and is neither `music` nor `publisher`
    - or a prior medium-gate rejection including absent `podcast:medium`
- Skip outcomes should be recorded explicitly as
  `skipped_known_irrelevant`.
- Do not default-skip prior successes in gossip mode.
- Do not default-skip `404`, `429`, transport failures, parse failures, or
  prior successful ingests.

Acceptance:

- A previously fetched non-music or non-publisher feed is not re-fetched on a
  future podping notification.
- A previously medium-absent rejection is not re-fetched on a future podping
  notification.
- Prior successful feeds still re-fetch on future podping notifications.
- README explains:
  - archive-backed mode versus live-only mode
  - what gossip feed memory stores
  - why known irrelevant feeds are skipped by default

## Recommended Delivery Shape

Split the work into small reviewable PRs:

1. Gossip state schema and archive cursor correctness.
2. Unified notification handling plus durable notification dedupe.
3. Replay/live handoff and archive reconciliation.
4. Known-irrelevant skip behavior, docs, and ADR.

This keeps the highest-risk correctness work isolated from operator policy and
documentation changes.

## Explicit Non-Goals

- No trust in podping `medium` metadata as a final relevance decision.
- No append-only gossip crawl history in the first pass.
- No requirement for `gossip-listener` to expose replay over SSE.
- No coupling to `gossip-listener` internals beyond the observed archive schema
  and current SSE payload format.
- No default skip policy for prior successful gossip crawls.
