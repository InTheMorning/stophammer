# ADR 0058 Task 008: A Copy Row Gives The Item Facts

Owner: [ADR 0058](../adr/0058-a-copy-of-a-feed-is-public.md) sections 1e and
3. musicindex.org request 13.

Repository: `stophammer`. The operator commits.

## Goal

Each copy row keeps and gives `item_durations`, `item_enclosure_urls` and
`item_pub_dates`, in the sequence of `item_guids`, and the channel `author`.
The four values stay out of the summary digest. A change of one of them signs
one `FeedCopyObserved` event.

## Files To Inspect

- `src/model.rs`: `CopySummary`, `copy_summary`, `summary_digest`,
  `CopySummaryDigestFields`, `web_url_or_none`
- `src/api.rs`: `record_feed_copy_for_ingest`, `feed_copy_observed_event_row`
- `src/event.rs`: `FeedCopyObservedPayload`
- `src/apply.rs`: the `FeedCopyObserved` arm
- `src/db.rs`: `FeedCopyRow`, `get_feed_copy`, `list_feed_copies`,
  `upsert_feed_copy_summary`, the `MIGRATIONS` array
- `src/query.rs`: `FeedCopyResponse`, `handle_get_feed_copies`
- `docs/tasks/adr-0058-task-006-copy-titles-and-image.md`: the same pattern
  for the titles and the image
- `tests/adr0058_copy_titles_tests.rs`: the fixtures of task 006

## Files Likely To Change

- `migrations/0052_copy_item_facts.sql`, new, and `src/schema.sql`
- `src/model.rs`, `src/api.rs`, `src/event.rs`, `src/apply.rs`, `src/db.rs`,
  `src/query.rs`, `src/openapi.rs` (only when the schema is not generated
  from the struct)
- `tests/migration_tests.rs`: each recorded version moves by one
- `tests/adr0058_copy_item_facts_tests.rs`, new
- `docs/API.md`: the copies route

## Constraints

- **Storage.** Migration 0052 adds four nullable columns to `feed_copies`:
  `item_durations TEXT`, `item_enclosure_urls TEXT` and `item_pub_dates TEXT`
  (each a JSON array in the sequence of `item_guids`, each entry a value or
  null), and `author TEXT`. A null array means that the row has no facts yet.
- **Summary.** `CopySummary` gets the four values, filled by `copy_summary`
  from the ingest data: `duration_secs`, `enclosure_url` and `pub_date` of
  each track, and `author_name` of the channel. An empty text gives null.
- **Digest.** `summary_digest` and `CopySummaryDigestFields` do not change.
- **Event.** `FeedCopyObservedPayload` gets the four values, each with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`. A new event
  always sets the three arrays. An old event applies as before, with the four
  columns null.
- **When to sign.** In `record_feed_copy_for_ingest`, a row that exists also
  signs one event when one of the four stored values differs from the new
  summary. Stored null arrays count as different, so an old row gets its
  values one time. The event carries the full summary, and the row stores it.
- **Response.** `FeedCopyResponse` gets `item_durations`,
  `item_enclosure_urls`, `item_pub_dates` and `author`, each an `Option`.
  Each enclosure URL goes through `web_url_or_none` (ADR 0054 §4). An entry
  that the rule refuses gives null in its position, so the sequence stays
  equal to `item_guids`.
- **Migration watermark.** A database can record a version after migration
  0052 and have no such columns. For that database, add an
  `ensure_copy_item_facts_schema` repair in `src/db.rs` (ADR 0046). Task 001
  of ADR 0070 did the same for `title_sort`.

## Do Not Touch

- `summary_digest`, `CopySummaryDigestFields`, the resolution, the relocation,
  the limit of 20 rows, and section 2
- `stophammer-crawler/`, `stophammer-parser/`. The ingest data already holds
  each value.
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- Each existing migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add migration 0052, the `MIGRATIONS` entry, the `src/schema.sql` columns,
   and the repair when the migration tests need it.
2. Add the four values to `CopySummary`, `copy_summary`, `FeedCopyRow`, the
   database reads and `upsert_feed_copy_summary`.
3. Add the four values to `FeedCopyObservedPayload`, the event row and the
   apply arm.
4. Extend the change check in `record_feed_copy_for_ingest`.
5. Add the four fields to `FeedCopyResponse`, with the URL rule.
6. Write the tests, then `docs/API.md`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0058_copy_item_facts_tests.rs`, or in
the test file that the item names.

- A copy with two items gives the two durations, the two enclosure URLs, the
  two dates and the channel author, in the sequence of `item_guids`.
- An item with no duration gives null in its position.
- An enclosure URL `javascript:alert(1)` gives null in its position, and the
  other positions keep their values.
- A second submission with only a new enclosure URL signs one
  `FeedCopyObserved` event, and does not change `summary_digest`.
- A resolved copy that changes only an enclosure URL stays resolved.
- A third submission with no change signs no event.
- A replica that applies the events gives the same four fields.
- An event from before this task, with none of the four fields, applies, and
  the row gives null in each.
- `tests/migration_tests.rs`: migration 0052 adds the four columns.
- The ADR 0044 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0058_copy_item_facts_tests
cargo test --test migration_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Deploy Step

After the deploy of the release, the operator replays the cached copy bodies
with `export-feed-cache-ndjson.py --copies` and the `ndjson` mode, as for
section 1c. No fetch is necessary. The release handoff gives the commands.

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A step needs a change to an ADR, or to a file of "Do Not Touch".
- The ingest data does not hold one of the four values.
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
- `docs/adr/0058-a-copy-of-a-feed-is-public.md`, sections 1c, 1e and 3
- `docs/tasks/adr-0058-task-008-copy-item-facts.md` (this task)
- `docs/tasks/adr-0058-task-006-copy-titles-and-image.md` (the same pattern)
- the files of "Files To Inspect" in this task

Goal:
- Each copy row keeps and gives `item_durations`, `item_enclosure_urls`, `item_pub_dates` and `author`, out of the summary digest, with one event when one of them changes.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- `summary_digest` does not change.
- Each enclosure URL goes through `web_url_or_none` on a read.
- Stop at each escalation trigger of this task.

Do not touch:
- `summary_digest`, `CopySummaryDigestFields`, the resolution, the relocation, the limit of 20 rows
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- each existing migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0058_copy_item_facts_tests`
- `cargo test --test migration_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
