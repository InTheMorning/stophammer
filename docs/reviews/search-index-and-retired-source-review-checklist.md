# Review Checklist: The Search Index And The Retired Source Hold

This checklist is advisory. The orchestrator uses it to review the diff of each
task of
[the phase plan](../plans/search-index-and-retired-source-phase-plan.md).
ADR 0078 and ADR 0079 own the rules.

## Each Task

- The diff changes only the files of "Files Likely To Change" of the task. Each
  other change is a deviation in the final report.
- The task touches no file of its "Do Not Touch" list.
- Each acceptance criterion of the task is a test, and the test fails without
  the change.
- The gate of `AGENTS.md` is green: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check`.
- Each new item names its ADR and section.
- A changed old test keeps its purpose. Only its arguments, its columns or its
  types change.
- No `#[allow(...)]` is new. An `#[expect(...)]` has a `reason`.

## ADR 0078 Task 001

- The table definition is the text of ADR 0078 §1.
- The new repair runs in `open_db` before
  `ensure_search_index_track_identity` and
  `ensure_search_index_publisher_feeds`.
- The repair keys on the `sql` of `search_index` in `sqlite_master`, not on the
  migration version. It also runs when `search_entities` has no row and the
  node holds a feed (ADR 0078 §6).
- The drop, the create, the delete of `search_entities` and the fill are in one
  transaction.
- No caller sends the medium or the old text to the index.
- The rowid hash and the collision check do not change.

## ADR 0078 Task 002

- No call of `delete_from_search_index` uses `let _ =` or `.ok()`.
- `delete_feed_sql` deletes the rows of the types `feed` and `publisher` and of
  each track. `delete_track_sql` deletes the row of the track.
- No retire rule, reason, event or block changes.
- The test of each retire path of ADR 0078 §3 exists: admin, `podcast:block`,
  gone source, GUID change and apply.

## ADR 0078 Task 003

- The `SELECT`, the `ORDER BY` and the keyset filter use the same quality
  value.
- No code writes a quality row of the type `publisher`.

## ADR 0078 Task 004

- Control characters are removed before the division into words.
- No `400` body of `GET /v1/search` gives SQLite text.
- The `q` description does not say "FTS5 syntax", in `src/openapi.rs` and in
  `docs/API.md`.
- The log lines of the query text do not change.
- `tests/issue21_fts5_sanitize_tests.rs` is deleted, and the new tests cover
  its cases that still apply.

## ADR 0079 Task 001

- Only `delete_feed_with_event` writes a hold, and only with an empty block
  list.
- The rule is in the `NewFeed` arm, before the ADR 0057 block check, in the
  writer section of ADR 0051 §3.
- The URL comparison is exact string equality, for `source_url` and for
  `canonical_url`.
- A rejected submission writes no row and signs no event.
- The admission from the held URL deletes the hold in the ingest transaction.
- `src/apply.rs` writes no hold.

## ADR 0079 Task 002

- The route checks the admin token before each database read.
- The route is on the primary only, and signs no event.
- The contract gives the route and `retired_source_conflict`.

## ADR 0079 Task 003

- The fill runs on the primary only, after the block seed.
- The fill takes the URL of the last `feed_upserted` event before the retire.
- A second start writes no row.

## After The Last Task

- The release record names each check of ADR 0078 and ADR 0079 and its test.
- After the deploy, the first start logs the rebuild of ADR 0078 §6 and the
  fill of ADR 0079 §4. The operator records the time and the counts.
- `AGENTS.md` and the ADR index change with the release record.
