# ADR 0049 Task 001: The Parser Reads `rel`

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §6.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md).

**This task changes a different repository.** The work is in
`stophammer-parser`. Read `stophammer-parser/AGENTS.md` first. The commit goes
in that repository and names ADR 0049.

## Goal

Each `IngestRemoteFeedRef` that the parser makes carries the raw `rel` attribute
of its `podcast:remoteItem`, or `None`.

## Files To Inspect

- `stophammer-parser/AGENTS.md`
- `src/types.rs`: `IngestRemoteFeedRef`
- `src/engine.rs`: `extract_feed_remote_items`, `append_remote_ref`,
  `extract_item_remote_items`
- the tests in `src/engine.rs` and `tests/` that make an `IngestRemoteFeedRef`

## Files Likely To Change

- `src/types.rs`
- `src/engine.rs`
- the tests that construct `IngestRemoteFeedRef` with a struct literal

## Do Not Touch

- the extraction of `rel` on `podcast:alternateEnclosure` and
  `podcast:transcript`
- the `valueTimeSplit` path
- the `stophammer` and `stophammer-crawler` repositories

## Constraints

- The field is `pub rel: Option<String>`. With the `serde` feature, it has
  `#[serde(default)]`, so JSON with no `rel` still decodes.
- Keep the value raw. Do not trim it, change its case or check it against a
  list. An empty attribute gives `None`. That is the only change.
- The doc comment says that the Podcast Namespace does not define `rel` on
  `podcast:remoteItem`.
- Both channel-level and item-level remote items get the field. A
  `podcast:remoteItem` inside `podcast:publisher` gets it too.

## Implementation Steps

1. Add the field to `IngestRemoteFeedRef` with its doc comment.
2. Read `remote.attribute("rel")` in `append_remote_ref` and in
   `extract_item_remote_items`.
3. Correct each struct literal in the tests.
4. Add the tests below.
5. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- A test proves that `<podcast:remoteItem medium="music" feedGuid="g"
  feedUrl="u" rel="label"/>` in a channel gives `rel == Some("label")`.
- A test proves that a remote item with no `rel` gives `None`.
- A test proves that a remote item inside `<podcast:publisher>` keeps its `rel`.
- A test proves that an item-level remote item keeps its `rel`.
- A test proves that `rel=" Producer "` stays `" Producer "`.
- A test proves that JSON with no `rel` key decodes into an
  `IngestRemoteFeedRef` with `rel == None`.

## Test Commands

```bash
cd stophammer-parser
cargo build
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

## Escalation Triggers

Stop and report when:

- a consumer outside this crate constructs `IngestRemoteFeedRef` with a struct
  literal and fails to build. `stophammer-crawler/src/crawl.rs` holds one.
  Report it. Do not edit the crawler
- the `serde` feature is not how the type crosses the wire

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

This work is in the `stophammer-parser` repository. Read its `AGENTS.md` first.

Read:
- `stophammer-parser/AGENTS.md`
- `src/types.rs`, the struct `IngestRemoteFeedRef`
- `src/engine.rs`, the functions `extract_feed_remote_items`,
  `append_remote_ref` and `extract_item_remote_items`

Goal:
- Each `IngestRemoteFeedRef` carries the raw `rel` attribute of its
  `podcast:remoteItem`, or `None`.

Constraints:
- Add `pub rel: Option<String>` with `#[serde(default)]` under the `serde`
  feature.
- Keep the value raw. An empty attribute gives `None`. Make no other change.
- The doc comment says that the Podcast Namespace does not define `rel` on
  `podcast:remoteItem`.
- Channel-level items, items inside `podcast:publisher` and item-level items
  all get the field.

Do not touch:
- `rel` on `podcast:alternateEnclosure` or `podcast:transcript`
- the `valueTimeSplit` path
- the `stophammer` and `stophammer-crawler` repositories

Acceptance criteria:
- The gate is green.
- Tests prove:
  - Channel item with `rel="label"` gives `Some("label")`.
  - No `rel` gives `None`.
  - An item inside `podcast:publisher` keeps its `rel`.
  - An item-level remote item keeps its `rel`.
  - `" Producer "` stays unchanged.
  - JSON with no `rel` key decodes to `None`.

Test commands:
- `cargo build`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
