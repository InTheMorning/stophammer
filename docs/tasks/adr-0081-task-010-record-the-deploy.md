# ADR 0081 Task 010: The Record Of The Deploy

Owner: [ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md),
with [ADR 0045](../adr/0045-governance-model-and-contract-ownership.md) for
the documents. The phase plan is
[the music list plan](../plans/podcastindex-music-list-phase-plan.md).

Repository: `stophammer`. The operator commits. This task starts after task
009, when release 0.17.0 is deployed and the first two passes ran.

## Goal

`AGENTS.md` says that release 0.17.0 is deployed and that ADR 0081 is in
production. A measurement record keeps the numbers of the first passes. The
phase plan, the review checklist and task packets 001 to 010 are deleted,
because their work is deployed. Task 011 stays.

## Context

`AGENTS.md` describes the present. `docs/plans/` holds a plan until its work
is complete, and `docs/tasks/` holds a packet until its work is deployed. A
measurement that an ADR needs moves to `docs/reviews/` before the plan is
deleted. The record of release 0.16.0 followed the same steps.

## Files To Inspect

- `AGENTS.md`: "Where The Work Stands", "Deployed", "Complete And Not
  Deployed", "Open Items" and "Operating Facts"
- `docs/reviews/podcastindex-music-list-review-checklist.md`, "Each Task" and
  "Merge Recommendation"
- `docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md`, "Status"
- `docs/reviews/adr-0050-second-pass-measurement.md`, for the form of a
  measurement record
- `tests/adr0045_governance_guards_tests.rs`

## Files Likely To Change

- `AGENTS.md`
- `docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md`: the
  "Status" section only
- New: `docs/reviews/adr-0081-first-pass-measurement.md`
- Deleted: `docs/plans/podcastindex-music-list-phase-plan.md`,
  `docs/reviews/podcastindex-music-list-review-checklist.md`, and
  `docs/tasks/adr-0081-task-001-*.md` to `docs/tasks/adr-0081-task-010-*.md`

## Constraints

- **`AGENTS.md`.** The first paragraph says that no release is open, and that
  release 0.17.0 of its date delivered ADR 0081. "Deployed" names release
  0.17.0 as the latest release. "Complete And Not Deployed" says "Nothing."
- **The two open bullets.** The "Open Items" bullet of §10 gives two dates:
  the first pass with the pipeline, and the day 28 days after it. The bullet
  links task 011. The Wavlake bullet of "Operating Facts" says that the daily
  pass finds a new Wavlake feed within one day.
- **The measurement record.** `docs/reviews/adr-0081-first-pass-measurement.md`
  gives the two report lines of task 009 with their dates, and compares them
  with the Context of the ADR. It gives each `429` line. It also keeps the
  results of the seven task reviews from the checklist. It states no rule.
- **The ADR.** The "Status" section keeps "Accepted on 2026-10-31", says that
  release 0.17.0 holds the mode, and links the measurement record. It links no
  deleted file.
- **No other link to a deleted file.** `rg 'podcastindex-music-list|adr-0081-task-0(0[1-9]|10)'`
  in `stophammer`, `stophammer-crawler` and `tests/` gives only task 011 and
  this record after the change.
- **Prose.** Each changed document passes the STE checker with no
  sentence-length or paragraph-length result on the new text.
- **Git.** The agent runs no git write command. It gives one commit command
  with `git rm` for the deleted files.

## Do Not Touch

- `docs/adr/` other than the "Status" section of ADR 0081
- `docs/tasks/adr-0081-task-011-import-gap-decision.md`
- `docs/operations.md`, `packaging/`, `docker-compose.yml`, `README.md`
- `stophammer-crawler/`, `stophammer-parser/`

## Implementation Steps

1. Write the measurement record from the report lines that the operator gave.
2. Change `AGENTS.md`.
3. Change the "Status" section of ADR 0081.
4. Delete the plan, the checklist and task packets 001 to 010.
5. Search for links to the deleted files, and correct each one.
6. Run the governance guard and the STE checker.

## Acceptance Criteria

Mechanical:

- `cargo test --test adr0045_governance_guards_tests` passes.
- The search of "Constraints" gives only task 011 and the measurement record.
- `ls docs/tasks/` gives only `adr-0081-task-011-import-gap-decision.md` and
  packets of other work.

Visual, for the operator:

- The "Where The Work Stands" section reads as the present: no release open,
  0.17.0 deployed, the §10 measurement the only open item of ADR 0081.

## Test Commands

```bash
cd ~/build/stophammer
cargo test --test adr0045_governance_guards_tests
rg 'podcastindex-music-list|adr-0081-task-0(0[1-9]|10)' . stophammer-crawler --glob '!target'
python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics AGENTS.md docs/reviews/adr-0081-first-pass-measurement.md docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- A test or a document outside "Files Likely To Change" links a deleted file.
- The report lines of task 009 are not available. The record needs them.

## Expected Final Report

1. files changed, created and deleted
2. the result of the governance guard and the STE checker
3. the commit command
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/AGENTS.md`
- `~/build/stophammer/docs/tasks/adr-0081-task-010-record-the-deploy.md` (this task)
- `~/build/stophammer/docs/reviews/podcastindex-music-list-review-checklist.md`
- `~/build/stophammer/docs/reviews/adr-0050-second-pass-measurement.md`, for the form
- the report lines of task 009, which the operator gives in the prompt

Goal:
- `AGENTS.md` describes the present after release 0.17.0, and a measurement record keeps the numbers of the first passes.
- The plan, the checklist and task packets 001 to 010 are deleted.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Write in Simplified Technical English, with the shared skill `~/.agents/skills/asd-ste100/SKILL.md`.
- Put no VPS host, user or directory in a file of the repository.
- Stop at each escalation trigger of this task.

Do not touch:
- `docs/adr/` other than the "Status" section of ADR 0081
- `docs/tasks/adr-0081-task-011-import-gap-decision.md`
- `docs/operations.md`, `packaging/`, `docker-compose.yml`, `README.md`
- `stophammer-crawler/`, `stophammer-parser/`
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
