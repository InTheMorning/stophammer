# ADR 0053 Phase Plan: A Correction Stays Applied

Owner: [ADR 0053](../adr/0053-a-correction-stays-applied.md). This plan
states no rule. Phase 4 of the
[remediation plan](feed-trust-remediation-plan.md) points here.

## Goal

A removed or blocked feed stays out of the index on each node. An older copy
of a feed does not replace a newer copy. A change of payment recipients is
visible.

## Non-Goals

- A hold or a delay of a payment change. ADR 0053 rejects it for now.
- The check of redirect hops. ADR 0052 adds the hops.
- A block for one track. ADR 0053 blocks a GUID or a URL of a feed.
- A change to the read API for a blocked feed. A block stops ingest only.

## Assumptions

- ADR 0051 is deployed first. The block check comes after the token check of
  ADR 0051 section 4.
- A community node on the current version cannot parse an event type that it
  does not know. Each community node gets the new version before the primary
  emits `FeedBlocked`.
- The signed events are never removed, so a route history from the events is
  complete.
- `feeds.last_build_date` holds Unix seconds (migration 0034).

## Affected Modules

| Task | Crate | Files |
|---|---|---|
| 001 | `stophammer` | `migrations/0038_feed_blocks.sql`, `src/schema.sql`, `src/db.rs`, `src/event.rs`, `src/apply.rs` |
| 002 | `stophammer` | `src/api.rs` (ingest check, admin routes, router), `src/openapi.rs`, `docs/API.md` |
| 003 | `stophammer` | `src/api.rs` (`handle_retire_feed`), `src/db.rs` (`delete_feed_with_event`) |
| 004 | `stophammer` | `src/blocks.rs` (new), `src/lib.rs`, `src/main.rs`, `src/verify.rs`, `src/verifiers/`, `AGENTS.md` module table, `docs/operations.md`, `docs/verifier-guide.md` |
| 005 | `stophammer` | `src/api.rs` (ingest write phase) |
| 006 | `stophammer` | `src/query.rs`, `src/api.rs` (ingest log), `src/openapi.rs`, `docs/API.md` |
| 007 | `stophammer-crawler` | `src/crawl.rs` |
| 008 | VPS | Operator deploy |

## Decisions For The Tasks

1. **The table.** `feed_blocks (block_id TEXT PRIMARY KEY, kind TEXT NOT NULL
   CHECK (kind IN ('guid','url')), value TEXT NOT NULL, reason TEXT NOT NULL,
   blocked_at INTEGER NOT NULL, UNIQUE (kind, value)) STRICT`. Migration
   `0038_feed_blocks.sql` is the next array position. `src/schema.sql` gets the
   same table.
2. **The kind.** `db::FeedBlockKind { Guid, Url }` with
   `serde(rename_all = "snake_case")`. `FeedBlockKind::normalize(&self, value)`
   gives lower case and trimmed for `Guid`, and trimmed only for `Url`. Each
   write and each check uses it.
3. **The events.** `EventType::FeedBlocked` with `FeedBlockedPayload
   { block_id, kind, value, reason, blocked_at }`, and `EventType::FeedUnblocked`
   with `FeedUnblockedPayload { block_id }`. The subject GUID is the
   `block_id`. Apply of `FeedBlocked` is `INSERT OR IGNORE`. Apply of
   `FeedUnblocked` deletes the row by `block_id`, and does nothing when no row
   exists.
4. **The database functions.** In `src/db.rs`:
   `insert_feed_block(conn, &FeedBlock) -> Result<bool>` (false when the pair
   exists), `delete_feed_block(conn, block_id) -> Result<bool>`,
   `list_feed_blocks(conn) -> Result<Vec<FeedBlock>>`,
   `get_feed_block_by_pair(conn, kind, value) -> Result<Option<FeedBlock>>`,
   and `find_feed_block(conn, feed_guid: Option<&str>, urls: &[&str]) ->
   Result<Option<FeedBlock>>`. The primary signs an event with the existing
   `db::insert_event` in the same transaction as the row.
5. **The ingest check.** After `authenticate` and before the verifier chain,
   on the reader connection. The GUID of `feed_data`, when present, and both
   URLs. A match answers `accepted: false`, `reason: "blocked"`, no event.
6. **The admin routes.** `POST /v1/blocks` answers `201` with the row, or
   `409` with the existing `block_id`. `GET /v1/blocks` answers `200` with the
   rows. `DELETE /v1/blocks/{block_id}` answers `204`, or `404`. Each needs
   `X-Admin-Token`. Each write fans out its event as the other write routes
   do.
7. **Retirement.** `delete_feed_with_event` gains a parameter for the blocks
   to write. It inserts each new pair and signs one `FeedBlocked` event for
   each, in its one transaction. A pair that exists writes nothing. The
   handler reads `?block=` with `Query`, default `true`. `block=false` without
   `X-Admin-Token` answers `403`.
