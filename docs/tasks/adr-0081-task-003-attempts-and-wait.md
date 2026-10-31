# ADR 0081 Task 003: A Failed Candidate Waits 7 Days

Owner: [ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md)
§5 and §7. The phase plan is
[the music list plan](../plans/podcastindex-music-list-phase-plan.md).

Repository: `stophammer-crawler`. The operator commits. This task starts
after task 002 when one agent does both, because the two tasks edit
`src/modes/podcastindex.rs`.

## Goal

The mode keeps one row for each candidate that it sent to the pipeline, in
`music_list_attempts` of `import_state.db`. A candidate with an outcome other
than `accepted` or `no_change` waits 7 days before the next try. The store
holds no list row that the mode did not fetch.

## Files To Inspect

- `src/modes/import.rs`: `open_state_connection` (line 1190),
  `initialize_state_schema` (near line 1130), `ProgressStore`, and the
  `import_feed_memory` upsert (near line 1240)
- `src/crawl.rs`: `CrawlOutcome::label` (line 227) and
  `CrawlOutcome::reason` (line 239)
- `src/feed_skip.rs`: `FeedSkipDb::open`, for the form of a small SQLite
  store with tests
- `docs/adr/0030-podcastindex-importer-durable-attempt-memory.md` of
  `stophammer`: the state database contract

## Files Likely To Change

- `src/modes/import.rs`: `open_state_connection` becomes `pub(crate)`
- `src/modes/podcastindex.rs`

## Constraints

- **The connection.** Make `open_state_connection` in `src/modes/import.rs`
  `pub(crate)`. Change nothing else there. The new store opens the same path
  with that function, so the `import` schema, the journal mode and the busy
  timeout stay the same.
- **The table.** In `src/modes/podcastindex.rs`, `pub(crate) struct
  AttemptStore { conn: Connection }` with `open(path: &str) -> Self`. The
  open runs:

  ```sql
  CREATE TABLE IF NOT EXISTS music_list_attempts (
      podcastindex_id   INTEGER PRIMARY KEY,
      feed_url          TEXT NOT NULL,
      outcome           TEXT NOT NULL,
      outcome_reason    TEXT,
      last_attempted_at INTEGER NOT NULL,
      attempt_count     INTEGER NOT NULL DEFAULT 1
  );
  ```

- **The write.** `record(&self, id: i64, url: &str, outcome: &str, reason:
  Option<&str>, now: i64)` upserts the row. On a conflict, it replaces
  `feed_url`, `outcome`, `outcome_reason` and `last_attempted_at`, and adds 1
  to `attempt_count`. `outcome` is `CrawlOutcome::label()`, and `reason` is
  `CrawlOutcome::reason()`.
- **The read.** `last_attempts(&self, ids: &[i64]) -> HashMap<i64, Attempt>`
  with `pub(crate) struct Attempt { outcome: String, last_attempted_at: i64
  }`. Read in chunks of at most 500 IDs for each statement.
- **The rule.** `pub(crate) const RETRY_WAIT_SECS: i64 = 7 * 86_400`.
  `pub(crate) fn waits(attempt: Option<&Attempt>, now: i64) -> bool` gives
  `true` only when `attempt` is `Some`, its `outcome` is neither `accepted`
  nor `no_change`, and `now - last_attempted_at < RETRY_WAIT_SECS`. A pure
  function, so the tests fix `now`.
- **The split.** `pub(crate) fn split_waiting(candidates: Vec<MusicListRow>,
  attempts: &HashMap<i64, Attempt>, now: i64) -> (Vec<MusicListRow>,
  usize)` gives the candidates that do not wait, in their sequence, and the
  number that wait.
- **§7.** The store writes only through `record`, and `record` is called only
  for a URL that the pipeline fetched. No function writes a list row before a
  fetch. State this in the doc comment of `AttemptStore`.
- **Tests.** Use a temporary directory from the `tempfile` crate for the
  database path, as the `feed_skip` tests do.

## Do Not Touch

- `import_progress`, `import_feed_memory` and each other statement of
  `src/modes/import.rs`
- `src/modes/batch.rs`, `src/main.rs`, `src/podcastindex_api.rs`
- `src/feed_skip.rs`
- `README.md`, `AGENTS.md`
- `stophammer/`, `stophammer-parser/`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Make `open_state_connection` `pub(crate)`. Run the `import` tests.
2. Write `AttemptStore::open`, `record` and `last_attempts`, with tests on a
   temporary file.
3. Write `waits` and `split_waiting`, with tests.

## Acceptance Criteria

Mechanical. Each is an inline test in `src/modes/podcastindex.rs`.

- `AttemptStore::open` on a path with no file makes the file and the table.
  A second open on the same path keeps the rows.
- `record` of a new ID gives `attempt_count` 1. A second `record` of the same
  ID with a new outcome gives the new outcome, the new time and
  `attempt_count` 2.
- `last_attempts` of 1,200 IDs, of which 3 have rows, gives 3 entries.
- ADR 0081 check 9: `waits` is `true` for a `rejected` attempt 6 days old,
  and `false` for a `rejected` attempt 7 days old.
- `waits` is `false` for an `accepted` attempt 1 hour old, for a `no_change`
  attempt 1 hour old, and for `None`.
- `waits` is `true` for a `fetch_error`, a `parse_error` and an
  `ingest_error` attempt 1 day old.
- `split_waiting` of three candidates, of which one waits, gives two
  candidates in their sequence and the count 1.
- `src/modes/import.rs`: the open of `import_state.db` through the new store
  leaves `import_progress` and `import_feed_memory` in place. An `import`
  test or a new test opens a file with both schemas and reads a stored
  cursor.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-crawler
cargo build
cargo test podcastindex
cargo test import
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- `open_state_connection` cannot become `pub(crate)` without a change to its
  callers.
- The `MEMORY` journal mode gives a test failure with two connections on one
  file.
- A step needs a change to a file of "Do Not Touch".

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
- `~/build/stophammer/stophammer-crawler/AGENTS.md`
- `~/build/stophammer/docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md`, §5 and §7
- `~/build/stophammer/docs/adr/0030-podcastindex-importer-durable-attempt-memory.md`, the state database contract
- `~/build/stophammer/docs/tasks/adr-0081-task-003-attempts-and-wait.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- `music_list_attempts` in `import_state.db` keeps one row for each fetched candidate. A candidate with an outcome other than `accepted` or `no_change` waits 7 days.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- `waits` and `split_waiting` are pure functions that take `now`.
- Stop at each escalation trigger of this task.

Do not touch:
- the other statements of `src/modes/import.rs`, `src/modes/batch.rs`, `src/main.rs`, `src/podcastindex_api.rs`, `src/feed_skip.rs`, `README.md`, `AGENTS.md`
- `stophammer/`, `stophammer-parser/`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test podcastindex`
- `cargo test import`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
