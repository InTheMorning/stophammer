# ADR 0064 Task 005: The View Flags On Each Row

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) §6 and §7, as
amended on 2026-10-01. It joins release 0.3.0.

Repository: `stophammer`. The operator commits.

## Goal

Each live row gives `in_now_view` and `in_upcoming_view`, in
`GET /v1/live-items` and in `live_items` of `GET /v1/feeds/{guid}`. Each one is
true exactly when the view of the same name gives the row at the time of the
read.

## Files To Inspect

- `src/live.rs`: `in_now_view`, `in_upcoming_view`, `is_confirming_relay`,
  `confirming_relay_hosts`
- `src/query.rs`: `LiveItemResponse`, `live_item_response`,
  `build_live_items_response`, and `LiveItemListResponse` with its builder.
  Also the function near line 3054 that takes `now` and `hosts`, and the view
  filter near line 2983.
- `src/openapi.rs`: only when the schema does not follow the struct
- `tests/adr0064_live_list_tests.rs`, `tests/adr0064_live_feed_read_tests.rs`:
  the helpers and the case table of the views
- `docs/API.md`: the live item fields

## Files Likely To Change

- `src/query.rs`, `docs/API.md`
- `tests/adr0064_view_flags_tests.rs`, new

## Do Not Touch

- `src/live.rs`. The rules do not change.
- `status` and each other field of a row.
- The ingest, the storage, the events.

## Constraints

- Add `in_now_view: bool` and `in_upcoming_view: bool` to
  `LiveItemResponse` and to `LiveItemListResponse`, after `confirming_relay`.
  Each has a doc comment that names ADR 0064 §6.
- Compute them only with `live::in_now_view` and `live::in_upcoming_view`.
  Pass the same `confirming` value that `confirming_relay` gives. Write no
  second form of a rule.
- The list route computes them with the `now` that it already uses for the
  view, so the flags and the view agree in one read.
- `build_live_items_response` takes `now: i64`. The feed read reads the clock
  one time, with `db::unix_now()`, and passes it.
- `docs/API.md` names the two fields, and says that `status` stays the value
  of the feed.

## Acceptance

Mechanical, each an integration test in `tests/adr0064_view_flags_tests.rs`:

- Through the list function with a fixed `now`, for each row of the case table
  of the existing view tests: `in_now_view` is true exactly when `view=now`
  gives the row, and `in_upcoming_view` is true exactly when `view=upcoming`
  gives it.
- A `live` row with no relay and an end 2 hours before `now` gives
  `status: "live"`, `in_now_view: false` and `in_upcoming_view: false`.
- Through `GET /v1/feeds/{guid}`, with row times set relative to the real
  time: a `live` row with an end 2 hours ago gives `in_now_view: false`, and a
  `live` row with an end 1 hour ahead gives `in_now_view: true`.
- The guards of ADR 0044 pass.
- The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --test adr0064_view_flags_tests
cargo test --test adr0044_schema_refs_tests
cargo test --test adr0044_contract_guard_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report without a workaround when:

- A view gives a row by a rule that is not in `src/live.rs`.
- The two response types cannot share the computation without a change to
  `src/live.rs`.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0064-task-005-view-flags-on-each-row.md
- /home/citizen/build/stophammer/docs/adr/0064-a-live-item-is-an-rss-fact.md (sections 6 and 7)
- /home/citizen/build/stophammer/AGENTS.md (Code Style, Tests)
- Only the parts of src/query.rs named in the task file. Use grep. The file is long.

Goal:
- Each live row gives in_now_view and in_upcoming_view, computed with the functions of src/live.rs, in the list route and in the feed read.

Constraints:
- The rules under "Constraints" in the task file.
- Write any scratch file under target/, not /tmp.

Do not touch:
- src/live.rs, the other row fields, the ingest, the storage, the events, stophammer-crawler/, stophammer-parser/.
- Git: run no git command that writes (no add, commit, stash, checkout, reset, push).

Acceptance criteria:
- The criteria under "Acceptance" in the task file.

Test commands:
- The commands under "Test Commands" in the task file.

At the end, report:
1. files changed
2. the real output of each gate command
3. behavior changed
4. deviations from task
5. unresolved concerns
