# ADR 0051 Task 006: Repair From The Fetch Cache

Owner: [ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md)
section 6. Plan: [phase plan](../plans/adr-0051-source-url-phase-plan.md).

**This task changes a different repository.** The script is in
`stophammer-crawler`. The commit goes in that repository and names ADR 0051.

Status: the script is written and tested on 2026-09-25. The operator steps
are open. They replace step 8 of
[task 005](adr-0051-task-005-deploy-and-repair.md) when a recent `refresh`
pass kept its bodies.

## Goal

The repair of ADR 0051 section 6 applies each source URL body again, with no
request to a feed host. A full `refresh` pass takes about 7 hours at the
Wavlake host delay. A replay of the fetch cache takes minutes.

## The Script

`stophammer-crawler/scripts/export-feed-cache-ndjson.py`. Python 3.9 or later,
standard library only.

- It opens `feed_cache.db` (ADR 0050) and the primary node database
  read-only. It writes only the output file.
- It exports a cache row only when the row URL is the stored `feed_url` of a
  feed. In a `refresh` pass, the requested URL is the stored URL, so each
  exported body came from a source URL.
- It skips each other row, for example a URL from a publisher-link wave. It
  counts the skipped rows.
- It writes the input format of the `ndjson` mode: `source_db` with the GUID,
  the stored URL and the title, `fetch` with the final URL, status `200` and
  the cached SHA-256, and `raw_xml`.
- `--since UNIX_SECONDS` limits the export to rows fetched at or after that
  time. Use the start time of the pass.
- It reports five counts to standard error, and exits with an error when it
  exports no row.

## Operator Steps

Do these on the VPS, after task 005 step 6. The node must run ADR 0051 before
the replay.

1. Make sure that the `refresh` pass ran with the fetch cache. The container
   has `FEED_CACHE_DB` or `--feed-cache`, and the file grew during the pass.
   If the pass ran without the cache, use `refresh --force` for the repair.
2. Make a consistent backup of the primary database.
3. Export:

   ```bash
   ./scripts/export-feed-cache-ndjson.py \
       --cache /data/feed_cache.db \
       --node-db /data/stophammer.db \
       --output /data/repair.ndjson \
       --since <pass start, Unix seconds>
   ```

   Record the five counts.
4. Replay with `--force`. The flag is necessary. Without it, an unchanged
   source body gives `no_change`, and the damaged content stays.

   ```bash
   CRAWL_TOKEN=... INGEST_URL=http://localhost:8008/ingest/feed \
     stophammer-crawler --force ndjson \
       --input /data/repair.ndjson --state /data/repair_state.db --reset
   ```

5. Continue with task 005 step 9.

Do the replay soon after the pass. A body shows the feed at its fetch time. A
feed that changed after the pass gets its older body back until the next
normal crawl.

## Reporting Defect

The crawler counts a `no_change` answer from the node as `accepted`. The node
answers `accepted: true` with `no_change: true`, and
`stophammer-crawler/src/crawl.rs` examines `accepted` first. Thus the replay
summary cannot show whether a row changed the node. Use the comparison of task
005 step 9, not the `accepted` count. A separate change can correct this
defect.

## Acceptance Criteria

Mechanical. These ran on 2026-09-25 against a local primary on a temporary
database, with the ADR 0051 code:

- A feed admitted at `https://victim.example/feed.xml`, then an attacker body
  at a different URL with the same GUID: the node answered `source_conflict`,
  and the title and route did not change.
- The damage of the old node, put in with SQL: the title and the route
  address changed to the attacker values.
- A cache with three rows: the source URL, the attacker URL and a URL that no
  feed holds. The export gave `exported: 1` and
  `skipped_not_a_source_url: 2`.
- A replay without `--force`: the title and the route stayed the attacker
  values.
- A replay with `--force`: the title and the route returned to the victim
  values.
- A missing database file: the script exits with status `1` and names the
  file.

Manual: the operator records the export counts and the task 005 comparison.
If the replay cannot run, report the gate as open.

## Escalation Triggers

Stop and report when:

- `skipped_undecodable` is not zero.
- `exported` is much smaller than the number of feeds that the pass fetched.
  The pass then did not keep its bodies, or the node database is not the one
  that the pass read.
