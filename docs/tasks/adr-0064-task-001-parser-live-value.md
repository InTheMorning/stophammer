# ADR 0064 Task 001: The Parser Reads The Relay Link

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) section 3. Plan:
[ADR 0064 phase plan](../plans/adr-0064-live-items-phase-plan.md).

Repository: `stophammer-parser`. The operator commits. The commit names
`stophammer` ADR 0064.

## Goal

The parser reads `<podcast:liveValue uri="…" protocol="…"/>` on each
`<podcast:liveItem>`, and gives the two attributes in `IngestLiveItemData`.

## Files To Inspect

- `src/engine.rs`: `parse_live_item`, `PODCAST_NS`, `PODCAST_NS_LEGACY`, and a
  helper that reads a child in the podcast namespace
- `src/types.rs`: `IngestLiveItemData`
- `tests/basic.rs`: the live item tests near line 107

## Files Likely To Change

- `src/types.rs`
- `src/engine.rs`
- `tests/basic.rs`, or a new `tests/live_value.rs`

## Do Not Touch

- Each repository other than `stophammer-parser`.
- Each field of `IngestLiveItemData` other than the two new ones.
- The parse of a normal `<item>` and of the channel.

## Constraints

- Add `live_value_uri: Option<String>` and `live_value_protocol:
  Option<String>` to `IngestLiveItemData`, with a doc comment each. Keep the
  `serde` derive of the struct as it is.
- Read only a `liveValue` that is a direct child of the live item. Accept the
  podcast namespace and its legacy form.
- Use the first such child. Trim white space. Keep a `uri` that is only an
  identifier as it is. A `liveValue` with no `uri`, or an empty `uri`, gives
  `None` for both fields.
- No new dependency.

## Steps

1. Add the two fields.
2. In `parse_live_item`, read the first direct `liveValue` child and fill the
   fields.
3. Correct each place that builds `IngestLiveItemData` by hand.
4. Add the tests of the acceptance list.

## Acceptance

Mechanical:

- A live item with `<podcast:liveValue uri="https://relay.example/event?event_id=a" protocol="socket.io"/>`
  gives both values.
- A live item with no `liveValue` gives `None` for both.
- A `uri` of `event-one` stays `event-one`.
- A `liveValue` on the channel, or in a normal item, does not fill a live item.
- The gate of this crate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer/stophammer-parser
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report without a workaround when:

- `IngestLiveItemData` has no `serde` derive, or a change breaks the build of
  a crate outside this repository.
- The namespace helper cannot read a direct child with an attribute.
- A test outside the live item tests fails.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0064-task-001-parser-live-value.md
- /home/citizen/build/stophammer/stophammer-parser/AGENTS.md
- /home/citizen/build/stophammer/stophammer-parser/src/engine.rs (parse_live_item and the namespace helpers only)
- /home/citizen/build/stophammer/stophammer-parser/src/types.rs (IngestLiveItemData)
- /home/citizen/build/stophammer/stophammer-parser/tests/basic.rs (the live item tests)

Goal:
- The parser reads the first direct `podcast:liveValue` child of each `podcast:liveItem`, and gives `live_value_uri` and `live_value_protocol` in `IngestLiveItemData`.

Constraints:
- Only the two new fields. Trim white space. Keep an identifier-only `uri`. No `uri` gives `None` for both.
- Only a direct child of the live item, in the podcast namespace or its legacy form.
- No new dependency. Follow the lint set of this crate.

Do not touch:
- Any repository other than `stophammer-parser`.
- Any other field or parse path.
- Git: run no git command that writes (no add, commit, stash, checkout, reset).

Acceptance criteria:
- The four test cases of the task file pass.
- The gate of this crate is green.

Test commands:
- cargo build
- cargo test
- cargo clippy --all-targets -- -D warnings
- cargo fmt -- --check

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
