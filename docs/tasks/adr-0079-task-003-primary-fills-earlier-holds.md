# ADR 0079 Task 003: The Primary Fills The Holds Of Earlier Retires

Owner: [ADR 0079](../adr/0079-a-retired-guid-returns-only-from-its-source.md)
§4. The phase plan is
[the search index and retired source plan](../plans/search-index-and-retired-source-phase-plan.md).

Repository: `stophammer`. The operator commits. This task starts after task
001.

## Goal

At its start, the primary writes a hold for each GUID that a retire with no
block freed before the deploy. The held URL comes from the event log.

## Files To Inspect

- `src/main.rs`: the primary start near line 145, where `blocks::seed_blocks`
  runs before the router. A community node does not seed.
- `src/blocks.rs`: `seed_blocks`, the pattern of a start step of the primary
- `src/db.rs`: the hold functions of task 001, and the `events` table reads
- `src/event.rs`: `FeedRetiredPayload`, `FeedUpsertedPayload`
- `src/schema.sql`: `events`, `feeds`, `feed_blocks`

## Files Likely To Change

- `src/db.rs`: a new function `fill_retired_source_holds`
- `src/main.rs`: one call after the block seed
- `tests/adr0079_retired_source_tests.rs`: new tests at the end of the file

## Constraints

- **The selection.** For each `subject_guid` of the events of the type
  `feed_retired`, take its last such event by `seq`. Write a hold only when all
  of these are true:
  - the `reason` of that event is `podcast_block`, `source_gone` or
    `admin retired via API`,
  - no row of `feeds` has the GUID,
  - no row of `feed_blocks` with `kind` `guid` has the GUID, in lower case,
  - no hold has the GUID.
- **The URL.** The held URL is `feed.feed_url` of the last `feed_upserted` event
  of the same GUID with a `seq` below the `seq` of the retire. When no such
  event exists, write no hold, and log one `tracing::warn!` with the GUID and a
  message that names ADR 0079 §4.
- **The other columns.** `reason` is the reason of the retire, and `retired_at`
  is its `created_at`.
- **One transaction.** The fill writes each hold in one transaction, and logs
  one `tracing::info!` with the count and the time.
- **The primary only.** Call the fill in `src/main.rs` after the block seed, in
  the same primary branch. A community node does not call it.
- **Idempotent.** A second start writes no row.
- **Comments.** The function names ADR 0079 §4.

## Do Not Touch

- The hold write in `delete_feed_with_event`, the `NewFeed` rule and the
  release route. Tasks 001 and 002 own them.
- `seed_blocks` and `feed_blocks`
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write `fill_retired_source_holds(conn) -> Result<usize, DbError>`.
2. Call it in `src/main.rs`, and map its error as the block seed does.
3. Write the tests.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0079_retired_source_tests.rs`.

- ADR 0079 check 7: a database has a `feed_upserted` event, then a
  `feed_retired` event with the reason `podcast_block`, and no record of the
  GUID. The fill writes one hold with the `feed_url` of that `feed_upserted`
  event.
- A second run of the fill writes no row.
- A GUID with a retire and a later `feed_upserted` event, so that a record
  exists again, gets no hold.
- A GUID with a `feed_blocks` row of the kind `guid` gets no hold.
- A GUID whose last retire has the reason `guid_superseded` gets no hold.
- A GUID with a retire and no earlier `feed_upserted` event gets no hold.
- When two `feed_upserted` events come before the retire with two different
  URLs, the hold has the URL of the later event.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0079_retired_source_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A `feed_upserted` payload has no `feed.feed_url`.
- The admin retire writes a reason that is not `admin retired via API`.
- The primary start has no single place that runs before the router.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/AGENTS.md` (the code style and the pre-commit gate)
- `~/build/stophammer/docs/adr/0079-a-retired-guid-returns-only-from-its-source.md`, §1 and §4
- `~/build/stophammer/docs/tasks/adr-0079-task-003-primary-fills-earlier-holds.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- At its start, the primary writes a hold for each GUID that a retire with no block freed before the deploy, with the URL of the last `feed_upserted` event before the retire.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Run the fill on the primary only, after the block seed.
- Write each hold in one transaction. A second start writes nothing.
- Stop at each escalation trigger of this task.

Do not touch:
- the hold write, the `NewFeed` rule, the release route, `seed_blocks`, `feed_blocks`
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0079_retired_source_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
