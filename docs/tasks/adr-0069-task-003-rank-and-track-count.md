# ADR 0069 Task 003: The Rank Of A Publisher Feed And The Track Count

Owner: [ADR 0069](../adr/0069-an-album-confirms-each-credit.md) section 3a,
and [ADR 0059](../adr/0059-an-entry-that-names-a-feed-gives-its-summary.md)
section 1. v4vmm request 7.

Repository: `stophammer`. The operator commits.

## Goal

Each row of the `publisher` view gives `album_names_rank`. Each `publisher`
entry and each `remote_items` entry gives `remote_episode_count`.

## Files To Inspect

- `src/query.rs`: `PublisherResponse`, `build_publisher_row`,
  `album_names_as_value`, `is_album_publisher_link`, `load_publisher`,
  `NamedFeedSummary`, `named_feed_summary`, `remote_item_summary`, the
  `remote_items` entry type, `resolve_publisher_feed_title`
- `src/db.rs`: `get_feed_remote_items_for_feed`
- `tests/adr0069_credit_tests.rs`, `tests/adr0059_entry_summary_tests.rs`:
  the fixtures

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs` (only when the schema is not generated
  from the struct)
- `tests/adr0069_rank_tests.rs`, new
- `tests/adr0059_entry_summary_tests.rs`
- `docs/API.md`

## Constraints

- **Rank.** Take the feed-level items of the album with `medium` `publisher`
  and `is_album_publisher_link(source)` true, in the sequence of `position`.
  The rank is the place of the matching item in that list, from 1. The
  matching item is the item that `album_names_as` uses. Each such item
  counts, also when the node does not hold its feed.
- **Null.** The rank is null when `album_names_as` is null.
- **Both directions.** A row of an album read and a row of a publisher read
  give the same rank for the same link.
- **Track count.** `NamedFeedSummary` gets `episode_count`, from the same row
  read. `remote_episode_count` is null when the node holds no feed for the
  entry. Each response type that gives `remote_feed_title` also gives
  `remote_episode_count`.
- **No storage change.** No migration, no event change and no ingest change.
- **No derived value.** The node gives no main artist and no release kind.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- The role rules, `role_agreement`, `album_names_as`, and the counts of ADR
  0061 and ADR 0068
- Each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add `episode_count` to `NamedFeedSummary` and its query, and
   `remote_episode_count` to each response type that gives
   `remote_feed_title`.
2. Write one function that gives the rank of the item of an album that names
   a publisher feed. Use it for both directions of the `publisher` view.
3. Add `album_names_rank: Option<u32>` to `PublisherResponse`.
4. Write the tests, then `docs/API.md`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0069_rank_tests.rs`, or in the test
file that the item names.

- An album names publisher feeds A and B in `<podcast:publisher>`, in that
  sequence. Its row of A gives rank 1, and its row of B gives rank 2. The
  publisher read of B gives rank 2 on its row of the album.
- An album whose first `<podcast:publisher>` item names a feed that the node
  does not hold gives rank 2 to the second named feed.
- A feed-level `remoteItem` outside `<podcast:publisher>` before the publisher
  items does not change the rank.
- A publisher that lists an album that does not name it gives a null rank.
- `tests/adr0059_entry_summary_tests.rs`: a `publisher` entry and a
  `remote_items` entry give the track count of the named feed in
  `remote_episode_count`. An entry for a feed that the node does not hold
  gives null.
- The ADR 0044 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0069_rank_tests
cargo test --test adr0059_entry_summary_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A step needs a change to an ADR, or to a file of "Do Not Touch".
- The rank needs a rule that ADR 0069 §3a does not state.
- One `publisher` view read gets more than one database read for each row.
- An existing test fails after the change, and the cause is not clear.

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
- `AGENTS.md` (the code style and the pre-commit gate)
- `docs/adr/0069-an-album-confirms-each-credit.md`, sections 1a, 3 and 3a
- `docs/adr/0059-an-entry-that-names-a-feed-gives-its-summary.md`, section 1
- `docs/tasks/adr-0069-task-003-rank-and-track-count.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- Each row of the `publisher` view gives `album_names_rank`, and each `publisher` and `remote_items` entry gives `remote_episode_count`.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- One function gives the rank, for both directions.
- No migration, no event change and no ingest change.
- Stop at each escalation trigger of this task.

Do not touch:
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- the role rules, `role_agreement`, `album_names_as`, and the counts of ADR 0061 and ADR 0068
- each migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0069_rank_tests`
- `cargo test --test adr0059_entry_summary_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
