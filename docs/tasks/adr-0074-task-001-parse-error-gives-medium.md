# ADR 0074 Task 001: A Parse Error Gives The Channel Medium

Owner: [ADR 0074](../adr/0074-a-feed-without-a-guid-is-invalid.md) §3a. The
phase plan is
[the compression, splits and GUID plan](../plans/compression-splits-guid-phase-plan.md).

Repository: `stophammer-parser`. The operator commits.

## Goal

A `ParseError` for a missing `<title>` or a missing `podcast:guid` gives the
`raw_medium` of the channel. The change adds to the public API, and removes
nothing. Task 003 removes the fallback.

## Files To Inspect

- `src/error.rs`: `ParseError`, `ErrorKind`, `is_missing_field`, `Display`
- `src/engine.rs`: `FeedParser::parse`, the validation of the title and the GUID,
  and the `raw_medium` field of the feed builder
- `tests/basic.rs`: `missing_guid_errors_without_fallback` and the test before
  it
- `tests/fixtures/no_guid.xml`

## Files Likely To Change

- `src/error.rs`
- `src/engine.rs`
- `tests/basic.rs`
- `tests/fixtures/`: new fixtures, if necessary

## Constraints

- **The kinds.** `ErrorKind::NoTitle` and `ErrorKind::NoGuid` each get a field
  `raw_medium: Option<String>`. `parse` fills it from the feed builder.
  `ErrorKind::NoChannel` and `ErrorKind::Xml` do not change.
- **The accessors.** Add two public methods to `ParseError`, each with a doc
  comment that names stophammer ADR 0074 §3a:
  - `pub fn raw_medium(&self) -> Option<&str>`: the value for `NoTitle` and
    `NoGuid`, and `None` for the other kinds.
  - `pub fn has_channel(&self) -> bool`: `true` for `NoTitle` and `NoGuid`.
- **The text.** `Display` for `NoGuid` gives `missing podcast:guid`. `Display`
  for `NoTitle` stays `missing <title> element`. The crawler matches these two
  texts at the start of a reason.
- **The value.** `raw_medium` is the same text that a successful parse gives
  in `IngestFeedData::raw_medium` for the same channel.

## Do Not Touch

- `fallback_guid`, `profile.rs`, `src/bin/stophammer-parse.rs`. Task 003 removes
  the fallback.
- `extract_value_time_splits` and `src/types.rs`. ADR 0073 task 003 changes
  them.
- `../stophammer-crawler/`, and the `stophammer` repository
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add the field to the two kinds, and fill it in `parse`.
2. Add the two accessors, and change the `NoGuid` text.
3. Write the tests.

## Acceptance Criteria

Mechanical. Each is a test in `tests/basic.rs`.

- A channel with `podcast:medium` `music` and no `podcast:guid` gives an error
  with `raw_medium() == Some("music")`, `has_channel() == true`, and the text
  `missing podcast:guid`.
- A channel with `podcast:medium` `podcast` and no `<title>` gives
  `raw_medium() == Some("podcast")` and the text `missing <title> element`.
- A channel with no `podcast:medium` and no `podcast:guid` gives
  `raw_medium() == None` and `has_channel() == true`.
- A document with no `<channel>` gives `has_channel() == false`.
- Malformed XML gives `has_channel() == false` and `raw_medium() == None`.
- Each existing parser test passes with no change.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-parser
cargo build
cargo test --test basic
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cd ../stophammer-crawler && cargo build && cargo test
```

The crawler commands check that the crawler still builds against the parser.

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- The feed builder applies the medium rule after the title or GUID check, so
  the value is not available at the error.
- The crawler does not build after the change.
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
- `~/build/stophammer/docs/adr/0074-a-feed-without-a-guid-is-invalid.md`, §3a
- `~/build/stophammer/docs/tasks/adr-0074-task-001-parse-error-gives-medium.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- A parse error for a missing `<title>` or `podcast:guid` gives the channel `raw_medium` through `ParseError::raw_medium()`, and `ParseError::has_channel()` tells if a channel was found.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Add to the public API. Remove nothing.
- Stop at each escalation trigger of this task.

Do not touch:
- `fallback_guid`, `profile.rs`, `src/bin/stophammer-parse.rs`, `extract_value_time_splits`, `src/types.rs`
- `../stophammer-crawler/`, the `stophammer` repository
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test basic`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cd ../stophammer-crawler && cargo build && cargo test`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
