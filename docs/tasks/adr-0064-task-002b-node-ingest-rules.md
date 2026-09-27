# ADR 0064 Task 002b: The Ingest Rules Of Live Items

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) sections 1 and 4.
Plan: [ADR 0064 phase plan](../plans/adr-0064-live-items-phase-plan.md).

Repository: `stophammer`. The operator commits.

## Goal

A new module, `src/live.rs`, holds the ingest rules as pure functions. The
ingest keeps `pending`, `live` and `ended` rows. It drops a `live` item that
section 4 bans, and applies the two caps. It warns about a non-web link. It
makes no track from a live item. `live_event_ended` fires when a row changes to
`ended`.

## Files To Inspect

- `src/api.rs`, only these parts:
  - `let live_items: &[ingest::IngestLiveItemData]` near `is_musicl`
  - `let pub_dates: Vec<i64>` and its `.chain(live_items…)`
  - `let live_events: Vec<model::LiveEvent>` and its `.filter(…)`
  - the loop `for live_item in live_items` that builds a track from an
    `ended` item (search `eq_ignore_ascii_case("ended")`)
  - `non_web_url_warnings`, `is_non_web`
  - `build_live_sse_frames_for_feed` and `live_sse_payload`
- `src/lib.rs`: the module list
- `src/ingest.rs`: `IngestLiveItemData`
- `docs/plans/adr-0064-live-item-cases.md`: rows L7, E1, E2, A1 to A3
- Task 002 as merged

## Files Likely To Change

- `src/live.rs`, new
- `src/lib.rs`, `src/api.rs`
- `AGENTS.md`: the module table (add `live`)
- `tests/adr0064_live_ingest_tests.rs`, new

## Do Not Touch

- `src/query.rs`, `src/openapi.rs`. Task 003 changes them.
- The track path of a normal `<item>`.
- `src/db.rs` other than a call that the new rules need.
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`.

## Constraints

- `src/live.rs` has a module doc comment, and pure functions with no database
  access and no clock access. The ingest passes each value in.
- The functions, as a minimum:
  - `has_relay_link(item) -> bool`: `live_value_uri` is present and not empty.
  - `is_banned_live_item(item) -> bool`: `status` is `live`, no relay link, and
    no `start_at`, no `end_at`, or `end_at - start_at > 6 * 3600`.
  - `select_live_items(items) -> (kept, warnings)` applies these steps in
    sequence:
    1. Keep the first item of each `live_item_guid`.
    2. Drop each banned item.
    3. Keep the first 10 `pending` and `live` items in RSS order.
    4. Keep the 10 `ended` items with the newest `start_at`. An item with no
       `start_at` sorts last.

    Each dropped item gives one warning string that names the rule.
- Constants: `MAX_ACTIVE_LIVE_ROWS = 10`, `MAX_ENDED_LIVE_ROWS = 10`,
  `MAX_UNRELAYED_LIVE_SECS = 6 * 3600`. Each has a doc comment that names
  ADR 0064 §4.
- The ingest keeps `ended` rows. Remove the filter to `pending` and `live`.
- Remove the loop that makes a track, payment routes and value time splits
  from an `ended` item. Remove live items from the `pub_dates` chain.
- A `musicL` feed keeps giving no live row.
- `non_web_url_warnings` also checks `content_link`, and `live_value_uri` when
  it starts with a scheme. An identifier-only `uri` gives no warning.
- `live_event_ended` fires when a row that was `pending` or `live` is now
  `ended`, or when it leaves the snapshot. It no longer needs a track.

## Steps

1. Write `src/live.rs` with unit tests. Register it in `src/lib.rs` and in the
   module table of `AGENTS.md`.
2. Call `select_live_items` in the ingest. Add its warnings to the ingest
   warnings.
3. Remove the filter, the promotion loop and the `pub_dates` chain entry.
4. Extend `non_web_url_warnings`.
5. Change the trigger of `live_event_ended`.
6. Write the integration tests.

## Acceptance

Mechanical:

- Unit tests in `src/live.rs` cover each branch of `is_banned_live_item` and
  `select_live_items`.
- Integration tests:
  - An `ended` item makes an `ended` row and no track, no payment route and no
    value time split.
  - A normal item of the same feed still becomes a track.
  - A `live` item with no relay link is dropped when it has no `end`, no
    `start`, or runs 6 hours and 1 second. It is kept at exactly 6 hours. A
    `pending` item with the same times is kept. A `live` item with a relay
    link and no `end` is kept.
  - 11 `live` items store 10, and 11 `ended` items keep the 10 newest, each
    with a warning.
  - A `javascript:` `content_link` gives a warning.
  - A `musicL` feed gives no row.
- The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --lib live
cargo test --test adr0064_live_ingest_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report without a workaround when:

- The promotion loop shares state with the normal track path, so a removal
  changes a normal item.
- An existing test asserts that an `ended` item becomes a track. Report its
  name. Do not delete it on your own. The planner decides, because ADR 0064
  supersedes that rule.
- The warning path cannot carry a new warning without a change to the event
  format.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0064-task-002b-node-ingest-rules.md
- /home/citizen/build/stophammer/docs/plans/adr-0064-live-item-cases.md (rows L7, E1, E2, A1 to A3)
- /home/citizen/build/stophammer/AGENTS.md (Code Style, Tests, Module Structure)
- Only the parts of src/api.rs named in the task file. Use grep. The file is long.

Goal:
- Create `src/live.rs` with the pure ingest rules, and use it in the ingest.
- Keep pending, live and ended rows. Drop banned live items. Apply the two caps.
- Warn about non-web links. Make no track from a live item.
- Fire `live_event_ended` when a row becomes ended.

Constraints:
- The function names, constants and rule order given in the task file.
- Pure functions: no database, no clock.
- A normal item still becomes a track.

Do not touch:
- `src/query.rs`, `src/openapi.rs`, `docs/adr/`, `stophammer-crawler/`, `stophammer-parser/`.
- The track path of a normal item.
- Git: run no git command that writes (no add, commit, stash, checkout, reset).
- Do not delete an existing test. If one asserts the old promotion, stop and report it.

Acceptance criteria:
- The criteria under "Acceptance" in the task file.

Test commands:
- cargo build
- cargo test --lib live
- cargo test --test adr0064_live_ingest_tests
- cargo test
- cargo clippy --all-targets -- -D warnings
- cargo fmt -- --check

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
