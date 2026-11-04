# ADR 0083 Task 003: A Move Refuses A Loop

Owner: [ADR 0083](../adr/0083-a-feed-move-refuses-a-loop.md) §2, §3, §4, §5 and
§6.

Repository: `stophammer`. The operator commits. This task starts after the commit
of task 002.

## Goal

`move_target` applies the refusal rule. A move writes the two declaration columns
and updates the left URLs. A refused body gets `source_conflict` with a warning.
Each loop scenario stops, and each chain scenario completes.

## Context

`move_target` in `src/api.rs` is the one test for a move. The read phase calls it
to set `force_reingest`. The write phase calls it in the `Update` case and in the
`Mirror` case. `ingest::IngestFeedData` holds `new_feed_url` and `links`. A self
link is the first link of the type `self_feed`.

The `Mirror` case with no target returns `source_conflict` with
`warnings: vec![]`. After `ingest_transaction`, the code writes the declarations
only when `record_move` is `None`. Its comment gives the reason for the current
rule, and that comment must change.

`.idea/research/feed-move-smaller-rule-patch.py` is the scratch patch that gave
the results in the ADR. It is not reviewed code, and it creates its table on use.
Use it as a reference for the behavior only.

## Files To Inspect

- `src/api.rs`: `MoveTarget`, `move_target`, its three callers, the `Mirror` case
  of `handle_ingest_feed`, and step 11a
- `tests/adr0052_move_trigger_tests.rs`, for the test fixture
- `.idea/research/feed-move-extra-scenarios.rs`

## Files Likely To Change

- `src/api.rs`
- New: `tests/adr0083_move_refusal_tests.rs`

## Constraints

- **The result type.** `move_target` takes the body and returns an enum with three
  values: no trigger, a move with its `MoveTarget`, and a refusal with the held
  URL that the body names. The read phase sets `force_reingest` only for a move.
- **The sequence.** The function finds the trigger as today. Only then does it
  apply ADR 0083 §2, in the sequence of the four rules. A submission with no
  trigger never gets the refusal warning.
- **Exact comparison.** Each comparison is exact string equality (ADR 0083 §8).
- **The redirect.** The `Update` case applies no rule of §2 (§5).
- **The declarations.** Step 11a writes the columns for an update, a new feed and
  a move. A mirror body with no move returns before this step, and it writes
  neither column.
- **The left URLs.** At step 11a, a move calls `add_left_url` with the old URL and
  `remove_left_url` with the new URL.
- **The warning.** The text is
  `move to <URL> refused: the body names <held URL> (ADR 0083)`. A `tracing::warn!`
  gives the GUID and the two URLs as structured fields.
- **No new field.** `IngestResponse`, `src/openapi.rs` and
  `tests/fixtures/v1-schema-fields.json` do not change.
- **Git.** The agent runs no git write command.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `migrations/`, `src/schema.sql`
- `docs/`
- Each assertion of an existing test. The scratch patch passes the full test set
  with no change. A failure here is a defect of the change or an escalation.

## Implementation Steps

1. Add the enum and the rule, and update the three callers.
2. Change step 11a and its comment, and the doc comment of `move_target`.
3. Add the warning in the `Mirror` case.
4. Write the tests, and run the gate.

## Acceptance Criteria

Mechanical, in `tests/adr0083_move_refusal_tests.rs`. Each test uses a new body
hash for each submission, unless the row says otherwise. A loop test sends each
body at least three times.

| Test | Bodies | Result |
|---|---|---|
| S3 | The tags of A and B name each other | 0 moves, at A |
| S3b | The same with one hash for each URL | 0 moves, at A |
| S10 | The self links of A and B name each other | 1 move, at B |
| S11 | The tag at A names B, the self link at B names A | 1 move, at B |
| S12 | The tags of A, B and X name the next URL | 1 move, at B |
| Three self links | The self links of A, B and X name the next URL | 2 moves, at X |
| S7 | A names B, B names B | 1 move, at B |
| S14 | A names B, B has no tag and no self link | 1 move, at B |
| S15 | The tag at B names B, the self link at B names A | 1 move, at B |
| S13 | A names B, B names X, X names X | 2 moves, at X |
| S16 | A chain of three hops | 3 moves, at the last URL |
| S20 | A chain with one hash for each URL | 2 moves, at X |
| S21 | B names X, and X is the source URL of a different record | 1 move, at B, and `record_conflict` at X |
| S23 | A move to B, B names A, A names A | 2 moves, at A |
| R1 | B names X, X is never served, B then names B | 1 move, at B |
| R3 | A relocation to Y during a chain, X then names X | At Y |

Also:

- A move writes the two columns from the body at the new URL.
- A mirror body with no move changes neither column.
- A `301` moves the record when the body at the new URL names the old URL by tag.
  The old URL is then a left URL.
- A refused body gets `source_conflict`, the warning, and a `feed_copies` row.
- A mirror body at a URL that no value names gets `source_conflict` and no warning.
- A return is refused when its self link names a held URL, and it moves when its
  tag names its own URL.

Mechanical, in the crate:

- `cargo test`, `cargo clippy --all-targets -- -D warnings` and
  `cargo fmt -- --check` pass, with no change to an existing assertion.

This task has no visual criterion.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0083_move_refusal_tests
cargo test --test adr0052_move_trigger_tests
cargo test --test adr0052_self_link_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- A result differs from the table.
- An existing assertion fails and the fix needs a change to it.
- The rule needs a new event, a new response field or a crawler change.

## Expected Final Report

1. files changed and created
2. tests run, with the result of the gate
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/docs/adr/0083-a-feed-move-refuses-a-loop.md`, each section
- `~/build/stophammer/docs/tasks/adr-0083-task-003-refusal-rule.md` (this task)
- `~/build/stophammer/src/api.rs`: `move_target`, its callers and step 11a. Use a search. Do not read the whole file.
- `~/build/stophammer/tests/adr0052_move_trigger_tests.rs`, for the fixture
- `~/build/stophammer/.idea/research/feed-move-smaller-rule-patch.py` and `feed-move-extra-scenarios.rs`, for the behavior only

Goal:
- `move_target` applies ADR 0083 §2. A move writes the declarations and the left URLs. A refused body gets `source_conflict` and the warning.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Write each comment in Simplified Technical English, with the shared skill `~/.agents/skills/asd-ste100/SKILL.md`.
- Use the `rust-dev` skill.

Do not touch:
- `stophammer-crawler/`, `stophammer-parser/`, `migrations/`, `src/schema.sql`, `docs/`
- each assertion of an existing test
- git: run no git write command

Acceptance criteria:
- Each mechanical item of the section "Acceptance Criteria" of this task passes.

Test commands:
- the block of the section "Test Commands" of this task

At the end, report:
1. files changed and created
2. tests run
3. behavior changed
4. deviations from this task
5. unresolved concerns
