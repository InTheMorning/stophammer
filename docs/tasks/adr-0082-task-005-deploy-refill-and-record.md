# ADR 0082 Task 005: The Deploy, The Refill Pass And The Record

Owner: [ADR 0082](../adr/0082-a-funding-link-is-a-source-link.md)
"Consequences", with [ADR 0077](../adr/0077-a-new-body-value-states-its-refill-pass.md)
for the pass and [ADR 0065](../adr/0065-a-release-promotes-its-candidate.md)
for the promotion. The phase plan is
[the funding links plan](../plans/funding-links-phase-plan.md).

Repositories: `stophammer`, and the VPS. The operator deploys and runs the
pass. The agent checks and writes the record. This task starts after the
candidate `v0.18.0-rc.1` exists.

## Goal

Release 0.18.0 runs in production. The refill pass read each held feed, and
the check of ADR 0082 "Consequences" passes. `AGENTS.md` describes the
present, a measurement record keeps the counts, and the plan, the checklist
and tasks 001 to 005 are deleted.

## Context

The node stores no raw namespace tag, so the count of feeds with the tag comes
from the crawler fetch cache. The cache keeps each body as `body_gzip` in
`feed_cache.db`. A search of the bodies for the text `podcast:funding` gives
the expected count.

The refill pass is `stophammer-crawler --force --no-revalidate refresh`.
`--no-revalidate` sends no conditional headers, so each feed server answers
with a body. `--force` makes the node read a body whose hash it holds. On the
VPS, the Compose service `stophammer-crawler` runs the binary with the
arguments of the command line. The handoff gives the exact block, with the
state paths under `/data`.

The check compares two counts: the feeds with the tag in the cache, and
`SELECT count(DISTINCT feed_guid) FROM source_entity_links WHERE link_type = 'funding'`
on a backup after the pass. One known feed with the tag gives its rows in
`GET /v1/feeds/{guid}?include=source_links`.

## Files To Inspect

- `.idea/releases/0.18.0/deploy-0.18.0.md`
- `docs/reviews/adr-0081-first-pass-measurement.md`, for the form of a record
- `AGENTS.md`: "Where The Work Stands", "Deployed", "Open Items"
- `docs/reviews/funding-links-review-checklist.md`, "Each Task"
- `tests/adr0045_governance_guards_tests.rs`

## Files Likely To Change

- `AGENTS.md`
- `docs/adr/0082-a-funding-link-is-a-source-link.md`: the "Status" section
  only
- New: `docs/reviews/adr-0082-refill-measurement.md`
- Deleted: `docs/plans/funding-links-phase-plan.md`,
  `docs/reviews/funding-links-review-checklist.md`, and
  `docs/tasks/adr-0082-task-001-*.md` to `docs/tasks/adr-0082-task-005-*.md`

## Constraints

- **The deploy.** `deploy.sh --release 0.18.0-rc.1 --backup`. The script
  ends with the revision of the candidate. The migration runs at the start of
  the primary, and the log shows no skipped migration.
- **The checks of the agent.** `GET /node/info` gives the commit of the
  candidate. `GET /openapi.json` gives `info.version` `0.18.0`, and
  `SourceEntityLinkResponse` holds `label`.
- **The pass.** The operator counts the cache bodies first, then runs the
  pass, and keeps its `fetch:` report line and its duration. The pass runs at
  a time when no `import` run and no `podcastindex` pass starts, because the
  three modes write the crawler state.
- **The check.** The agent compares the two counts and reads one known feed.
  The record gives both counts, the ratio, and the feed.
- **The promotion.** After the check, the operator tags `v0.18.0`, sets the
  notes on the release page, and runs `deploy.sh --release 0.18.0`.
- **`AGENTS.md`.** The first paragraph says that no release is open and that
  release 0.18.0 of its date delivered ADR 0082. "Deployed" names 0.18.0.
  "Complete And Not Deployed" says "Nothing." The ADR 0082 item of "Open
  Items" is gone when the check passed. When the check did not pass, the item
  names the pass and the result (ADR 0077 §2).
- **The record.** `docs/reviews/adr-0082-refill-measurement.md` gives the two
  counts, the report line and the duration of the pass, and the known feed. It
  keeps the results of the task reviews from the checklist. It states no
  rule.
