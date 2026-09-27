# ADR 0064 Task 004: Deploy And Check

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md). Plan:
[ADR 0064 phase plan](../plans/adr-0064-live-items-phase-plan.md).

Repositories: `stophammer-crawler` for the build, then the operator.

## Goal

The crawler sends the relay link. The deploy follows the order of the phase
plan, and production agrees with the case table.

## Files To Inspect

- `stophammer-crawler/Cargo.toml`: the path dependency on the parser
- Each place in `stophammer-crawler/src/` that builds `IngestLiveItemData` or
  `live_items` (search `live_items`)

## Files Likely To Change

- A test fixture in `stophammer-crawler/src/` that builds a live item, if one
  does. No other file.

## Do Not Touch

- Each crawler file other than a fixture that no longer compiles.
- `stophammer/`, `stophammer-parser/`.

## Steps

1. In `stophammer-crawler`, build and test with the parser of task 001.
   Correct a test fixture only if it no longer compiles.
2. Before the deploy, the operator records the counts of `live_events` rows,
   of `TrackRemoved` events and of `LiveEventsReplaced` events.
3. The operator deploys the node, then the crawler.
4. After the next crawl of the feeds in the phase plan, the operator checks
   "Checks After The Deploy" of the phase plan.

## Acceptance

Mechanical:

- The gate of `stophammer-crawler` is green.

Operator checks, because a test cannot touch production:

- The 6 tracks from `ended` items are removed, and their items are `ended`
  rows.
- The "100% Retro" feeds emit no new `LiveEventsReplaced` on a crawl with no
  change.
- `GET /v1/live-items` gives no row, and `view=all` gives the rows of
  production.

## Test Commands

```bash
cd /home/citizen/build/stophammer/stophammer-crawler
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report when the crawler needs a change other than a fixture.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0064-task-004-deploy.md
- /home/citizen/build/stophammer/stophammer-crawler/AGENTS.md

Goal:
- Build and test `stophammer-crawler` with the current `stophammer-parser`. Correct a test fixture only if it no longer compiles.

Constraints:
- No change other than a fixture that no longer compiles.

Do not touch:
- `stophammer/`, `stophammer-parser/`, and each crawler file that is not a failing fixture.
- Git: run no git command that writes (no add, commit, stash, checkout, reset).

Acceptance criteria:
- The gate of `stophammer-crawler` is green.

Test commands:
- cargo build
- cargo test
- cargo clippy --all-targets -- -D warnings
- cargo fmt -- --check

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
