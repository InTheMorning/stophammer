# ADR 0059 Task 003: A Copies Row And A Live Row Give The Image Of Their Feed

Owner: [ADR 0059](../adr/0059-an-entry-that-names-a-feed-gives-its-summary.md)
§6. The phase plan is [the summary rows plan](../plans/summary-rows-phase-plan.md).

Repository: `stophammer`. The operator commits. This task does not depend on
task 004.

## Goal

A row of `GET /v1/copies` gives `feed_image_url`. A row of
`GET /v1/live-items` gives `feed_title` and `feed_image_url`. Each value is of
the indexed feed that `feed_guid` of the row names.

## Files To Inspect

- `src/query.rs`:
  - `CopyRecordResponse` and `handle_list_copy_records` (`GET /v1/copies`)
  - `LiveItemListResponse` and `live_item_list_response` (`GET /v1/live-items`)
  - `named_feed_summary` and `web_url_or_none` (ADR 0059 §1 and §3)
- `tests/fixtures/v1-schema-fields.json`: `CopyRecordResponse` and
  `LiveItemListResponse`
- `docs/API.md`: the sections `GET /v1/copies` and `GET /v1/live-items`
- `tests/adr0059_entry_summary_tests.rs`: the test setup and the image test
- `tests/adr0064_live_list_tests.rs`: how a test makes a live item

## Files Likely To Change

- `src/query.rs`, `tests/fixtures/v1-schema-fields.json`, `docs/API.md`
- `tests/adr0059_list_row_image_tests.rs`, new

## Constraints

- **The copies row.** Add `feed_image_url: Option<String>` to
  `CopyRecordResponse`, after `title`. The handler already reads the feed with
  `db::get_feed_by_guid`. Give `web_url_or_none(feed.image_url.as_deref())`.
  Add no query.
- **The live row.** Add `feed_title: Option<String>` and
  `feed_image_url: Option<String>` to `LiveItemListResponse`, after
  `feed_guid`. In `live_item_list_response`, read them with
  `named_feed_summary(conn, Some(&event.feed_guid))`. That function already
  passes the image through `web_url_or_none`. Each field is null when the node
  holds no such feed.
- **The image of the feed.** The copies row gives the image of the indexed
  feed, never the image of a copy.
- **No other change.** Each other field of the two rows keeps its name, type,
  sequence and value. `GET /v1/feeds/{guid}/copies` and the `live_items` view
  of `GET /v1/feeds/{guid}` do not change.
- **The contract.** Add each new property to
  `tests/fixtures/v1-schema-fields.json`. `src/openapi.rs` refers to each
  schema with `$ref`, so it needs no change. `docs/API.md` gives each new field
  and its null rule in the two sections.
- **Comments.** A doc comment on each new field names ADR 0059 §6.

## Do Not Touch

- `VtsResponse` and the value time splits. Task 004 changes them.
- `FeedCopyResponse`, `LiveItemResponse`, `live_item_response` and
  `list_live_items`, other than the call of `live_item_list_response`
- The migrations, `src/schema.sql`, `src/db.rs` and the events
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the tests of the acceptance criteria.
2. Add the copies field.
3. Add the two live fields.
4. Change the fixture and `docs/API.md`.
5. Run the test commands.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0059_list_row_image_tests.rs`, through
the HTTP router.

- A `/v1/copies` row of a feed with an `https` image gives that image in
  `feed_image_url`.
- The copy of that feed has a different image. The `/v1/copies` row gives the
  image of the indexed feed, not the image of the copy.
- A `/v1/copies` row of a feed with a `javascript:` image gives
  `feed_image_url` null. A feed with no image gives null.
- A `/v1/live-items` row gives the `title` of its feed in `feed_title`, and the
  image of its feed in `feed_image_url`.
- A `/v1/live-items` row of a feed with a `javascript:` image gives
  `feed_image_url` null.
- The `title` of a `/v1/live-items` row stays the title of the live item.
- The existing tests `adr0058_*` and `adr0064_*` pass with no change.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0059_list_row_image_tests
cargo test --test adr0064_live_list_tests
cargo test --test adr0044_field_removal_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A live row needs more than one more query.
- The copies handler does not hold the indexed feed of each row.
- An existing test fails because of the new fields.
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
- `~/build/stophammer/AGENTS.md` (the code style, the API steps and the pre-commit gate)
- `~/build/stophammer/docs/adr/0059-an-entry-that-names-a-feed-gives-its-summary.md`, §1, §3 and §6
- `~/build/stophammer/docs/tasks/adr-0059-task-003-list-row-feed-image.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- A row of `GET /v1/copies` gives `feed_image_url`, and a row of `GET /v1/live-items` gives `feed_title` and `feed_image_url`, of the indexed feed that `feed_guid` names.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- The copies row adds no query. The live row adds one point read with `named_feed_summary`.
- Each image passes through `web_url_or_none`. Each other field stays the same.
- Stop at each escalation trigger of this task.

Do not touch:
- `VtsResponse` and the value time splits
- `FeedCopyResponse`, `LiveItemResponse`, `live_item_response`, the migrations, `src/schema.sql`, `src/db.rs`
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0059_list_row_image_tests`
- `cargo test --test adr0064_live_list_tests`
- `cargo test --test adr0044_field_removal_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cargo run --bin gen_openapi > /dev/null`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
