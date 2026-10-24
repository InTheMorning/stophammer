# ADR 0079 Task 001: A Retire Keeps A Hold, And A Held GUID Returns Only From Its Source

Owner: [ADR 0079](../adr/0079-a-retired-guid-returns-only-from-its-source.md)
§1 and §2. The phase plan is
[the search index and retired source plan](../plans/search-index-and-retired-source-phase-plan.md).

Repository: `stophammer`. The operator commits. This task starts after ADR 0078
task 002.

## Goal

A retire that writes no block of ADR 0053 writes a row in
`retired_source_holds`. A `NewFeed` submission for a held GUID is admitted only
from the held URL. A submission from a different URL writes nothing and gets
`retired_source_conflict`.

## Files To Inspect

- `src/schema.sql`: `feed_blocks`, `source_gone_answers`,
  `feed_guid_supersessions`
- `src/db.rs`: `MIGRATIONS`, `delete_feed_with_event` (near line 2830),
  `delete_feed_sql`, `classify_submission` and `SubmissionClass` (near line
  6337), `ingest_transaction`
- `src/api.rs`: the ADR 0053 block check (near line 2590), the `NewFeed` arm
  (near line 2949), the three callers of `delete_feed_with_event` (near lines
  2196, 2886 and 4853), and the GUID change retire (near lines 1240 to 1280)
- `src/event.rs`: `FeedRetiredPayload`
- `src/apply.rs`: the `FeedRetired` arm
- `docs/adr/archive/0051-feed-content-comes-from-its-source-url.md` §2
- `docs/adr/0046-migration-versions-are-array-positions.md`
- `tests/adr0057_podcast_block_tests.rs`, `tests/adr0067_source_gone_tests.rs`,
  `tests/adr0053_retirement_tests.rs`, `tests/adr0051_source_url_tests.rs`

## Files Likely To Change

- `migrations/0055_retired_source_holds.sql`, new. Use the next free number
  after the migration of ADR 0078 task 001.
- `tests/fixtures/migration_sha256.txt`: one new line
- `src/schema.sql`, `src/db.rs`, `src/api.rs`
- `tests/migration_tests.rs`: each recorded version moves by one
- `tests/adr0079_retired_source_tests.rs`, new

## Constraints

- **The table.**

  ```sql
  CREATE TABLE IF NOT EXISTS retired_source_holds (
      feed_guid   TEXT PRIMARY KEY,
      source_url  TEXT NOT NULL,
      reason      TEXT NOT NULL,
      retired_at  INTEGER NOT NULL
  ) STRICT;
  ```

  Add a comment in `src/schema.sql`: local to the primary, makes no event,
  ADR 0079 §1.
- **The write.** In `delete_feed_with_event`, when `blocks` is empty, read the
  `feed_url` of the feed before `delete_feed_sql`, in the same transaction.
  Then write the hold with `INSERT OR REPLACE`. `feed_guid` is in lower case.
  `reason` is the `reason` of the `FeedRetiredPayload` in `payload_json`, or
  `""` when it has none. `retired_at` is `created_at`.
- **No other write.** `delete_feed_sql`, `delete_feed` and the apply of
  `FeedRetired` write no hold. The GUID change retire of ADR 0052 calls
  `delete_feed_sql` and writes no hold. A retire with a block writes no hold.
- **The read.** Add `get_retired_source_hold(conn, feed_guid)` in `src/db.rs`.
  It compares the GUID in lower case.
- **The rule.** In the `NewFeed` arm of `src/api.rs`, before the ADR 0057 block
  check, read the hold of the declared GUID:
  - No hold: no change.
  - A hold, and `source_url` or `canonical_url` of the submission is equal to
    the held URL with exact string equality: no change. When the ingest admits
    the record, delete the hold in the ingest transaction.
  - A hold, and neither URL is equal: give `accepted: false`, `no_change:
    false`, reason `retired_source_conflict`, and no events. Write no row. Log
    one `tracing::warn!` with `feed_guid`, `held_url`, `source_url` and
    `canonical_url`, and a message that names ADR 0079 §2.
