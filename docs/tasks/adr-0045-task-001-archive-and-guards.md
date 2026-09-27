# ADR 0045 Task 001: The ADR Archive And The Guards

Owner: [ADR 0045](../adr/0045-governance-model-and-contract-ownership.md) §6
and "Guards". Plan: item 14 of the [remaining accepted work
plan](../plans/remaining-accepted-work-plan.md).

Repository: `stophammer`. The operator commits.

## Goal

Each superseded ADR is in `docs/adr/archive/`, and each link to it is correct.
Two tests guard the governance model: `AGENTS.md` is tracked, and the ADR files
agree with the rows of `docs/adr/README.md`.

## Files To Inspect

- `docs/adr/README.md`: the section "Superseded" names the ADRs to move
- Each file that links to one of those ADRs. Find them with
  `git grep -n -e '<file name>'` for each file name
- `tests/`: an existing test that reads a file of the repository, for the
  path form (`env!("CARGO_MANIFEST_DIR")`)

## Files Likely To Change

- `docs/adr/archive/`, new, with the moved files
- `docs/adr/README.md`, and each document that links to a moved ADR
- `tests/adr0045_governance_guards_tests.rs`, new

## Do Not Touch

- The text of a moved ADR, other than its relative links to other ADRs, which
  gain `../`.
- `src/`, migrations, `stophammer-crawler/`, `stophammer-parser/`.

## Constraints

- Move exactly the ten ADRs of the section "Superseded": 0007, 0010, 0012,
  0013, 0014, 0018, 0020, 0021, 0029 and 0035. Use `mv`, not a git command.
  The operator stages the move.
- In each moved file, a link to another ADR in `docs/adr/` gains `../`. A link
  between two moved files stays as it is.
- In each other file, a link to a moved ADR gains `archive/` in its path.
- The section "Superseded" of the index links into `archive/`, and its first
  sentence says that the files are in `docs/adr/archive/`.
- Test 1 runs `git ls-files AGENTS.md` in the repository root and fails when
  the output is empty. When git is not available, the test fails with a clear
  message.
- Test 2 lists the `.md` files in `docs/adr/` and `docs/adr/archive/`, except
  `README.md` and `templates/`. It reads each link target of the form
  `NNNN-…md` or `archive/NNNN-…md` from the index. It fails when a file has no
  row, or when a row names no file. A file can have more than one row.
- Each failure message names ADR 0045 and the fix, in the form:
  `ADR 0045 §Guards: docs/adr/0070-x.md has no row in docs/adr/README.md. Add a row to the index.`

## Steps

1. Create `docs/adr/archive/` and move the ten files.
2. Correct the links in the moved files, the index and each other document.
3. Write the two tests.
4. Run the gate, and check that no link to a moved file is left:
   `git grep -n -E '\]\((\.\./adr/|docs/adr/)?00(07|10|12|13|14|18|20|21|29|35)-'`
   must show only links that go through `archive/`.

## Acceptance

Mechanical:

- The two tests pass. Each fails when you make its case false: remove a row
  from the index, or name a missing file.
- The link check of step 4 shows no broken link.
- The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo test --test adr0045_governance_guards_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report without a workaround when:

- An ADR outside the ten names is marked superseded in its own file but not in
  the index.
- A test or a source file reads one of the ten files by path.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0045-task-001-archive-and-guards.md
- /home/citizen/build/stophammer/docs/adr/README.md
- /home/citizen/build/stophammer/AGENTS.md (Tests)

Goal:
- Move the ten superseded ADRs to docs/adr/archive/, correct every link to them, and add the two governance guard tests.

Constraints:
- The rules under "Constraints" in the task file.
- Use mv for the files. The operator stages the move.

Do not touch:
- src/, migrations, stophammer-crawler/, stophammer-parser/.
- The text of a moved ADR other than its relative links.
- Git: run no git command that writes (no add, commit, mv, rm, stash, checkout, reset, push). Read-only git (grep, ls-files, status) is fine.

Acceptance criteria:
- The criteria under "Acceptance" in the task file.

Test commands:
- The commands under "Test Commands" in the task file.

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
