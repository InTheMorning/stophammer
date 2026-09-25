# ADR 0052 Task 004: Parser Move Fields

Owner: [ADR 0052](../adr/0052-a-source-moves-its-own-feed.md) sections 2 and
3. Plan: [phase plan 2](../plans/adr-0052-moves-and-guid-changes-phase-plan.md),
decision 1.

Repository: `stophammer-parser`. Read `stophammer-parser/AGENTS.md` first.

## Goal

`IngestFeedData` carries the channel `itunes:new-feed-url` and the channel
`podcast:locked` as typed fields.

## Files To Inspect

- `stophammer-parser/AGENTS.md`
- `stophammer-parser/src/types.rs`: `IngestFeedData`
- `stophammer-parser/src/profile.rs`: the rule set, for example the rules of
  `AuthorName`, `OwnerName` and `LastBuildDate`
- `stophammer-parser/src/engine.rs`: `FeedField`, `is_set`, the building of
  `IngestFeedData`
- `stophammer-parser/tests/` and `tests/fixtures/`

## Files Likely To Change

- `stophammer-parser/src/types.rs`, `src/profile.rs`, `src/engine.rs`
- A new test file in `stophammer-parser/tests/`, and one or two fixtures

## Do Not Touch

- `stophammer`, `stophammer-crawler`
- The existing rules and their sequence

## Constraints

- **Three fields** on `IngestFeedData`, each with a doc comment and
  `#[serde(default, skip_serializing_if = "Option::is_none")]`:
  - `new_feed_url: Option<String>`: the text of the channel
    `itunes:new-feed-url`, trimmed. An empty value is `None`.
  - `locked: Option<bool>`: the text of the channel `podcast:locked`,
    trimmed, case-insensitive: `yes` is `true`, `no` is `false`, any other
    value is `None`.
  - `locked_owner: Option<String>`: the `owner` attribute of the channel
    `podcast:locked`, trimmed. An empty value is `None`.
- Add each as a rule in `src/profile.rs` and a `FeedField` variant, as the
  crate rules say. No branch in the engine for one element.
- An item-level element of the same name has no effect.
- The raw tag stays in `podcast_namespace`, if that collection keeps it now.
- Each doc comment names stophammer ADR 0052.

## Implementation Steps

1. Add the fields, the `FeedField` variants and the rules.
2. Add the fixtures and the tests below.
3. Run the gate of this crate, then the gate of `stophammer-crawler`, which
   depends on this crate by path.

## Acceptance Criteria

Mechanical. Tests in a new file in `stophammer-parser/tests/`:

- A channel with `<itunes:new-feed-url> https://new.example/feed.xml
  </itunes:new-feed-url>` gives `new_feed_url` =
  `https://new.example/feed.xml`.
- A channel with an empty `itunes:new-feed-url` gives `None`.
- An item with `itunes:new-feed-url` and a channel with none gives `None`.
- `<podcast:locked owner="a@b.example">yes</podcast:locked>` gives
  `locked` = `Some(true)` and `locked_owner` = `Some("a@b.example")`.
- `<podcast:locked>No</podcast:locked>` gives `Some(false)` and no owner.
- `<podcast:locked>maybe</podcast:locked>` gives `locked` = `None`.
- A feed with neither element gives three `None` values, and its JSON has
  none of the three keys.
- The gate of this crate is green. The gate of `stophammer-crawler` is green.

## Test Commands

```bash
cd stophammer-parser
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cd ../stophammer-crawler
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- The crawler does not compile because it builds `IngestFeedData` with a
  struct literal. List each place, and do not change the crawler.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0052-task-004-parser-move-fields.md`, all of it
- `stophammer-parser/AGENTS.md`
- The files in "Files To Inspect"

Goal:
- Add `new_feed_url`, `locked` and `locked_owner` to `IngestFeedData` from
  the channel `itunes:new-feed-url` and `podcast:locked`, as rules.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `stophammer`, `stophammer-crawler`
- The existing rules and their sequence

Acceptance criteria:
- The tests of "Acceptance Criteria".
- The two gates are green.

Test commands:
- The commands of "Test Commands".

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
