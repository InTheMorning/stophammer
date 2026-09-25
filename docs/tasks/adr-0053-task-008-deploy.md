# ADR 0053 Task 008: Deploy

Owner: [ADR 0053](../adr/0053-a-correction-stays-applied.md). Plan:
[phase plan](../plans/adr-0053-durable-corrections-phase-plan.md).

**The operator does this task.** It is not for a coding model. Tasks 001 to
007 must be merged and their gates green. ADR 0051 must be deployed.

## Goal

Each node runs ADR 0053, and each community node can apply the two new event
types before the primary emits one.

## Steps

1. Make sure that the ADR 0051 repair ran: ADR 0051 task 006, or a
   `refresh --force` pass. The stale rule of ADR 0053 section 3 rejects the
   repair of a record that a mirror changed, so the repair must come first.
   The [research record](../reviews/last-build-date-behavior-research.md)
   gives the reason.
2. Run this query on the primary. The count must be `0`. If it is not, stop,
   and ask for the clamp of option B of the research record first.

   ```sql
   SELECT COUNT(*) FROM feeds
   WHERE last_build_date > CAST(strftime('%s','now') AS INTEGER) + 86400;
   ```

3. Read `BLOCKED_FEED_GUIDS` and `BLOCKED_FEED_URLS` on the primary. Each
   value becomes one signed `FeedBlocked` event at the first start. Record
   the count.
4. Read `VERIFIER_CHAIN` on the primary. A value that names `feed_blocklist`
   still starts. Remove the name to stop the warning.
5. Deploy the new image to each community node. Make sure that each one
   answers `GET /health` and continues to sync.
6. Make a consistent backup of the primary database.
7. Deploy the primary. Make sure that the start log gives the seed count of
   step 3.
8. Deploy the crawler image.
9. Make sure that `GET /v1/blocks` with the admin token lists the seeded
   blocks.
10. Make sure that one community node has the same `feed_blocks` rows as the
   primary.

## Acceptance Criteria

Mechanical:

- Step 2 gives `0`.
- Step 7 logs the seed count, and it equals the count of step 3.
- Step 10 gives the same rows on both nodes.

Manual. If a check cannot run, report it as open:

- The operator records the deploy in `AGENTS.md` under "Where The Work
  Stands".

## Rollback

Deploy the ADR 0051 image to the primary. The table and its events stay, and
that binary ignores them. The environment blocklist is active again. Community
nodes on the new version keep working.
