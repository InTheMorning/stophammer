# ADR 0057 Task 002: The Node Applies podcast:block

Owner: [ADR 0057](../adr/0057-a-feed-can-block-this-index.md) §2 to §5.
Plan: [ADR 0057 phase plan](../plans/adr-0057-podcast-block-phase-plan.md).

Repository: `stophammer`. Start after ADR 0044 task 003 is merged.

## Goal

A submission from the source URL with a `podcast:block` that applies to this
index retires the record, or writes nothing for a new feed. The answer gives
the reason `source_blocked`.

## Files To Inspect

- `docs/adr/0057-a-feed-can-block-this-index.md`, all of it
- `src/ingest.rs`: `IngestFeedData`
- `src/api.rs`: `handle_ingest_feed`, the ADR 0051 classification, the check
  of an operator block of ADR 0053, and the retirement of ADR 0053 task 003
- `src/db.rs`: `delete_feed_with_event`
- `src/openapi.rs`: the ingest route and its reasons
- `docs/API.md`: the ingest reasons
- `tests/`: the tests of ADR 0051 and of ADR 0053 task 003

## Files Likely To Change

- `src/ingest.rs`, `src/api.rs`, `src/openapi.rs`, `docs/API.md`
- `src/db.rs` only when `delete_feed_with_event` cannot take an empty block
  list
- `tests/adr0057_podcast_block_tests.rs`, new

## Do Not Touch

- The ADR 0051 classification, and its results for a mirror, a record
  conflict and a GUID change
- The operator block of ADR 0053. It stays before this rule
- `feed_blocks`. This rule writes no row in it

## Constraints

- **The field.** `IngestFeedData` gets
  `#[serde(default)] pub blocks: Vec<IngestBlockTag>`, with
  `IngestBlockTag { id: Option<String>, value: String }`. An older crawler
  sends no field, and the node applies no rule.
- **The rule.** One pure function, for example
  `fn source_blocks_this_index(blocks: &[IngestBlockTag]) -> bool`, applies
  ADR 0057 §2 in its sequence. Compare the trimmed `value` and `id` with no
  case sensitivity. The slug is a constant, `INDEX_SLUG = "musicindex"`.
- **The place.** Apply the rule after the check of an operator block and
  after the ADR 0051 classification. Apply it only to the cases Update and
  New feed.
- **Update and blocked.** Retire the record in one transaction with
  `FeedRetired`, reason `podcast_block`, and no block row. Use the retirement
  path of ADR 0053 task 003. The answer is `accepted: false`,
  `reason: "source_blocked"`, with the ID of the `FeedRetired` event in
  `events_emitted`.
- **New feed and blocked.** Write nothing, and sign no event. The answer is
  `accepted: false`, `reason: "source_blocked"`, with an empty
  `events_emitted`.
- Add `source_blocked` to the reasons in `src/openapi.rs` and `docs/API.md`.
  Write in ASD-STE100 Simplified Technical English, and run
  `python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --check --no-heuristics docs/API.md`.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0057_podcast_block_tests.rs`. Each failure
message names ADR 0057 and its guard:

- An unbounded `yes` from the source URL retires the record. A read of the
  feed answers `404`. The event log holds one `FeedRetired`, and
  `feed_blocks` holds no row.
- `id="musicindex"` with `no` beside an unbounded `yes` admits the feed.
- `id="podcastindex"` with `yes` admits the feed.
- `id="MusicIndex"` with ` YES ` retires the record.
- A `yes` tag in a mirror body changes nothing, and the answer is the ADR
  0051 answer for a mirror.
- A new feed with an unbounded `yes` writes no feed, and the answer is
  `source_blocked` with no event.
- A feed that removes its tag after a retirement is admitted on the next
  submission, with the same track IDs as before.
- An operator block of ADR 0053 still answers `blocked`, also when the body
  has an unbounded `yes`.
- The unit tests of the rule function cover each row of ADR 0057 §2.
- The gate is green: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`, and
  the ADR 0044 guards.

## Escalation Triggers

Stop and report when:

- The retirement path of ADR 0053 task 003 cannot retire a record with no
  block.
- The classification of ADR 0051 runs after the point where a record is
  written.

## Expected Final Report

1. files changed
2. the real output of each gate command
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0057-task-002-node-applies-block.md`, all of it
- `docs/adr/0057-a-feed-can-block-this-index.md`
- The files in "Files To Inspect"

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.
- Write each test with real assertions. Paste the real output of each gate
  command.

At the end, report the five items of "Expected Final Report".
