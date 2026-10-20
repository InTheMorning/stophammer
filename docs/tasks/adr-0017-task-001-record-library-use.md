# ADR 0017 Task 001: Record That The Crawler Uses The Parser As A Library

Owner: [ADR 0017](../adr/0017-canonical-rss-parser-crate.md).
[ADR 0045](../adr/0045-governance-model-and-contract-ownership.md) owns the
index. This task changes no code, and adds no rule. It makes the text of ADR
0017 agree with the code.

Repository: `stophammer`. The operator commits.

## The Defect

ADR 0017 describes TypeScript crawlers that start `stophammer-parse` as a
subprocess. Commit `c02bbae` of 2026-03-12 rewrote the crawler in Rust. Since
then, `stophammer-crawler` calls the parser as a library, through
`stophammer-parser = { path = "../stophammer-parser" }` in its `Cargo.toml`.
ADR 0074 task 003 removed `--fallback-guid` from the CLI. ADR 0011 moved to the
archive on 2026-10-20.

These statements of ADR 0017 are not true now:

| Section | Text that is not true |
|---|---|
| "Single Rust library crate" | the TypeScript crawlers invoke it through its CLI binary |
| "CLI interface" | TypeScript crawlers start it as a subprocess, the `Bun.spawn` example, `--fallback-guid` |
| "Consequences" | "Subprocess overhead", "Crawlers must ship the binary", "ADR 0011 is not superseded", "all three crawlers will share" |

## Goal

ADR 0017 says that the crawler calls the parser as a library, and that the CLI
stays for operators and tests. Each other decision of ADR 0017 stays unchanged.

## Files To Inspect

- `docs/adr/0017-canonical-rss-parser-crate.md`
- `docs/adr/README.md`: the row of ADR 0017, and the row of ADR 0059 for the
  form of an amendment note
- `stophammer-parser/src/bin/stophammer-parse.rs`: the arguments that the CLI
  accepts now

## Files Likely To Change

- `docs/adr/0017-canonical-rss-parser-crate.md`
- `docs/adr/README.md`

## Constraints

Make exactly these edits. Keep each other line of ADR 0017.

1. **Status.** Replace `Accepted` with:

   ```text
   Accepted. Amended on 2026-10-20: the crawler calls the parser as a
   library, and the CLI stays for operators and tests.
   ```

2. **"Single Rust library crate".** Replace the text "it is a separate project
   that the TypeScript crawlers invoke via its CLI binary (`stophammer-parse`)."
   with:

   ```text
   it is a separate project. `stophammer-crawler` calls it as a library,
   through a path dependency. The CLI binary `stophammer-parse` stays for
   operators and tests.
   ```

3. **"CLI interface: `stophammer-parse`".** Replace the paragraph "TypeScript
   crawlers invoke it as a subprocess:" and the `typescript` code block with:

   ```text
   An operator uses it to examine one feed.
   ```

   Delete the bullet of `--fallback-guid`. Keep the bullet of `--phases` and
   the line of the exit codes.

4. **"Consequences".**
   - Replace "all three crawlers will share the same parsing logic" with "each
     crawler mode shares the same parsing logic".
   - Replace the bullets "Subprocess overhead" and "Crawlers must ship the
     binary" with one bullet:

     ```text
     - **No subprocess** — the crawler calls the parser in its own process.
       A parser change needs a new build of the crawler.
     ```

   - Replace the bullet "ADR 0011 is not superseded" with:

     ```text
     - **ADR 0011 is archived** — ADR 0072, ADR 0073 and ADR 0074 supersede
       its last sections in force. ADR 0073 §5 names the owner of each other
       section.
     ```

5. **The index.** In `docs/adr/README.md`, the status cell of ADR 0017 becomes
   `Accepted. Amended on 2026-10-20: the crawler calls the parser as a library`.

- Write no history note. Keep the "Context" section as it is: it records why the
  decision was made.
- Apply the shared checker of the operator to the changed lines:
  `python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --check --no-heuristics docs/adr/0017-canonical-rss-parser-crate.md`.
  Correct only a confirmed error in the text that this task adds.

## Do Not Touch

- Each other ADR, and `docs/adr/archive/`
- The "Context" section of ADR 0017, and each decision that this task does not
  name
- Code in each repository
- `AGENTS.md`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Make the five edits.
2. Run the governance guard and the checker.

## Acceptance Criteria

Mechanical.

- `grep -n "TypeScript crawlers\|Bun.spawn\|fallback-guid\|not superseded\|Subprocess overhead" docs/adr/0017-canonical-rss-parser-crate.md`
  finds nothing outside the "Context" section.
- `cargo test --test adr0045_governance_guards_tests` passes.

Visual. The operator reads the changed sections, and checks that each other
decision of ADR 0017 stays.

## Test Commands

```bash
cd ~/build/stophammer
grep -n "TypeScript crawlers\|Bun.spawn\|fallback-guid\|not superseded\|Subprocess overhead" docs/adr/0017-canonical-rss-parser-crate.md
cargo test --test adr0045_governance_guards_tests
python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --check --no-heuristics docs/adr/0017-canonical-rss-parser-crate.md
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A text that this task replaces is not in the file in the same words.
- Another section of ADR 0017 states a rule that the code does not follow.
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
- `~/build/stophammer/AGENTS.md`
- `~/.agents/skills/asd-ste100/SKILL.md`
- `~/build/stophammer/docs/tasks/adr-0017-task-001-record-library-use.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- ADR 0017 says that the crawler calls the parser as a library, and that the CLI stays for operators and tests. Each other decision stays.

Constraints:
- Make exactly the five edits of the section "Constraints" of this task, with the given text.
- Keep the "Context" section and each other decision.
- Stop at each escalation trigger of this task.

Do not touch:
- each other ADR, `docs/adr/archive/`, code in each repository, `AGENTS.md`
- git: run no git write command

Acceptance criteria:
- Each mechanical item of the section "Acceptance Criteria" of this task is met.

Test commands:
- `grep -n "TypeScript crawlers\|Bun.spawn\|fallback-guid\|not superseded\|Subprocess overhead" docs/adr/0017-canonical-rss-parser-crate.md`
- `cargo test --test adr0045_governance_guards_tests`
- `python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --check --no-heuristics docs/adr/0017-canonical-rss-parser-crate.md`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
