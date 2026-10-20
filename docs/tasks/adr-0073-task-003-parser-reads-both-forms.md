# ADR 0073 Task 003: The Parser Reads Both Forms Of A Split

Owner: [ADR 0073](../adr/0073-a-value-time-split-keeps-both-forms.md) §1. The
phase plan is
[the compression, splits and GUID plan](../plans/compression-splits-guid-phase-plan.md).

Repository: `stophammer-parser`. The operator commits. ADR 0073 task 001 is
complete, so the node accepts the new shape. ADR 0074 task 003 is complete.

## Goal

The parser keeps each `podcast:valueTimeSplit` that the specification permits,
in the remote form and the local form, with fractional times and the stated
attributes. It ignores the non-spec `split` attribute.

## Files To Inspect

- `src/engine.rs`: `extract_value_time_splits`, `extract_payment_routes`
- `src/types.rs`: `IngestValueTimeSplit`, `IngestPaymentRoute`
- `src/lib.rs`: the exports
- `tests/regression.rs`: `vts_reads_remote_item_child`
- `tests/payment.rs`: `extracts_value_time_splits`, and
  `tests/fixtures/payment.xml`
- `tests/phase.rs`: the two split assertions
- `~/build/musicindex-research/sources/captures/2026-10-20-stophammer-claim/tests/claim_value_time_split.rs`:
  the six tests of the research request
- `~/build/musicindex-research/sources/captures/2026-10-20-stophammer-claim/podhome-satellite-skirmish.xml`
- `~/build/stophammer/src/ingest.rs`: the node `IngestValueTimeSplit` after
  task 001. The JSON of the two types must agree.

## Files Likely To Change

- `src/engine.rs`, `src/types.rs`, `src/lib.rs`
- `tests/value_time_split.rs`, new, from the six research tests
- `tests/fixtures/podhome_satellite_skirmish.xml`, new
- `tests/regression.rs`, `tests/payment.rs`, `tests/fixtures/payment.xml`: only
  where they assert the `split` field

## Constraints

- **The type.** `IngestValueTimeSplit` gets the fields of the node DTO, with
  the same JSON names: `form` (a new public enum `ValueTimeSplitForm`,
  `Remote` and `Local`, snake case), `start_time_secs: f64`,
  `duration_secs: Option<f64>`, `remote_feed_guid: Option<String>`,
  `remote_item_guid: Option<String>`, `remote_start_time_secs: Option<f64>`,
  `remote_percentage: Option<f64>`, `recipients: Vec<IngestPaymentRoute>`.
  Remove `split`.
- **The rules.** Apply ADR 0073 §1 in this order:
  1. `startTime` must parse as a finite `f64` of 0 or more. Else drop.
  2. Count the `podcast:remoteItem` and `podcast:valueRecipient` children.
     Children of both kinds, two or more `remoteItem`, or no child: drop.
  3. Remote form: `feedGuid` must be non-empty after trim. Else drop.
     `itemGuid` is optional.
  4. Local form: read each recipient with the same code as a track payment
     route. Move the recipient code of `extract_payment_routes` into one
     function, and call it from both places. If no recipient stays, drop.
  5. `remotePercentage`: absent gives `None`. A value that does not parse as a
     finite `f64` drops the split. Store the stated number with no limit.
  6. `duration` and `remoteStartTime`: a value that does not parse as a finite
     `f64` gives `None`.
- **No `split`.** The parser does not read a `split` attribute on the element.
- **Comments.** The function doc names stophammer ADR 0073 §1, and replaces the
  doc that names only `podcast:remoteItem` children.

## Do Not Touch

- The rest of `extract_payment_routes`. The track payment routes must not
  change.
- `ParseError`, `profile.rs`, `src/bin/stophammer-parse.rs`
- `../stophammer-crawler/`, and the `stophammer` repository
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Change the type and the export.
2. Move the recipient code into one function, with no change of behavior.
3. Rewrite `extract_value_time_splits` with the rules.
4. Copy the six research tests to `tests/value_time_split.rs`, and the fixture.
   Change their assertions to this model. See the acceptance criteria.
5. Change the old tests that assert `split`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/value_time_split.rs`, unless it names a
file.

- `spec_remote_example_keeps_both_splits`: 2 remote splits, with
  `remote_percentage` `Some(95.0)`, and the second with
  `remote_start_time_secs` `Some(174.0)`.
- `spec_local_example_keeps_both_splits`: 2 local splits, each with 2
  recipients, with splits 85 and 10.
- `fractional_start_time_is_kept`: 2 splits, and one has `start_time_secs`
  12.5.
- `absent_remote_percentage_means_100` becomes
  `absent_remote_percentage_is_none`: each split gives `None`. The response of
  task 002 applies the default.
- `podhome_feed_keeps_its_three_splits`: 3 remote splits with
  `remote_percentage` `Some(95.0)`.
- `control_non_spec_split_attribute_parses` becomes
  `non_spec_split_attribute_is_ignored`: the split stays, with
  `remote_percentage` `None`.
- New: a split with a `remoteItem` and a `valueRecipient` is dropped.
- New: a split with `remotePercentage="abc"` is dropped.
- New: a remote split with no `itemGuid` is kept, with `remote_item_guid`
  `None`.
- `tests/payment.rs`: the track payment routes of `payment.xml` do not change.
- The JSON of one remote split and one local split deserializes into the node
  type `stophammer::ingest::IngestValueTimeSplit`. Check this by hand one time,
  and give the result in the report. The parser has no dependency on the node.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-parser
cargo build
cargo test --test value_time_split
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cd ../stophammer-crawler && cargo build && cargo test
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- The node DTO of task 001 has a different field name or type.
- A track payment route changes after the move of the recipient code.
- The crawler reads a field of `IngestValueTimeSplit`.
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
- `~/build/stophammer/docs/adr/0073-a-value-time-split-keeps-both-forms.md`, §1
- `~/build/stophammer/docs/tasks/adr-0073-task-003-parser-reads-both-forms.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- The parser keeps each value time split that the specification permits, in both forms, with fractional times and the stated attributes, and ignores `split`.

Constraints:
- Follow the rules in the order of the section "Constraints" of this task.
- One function reads a recipient for a track route and for a local split.
- The JSON names agree with the node type `IngestValueTimeSplit`.
- Stop at each escalation trigger of this task.

Do not touch:
- the rest of `extract_payment_routes`, `ParseError`, `profile.rs`, `src/bin/stophammer-parse.rs`
- `../stophammer-crawler/`, the `stophammer` repository
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is met.

Test commands:
- `cargo build`
- `cargo test --test value_time_split`
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
