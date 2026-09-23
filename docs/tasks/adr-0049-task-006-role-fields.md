# ADR 0049 Task 006: The Role Fields

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §6.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md),
decisions 7 and 8.
Needs: task 003 and task 005.

## Goal

Each row of the `publisher` view reports the raw `rel` of each side, a `role`
and the source of that role.

## Files To Inspect

- `src/query.rs`: `PublisherResponse`, `load_publisher`,
  `load_track_publisher`, as task 005 left them
- `src/model.rs`: `FeedRemoteItemRaw.rel`, `TrackRemoteItemRaw.rel` from
  task 003
- `src/openapi.rs`: the `include=publisher` examples
- the task 002 fixtures `sirlibre-label`, `jimmyv-publisher`,
  `jimmyv-produced-album`, `detox-artist`

## Files Likely To Change

- `src/query.rs`
- `src/openapi.rs`
- `tests/adr0049_role_tests.rs`, new

## Do Not Touch

- `db::resolve_listed_feed` and the rules of task 005
- the storage of `rel`
- `src/api.rs`

## Constraints

- `publisher_rel` is the raw `rel` of the publisher-side item: the
  `medium="music"` item of the publisher feed that lists the album. It is
  `null` when that item is missing or has no `rel`.
- `music_rel` is the raw `rel` of the album-side item: the publisher item of the
  album that names the publisher. It is `null` when that item is missing or has
  no `rel`.
- The two items are the same items that task 005 matched.
- Normalize a value for comparison with trim and ASCII lowercase. An empty
  value after trim counts as no value.
- `role` and `role_source`:

| `publisher_rel` | `music_rel` | `role` | `role_source` |
|---|---|---|---|
| value X | none | X normalized | `"publisher_rel"` |
| none | value Y | Y normalized | `"music_rel"` |
| value X | value Y, equal to X after normalization | X normalized | `"publisher_rel"` |
| value X | value Y, different | `null` | `"conflict"` |
| none | none | `"artist"` | `"default"` |

- The doc comments of `publisher_rel` and `music_rel` say that the Podcast
  Namespace does not define `rel` on `podcast:remoteItem`, so the value is
  non-standard.
- The doc comment of `role` says that `"artist"` with `role_source =
  "default"` is an assumption, not a statement of the feed.
- Put the role logic in one pure function with a unit test for each table row.

## Implementation Steps

1. Add the pure function and its unit tests.
2. Add the four fields to `PublisherResponse`.
3. Fill them in both views from the items that task 005 matched.
4. Add the OpenAPI examples.
5. Add the fixture tests below.
6. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- A unit test covers each row of the table.
- A unit test proves that `" Label "` and `"label"` are equal.
- Sir Libre test: the `sirlibre-label` view gives a row for `sirlibre-album`
  with `publisher_rel = "label"`, `role = "label"`,
  `role_source = "publisher_rel"`.
- Jimmy V test: the `jimmyv-publisher` view gives a row for
  `jimmyv-produced-album` with `role = "producer"`.
- DETOX test: a row between `detox-artist` and `detox-album` gives
  `role = "artist"` and `role_source = "default"`.

## Test Commands

```bash
cargo build
cargo test --test adr0049_role_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- a fixture holds a `rel` on both sides with different values. Report the
  values
- the view has no access to the matched items without a change to the task 005
  rules

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0049-task-006-role-fields.md`, the section "Constraints"
- `src/query.rs`: `PublisherResponse`, `load_publisher`,
  `load_track_publisher`
- `src/model.rs`: the `rel` fields

Goal:
- Add `publisher_rel`, `music_rel`, `role` and `role_source` to each row of the
  `publisher` view.

Constraints:
- `publisher_rel` and `music_rel` are raw, from the two items that task 005
  matched, or `null`.
- Compare after trim and ASCII lowercase. Empty after trim is no value.
- Use the table in the task file for `role` and `role_source`. A conflict gives
  `role = null` and `role_source = "conflict"`.
- The doc comments mark `rel` as non-standard, and mark the default `"artist"`
  as an assumption.
- The role logic is one pure function with a unit test for each table row.

Do not touch:
- `db::resolve_listed_feed` and the task 005 rules
- the storage of `rel`
- `src/api.rs`

Acceptance criteria:
- The gate is green.
- Unit tests cover each table row and prove that `" Label "` equals `"label"`.
- Sir Libre gives `role = "label"`, `role_source = "publisher_rel"`. Jimmy V
  gives `role = "producer"` for the produced album. DETOX gives `role =
  "artist"`, `role_source = "default"`.

Test commands:
- `cargo build`
- `cargo test --test adr0049_role_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cargo run --bin gen_openapi > /dev/null`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