- **The writer lock.** The read of the hold is in the same writer section as
  `classify_submission` (ADR 0051 §3).
- **Comments.** Each new item names ADR 0079 and its section.

## Do Not Touch

- `classify_submission` and its five cases
- The ADR 0053 block check and `feed_blocks`
- The release route. Task 002 owns it, with the contract text.
- The fill of earlier retires. Task 003 owns it.
- `src/apply.rs`, `src/community.rs`
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`, `docs/API.md`, `src/openapi.rs`,
  each existing migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the migration, its `MIGRATIONS` entry, its hash line and the change to
   `src/schema.sql`. Add an `ensure_` repair only when `tests/migration_tests.rs`
   shows the case of ADR 0046.
2. Write the hold in `delete_feed_with_event`.
3. Add the read and the rule in the `NewFeed` arm, and the delete of the hold
   on admission.
4. Write the tests.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0079_retired_source_tests.rs`.

- ADR 0079 check 1: a test retires a feed by a `podcast:block` ingest, by two
  gone answers and by `DELETE /v1/feeds/{guid}?block=false`. After each
  retire, `retired_source_holds` has one row with the source URL of the record
  and the reason of the event.
- Check 2: a retire with a block, and a retire by a GUID change, write no hold.
- Check 3: after each retire of check 1, a submission with the same GUID from a
  different URL gets `retired_source_conflict`. The node then holds no record
  of the GUID, and the event log has no new event.
- Check 4: after each retire of check 1, a submission from the held URL without
  the block tag admits the feed. The tracks keep their feed-scoped identities,
  and the hold is deleted.
- Check 4: a submission whose `canonical_url` is the held URL, and whose
  `source_url` is a different URL, admits the feed.
- Check 5: a submission from the held URL that keeps the block tag gets
  `source_blocked`, and the hold stays.
- Check 8: a community node that applies a `FeedRetired` event writes no hold.
- The test `feed_removes_block_is_readmitted_with_same_track_ids` and each test
  of the four test files of "Files To Inspect" pass.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0079_retired_source_tests
cargo test --test adr0057_podcast_block_tests
cargo test --test adr0067_source_gone_tests
cargo test --test migration_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A retire path deletes a feed with no call of `delete_feed_with_event`, and
  it is not the GUID change retire or the apply of `FeedRetired`.
- An existing test admits a retired GUID from a different URL.
- The `NewFeed` arm has a different sequence than "Files To Inspect" gives.
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
- `~/build/stophammer/AGENTS.md` (the code style, the migration steps and the pre-commit gate)
- `~/build/stophammer/docs/adr/0079-a-retired-guid-returns-only-from-its-source.md`, §1 and §2
- `~/build/stophammer/docs/adr/archive/0051-feed-content-comes-from-its-source-url.md`, §2 and §3
- `~/build/stophammer/docs/adr/0046-migration-versions-are-array-positions.md`
- `~/build/stophammer/docs/tasks/adr-0079-task-001-retire-keeps-a-hold.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- A retire with no block writes a hold, and a `NewFeed` for a held GUID is admitted only from the held URL.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Write the hold only in `delete_feed_with_event`, only when `blocks` is empty.
- Apply the rule in the `NewFeed` arm before the ADR 0057 block check.
- Compare URLs with exact string equality.
- Stop at each escalation trigger of this task.

Do not touch:
- `classify_submission`, the ADR 0053 block check, `feed_blocks`, `src/apply.rs`, `src/community.rs`
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`, `docs/API.md`, `src/openapi.rs`, each existing migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0079_retired_source_tests`
- `cargo test --test adr0057_podcast_block_tests`
- `cargo test --test adr0067_source_gone_tests`
- `cargo test --test migration_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
