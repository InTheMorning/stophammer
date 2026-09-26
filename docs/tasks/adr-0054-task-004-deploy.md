# ADR 0054 Task 004: Deploy

Owner: [ADR 0054](../adr/0054-a-fetch-reaches-only-public-feed-hosts.md).
Plan: [phase plan](../plans/adr-0054-fetch-rule-phase-plan.md).

**The operator does this task.** It is not for a coding model. Tasks 001 to
003 must be merged in their two repositories, and each gate must be green.

## Goal

The crawler and the node apply the fetch rule.

## Steps

1. Deploy the node: `./deploy.sh indexer`. It has no migration.
2. Deploy the crawler: `./deploy.sh crawler`.
3. After a day of podpings, count the rejections in the gossip log:

   ```bash
   docker compose logs --no-log-prefix --no-color --since 24h gossip \
     | sed 's/\x1b\[[0-9;]*m//g' \
     | grep -E "fetch_target_not_public|body_too_large" | sort | uniq -c | sort -rn | head
   ```

4. Read each host in the output. A music feed host in the list is a defect.

## Acceptance Criteria

Mechanical:

- Step 3 runs. Each line names a non-public host or a body over 16 MiB.

Manual. If a check cannot run, report it as open:

- The operator records the deploy in `AGENTS.md` under "Where The Work
  Stands".

## Rollback

Deploy the previous images. No stored data changes.