8. **The seed.** A new module `src/blocks.rs` reads `BLOCKED_FEED_GUIDS` and
   `BLOCKED_FEED_URLS` with the parser of `feed_blocklist.rs`. On a primary
   only, at startup, it inserts each missing pair with a signed `FeedBlocked`
   event and the reason `seeded from environment`. `src/verifiers/feed_blocklist.rs`
   is deleted. `build_chain` skips the name `feed_blocklist` with a
   `tracing::warn!` that names ADR 0053 section 2. `ChainSpec::DEFAULT` drops
   it.
9. **The stale rule.** In the ingest write phase, after the classification of
   ADR 0051 gives `Update`, and before step 3b. Stored and submitted
   `last_build_date` both present and submitted smaller: `accepted: false`,
   `reason: "stale_submission"`, no write. `force_reingest` does not change
   this.
10. **The route history.** `GET /v1/feeds/{guid}/route-history` in
    `src/query.rs`, in the `QueryResponse` envelope of the other query
    routes.
    - It reads the events of types `feed_routes_replaced`, `routes_replaced`
      and `track_upserted` whose payload names the feed, in `seq` order. It
      parses each payload with `EventPayload`.
    - A recipient set is the ordered list of `(address, split)`.
    - An entry is written only when the set of a subject differs from its
      last set.
    - Entry fields: `subject` (`feed` or `track`), `track_guid` (null for the
      feed), `event_id`, `seq`, `changed_at`, `old_recipients` (null for the
      first set) and `new_recipients`.
    - At most 1,000 entries, newest last.
11. **The log.** In the ingest write phase, before the ingest transaction, the
    handler compares two sets for the feed and for each track: the stored
    recipient set and the new set. It logs each difference with `tracing::warn!` and the
    fields `feed_guid`, `track_guid`, `old_recipients` and `new_recipients`.
12. **The crawler.** `NODE_CONFLICT_REASONS` becomes `UNCACHED_NODE_REASONS`
    with five strings: the three of ADR 0051, `blocked` and
    `stale_submission`. `is_node_conflict` becomes `is_uncached_node_answer`.

## Sequence

| Task | Crate | Needs |
|---|---|---|
| [001](../tasks/adr-0053-task-001-block-storage-and-events.md) Block storage and events | `stophammer` | Nothing |
| [002](../tasks/adr-0053-task-002-ingest-check-and-admin-routes.md) Ingest check and admin routes | `stophammer` | 001 |
| [003](../tasks/adr-0053-task-003-retirement-blocks.md) Retirement blocks | `stophammer` | 002 |
| [004](../tasks/adr-0053-task-004-environment-seed.md) Environment seed | `stophammer` | 003 |
| [005](../tasks/adr-0053-task-005-stale-submission.md) Stale submission | `stophammer` | 004 |
| [006](../tasks/adr-0053-task-006-route-history.md) Route history | `stophammer` | 005 |
| [007](../tasks/adr-0053-task-007-crawler-uncached-reasons.md) Crawler reasons | `stophammer-crawler` | Nothing |
| [008](../tasks/adr-0053-task-008-deploy.md) Deploy | VPS | 001 to 007 |

Tasks 002 to 006 change `src/api.rs` or `src/openapi.rs`, so they run in
sequence. Task 007 changes a different repository and can run at any time.

## Schema And API Implications

- One migration and one table. No change to an existing table.
- Two new event types. A community node needs the new version first.
- Four new routes: three admin routes and one public query route. The OpenAPI
  document declares each one (ADR 0044).
- Two new ingest rejection reasons: `blocked` and `stale_submission`.
- `DELETE /v1/feeds/{guid}` writes a block by default.
- `VERIFIER_CHAIN` stops naming `feed_blocklist`. A value that still names it
  starts with a warning.

## Risk Areas

- **The rollout order.** A primary that emits `FeedBlocked` before a
  community node knows the type breaks that node's sync.
- **The seed at the first start.** It emits one event for each value in the
  environment. Check the size of `BLOCKED_FEED_GUIDS` first.
- **The stale rule for a clock that goes back.** The operator clears it with a
  retire and `?block=false`.
- **The route history cost.** A feed with many tracks has many
  `track_upserted` events. The cap of 1,000 entries limits the answer, not the
  read.

## Test Strategy

- Migration and apply tests for task 001, with a replay on a second database.
- Integration tests through the HTTP router for tasks 002, 003, 005 and 006.
- A startup test for task 004 that calls the seed function with fixed values.
- Inline tests in the crawler for task 007.
- The full gate in each crate after each task.

## Rollback

- A rollback to the ADR 0051 binary leaves the `feed_blocks` table and its
  events. That binary ignores the table. The environment blocklist returns.
- A community node on the new version keeps working with an older primary.
- Task 007 alone is safe to roll back.