- **The ADR.** The "Status" section keeps "Accepted on 2026-10-31", says that
  release 0.18.0 holds the change, and links the record. It links no deleted
  file.
- **No link to a deleted file.** `rg 'funding-links|adr-0082-task-00[1-5]'`
  in `stophammer`, `stophammer-parser` and `stophammer-crawler` gives only
  the record after the change.
- **Prose.** Each changed document passes the STE checker with no
  sentence-length or paragraph-length result on the new text.
- **Git.** The agent runs no git write command. It gives one commit command
  with `git rm` for the deleted files.

## Do Not Touch

- `docs/adr/`, other than the "Status" section of ADR 0082
- `src/`, `migrations/`, `stophammer-parser/`, `stophammer-crawler/`
- The state files on the VPS, other than through the pass and a read-only
  copy

## Implementation Steps

The operator, with the handoff:

1. Deploys the candidate with a backup, and pulls the Compose checkout.
2. Counts the cache bodies with the tag, and gives the count to the agent.
3. Runs the refill pass, and gives the report line and the duration.
4. Makes a backup after the pass, and gives the count of the SQL statement.

The agent:

5. Reads `GET /node/info`, `GET /openapi.json` and one known feed.
6. Compares the counts, and reports pass or fail.

The operator:

7. Promotes the release, sets the notes and deploys it.

The agent:

8. Writes the record and the `AGENTS.md` change, and changes the ADR
   "Status". Deletes the plan, the checklist and the five packets. Runs the
   link search, the governance guard and the STE checker, and gives the commit
   command.

## Acceptance Criteria

Mechanical:

- `cargo test --test adr0045_governance_guards_tests` passes.
- The link search of "Constraints" gives only the record.
- `ls docs/tasks/` gives no `adr-0082-*` file.

Visual, for the operator:

- The backup count is near the cache count. A difference has three possible
  causes: a feed that is not in the cache, a feed server that gave no body in
  the pass, and a tag with no usable URL. The record names the difference.
- The known feed gives its funding rows with their labels on the live API.

## Test Commands

```bash
cd ~/build/stophammer
cargo test --test adr0045_governance_guards_tests
rg 'funding-links|adr-0082-task-00[1-5]' . stophammer-parser stophammer-crawler --glob '!target' --glob '!.idea'
python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics AGENTS.md docs/reviews/adr-0082-refill-measurement.md docs/adr/0082-a-funding-link-is-a-source-link.md
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- The deploy log reports a skipped migration.
- `GET /openapi.json` gives no `label` in `SourceEntityLinkResponse`.
- The pass stops before its end, or its report line is not available.
- The backup count is below half of the cache count.
- The known feed gives no `funding` row after the pass.

## Expected Final Report

1. the result of each check, with the two counts
2. files changed, created and deleted
3. the result of the governance guard and the STE checker
4. the commit command
5. deviations from this task
6. unresolved concerns

## Prompt for lower-context coding model

Step 8 of this task fits a lower-context model. Steps 5 and 6 do not. The
planning model does them.

You are implementing one bounded task from a larger plan.

Implement only step 8 of this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/AGENTS.md`
- `~/build/stophammer/docs/tasks/adr-0082-task-005-deploy-refill-and-record.md` (this task)
- `~/build/stophammer/docs/reviews/funding-links-review-checklist.md`
- `~/build/stophammer/docs/reviews/adr-0081-first-pass-measurement.md`, for the form
- the counts, the report line and the check results that the operator and the planning model give in the prompt

Goal:
- `AGENTS.md` describes the present after release 0.18.0, and a measurement record keeps the counts of the refill pass.
- The plan, the checklist and task packets 001 to 005 are deleted.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Write in Simplified Technical English, with the shared skill `~/.agents/skills/asd-ste100/SKILL.md`.
- Put no VPS host, user or directory in a file of the repository.
- Stop at each escalation trigger of this task.

Do not touch:
- `docs/adr/` other than the "Status" section of ADR 0082
- `src/`, `migrations/`, `stophammer-parser/`, `stophammer-crawler/`
- git: run no git write command. Give the commit command with `git rm`.

Acceptance criteria:
- Each mechanical item of the section "Acceptance Criteria" of this task passes.

Test commands:
- the block of the section "Test Commands" of this task

At the end, report:
1. files changed, created and deleted
2. the result of the governance guard and the STE checker
3. the commit command
4. deviations from this task
5. unresolved concerns
