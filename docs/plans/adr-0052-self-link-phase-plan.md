# ADR 0052 Phase Plan 1: The Self-Link Move

Owner: [ADR 0052](../adr/0052-a-source-moves-its-own-feed.md), the self-link
trigger of sections 1 and 2, and section 6. This plan states no rule. The
other triggers of ADR 0052 get their own plan.

## Goal

A record moves to the URL that its source body names in
`atom:link rel="self"`. The 1,429 Wavlake records stored under
`https://wavlake.com/feed/<id>` then update from each podping and publisher
link for their `music` form.

## Non-Goals

- A move by a redirect, by `itunes:new-feed-url`, or by a GUID change.
- The collision check on `PATCH /v1/feeds/{guid}`.
- The fetch rules of ADR 0054.
- Any read of `source_entity_links` for a move decision.

## Assumptions

- ADR 0051 is deployed. The update case and the mirror case exist.
- The ADR 0051 repair runs before this phase is deployed. The repair replays
  a source body for each record, and that ingest writes the new column.
- The ingest request carries the self link in `feed_data.links` with the link
  type `self_feed` (`stophammer-parser/src/engine.rs:851`).
- On 2026-09-24, each of the 1,429 bare-form records had exactly one stored
  self link, and it named the `music` form. No feed had two self links.

## Affected Modules

| Task | Crate | Files |
|---|---|---|
| 001 | `stophammer` | `migrations/0039_feed_declared_self_url.sql`, `src/schema.sql`, `src/db.rs`, `src/api.rs` (ingest handler), `tests/migration_tests.rs`, `docs/API.md` |
| 002 | VPS | Operator deploy, with ADR 0053 |

## Decisions For The Tasks

1. **The column.** `ALTER TABLE feeds ADD COLUMN declared_self_url TEXT;`
   in migration `0039_feed_declared_self_url.sql`, the next array position
   after `0038`. `src/schema.sql` gets the same column. The `Feed` model
   does not get the field. The column never enters an event, because only
   the primary decides a move.
2. **The write.** The update case and the new-feed case of ADR 0051 write
   the column in the ingest transaction. The value is the URL of the first
   link with the type `self_feed` in `feed_data.links`. With no such link,
   the value is null. No other path writes the column.
3. **The move.** In the mirror case, before the observation step: read the
   `declared_self_url` of the record. When it equals `req.source_url` or
   `req.canonical_url`, exactly, the submission becomes an update with a
   move:
   - The new source URL is the recorded self URL.
   - Step 7b uses the new URL, not the stored URL.
   - The ingest transaction writes the content and the new `feed_url`, and
     revokes the proof tokens of the record with
     `proof::revoke_tokens_for_feed`. ADR 0056 task 001 removes this step
     with the tokens.
   - The normal diff emits `FeedUpserted` with the new URL.
   - The URL observations are recorded as for an update.
   - The response is `accepted: true`, with the warning
     `moved from <old> to <new> (ADR 0052 self link)`.
4. **No move otherwise.** The mirror case keeps its ADR 0051 behavior when the
   column is null or holds a different URL.
5. **The log.** Each move logs `tracing::info!` with `feed_guid`,
   `old_feed_url` and `new_feed_url`.

The mirror case of ADR 0051 already guarantees two checks of ADR 0052 section
1. The body declares the GUID of the record. No other record holds the new URL.

## Sequence

| Task | Crate | Needs |
|---|---|---|
| [001](../tasks/adr-0052-task-001-self-link-move.md) The self-link move | `stophammer` | ADR 0053 task 003 merged in the working tree |
| [002](../tasks/adr-0052-task-002-self-link-deploy.md) Deploy | VPS | 001, the ADR 0051 repair |

Task 001 changes the ingest handler. It runs after ADR 0053 task 003 and
before ADR 0053 task 004, so no two agents change the handler at once.

## Schema And API Implications

- One migration, one nullable column. No event type. No parser change.
- An ingest that was a `source_conflict` can now be an accepted move.
- `refresh` reads the stored URLs, so after a move it crawls the `music` form.

## Risk Areas

- **A self link from before the deploy.** Decision 2 prevents it: only a
  source ingest under the new code writes the column.
- **A wrong self link at the source.** A source that names a different feed
  cannot move the record. The body at the new URL must declare the same GUID.
  The mirror case also requires that no other record holds that
  URL.
- **A host that names a URL that then stops working.** The record moves, and
  the next crawls of the new URL fail. The source URL then decides again only
  through a new trigger. This is the same risk as `itunes:new-feed-url`.

## Test Strategy

Integration tests through `POST /ingest/feed`, in
`tests/adr0052_self_link_tests.rs`, and the migration tests.

## Rollback

Deploy the previous image. The column stays and that binary ignores it. The
records that moved keep their new `feed_url`, which is the URL their source
names.
