# ADR 0057 Task 001: The Parser Reads podcast:block

Owner: [ADR 0057](../adr/0057-a-feed-can-block-this-index.md) §5. Plan:
[ADR 0057 phase plan](../plans/adr-0057-podcast-block-phase-plan.md).

Repository: `stophammer-parser`. Read `stophammer-parser/AGENTS.md`.

## Goal

`IngestFeedData` gives each channel-level `podcast:block` tag as a typed
entry with its raw values.

## Files To Inspect

- `stophammer-parser/src/types.rs`: `IngestFeedData`
- `stophammer-parser/src/engine.rs`: the channel pass, and
  `is_podcast_namespace`
- `stophammer-crawler/src`: each struct literal of `IngestFeedData`

## Files Likely To Change

- `stophammer-parser/src/types.rs`, `stophammer-parser/src/engine.rs`
- A parser test file
- Each crawler struct literal of `IngestFeedData`: add the field with an
  empty list, and nothing more

## Do Not Touch

- The rule of ADR 0057 §2. The node applies it in task 002. The parser keeps
  the raw values and decides nothing
- `itunes:block`
- A `podcast:block` inside an item

## Constraints

- Add `pub struct IngestBlockTag { pub id: Option<String>, pub value: String }`
  and the field `pub blocks: Vec<IngestBlockTag>` on `IngestFeedData`.
- Read each channel child `podcast:block` in the Podcast namespace, in source
  order. `value` is the text of the element, trimmed. `id` is the `id`
  attribute, trimmed. An empty `id` gives `None`. An element with an empty
  text is kept, with an empty `value`.
- Keep the case of each value. The node compares with no case sensitivity.
- The serde form of `blocks` has
  `#[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Vec::is_empty"))]`.
  A feed with no tag gives the same JSON as today.

## Acceptance Criteria

Mechanical, tests in the parser crate. Each failure message names ADR 0057
§5:

- A channel with `<podcast:block>yes</podcast:block>` and
  `<podcast:block id="musicindex">no</podcast:block>` gives two entries, in
  that order, with the raw values.
- `<podcast:block id=" podcastindex ">Yes</podcast:block>` gives `id`
  `podcastindex` and `value` `Yes`.
- A `podcast:block` inside an item gives no entry.
- `<itunes:block>Yes</itunes:block>` gives no entry.
- A feed with no tag serializes with no `blocks` key.
- The parser gate and the crawler gate are green.

## Test Commands

```bash
cd stophammer-parser
cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
cd ../stophammer-crawler
cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
```

## Expected Final Report

1. files changed, in each repository
2. the real output of each gate command
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0057-task-001-parser-reads-block.md`, all of it
- `docs/adr/0057-a-feed-can-block-this-index.md`
- `stophammer-parser/AGENTS.md`
- The files in "Files To Inspect"

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.
- Write each test with real assertions. Paste the real output of each gate
  command.

At the end, report the five items of "Expected Final Report".
