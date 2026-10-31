# ADR 0081 Task 005: Each Pass Measures The `import` Gap

Owner: [ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md)
§10. The phase plan is
[the music list plan](../plans/podcastindex-music-list-phase-plan.md).

Repository: `stophammer-crawler`. The operator commits. This task starts
after task 004.

## Goal

Each pass counts the feeds that `import` accepted since the previous pass and
that the music list does not hold. The log gives the count. Two tables of
`import_state.db` keep the count and the counted rows. After 28 days, the
operator uses them to decide the schedule of `import`.

## Files To Inspect

- `src/modes/podcastindex.rs`: `run`, `AttemptStore`, `PassReport`, and the
  `podcastindex_pass_started_at` write of task 004
- `src/modes/import.rs`: the `import_feed_memory` schema (near line 1136) and
  its upsert (near line 1240). `fetch_outcome` holds `CrawlOutcome::label()`.
- `src/crawl.rs`: `CrawlOutcome::label` (line 227)

## Files Likely To Change

- `src/modes/podcastindex.rs`

## Constraints

- **The tables.** `AttemptStore::open` also runs:

  ```sql
  CREATE TABLE IF NOT EXISTS import_gap_passes (
      pass_at INTEGER PRIMARY KEY,
      counted INTEGER NOT NULL
  );
  CREATE TABLE IF NOT EXISTS import_gap_rows (
      pass_at          INTEGER NOT NULL,
      podcastindex_id  INTEGER NOT NULL,
      feed_url         TEXT NOT NULL,
      PRIMARY KEY (pass_at, podcastindex_id)
  );
  ```

- **The read.** `accepted_imports_since(&self, since: i64) -> Vec<(i64,
  String)>` reads `podcastindex_id` and `feed_url` from `import_feed_memory`
  where `fetch_outcome = 'accepted'` and `last_attempted_at >= since`. The
  statement is in the new module. It changes no statement of
  `src/modes/import.rs`.
- **The count.** `pub(crate) fn count_import_gap(accepted: &[(i64, String)],
  list_ids: &HashSet<i64>) -> Vec<(i64, String)>` gives each pair whose ID is
  not in `list_ids`, in the sequence of `accepted`. A pure function.
- **The write.** `record_import_gap(&self, pass_at: i64, rows: &[(i64,
  String)])` writes one `import_gap_passes` row with `counted = rows.len()`,
  and one `import_gap_rows` row for each pair, in one transaction.
- **The place in the pass.** After the classification and before the
  pipeline. The pass uses the old value of `podcastindex_pass_started_at` as
  `since`. When there is no old value, the pass prints
  `podcastindex: import gap: no previous pass, nothing counted`, writes no
  row, and continues. With a value, it prints
  `podcastindex: import gap since {since}: {count} accepted import feeds not in the music list`,
  followed by one line for each counted row with its ID and URL, at most 50
  lines. A `--dry-run` pass does the measurement and the writes too, because
  the measurement sends nothing to the node.
- **The report.** `PassReport` gets `import_gap: Option<usize>`, printed as
  `import_gap=N` or `import_gap=none`.
- **§7.** The counted rows are results of `import` fetches. The doc comment of
  `record_import_gap` says this and names §7.

## Do Not Touch

- `src/modes/import.rs`
- `src/modes/batch.rs`, `src/main.rs`, `src/podcastindex_api.rs`
- `README.md`, `AGENTS.md`
- `stophammer/`, `stophammer-parser/`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add the two tables to `AttemptStore::open`.
2. Write `accepted_imports_since`, with a test that inserts rows into an
   `import_feed_memory` table of a temporary file.
3. Write `count_import_gap`, with tests.
4. Write `record_import_gap`, with a test that reads the two tables back.
5. Add the step to the pass and the field to the report.

## Acceptance Criteria

Mechanical. Each is an inline test in `src/modes/podcastindex.rs`.

- ADR 0081 check 10: with three `import_feed_memory` rows, an `accepted` row
  after `since` with an ID not in the list is counted. These rows are not
  counted:
  - an `accepted` row with an ID in the list,
  - a `rejected` row with an ID not in the list,
  - an `accepted` row before `since`.
- `count_import_gap` keeps the sequence of its input.
- `record_import_gap` with two rows gives one `import_gap_passes` row with
  `counted` 2 and two `import_gap_rows` rows. A second call with the same
  `pass_at` fails or replaces, and the test states which. No partial write
  remains after a failure.
- The pass function with no previous start time calls `record_import_gap`
  zero times and gives `import_gap` `None`.
- The pass function with a previous start time and one counted row gives
  `import_gap` `Some(1)`.
- `PassReport` `Display` holds `import_gap=1` and `import_gap=none` for the
  two cases.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-crawler
cargo build
cargo test podcastindex
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- `import_feed_memory` has no `last_attempted_at` column in the schema that
  `open_state_connection` makes.
- The pass needs a second API request.
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
- `~/build/stophammer/docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md`, §7 and §10
- `~/build/stophammer/docs/tasks/adr-0081-task-005-import-gap-measurement.md` (this task)
- `~/build/stophammer/docs/tasks/adr-0081-task-004-pass-and-cli.md` (the pass this task extends)
- the files of "Files To Inspect" in this task

Goal:
- Each pass counts the `accepted` rows of `import_feed_memory` since the previous pass whose ID is not in the music list. It logs the count, and keeps the count and the rows in `import_gap_passes` and `import_gap_rows`.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Change no statement of `src/modes/import.rs`.
- `count_import_gap` is a pure function.
- Stop at each escalation trigger of this task.

Do not touch:
- `src/modes/import.rs`, `src/modes/batch.rs`, `src/main.rs`, `src/podcastindex_api.rs`, `README.md`, `AGENTS.md`
- `stophammer/`, `stophammer-parser/`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test podcastindex`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
