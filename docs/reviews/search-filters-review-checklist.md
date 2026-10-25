# Review Checklist: Search Phrases And Field Filters

This checklist is advisory. The orchestrator uses it to review the diff of each
task of [the phase plan](../plans/search-filters-phase-plan.md). ADR 0080 owns
the rules.

## Each Task

- The diff changes only the files of "Files Likely To Change" of the task. Each
  other change is a deviation in the final report.
- The task touches no file of its "Do Not Touch" list.
- Each acceptance criterion of the task is a test, and the test fails without
  the change.
- The gate of `AGENTS.md` is green: `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check`.
- Each new item names ADR 0080 and its section.
- `src/openapi.rs`, `docs/API.md` and `tests/fixtures/v1-schema-fields.json`
  change in the same diff as the API surface that they show.
- No `#[allow(...)]` is new. An `#[expect(...)]` has a `reason`.
- No new dependency and no new crate feature.

## ADR 0080 Task 001

- `fts5_query_from_words` keeps its signature, and joins the result of
  `fts5_phrases`.
- An unpaired double quote stays part of its word, and the output doubles it.
- The only changed case of `tests/adr0078_query_words_tests.rs` is `a "b"`.

## ADR 0080 Task 002

- No request text goes into the SQL text. Each value is a bound parameter.
- Each filter is in the outer `WHERE`, not in the `MATCH` of `q`. A test proves
  that the rank does not change.
- The handler, `GET /v1/node/capabilities` and the contract read one table,
  `SEARCH_FILTERS`.
- The checks give their errors in the sequence of the task: unknown name, type,
  value, positive term.
- The body of each error has `error` and `code`. Each code other than
  `missing_positive_term` has `filter`.
- A request that the old handler accepted, with only the old parameters, gives
  the same rows and the same cursor.

## ADR 0080 Task 003

- `kind=unknown` also matches a NULL kind.
- `after` and `before` include their day in UTC.
- The `time` crate has no new feature.
- A track reads the kind and the count of its feed.

## ADR 0080 Task 004

- The migration only adds the column. A repair of ADR 0046 fills it.
- Each statement that writes `feed_url` writes `feed_host` in the same
  statement.
- No event payload and no `Feed` field changes.
- `host=wavlake.com` does not match `notwavlake.com`.

## ADR 0080 Task 005

- The query starts from `search_entities`, so a feed that the index does not
  hold does not show.
- The rows are newest first, a row with no date comes last, and the rowid
  breaks a tie.
- A cursor of one form gives `400` on the other form.
- A search with `q` keeps its order, its rank and its cursor.
