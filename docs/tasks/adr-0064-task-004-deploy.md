# ADR 0064 Task 004: Deploy And Check

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md). Plan:
[ADR 0064 phase plan](../plans/adr-0064-live-items-phase-plan.md).

Repositories: `stophammer-crawler` for the build, then the operator.

## Goal

The crawler sends the relay link, the deploy follows the order of the phase
plan, and production agrees with the case table.

## Steps

1. In `stophammer-crawler`, build and test with the parser of task 001. The
   crawler sends `IngestFeedData` of the parser, so no code change is
   expected. Correct a test fixture if it builds a live item.
2. The operator deploys the node, then the crawler.
3. The operator records the count of `live_events` rows, `TrackRemoved` events
   and `LiveEventsReplaced` events before the deploy.
4. After the next crawl of the feeds in the phase plan, the operator checks
   the three items of "Checks After The Deploy".

## Acceptance

Mechanical:

- The gate of `stophammer-crawler` is green.

Operator checks, because a test cannot reach production:

- The 6 tracks from `ended` items are removed, and their items are `ended`
  rows.
- The "100% Retro" feeds emit no new `LiveEventsReplaced` on a crawl with no
  change.
- `GET /v1/live-items` gives no row, and `view=all` gives the rows of
  production.
