# ADR 0060 Task 001: The Parser Reads itemGuid And title

Owner: [ADR 0060](../adr/0060-a-list-feed-keeps-its-items.md) section 1.
Plan: [ADR 0060 phase plan](../plans/adr-0060-list-feeds-phase-plan.md).

Repository: `stophammer-parser`. The crate is in `stophammer-parser/`. Read
`stophammer-parser/AGENTS.md`.

## Goal

Each channel `podcast:remoteItem` gives its `itemGuid` and `title`
attributes.

## Files To Inspect

- `stophammer-parser/src/types.rs`: `IngestRemoteFeedRef`
- `stophammer-parser/src/engine.rs`: `extract_feed_remote_items`,
  `append_remote_ref`, `extract_item_remote_items`
- The parser tests of remote items
- `stophammer-crawler/src/follow.rs`, tests: the crawler builds
  `IngestRemoteFeedRef` with a struct literal

## Files Likely To Change

- `stophammer-parser/src/types.rs`, `stophammer-parser/src/engine.rs`
- A parser test file
- `stophammer-crawler/src/follow.rs` and each other crawler file that builds
  `IngestRemoteFeedRef`: add the two fields with `None`, and nothing more

## Do Not Touch

- The node crate `stophammer`
- The crawler behavior. Task 003 changes it

## Constraints

- Add two fields to `IngestRemoteFeedRef`, after `rel`:
  - `item_guid: Option<String>`, from the `itemGuid` attribute
  - `item_title: Option<String>`, from the `title` attribute
- Each value is trimmed. An empty value gives `None`. Keep the value
  unchanged otherwise.
- Each field has `#[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]`.
  A submission without the fields stays valid, and the JSON of an element
  without them does not change.
- The serde name of each field is `item_guid` and `item_title`. The node
  ingest contract of task 002 reads these names.
- `append_remote_ref` fills the fields for a channel element. An item-level
  element also goes through it. That is correct, and the node ignores them
  for an item-level element in this ADR.

## Acceptance Criteria

Mechanical, tests in the parser crate:

- A channel `remoteItem` with `feedGuid`, `itemGuid="abc"` and
  `title="Song"` gives `item_guid = Some("abc")` and
  `item_title = Some("Song")`. The failure message names ADR 0060 section 1.
- An element with no `itemGuid` and no `title` gives `None` in each.
- An element with `itemGuid="  "` gives `None`.
- The serialized JSON of an element with no `itemGuid` and no `title` holds
  neither key.
- The parser gate is green, and the crawler gate is green after the struct
  literal change.

## Test Commands

```bash
cd stophammer-parser
cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
cd ../stophammer-crawler
cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
```

## Expected Final Report

1. files changed, in each repository
2. tests run and their real results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0060-task-001-parser-item-guid.md`, all of it
- `docs/adr/0060-a-list-feed-keeps-its-items.md`, section 1
- `stophammer-parser/AGENTS.md`
- The files in "Files To Inspect"

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.
- Report the real output of each gate command.

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
