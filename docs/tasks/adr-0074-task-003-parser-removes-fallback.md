# ADR 0074 Task 003: The Parser Removes The Fallback GUID

Owner: [ADR 0074](../adr/0074-a-feed-without-a-guid-is-invalid.md) §2. The
phase plan is
[the compression, splits and GUID plan](../plans/compression-splits-guid-phase-plan.md).

Repository: `stophammer-parser`. The operator commits. ADR 0074 task 002 is
complete, so the crawler uses no fallback function.

## Goal

The parser has no fallback GUID. A feed with no `podcast:guid` is always a
parse error.

## Files To Inspect

- `src/profile.rs`: `stophammer_with_fallback`, `stophammer_with_phases`,
  `stophammer_phases_only`
- `src/engine.rs`: the `fallback_guid` fields of the builder and the parser,
  the method `fallback_guid`, and the fallback step in `parse`
- `src/error.rs`: the doc comment of `NoGuid`
- `src/bin/stophammer-parse.rs`: the argument `--fallback-guid`
- `tests/basic.rs`: `fallback_guid_works`
- `README.md` and `AGENTS.md` of the parser: each text about the fallback
- `../stophammer-crawler/src/`: a search for `fallback` must find no parser call

## Files Likely To Change

- `src/profile.rs`, `src/engine.rs`, `src/error.rs`, `src/lib.rs` (only if it
  exports a removed item)
- `src/bin/stophammer-parse.rs`
- `tests/basic.rs`
- `README.md`, `AGENTS.md` of the parser

## Constraints

- **Removal.** Delete `stophammer_with_fallback`, the method `fallback_guid`,
  the two `fallback_guid` fields, and the fallback step in `parse`. Delete
  `stophammer_with_phases(guid, phases)`. `stophammer_phases_only(phases)`
  stays.
- **The CLI.** Delete `--fallback-guid`. `--phases` uses
  `stophammer_phases_only`. An unknown argument keeps its present behavior.
- **Tests.** Delete `fallback_guid_works`. Keep
  `missing_guid_errors_without_fallback`, and rename it
  `missing_guid_is_an_error`.
- **Docs.** Remove each text about the fallback from the parser `README.md`
  and `AGENTS.md`. Write no history note.

## Do Not Touch

- `ParseError::raw_medium` and `ParseError::has_channel` of task 001
- `extract_value_time_splits` and `src/types.rs`
- `../stophammer-crawler/`, and the `stophammer` repository
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Search the crawler for each removed name. Stop if a call exists.
2. Delete the items, and follow the compile errors in the parser.
3. Change the CLI.
4. Change the tests and the docs.

## Acceptance Criteria

Mechanical.

- `grep -rn "fallback" src tests` in the parser finds no fallback GUID code.
- `tests/basic.rs`: `missing_guid_is_an_error` passes.
- `stophammer-parse --phases rss2core < tests/fixtures/no_guid.xml` exits with
  an error and prints the text `missing podcast:guid`.
- The crawler builds, and its tests pass, against the changed parser.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-parser
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin stophammer-parse -- --phases rss2core < tests/fixtures/no_guid.xml
cd ../stophammer-crawler && cargo build && cargo test
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- The crawler or another repository calls a function that this task removes.
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
- `~/build/stophammer/stophammer-parser/AGENTS.md`
- `~/build/stophammer/docs/adr/0074-a-feed-without-a-guid-is-invalid.md`, §2
- `~/build/stophammer/docs/tasks/adr-0074-task-003-parser-removes-fallback.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- The parser has no fallback GUID. The functions, the fields, the CLI argument and the fallback test are deleted.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Check first that the crawler calls no function that you delete.
- Stop at each escalation trigger of this task.

Do not touch:
- `ParseError::raw_medium`, `ParseError::has_channel`, `extract_value_time_splits`, `src/types.rs`
- `../stophammer-crawler/`, the `stophammer` repository
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is met.

Test commands:
- `cargo build`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cargo run --bin stophammer-parse -- --phases rss2core < tests/fixtures/no_guid.xml`
- `cd ../stophammer-crawler && cargo build && cargo test`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
