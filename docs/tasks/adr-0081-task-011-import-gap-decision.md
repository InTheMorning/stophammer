# ADR 0081 Task 011: The §10 Measurement Decides The Schedule Of `import`

Owner: [ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md)
§10, with [ADR 0030](../adr/0030-podcastindex-importer-durable-attempt-memory.md)
and [ADR 0033](../adr/0033-music-first-import-cursor-and-conditional-snapshot-refresh.md)
for the `import` mode.

Repository: `stophammer`. The operator runs the queries on the VPS and
commits. The agent examines the rows and writes the record and the ADR. This
task starts 28 days after the first pass with the pipeline. The first pass
ran on 2026-10-31, so the task starts on 2026-11-28.

## Goal

A measurement record gives the feeds that `import` accepted in 28 days and
that the music list did not hold. A decision follows the two branches of
ADR 0081 §10. When the count is none or almost none, a new ADR stops the
schedule of `import`. When the counted rows are real music feeds, the record
gives the size of the gap, and `import` keeps its schedule.

## Context

Each pass writes two tables of `import_state.db`. `import_gap_passes` holds
`pass_at` and `counted` for each pass. `import_gap_rows` holds `pass_at`,
`podcastindex_id` and `feed_url` for each counted row. A counted row is an
`import_feed_memory` row with the outcome `accepted` and a `last_attempted_at`
after the previous pass. Its ID is not in the music list of the pass.

Two limits apply to the count:

- PodcastIndex can move a counted feed to the music list after the pass. The
  pass does not count it again, and the record does not remove it.
- A pass that stopped under §6 wrote no count for its day. The record names
  each such day from the run log.

The crawler volume holds the state files. A query opens a copy in read-only
mode and changes nothing. The handoff gives the exact block, as the VPS check
of 2026-10-31 did.

## Files To Inspect

- ADR 0081 §10 and "Open Questions", answer 5
- ADR 0030 and ADR 0033, for the rules that a new ADR amends
- `stophammer-crawler/src/modes/podcastindex.rs`, `measure_import_gap` and
  the three `CREATE TABLE` statements
- `docs/reviews/adr-0074-feeds-without-guid-count.md`, for the form of a
  count record
- `AGENTS.md`, "Open Items" and "Crawler Subcommands"

## Files Likely To Change

- New: `docs/reviews/adr-0081-import-gap-measurement.md`
- Branch "stop": new `docs/adr/00NN-import-runs-on-request.md` with the next
  free number, `docs/adr/README.md`, `AGENTS.md`, `docs/operations.md`
- Branch "keep": `AGENTS.md`
- Deleted: this packet, after the commit of the decision

## Constraints

- **The queries.** The operator runs these statements on a read-only copy of
  `import_state.db`, and gives the output to the agent:

  ```sql
  SELECT datetime(pass_at, 'unixepoch'), counted FROM import_gap_passes ORDER BY pass_at;
  SELECT datetime(pass_at, 'unixepoch'), podcastindex_id, feed_url FROM import_gap_rows ORDER BY pass_at, podcastindex_id;
  SELECT count(*) FROM import_feed_memory WHERE fetch_outcome = 'accepted' AND last_attempted_at >= (SELECT min(pass_at) FROM import_gap_passes);
  SELECT outcome, count(*) FROM music_list_attempts GROUP BY outcome ORDER BY 2 DESC;
  ```

  The third statement gives the number of `import` acceptances in the window.
  The record gives the counted rows as a part of that number.
- **The examination of each counted row.** For each row, the agent records
  the medium that the RSS states, from `stophammer-parse <url>`. It also
  records whether the node holds the feed, from
  `GET /v1/feeds/recent?medium=all` or the search of the node. A row whose RSS states `music` and that the node holds
  is a real music feed that only `import` found. A row whose RSS states
  another medium is not a gap.
- **The record.** `docs/reviews/adr-0081-import-gap-measurement.md` gives the
  window, the passes and the days with no count. It gives the counted rows
  with their examination, the number of `import` acceptances, and the
  outcomes of `music_list_attempts`. It states no rule.
