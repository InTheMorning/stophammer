# ADR 0049 Task 006b: A Comma Separates The Roles In `rel`

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §6.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md),
decisions 8 and 13.
Needs: task 006 merged.

## Why This Task Exists

Task 006 read a `rel` value as one value. So `"artist, producer"` and
`"producer, artist"` gave a `conflict`.

On 2026-09-24 the operator decided that a comma separates the roles. A space
does not, because a role can have two words, such as `sound engineer`. The
Podcast Namespace does not define `rel` on `podcast:remoteItem` yet. Its
discussion #579 has a comma proposal and a space proposal. The one real feed
with more than one role, Jimmy V, uses a comma.

## Goal

The node reads a `rel` value as a set of roles, and it compares the two sides as
sets.

## Files To Inspect

- `src/query.rs`: `normalize_rel`, `resolve_role`, their unit tests, and the
  call in `build_publisher_row`
- `tests/adr0049_role_tests.rs`
- `docs/API.md`: the text for `role` and `role_source`

## Files Likely To Change

- `src/query.rs`
- `tests/adr0049_role_tests.rs`, only to add tests
- `docs/API.md`

## Do Not Touch

- `publisher_rel` and `music_rel`. They stay raw
- the response shape. `role` stays one string or `null`
- the resolver, and the rules of task 005 for which items match
- the stored `rel`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

The role set of a raw value:

1. Split the value on each comma.
2. Remove the white space at the start and at the end of each part.
3. In each part, replace each internal run of white space with one space.
4. Apply ASCII lowercase to each part.
5. Remove each empty part and each duplicate.
6. An empty set counts as no value.

The rule of `role` and `role_source` keeps its table from task 006, with sets:

| `publisher_rel` set | `music_rel` set | `role` | `role_source` |
|---|---|---|---|
| X | none | X | `"publisher_rel"` |
| none | Y | Y | `"music_rel"` |
| X | Y, equal to X | X | `"publisher_rel"` |
| X | Y, different | `null` | `"conflict"` |
| none | none | `"artist"` | `"default"` |

- `role` is the set, sorted, and joined by `", "`.
- Two sets are equal when they hold the same roles. The order in the feed does
  not matter.
- Update the doc comments of `role`, `publisher_rel` and `music_rel`: a comma
  separates the roles, and `role` joins the sorted roles with `", "`.
- Update `docs/API.md` in the same way.

## Implementation Steps

1. Change `normalize_rel` so that it gives the set, and `resolve_role` so that
   it compares sets.
2. Correct the unit test `normalize_rel_keeps_a_comma_as_one_value`. It states
   the rule that this task replaces. Report it.
3. Add the tests below.
4. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- Unit tests:
  - `"Artist, Producer"` gives the set `{artist, producer}`.
  - `"sound engineer,  Mastering   Engineer"` gives
    `{mastering engineer, sound engineer}`.
  - `"artist producer"` gives the set `{artist producer}`, one role.
  - `" , ,"` gives no value.
  - `"Label, label"` gives `{label}`.
  - `"producer, artist"` against `"Artist, Producer"` gives
    `role = "artist, producer"` and `role_source = "publisher_rel"`.
  - `"artist, producer"` against `"artist"` gives `"conflict"`.
- The five table rows of task 006 still pass.
- The fixture tests in `tests/adr0049_role_tests.rs` pass with no edit. The
  Sir Libre row still gives `role = "label"`. The inline
  `"Artist, Producer"` row still gives `role = "artist, producer"`.

## Test Commands

```bash
cargo build
cargo test --test adr0049_role_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns
6. each existing test that changed, with the old and the new expectation

## Escalation Triggers

Stop and report when:

- a test other than `normalize_rel_keeps_a_comma_as_one_value` needs a changed
  expectation
- the role set cannot pass to `build_publisher_row` without a change to the
  task 005 rules

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0049-task-006b-rel-is-a-comma-list.md`, in full
- `src/query.rs`: `normalize_rel`, `resolve_role`, their unit tests, and
  `build_publisher_row`
- `tests/adr0049_role_tests.rs`

Goal:
- Read a `rel` value as a set of roles separated by commas, and compare the two
  sides as sets.

Constraints:
- Split on commas only. Trim each part, join internal white space into one
  space, ASCII-lowercase, remove empty parts and duplicates. An empty set is no
  value.
- A value with no comma is one role, even with a space in it.
- Keep the table of task 006, with set equality.
- `role` is the sorted set joined by `", "`. It stays a string or `null`.
- `publisher_rel` and `music_rel` stay raw.
- Update the doc comments and `docs/API.md`.

Do not touch:
- the response shape, the resolver, the task 005 rules, the stored `rel`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- Unit tests prove each example in the task file.
- The five table rows of task 006 still pass.
- `tests/adr0049_role_tests.rs` passes unedited.
- Only `normalize_rel_keeps_a_comma_as_one_value` changes its expectation.

Test commands:
- `cargo build`
- `cargo test --test adr0049_role_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
