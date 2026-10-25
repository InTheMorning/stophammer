# ADR 0080 Task 001: A Phrase In `q` Is Quoted Text

Owner: [ADR 0080](../adr/0080-a-search-takes-phrases-and-field-filters.md)
§1. The phase plan is
[the search filters plan](../plans/search-filters-phase-plan.md).

Repository: `stophammer`. The operator commits. Task 002 starts after this
task, because it uses `fts5_phrases`.

## Goal

Text between two double quotes in `q` is one FTS5 phrase, so it matches its
words in sequence. Each other rule of ADR 0078 §5 stays.

## Files To Inspect

- `src/search.rs`: `fts5_query_from_words` (near line 199) and its call in
  `search` (near line 235)
- `src/query.rs`: the empty check in `handle_search` (near line 5090)
- `src/openapi.rs`: the `q` parameter of `/v1/search` (near line 527)
- `docs/API.md`: the section `GET /v1/search` (near line 1937)
- `tests/adr0078_query_words_tests.rs`

## Files Likely To Change

- `src/search.rs`, `src/openapi.rs`, `docs/API.md`
- `tests/adr0078_query_words_tests.rs`: only the expected value of the case
  `a "b"`
- `tests/adr0080_phrase_tests.rs`, new

## Constraints

- **The function.** Keep the name and the signature of
  `fts5_query_from_words(input: &str) -> String`. Its new steps:
  1. Remove each character for which `char::is_control` is true.
  2. Read the rest from left to right. A double quote opens a phrase only when
     a second double quote comes after it. The phrase ends at that second
     double quote.
  3. A double quote with no second double quote after it is part of its word.
     Double it in the output, as ADR 0078 §5 states.
  4. Divide the text of a phrase with `split_whitespace`, and join its words
     with one space inside one pair of double quotes. A phrase with no word
     adds nothing.
  5. Divide each other text with `split_whitespace`. Each word is one quoted
     FTS5 phrase.
  6. Join the phrases with one space, in the sequence of the input.
- **The phrase list.** Add `pub fn fts5_phrases(input: &str) -> Vec<String>`
  in `src/search.rs`. It gives each quoted FTS5 phrase of steps 1 to 5, in
  sequence. `fts5_query_from_words` joins its result. Task 002 uses it to make
  a column filter for each phrase.
- **A quote inside a word.** An opening quote ends the word before it. Thus
  `a"b c"d` gives `"a" "b c" "d"`.
- **No other change.** No character is an FTS5 operator, a column filter or a
  prefix. The empty result still gives `400` with `q has no word`.
- **The contract.** The `q` description in `src/openapi.rs` and in
  `docs/API.md` says: words and phrases. A phrase is text in double quotes,
  and its words must be in sequence.
- **Comments.** The doc comment of the function names ADR 0080 §1 and
  ADR 0078 §5.

## Do Not Touch

- The `search` function, other than its doc comment. Task 002 changes it.
- `SearchQuery` and each other parameter of `GET /v1/search`
- The index functions, the table and the repairs
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the unit cases of the acceptance criteria as a failing test.
2. Change the function.
3. Change the expected value of the case `a "b"` in
   `tests/adr0078_query_words_tests.rs` to `"a" "b"`. Each other case of that
   file stays.
4. Change the contract text.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0080_phrase_tests.rs`.

- ADR 0080 check 1: `q="night drive"` gives a feed with the title
  `Night Drive`, and not a feed with the title `Drive Night`. `q=night drive`
  gives the two feeds.
- Check 2: `GET /v1/search` gives `200` for each of these `q` values: `"cut`,
  `cut"`, `a"b`, `""` with the word `cut`, `"cut" "`, an apostrophe, and
  `title:"cut"`.
- `q=""` and `q="  "` give `400` with the message `q has no word`.
- `fts5_query_from_words` gives these results:

  | Input | Result |
  |---|---|
  | `"night drive"` | `"night drive"` |
  | `a "b"` | `"a" "b"` |
  | `a"b c"d` | `"a" "b c" "d"` |
  | `"unclosed` | `"""unclosed"` |
  | `"a" "b` | `"a" """b"` |
  | `" x\0y "` | `"xy"` |

- The `q` description in the OpenAPI document contains `double quotes`.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0080_phrase_tests
cargo test --test adr0078_query_words_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A query with a phrase gives an FTS5 error.
- A test of a different file asserts the old result of a quoted word.
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
- `~/build/stophammer/AGENTS.md` (the code style and the pre-commit gate)
- `~/build/stophammer/docs/adr/0080-a-search-takes-phrases-and-field-filters.md`, §1
- `~/build/stophammer/docs/adr/0078-a-search-index-row-holds-only-searchable-text.md`, §5
- `~/build/stophammer/docs/tasks/adr-0080-task-001-a-phrase-is-quoted-text.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- Text between two double quotes in `q` becomes one FTS5 phrase. An unpaired double quote stays part of its word.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Keep the name and the signature of `fts5_query_from_words`.
- Stop at each escalation trigger of this task.

Do not touch:
- the `search` function body, `SearchQuery`, the index functions, the repairs
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`, each migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0080_phrase_tests`
- `cargo test --test adr0078_query_words_tests`
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