- **The decision.** The agent proposes the branch with its reason. The
  operator decides. In the branch "stop", the new ADR amends ADR 0030 and ADR
  0033. It keeps `import` as a manual tool with no API key, and names the
  record as its evidence. The ADR, the index, `AGENTS.md` and
  `docs/operations.md` move in one commit. In the branch "keep", `AGENTS.md`
  closes the open item and names the record.
- **The API.** The agent sends no request to the PodcastIndex API. Only the
  pass does (ADR 0081 §1).
- **Prose.** Each changed document passes the STE checker with no
  sentence-length or paragraph-length result on the new text.
- **Git.** The agent runs no git write command.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`, other than reads
- The state files on the VPS, other than through a read-only copy
- `docker-compose.yml`, `packaging/`, in the branch "keep"

## Implementation Steps

On the VPS:

1. Copy `import_state.db` from the crawler volume, and open the copy in
   read-only mode. Run the four statements, and give the output to the agent.

Here, the agent:

2. Writes the record from the output.
3. Examines each counted row, and adds the result to the record.
4. Proposes the branch, with the count and the reason.

The operator:

5. Decides the branch.

The agent:

6. In the branch "stop", writes the new ADR, the index row, the `AGENTS.md`
   change and the `docs/operations.md` change. In the branch "keep", changes
   `AGENTS.md`.
7. Runs the governance guard and the STE checker, and gives the commit command
   with `git rm` of this packet.

## Acceptance Criteria

Mechanical:

- `cargo test --test adr0045_governance_guards_tests` passes after the
  change.
- In the branch "stop", the new ADR has a row in `docs/adr/README.md`, and
  ADR 0030 and ADR 0033 name it in their "Status" sections.

Visual, for the operator:

- The record gives each counted row with its examination, and the proposed
  branch follows from the rows.
- "Open Items" of `AGENTS.md` holds no item of ADR 0081 after the commit.

## Test Commands

```bash
cd ~/build/stophammer
cargo test --test adr0045_governance_guards_tests
python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics docs/reviews/adr-0081-import-gap-measurement.md AGENTS.md
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- Fewer than 20 days of the window have a row in `import_gap_passes`. A timer
  starts a pass each 3 hours (ADR 0081 §1), so a normal window has
  approximately 224 rows. The passes did not run, and the window must
  extend.
- A counted row is a feed that the node does not hold, although `import`
  accepted it. That is a defect of the node or of the crawler, not a gap.
- The output of a statement is not available.

## Expected Final Report

1. the window, the number of passes and the days with no count
2. the counted rows with their examination
3. the proposed branch and its reason
4. files changed and created
5. the result of the governance guard and the STE checker
6. the commit command
7. unresolved concerns

## Prompt for lower-context coding model

Step 2 and step 3 of this task fit a lower-context model. The ADR of step 6
does not. The planning model writes it.

You are implementing one bounded task from a larger plan.

Implement only steps 2 and 3 of this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md`, §10
- `~/build/stophammer/docs/tasks/adr-0081-task-011-import-gap-decision.md` (this task)
- `~/build/stophammer/docs/reviews/adr-0074-feeds-without-guid-count.md`, for the form
- the query output that the operator gives in the prompt

Goal:
- `docs/reviews/adr-0081-import-gap-measurement.md` gives the window, the passes, the counted rows with their examination, and the outcomes of `music_list_attempts`.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Write in Simplified Technical English, with the shared skill `~/.agents/skills/asd-ste100/SKILL.md`.
- Send no request to the PodcastIndex API.
- Put no VPS host, user or directory in a file of the repository.
- Stop at each escalation trigger of this task.

Do not touch:
- `docs/adr/`, `AGENTS.md`, `docs/operations.md`
- `stophammer-crawler/`, `stophammer-parser/`, other than reads
- git: run no git write command

Acceptance criteria:
- The record gives each counted row with the medium of its RSS and whether the node holds it.
- The STE checker gives no sentence-length or paragraph-length result on the record.

Test commands:
- `python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics docs/reviews/adr-0081-import-gap-measurement.md`

At the end, report:
1. the window, the number of passes and the days with no count
2. the counted rows with their examination
3. files created
4. deviations from this task
5. unresolved concerns
