# ADR 0052 Task 008: Deploy Moves And GUID Changes

Owner: [ADR 0052](../adr/0052-a-source-moves-its-own-feed.md). Plan:
[phase plan 2](../plans/adr-0052-moves-and-guid-changes-phase-plan.md).

**The operator does this task.** It is not for a coding model. Tasks 004 to
007 must be merged in their three repositories, and each gate must be green.

## Goal

The node accepts the new request fields and applies the three move triggers
and the GUID-change rules. Then the crawler sends the hops and follows a
declared `itunes:new-feed-url`.

## Steps

1. Make a consistent backup of the primary database.
2. Deploy the primary: `./deploy.sh indexer`. The node accepts a request with
   no `redirects`, so the current crawler keeps working.
3. Read `GET /v1/guid-changes`. Record the rows. The four Doerfelverse
   releases of Elijah Lied appear after their next crawl, unless Doerfelverse
   reverts them first.
4. Deploy the crawler: `./deploy.sh crawler`.
5. Read the primary log for `ADR 0052 permanent redirect` and
   `ADR 0052 new-feed-url` after the next `refresh` pass or a day of podpings.
   Read one move of each trigger by hand.

## Acceptance Criteria

Mechanical:

- Step 3 answers `200`.
- Step 5 finds no move to a URL that declares a different GUID. The node
  cannot make one, and a finding is a defect.

Manual. If a check cannot run, report it as open:

- The operator decides each pending GUID change with the checks of ADR 0052
  section 4.
- The operator records the deploy in `AGENTS.md` under "Where The Work
  Stands".

## Rollback

Deploy the previous images. The columns, the tables and the events stay, and
those binaries ignore them. A move or a GUID change that applied stays
applied.
