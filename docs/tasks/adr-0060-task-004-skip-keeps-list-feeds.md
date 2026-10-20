# ADR 0060 Task 004: A Skip Store Keeps A List Feed

Owner: [ADR 0060](../adr/0060-a-list-feed-keeps-its-items.md) §5, and
[ADR 0074](../adr/0074-a-feed-without-a-guid-is-invalid.md) §3b. This task
corrects a defect. It adds no rule.

Repository: `stophammer-crawler`. The operator commits.

## The Defect

The three skip stores of the crawler treat `musicL` as a non-music medium:

| Store | Function | Rule today |
|---|---|---|
| `feed_skip.db` | `FeedSkipDb::should_skip` in `src/feed_skip.rs` | Skips a `200` with a medium other than `music` or `publisher` |
| `gossip_feed_memory` | `should_skip_feed` in `src/modes/gossip.rs` | The same, and the comparison is case-sensitive |
| `import_feed_memory` | `is_skip_known_non_music` in `src/modes/import.rs` | The same |

The rules date from 2026-03-27 and 2026-03-28. ADR 0060 made `musicL` a medium
of the index on 2026-09-26. The node accepts `music`, `publisher` and `musicL`,
with no regard to letter case (`src/medium.rs` in `stophammer`).

The `gossip` mode reads the first two stores by default
(`GOSSIP_SKIP_KNOWN_NON_MUSIC=1`), with no time limit by default. Thus after
the first crawl of a `musicL` feed, `gossip` skips each subsequent podping of that
feed. ADR 0060 §5 requires the crawler to follow a list feed. ADR 0074 §3b gives
a `musicL` body with no GUID the 7-day rule, and not a skip with no time limit.

## Goal

Each skip store treats `music`, `publisher` and `musicL` as media of the index,
with no regard to letter case. One function gives that set to each store.

## Files To Inspect

- `src/feed_skip.rs`: `should_skip`, `is_unparsed_channel_without_medium`, the
  tests
- `src/modes/gossip.rs`: `should_skip_feed`, and its tests near
  `should_skip_feed("https://example.com/podcast.xml", None)`
- `src/modes/import.rs`: `is_skip_known_non_music`, `is_audit_accepted_medium`,
  `known_skip_kind`, and the test that asserts `is_skip_known_non_music`
- `~/build/stophammer/src/medium.rs`: the node rule

## Files Likely To Change

- `src/feed_skip.rs`
- `src/modes/gossip.rs`
- `src/modes/import.rs`

## Constraints

- **One function.** Add to `src/feed_skip.rs`:

  ```rust
  pub(crate) fn is_index_medium(raw_medium: &str) -> bool
  ```

  It gives `true` for `music`, `publisher` and `musicL`, with
  `eq_ignore_ascii_case`. Its doc comment names `stophammer` ADR 0060 §5 and
  says that the set is the set of the node medium gate.
- **The three stores.** `should_skip`, `should_skip_feed` and
  `is_skip_known_non_music` use `!is_index_medium(m)` for the rule "known
  non-music medium". Remove the literal comparisons.
- **The audit.** `is_audit_accepted_medium` gives `is_index_medium`. Its
  behavior does not change.
- **No other rule changes.** These rules keep their code: the medium-gate
  rejection, the rule of ADR 0074 §3b, the 7-day rule, the `413` rule, the
  known success and the TTL.
- **Comments.** Each change names ADR 0060 §5.

## Do Not Touch

- `src/crawl.rs`, `src/follow.rs`, `src/feed_cache.rs`
- The recorded rows. Each existing row of `musicL` stops its skip when the
  new code reads it, with no migration.
- `../stophammer-parser/`, and the `stophammer` repository
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add `is_index_medium`, with a unit test.
2. Use it in the three stores and in the audit.
3. Write the tests.

## Acceptance Criteria

Mechanical. Each is a test in the `tests` module of the file that it names.

- `src/feed_skip.rs`: `is_index_medium` gives `true` for `music`, `MUSIC`,
  `publisher`, `musicL` and `musicl`, and `false` for `podcast`, `video` and
  an empty text.
- `src/feed_skip.rs`: a `200` accepted row with `raw_medium` `musicL` gives
  `should_skip == None`.
- `src/feed_skip.rs`: a `200` parse error of `missing podcast:guid` with
  `raw_medium` `musicL`, seen now, gives `Some("prior parse error: ...")`. Seen 8
  days ago, it gives `None`. This is the `musicL` case of ADR 0074 §3b.
- `src/modes/gossip.rs`: a `200` row with `raw_medium` `musicL` gives
  `should_skip_feed == None`. A row with `MUSIC` gives `None`. A row with
  `podcast` still gives `Some`.
- `src/modes/import.rs`: `is_skip_known_non_music("musicL")` is `false`, and
  `known_skip_kind` gives `None` for a `200` row with `raw_medium` `musicL`
  and `skip_known_non_music` set.
- Each existing test passes with no change.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-crawler
cargo build
cargo test is_index_medium
cargo test feed_skip
cargo test should_skip_feed
cargo test known_skip
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Deploy Step

After the deploy of the gossip crawler, a podping for a `musicL` feed gives a
crawl again. The operator reads the gossip log for a line `skipped (shared:
known non-music medium: musicL)`. After the deploy, no such line occurs.

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- An existing test asserts that `musicL` is skipped.
- A store reads a medium in a form other than the raw text.
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
- `~/build/stophammer/stophammer-crawler/AGENTS.md`
- `~/build/stophammer/docs/adr/0060-a-list-feed-keeps-its-items.md`, §5
- `~/build/stophammer/docs/adr/0074-a-feed-without-a-guid-is-invalid.md`, §3b
- `~/build/stophammer/docs/tasks/adr-0060-task-004-skip-keeps-list-feeds.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- Each skip store treats `music`, `publisher` and `musicL` as media of the index, with no regard to letter case, through one function `is_index_medium`.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Change only the rule "known non-music medium" and the audit medium check.
- Stop at each escalation trigger of this task.

Do not touch:
- `src/crawl.rs`, `src/follow.rs`, `src/feed_cache.rs`
- `../stophammer-parser/`, the `stophammer` repository
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test is_index_medium`
- `cargo test feed_skip`
- `cargo test should_skip_feed`
- `cargo test known_skip`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
