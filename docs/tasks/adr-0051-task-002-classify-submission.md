# ADR 0051 Task 002: The Classification Function

Owner: [ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md)
section 2. Plan: [phase plan](../plans/adr-0051-source-url-phase-plan.md),
decision 4.

## Goal

One read-only function gives the case of ADR 0051 section 2 for a submission.
No handler calls it in this task.

## Files To Inspect

- `docs/adr/0051-feed-content-comes-from-its-source-url.md`, section 2
- `src/db.rs`: `get_feed`, `get_existing_feed`, the `DbError` type, and the
  section comments of the form `// ── name ──`
- `tests/db_tests.rs`: how a test inserts a feed row with its artist credit
- `tests/common/mod.rs`: `test_db`

## Files Likely To Change

- `src/db.rs`: one enum and one function, near `get_existing_feed`
- `tests/adr0051_classify_tests.rs`, new

## Do Not Touch

- `src/api.rs`, `src/verify.rs`, `src/ingest.rs`, `src/openapi.rs`
- `tests/adr0051_source_url_tests.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

The enum and the function:

```rust
/// The case of ADR 0051 section 2 for one submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmissionClass {
    /// `source_url` or `canonical_url` is the source URL of a record with a
    /// different GUID, and a record has the declared GUID.
    RecordConflict,
    /// `source_url` or `canonical_url` is the source URL of a record with a
    /// different GUID, and no record has the declared GUID.
    GuidChange { held_guid: String },
    /// The record with the declared GUID has `source_url` or `canonical_url`
    /// as its source URL.
    Update,
    /// A record has the declared GUID, and its source URL is a different URL.
    Mirror { source_url: String },
    /// No record has the declared GUID.
    NewFeed,
}

pub fn classify_submission(
    conn: &Connection,
    feed_guid: &str,
    source_url: &str,
    canonical_url: &str,
) -> Result<SubmissionClass, DbError>
```

- The function applies the cases in the order of the enum. The first case
  that matches is the result.
- Two queries: the `feed_url` of the row with `feed_guid = ?1`, and the
  `feed_guid` of a row with `feed_url IN (?2, ?3) AND feed_guid <> ?1`.
- The URL comparison is exact string equality. No normalization, no trim, no
  change of case.
- The function only reads. It writes no row and opens no transaction.
- A doc comment on the function names ADR 0051 section 2.
- The `stophammer` lints apply. Read `[lints]` in `Cargo.toml`.

## Implementation Steps

1. Add the enum and the function to `src/db.rs`, with a section comment in
   the style of the file.
2. Create `tests/adr0051_classify_tests.rs` with one test for each case below.
3. Run the gate.

## Acceptance Criteria

Mechanical. Each row is one test in `tests/adr0051_classify_tests.rs`. Feed A
has GUID `GA` at URL `UA`. Feed B has GUID `GB` at URL `UB`.

| Submission (`guid`, `source_url`, `canonical_url`) | Result |
|---|---|
| `GA`, `UA`, `UA` | `Update` |
| `GA`, `UA`, `UX` (a redirect from the source URL) | `Update` |
| `GA`, `UX`, `UA` (a redirect to the source URL) | `Update` |
| `GA`, `UX`, `UX` | `Mirror { source_url: UA }` |
| `GA`, `UB`, `UB` | `RecordConflict` |
| `GA`, `UA`, `UB` | `RecordConflict` |
| `GN` (not held), `UA`, `UA` | `GuidChange { held_guid: GA }` |
| `GN`, `UX`, `UX` | `NewFeed` |
| `GA`, `UA/`, `UA/` (a trailing slash added) | `Mirror { source_url: UA }` |

- A test counts the rows of `feeds` before and after one call. The count and
  the content do not change.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0051_classify_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- `feeds.feed_url` is not unique in the schema.
- A feed row cannot be inserted in a test without the ingest handler.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0051-task-002-classify-submission.md`, the sections
  "Constraints" and "Acceptance Criteria"
- `docs/adr/0051-feed-content-comes-from-its-source-url.md`, section 2
- `src/db.rs`: `get_feed`, `get_existing_feed`, `DbError`
- `tests/db_tests.rs`: how a test inserts a feed row
- `tests/common/mod.rs`

Goal:
- Add `SubmissionClass` and `classify_submission` to `src/db.rs`, exactly as
  the task file gives them, with tests.

Constraints:
- The cases apply in the order of the enum. The first match is the result.
- Exact string comparison of URLs. No normalization.
- Read only. No write, no transaction.
- A doc comment names ADR 0051 section 2.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `src/api.rs`, `src/verify.rs`, `src/ingest.rs`, `src/openapi.rs`
- `tests/adr0051_source_url_tests.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- One test for each row of the table in the task file, in the new file
  `tests/adr0051_classify_tests.rs`.
- A test shows that a call changes no row.
- The gate is green.

Test commands:
- `cargo build`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
