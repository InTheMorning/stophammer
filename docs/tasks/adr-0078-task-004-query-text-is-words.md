# ADR 0078 Task 004: The Text Of `q` Is Words

Owner: [ADR 0078](../adr/0078-a-search-index-row-holds-only-searchable-text.md)
§5. The phase plan is
[the search index and retired source plan](../plans/search-index-and-retired-source-phase-plan.md).

Repository: `stophammer`. The operator commits. This task can run at the same
time as tasks 001 to 003.

## Goal

The node makes an FTS5 query of quoted phrases from `q`. Each `q` with a word
gives `200`. A `400` of `GET /v1/search` gives no SQLite text. The contract
describes `q` as words.

## Files To Inspect

- `src/search.rs`: `sanitize_fts5_query` (near line 224) and its call in
  `search` (near line 272)
- `src/query.rs`: the search handler and its error mapping (near lines 5080 to
  5195)
- `src/openapi.rs`: the `q` parameter of `/v1/search` (near line 527)
- `docs/API.md`: the section `GET /v1/search` (near line 1932)
- `tests/issue21_fts5_sanitize_tests.rs`
- `tests/search_tests.rs`, `tests/api_docs_tests.rs`

## Files Likely To Change

- `src/search.rs`, `src/query.rs`, `src/openapi.rs`, `docs/API.md`
- `tests/issue21_fts5_sanitize_tests.rs`, deleted
- `tests/adr0078_query_words_tests.rs`, new

## Constraints

- **The function.** Replace `sanitize_fts5_query` with
  `fts5_query_from_words(input: &str) -> String`, in `src/search.rs`:
  1. Remove each character for which `char::is_control` is true.
  2. Divide the rest with `split_whitespace`.
  3. Put each word in double quotes, and double each `"` in the word.
  4. Join the phrases with one space.
- **No word.** When the result is empty, the handler gives `400` with the
  message `q has no word`. Today `search` gives an empty list with `200` for
  this case. `q` is a required `String` of `SearchQuery`, so Axum already gives
  `400` for a missing `q`.
- **The error message.** A `400` of the handler gives a message that the
  handler writes, with no text from SQLite. Log the SQLite text at `warn`
  level with `tracing`. The `unsupported search type filter` message stays,
  because the node writes it.
- **The contract.** The `q` description in `src/openapi.rs` and in
  `docs/API.md` says: words, and a hit contains each word. It no longer says
  "FTS5 syntax".
- **The old tests.** Delete `tests/issue21_fts5_sanitize_tests.rs`. The tests of
  this task replace it. Keep the purpose of each old test that still applies:
  an unclosed quote, an operator word and a bracket each give no error.
- **The log of `q`.** Do not change the `tracing::info!` lines that log the
  query text. A different decision owns them.
- **Comments.** The new function names ADR 0078 §5. Remove the comment
  `Issue-21 FTS5 sanitize` with the old function.

## Do Not Touch

- The index functions, the table and the repairs. Task 001 owns them.
- The quality join. Task 003 owns it.
- The `type`, `limit`, `cursor` and `include` parameters
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the new function and its unit tests.
2. Change the call in `search` and the empty case in the handler.
3. Change the error mapping.
4. Change the contract text in `src/openapi.rs` and `docs/API.md`.
5. Delete the old test file, and write the new tests.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0078_query_words_tests.rs`.

- ADR 0078 check 8: `GET /v1/search` with each of these `q` values gives `200`:
  `haleen'`, `a"b`, `"unclosed`, `title:foo`, `artist:foo`, `cut*`, `-cut`,
  `(cut)`, `^cut`, `AND`, `NEAR`, `cut OR band`, a value with a NUL character,
  and `'` alone.
- `q=title:foo` does not filter the `title` column. A feed with `foo` in its
  description and not in its title is a hit.
- A `q` of only white space, and a `q` of only control characters, give `400`
  with the message `q has no word`.
- Check 9: no `400` body of `GET /v1/search` contains `SQLite` or `fts5`.
- The `q` description in the OpenAPI document does not contain `FTS5 syntax`.
- `fts5_query_from_words("a \"b\"")` gives `"a" """b"""`.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0078_query_words_tests
cargo test --test api_docs_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A test asserts a column filter, a prefix match or an FTS5 operator in `q`.
- A client document in this repository depends on FTS5 syntax in `q`.
- A query with a word gives an FTS5 error after this change.
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
- `~/build/stophammer/docs/adr/0078-a-search-index-row-holds-only-searchable-text.md`, §5
- `~/build/stophammer/docs/tasks/adr-0078-task-004-query-text-is-words.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- Each word of `q` becomes a quoted FTS5 phrase, each `q` with a word gives `200`, and a `400` gives no SQLite text.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Remove control characters before the division into words.
- Do not change the log lines of the query text.
- Stop at each escalation trigger of this task.

Do not touch:
- the index functions, the table, the repairs, the quality join, the other search parameters
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0078_query_words_tests`
- `cargo test --test api_docs_tests`
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
