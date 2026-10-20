# ADR 0073 Task 002: The Track Response Gives Both Forms

Owner: [ADR 0073](../adr/0073-a-value-time-split-keeps-both-forms.md) §4. The
phase plan is
[the compression, splits and GUID plan](../plans/compression-splits-guid-phase-plan.md).

Repository: `stophammer`. The operator commits. Task 001 is complete.

## Goal

`include=value_time_splits` keeps each `v1` field with its type and adds the
stated values. The new include `local_value_time_splits` gives the local form.

## Files To Inspect

- `src/query.rs`: `VtsResponse`, `RouteResponse`, `TrackResponse`,
  `TRACK_INCLUDES`, the include handling of `handle_get_track` and
  `handle_get_feed_track`
- `src/openapi.rs`: the track routes, their `include` parameter, and the
  examples with `"value_time_splits": []`
- `tests/adr0044_schema_refs_tests.rs`, `tests/adr0044_contract_guard_tests.rs`,
  `tests/adr0071_row_link_guard_tests.rs`: the contract guards
- `docs/API.md`: the track routes and the `value_time_splits` example

## Files Likely To Change

- `src/query.rs`
- `src/openapi.rs`
- `docs/API.md`
- `tests/adr0073_split_response_tests.rs`, new

## Constraints

- **The remote list.** `value_time_splits` gives only remote-form splits, in
  `position` order. The present fields keep their names and types:

  | Field | Value |
  |---|---|
  | `start_time_secs` (integer) | The stated value rounded down |
  | `duration_secs` (integer or null) | The stated value rounded down |
  | `remote_feed_guid` (string) | The stated value |
  | `remote_item_guid` (string) | The stated value, or `""` when none |
  | `split` (integer) | The stated `remote_percentage` limited to 0 to 100 and rounded down, or 100 when null |

- **The added fields** on each remote row: `start_time` (number),
  `duration` (number or null), `remote_start_time` (number or null) and
  `remote_percentage` (number or null). Each gives the stored value. A null
  means that the source states none.
- **The local include.** Add `local_value_time_splits` to `TRACK_INCLUDES`. It
  gives `Vec<LocalVtsResponse>`, in `position` order. Each row has `position`,
  `start_time`, `duration` and `recipients: Vec<RouteResponse>`, in source
  order.
- **The contract.** Register `LocalVtsResponse` and declare each new field and
  the new include in `src/openapi.rs`, so the guards of ADR 0044 and ADR 0071
  §1 pass.
- **The docs.** `docs/API.md` gives the new fields, the new include, and an
  example of each form.
- **Comments.** Each new item names ADR 0073 §4.

## Orchestrator Decision Of 2026-10-20

The first run stopped at the trigger "a guard needs a change to its own rule".
`each_example_is_unchanged` in `tests/adr0044_schema_refs_tests.rs` compares
each example with the snapshot of ADR 0044 task 002. The example of
`GET /v1/node/capabilities` reads `TRACK_INCLUDES`, so the new include changed
it. The orchestrator decided: the guard removes `include_params` from that one
example before the comparison. The snapshot file does not change.

## Do Not Touch

- The store, the ingest, the event and the apply code of task 001
- The other includes and the other routes
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Change `VtsResponse` and its mapping.
2. Add `LocalVtsResponse` and the include.
3. Change the contract in `src/openapi.rs`, and run `cargo run --bin
   gen_openapi` to check it.
4. Change `docs/API.md`.
5. Write the tests.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0073_split_response_tests.rs`, unless
the item names a guard.

- A remote split with `start_time_secs` 60.7, `remote_percentage` 95 and no
  `remote_item_guid` gives `start_time_secs` 60, `start_time` 60.7, `split` 95,
  `remote_percentage` 95 and `remote_item_guid` `""`.
- A remote split with no `remote_percentage` gives `split` 100 and
  `remote_percentage` null.
- A remote split with `remote_percentage` 150 gives `split` 100 and
  `remote_percentage` 150. The value -5 gives `split` 0.
- A local split is not in `value_time_splits`.
- `include=local_value_time_splits` gives the local split with its recipients,
  in source order.
- A track with no split gives `[]` for each include.
- The ADR 0044 and ADR 0071 guards pass.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo run --bin gen_openapi > /dev/null
cargo test --test adr0073_split_response_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A guard needs a change to its own rule to pass.
- `handle_get_feed_track` and `handle_get_track` use different include lists.
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
- `~/build/stophammer/AGENTS.md` (the steps for an API change and the pre-commit gate)
- `~/build/stophammer/docs/adr/0073-a-value-time-split-keeps-both-forms.md`, §4
- `~/build/stophammer/docs/adr/0044-api-contract-declares-its-fields.md`
- `~/build/stophammer/docs/tasks/adr-0073-task-002-track-response-gives-both-forms.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- `include=value_time_splits` keeps each `v1` field and adds the stated values. `include=local_value_time_splits` gives the local form.

Constraints:
- Follow the table and the sections "Constraints" and "Implementation Steps" of this task.
- No `v1` field changes its name or its type.
- Stop at each escalation trigger of this task.

Do not touch:
- the store, ingest, event and apply code, the other includes and routes
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo run --bin gen_openapi > /dev/null`
- `cargo test --test adr0073_split_response_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
