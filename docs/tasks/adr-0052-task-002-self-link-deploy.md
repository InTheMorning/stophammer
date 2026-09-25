# ADR 0052 Task 002: Deploy The Self-Link Move

Owner: [ADR 0052](../adr/0052-a-source-moves-its-own-feed.md) section 6.
Plan: [phase plan 1](../plans/adr-0052-self-link-phase-plan.md).

**The operator does this task.** It is not for a coding model.

## Goal

The self-link move runs on the primary, after the ADR 0051 repair.

## Steps

1. Make sure that the ADR 0051 repair ran: ADR 0051 task 006, or a
   `refresh --force` pass.
2. Deploy the image that holds this phase. It can be the same deploy as ADR
   0053. That deploy upgrades each community node first.
3. Fill the column. Replay the fetch cache of the last `refresh` pass with
   ADR 0051 task 006, or run `refresh --force`. Each source ingest records its
   self link. A pass without `--force` does not fill the column, because the
   node answers `no_change` for an unchanged body and does not write it.
4. Count the records that can move:

   ```sql
   SELECT COUNT(*) FROM feeds
   WHERE declared_self_url IS NOT NULL AND declared_self_url <> feed_url;
   ```

   On 2026-09-24, about 1,429 Wavlake records were in this condition.
5. After the next podpings and publisher-link crawls, count again. The count
   goes down as records move. Each move logs `ADR 0052`.

## Acceptance Criteria

Mechanical:

- Step 4 gives a count near 1,429 after step 3.
- The count of step 5 is lower than the count of step 4.

Manual. If a check cannot run, report it as open:

- The operator reads the logged moves of one hour and finds no move to a URL
  on a different host.
