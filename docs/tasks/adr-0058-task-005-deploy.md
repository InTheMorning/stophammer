# ADR 0058 Task 005: Deploy

Owner: [ADR 0058](../adr/0058-a-copy-of-a-feed-is-public.md). Plan:
[phase plan](../plans/adr-0058-feed-copies-phase-plan.md).

**The operator does this task.** It is not for a coding model. Tasks 001 to
004 must be merged and their gates green.

## Goal

Each node runs ADR 0058. Each community node can apply the two new event types
before the primary emits one.

## Steps

1. Deploy the new image to each community node. Make sure that each one
   answers `GET /health` and continues to sync.
2. Make a consistent backup of the primary database.
3. Deploy the primary: `./deploy.sh indexer`.
4. Tell each client that uses `PATCH /v1/feeds/{guid}` with `feed_url` that
   the body now needs a `reason`.
5. Wait for crawls of mirror URLs. The next `refresh` pass, podpings and
   publisher links make the rows.
6. Read `GET /v1/copies`. Make sure that it lists
   `7192ec54-3aa2-5c61-987b-51bf75f68568` and that its Wavlake row has
   `guid_origin: true`. Also look for `27735a10-3775-503d-b038-70b4b7d536ab`
   and `c17b43a2`.
7. Count the rows and the events on the primary:

   ```sql
   SELECT COUNT(*) FROM feed_copies;
   SELECT COUNT(*) FROM events WHERE event_type = 'feed_copy_observed';
   ```

   After the first pass, the event count is near the row count. After a second
   pass, the event count grows only by the summaries that changed.

## Acceptance Criteria

Mechanical:

- Step 6 lists the album, with `guid_origin: true` on the Wavlake row.
- Step 7: a second pass adds few events. A second pass that adds about one
  event for each row shows a digest that is not stable. Stop, and report it.

Manual. If a check cannot run, report it as open:

- The operator decides the three known cases with the checks of ADR 0058
  section 4.
- The operator records the deploy in `AGENTS.md` under "Where The Work
  Stands".

## Rollback

Deploy the previous image to the primary. The tables and the events stay, and
that binary ignores them. `PATCH` accepts a relocation with no reason again.
