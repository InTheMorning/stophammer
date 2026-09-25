# ADR 0058 Phase Plan: A Copy Of A Feed Is Public

Owner: [ADR 0058](../adr/0058-a-copy-of-a-feed-is-public.md). This plan
states no rule.

## Goal

The node keeps a summary of each mirror body, the public API shows each copy,
and the operator keeps the source or relocates the record.

## Non-Goals

- The `record_conflict` and `guid_change_pending` cases.
- A fetch by the node.
- A change to the crawler or the parser. The node gets the mirror body from
  the ingest request that it already receives.

## Assumptions

- `handle_ingest_feed` in `src/api.rs` classifies a mirror in two places: the
  write phase, and the `ReadPhaseOutcome::NoChange` branch. Both have the
  parsed body in `req.feed_data`.
- `db::insert_feed_block` is the one function that writes a block row. The
  apply step, the environment seed and the admin route call it.
- `PATCH /v1/feeds/{guid}` with `feed_url` is the relocation of ADR 0052. It
  changes only `feeds.feed_url` today.
- Migration 0039 is the last migration. The next file is
  `0040_feed_copies.sql`.

## Decisions For The Tasks

1. **The table.** `feed_copies`, `STRICT`, primary key `(feed_guid, url)`.
   Columns: `first_seen`, `last_seen`, `title`, `item_guids` (JSON array, in
   order), `feed_recipients` (JSON array), `track_recipients` (JSON object,
   item GUID to array), `summary_digest`, and the resolution columns
   `resolution` (`keep_source` or `relocate`, nullable), `resolution_reason`,
   `resolved_at`, `resolved_digest`. `last_seen` is written only by the
   primary ingest path.
2. **The overflow counter.** `feed_copy_overflow (feed_guid PRIMARY KEY,
   count)`, written only by the primary ingest path.
3. **The summary.** Built from `IngestFeedData`: the title, the item GUIDs in
   order, the feed recipient set, and a map of item GUID to recipient set. A
   recipient is `model::RouteRecipient`. The digest is the SHA-256 hex of the
   canonical JSON of the summary without the title, so a title change alone
   makes no event.
4. **The events.** `FeedCopyObserved { feed_guid, url, first_seen, title,
   item_guids, feed_recipients, track_recipients, summary_digest }` and
   `FeedCopyResolved { feed_guid, url, decision, reason, resolved_at,
   resolved_digest }`. Both are additive and idempotent in `apply.rs`.
   `subject_guid` is the feed GUID.
5. **The limit.** 20 rows for each GUID, a constant in `src/db.rs`. A new URL
   past the limit writes no row and no event, and adds one to the counter.
6. **A URL block.** `db::insert_feed_block` deletes each `feed_copies` row
   with that URL when it inserts a URL block. A feed delete deletes the rows
   and the counter of its GUID.
7. **The differences.** Computed at read time against the current record:
   `differs_tracks` compares the item GUID sets. `differs_recipients` compares
   the feed recipient set, and the set of each item GUID in both. The helper
   lives in `src/db.rs` or `src/model.rs`, and the list route and the detail
   route use the same helper.
8. **`guid_origin`.** True when the GUID equals the UUIDv5 of the row URL and
   does not equal the UUIDv5 of the source URL. The namespace UUID is
   `ead4c236-bf58-58c6-a2c6-a6b28d128cb6`. The input is the URL with the
   scheme and a trailing slash removed.
9. **Open.** A copy is open when it differs and `resolved_digest` is null or
   not equal to `summary_digest`.
10. **The relocation.** One function in `src/api.rs` or `src/db.rs` does the
    relocation for both routes, in one transaction: it sets `feed_url`,
    clears `last_build_date` and `declared_self_url`, and signs
    `FeedUpserted`. `FeedUpsertedPayload` gets an optional `reason` field,
    with `#[serde(default, skip_serializing_if = "Option::is_none")]`. A
    `PATCH` with `feed_url` and no `reason` answers `400`.
11. **The routes.** `GET /v1/feeds/{guid}/copies` and `GET /v1/copies` in
    `query_routes`. `copy_count` in the feed response. `POST
    /v1/feeds/{guid}/copies/resolve` in the admin routes of `build_router`.

## Sequence

| Task | Crate | Needs |
|---|---|---|
| [001](../tasks/adr-0058-task-001-copy-storage-and-events.md) Storage, events, summary | `stophammer` | — |
| [002](../tasks/adr-0058-task-002-ingest-records-copies.md) Ingest records copies | `stophammer` | 001 |
| [003](../tasks/adr-0058-task-003-public-copy-routes.md) Public routes | `stophammer` | 002 |
| [004](../tasks/adr-0058-task-004-resolve-and-relocate.md) Resolve and relocate | `stophammer` | 003 |
| [005](../tasks/adr-0058-task-005-deploy.md) Deploy | VPS | 004 |

The tasks run one after another, because 002, 003 and 004 each change
`src/api.rs`, `src/openapi.rs` or `docs/API.md`.

## Schema And API Implications

- One migration, `0040_feed_copies.sql`, with two tables.
- Two event types. Community nodes get the version before the primary.
- Three public routes, one admin route, and one new field in the feed
  response.
- `PATCH /v1/feeds/{guid}` with `feed_url` needs a `reason`.

## Risk Areas

- **The NoChange branch.** A mirror with the same body hash as its last
  submission reaches the `NoChange` branch, not the write phase. Task 002
  records the copy in both places.
- **Replicated state.** A community node must not write `last_seen` or the
  counter.
- **Event volume.** The first crawl after the deploy writes one event for
  each mirror pair, about 1,600. After that, only a changed summary writes one.

## Test Strategy

- The guards of ADR 0058, one test file for each task.
- The replica test: apply the events of the primary to a second database, and
  compare the answers of each public route.
- The full gate and the migration tests.

## Rollback

Deploy the previous image. The two tables and the events stay, and that binary
ignores them. `PATCH` accepts a relocation with no reason again.
