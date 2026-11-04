# ADR 0083 Task 004: The Reference Documents Give The Rule

Owner: [ADR 0083](../adr/0083-a-feed-move-refuses-a-loop.md). The owner of each
stated rule stays the ADR, and each document names it.

Repository: `stophammer`. The operator commits. This task starts after the commit
of task 003.

## Goal

`docs/API.md`, `docs/schema-reference.md` and `docs/operations.md` describe the
behavior that task 003 delivers.

## Context

`docs/API.md` near line 376 says that `source_conflict` writes no row to the
database. A mirror body writes a `feed_copies` row (ADR 0058), so this text is
already incorrect. `docs/API.md` near line 430 says that only an update and a new
feed write the two declaration values. A move now writes them too.

## Files To Inspect

- `docs/API.md`: the reasons of an ingest answer and the move section
- `docs/schema-reference.md`: `feed_url_observations` and `feeds`
- `docs/operations.md`: the ingest troubleshooting sections
- `src/api.rs`: the warning text of task 003

## Files Likely To Change

- `docs/API.md`
- `docs/schema-reference.md`
- `docs/operations.md`

## Constraints

- **`docs/API.md`.** Correct the `source_conflict` text. In the move section, add
  the refusal, the warning text and the point that a chain moves one hop in each
  pass. State that a move writes the two values.
- **`docs/schema-reference.md`.** Add `feed_left_urls`. State that it is local to
  the primary, and that an operator relocation clears it.
- **`docs/operations.md`.** Add a section for the warning. It tells the operator
  what the warning means, and gives the two ways to end a refusal: the publisher
  corrects the feed, or the operator relocates the record with
  `PATCH /v1/feeds/{guid}`.
- **`api.html`.** It does not change, because the API surface does not change.
- **Rules.** Each text names ADR 0083. No text creates a rule.
- **Git.** The agent runs no git write command.

## Do Not Touch

- `src/`, `tests/`, `migrations/`
- `docs/adr/`

## Implementation Steps

1. Change the three documents.
2. Run the STE checker on the changed text, and correct each confirmed error.

## Acceptance Criteria

Mechanical:

- `grep -c 'ADR 0083' docs/API.md docs/schema-reference.md docs/operations.md`
  gives at least 1 for each file.
- `grep -n 'write no row' docs/API.md` no longer gives the `source_conflict` text.
- The warning text in `docs/API.md` agrees with the text in `src/api.rs`.

This task has no visual criterion.

## Test Commands

```bash
cd ~/build/stophammer
grep -c 'ADR 0083' docs/API.md docs/schema-reference.md docs/operations.md
grep -n 'write no row' docs/API.md
grep -n 'ADR 0083)' docs/API.md src/api.rs
python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --no-heuristics docs/operations.md
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- A document states a rule of ADR 0052 that ADR 0083 did not amend, and the rule
  disagrees with the code.

## Expected Final Report

1. files changed
2. the result of each check
3. deviations from this task
4. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/docs/adr/0083-a-feed-move-refuses-a-loop.md`
- `~/build/stophammer/docs/tasks/adr-0083-task-004-documents.md` (this task)
- the sections of the three documents that the task names. Use a search.

Goal:
- The three reference documents describe the refusal, the warning, the table and the two ways to end a refusal.

Constraints:
- Follow the section "Constraints" of this task.
- Write in Simplified Technical English, with the shared skill `~/.agents/skills/asd-ste100/SKILL.md`.

Do not touch:
- `src/`, `tests/`, `migrations/`, `docs/adr/`
- git: run no git write command

Acceptance criteria:
- Each mechanical item of the section "Acceptance Criteria" of this task passes.

At the end, report:
1. files changed
2. the result of each check
3. deviations from this task
4. unresolved concerns
