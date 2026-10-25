# Summary Rows Review Checklist

The orchestrator uses this checklist to examine the diff of tasks 003 and 004
of ADR 0059. The checklist is advisory.
[ADR 0059](../adr/0059-an-entry-that-names-a-feed-gives-its-summary.md) §6 and
§7 own each rule.

## Each Task

- The diff changes only the files of "Files Likely To Change" of the task.
- The task gate is green: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check`.
- `cargo run --bin gen_openapi` gives each new field in its schema.
- `tests/fixtures/v1-schema-fields.json` holds each new property, and no
  property is removed.
- No field of a row changes its name, its type, its sequence or its value.
- Each image passes through `web_url_or_none`.
- Each new field has a doc comment that names ADR 0059 §6 or §7.
- `docs/API.md` gives each new field and its null rule.
- No migration, no event, no stored summary and no new index.
- No change to `docs/adr/`, `docs/plans/`, `AGENTS.md`, `stophammer-crawler/`
  or `stophammer-parser/`.

## Task 003

- The copies handler adds no query. It reads the image from the feed that it
  already holds.
- The copies image is the image of the indexed feed, and a test proves that it
  is not the image of a copy.
- A live row adds one point read, through `named_feed_summary`.
- The `title` of a live row stays the title of the live item.
- `GET /v1/feeds/{guid}/copies` and the `live_items` view of a feed read do not
  change.

## Task 004

- The feed of a split is read by `remote_feed_guid` only.
- One function holds the track lookup, and `feed_remote_item_response` and the
  split row both call it.
- The tests of `remote_items` pass with no change.
- `remote_item_guid` `""` gives null track fields, and the node sends no track
  query for it.
- A local split sends no query, and `include=local_value_time_splits` does not
  change.
- The tests of `adr0073_split_response_tests.rs` pass with no change.

## After The Two Tasks

- The orchestrator updates `docs/API.md` only where a task left a gap.
- The release record moves requests 17 and 18 to "Deployed" in the two copies
  of the musicindex.org requests. It deletes the plan, this checklist and the
  two packets.
