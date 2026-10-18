# ADR 0064 Task 006: Each Live Row Gives Its Link

Owner: [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md) section 6, as
amended on 2026-10-17. musicindex.org request 14.

Repository: `stophammer`. The operator commits. ADR 0054 task 005 comes first.

## Goal

Each live row of `GET /v1/live-items` and of `live_items` in a feed read gives
`link` and `link_scheme_added`, from the stored RSS `<link>` of the live item.

## Files To Inspect

- `src/query.rs`: `LiveItemResponse`, `live_item_response` and its two
  callers (the feed read and the list route)
- `src/db.rs`: `get_source_entity_links_for_feed_entity`
- `src/model.rs`: `web_link_for_read`, from ADR 0054 task 005
- `src/openapi.rs`: the schema of `LiveItemResponse`
- `tests/adr0064_live_feed_read_tests.rs`, `tests/adr0064_live_list_tests.rs`

## Files Likely To Change

- `src/query.rs`, `src/db.rs` (only for a batch read), `src/openapi.rs` (only
  when the schema is not generated from the struct)
- `tests/adr0064_live_item_link_tests.rs`, new
- `docs/API.md`

## Constraints

- **Source.** The stored link of the live item: `entity_type` `live_item`,
  `entity_id` the `live_item_guid`, the `feed_guid` of the row,
  `link_type` `web_page` and `extraction_path` `entity.link`. Only that row
  gives the value. An Atom link does not.
- **Rule.** `model::web_link_for_read` of the stored value gives `link` and
  `link_scheme_added`. With no stored row, `link` is null and
  `link_scheme_added` is false.
- **Cost.** A page of the list route gives at most 200 rows. One query for
  each row is acceptable. When a batch query for the page is simple, use it.
- **Storage.** The parser and the ingest already store the value. No
  migration, no parser change and no ingest change.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- The views, the filters and the other fields of a live row
- Each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add `link: Option<String>` and `link_scheme_added: bool` to
   `LiveItemResponse`.
2. Read the stored link for each row, and fill the two fields with
   `web_link_for_read`.
3. Add the fields to the OpenAPI schema when the schema needs it.
4. Write the integration tests, then `docs/API.md`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0064_live_item_link_tests.rs`.

- A live item with `<link>https://www.pixelpalace.party/</link>` gives that
  `link` and `link_scheme_added` false, in the feed read and in
  `GET /v1/live-items?view=all`.
- A live item with `<link>www.pixelpalace.party</link>` gives
  `https://www.pixelpalace.party` and `link_scheme_added` true.
- A live item with no `<link>` gives `link` null and `link_scheme_added`
  false.
- A live item with only an Atom link gives `link` null.
- Two feeds with the same `live_item_guid` each give their own link.
- The ADR 0044 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0064_live_item_link_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A step needs a change to an ADR, or to a file of "Do Not Touch".
- The ingest does not store the `<link>` of a live item as this task states.
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
- `docs/adr/0064-a-live-item-is-an-rss-fact.md`, section 6
- `docs/adr/0054-a-fetch-reaches-only-public-feed-hosts.md`, section 4a
- `docs/tasks/adr-0064-task-006-live-item-link.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- Each live row gives `link` and `link_scheme_added`, from the stored RSS `<link>` of the live item.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Use `model::web_link_for_read`. Do not write a second URL rule.
- No migration, no parser change and no ingest change.
- Stop at each escalation trigger of this task.

Do not touch:
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- the views, the filters and the other fields of a live row
- each migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0064_live_item_link_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
