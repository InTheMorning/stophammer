# ADR 0052 Task 001: The Self-Link Move

Owner: [ADR 0052](../adr/0052-a-source-moves-its-own-feed.md), the self-link
trigger. Plan: [phase plan 1](../plans/adr-0052-self-link-phase-plan.md),
decisions 1 to 5.

## Goal

A source ingest records the self link of its body. A later mirror submission
at exactly that URL moves the record there and applies the content.

## Files To Inspect

- `docs/adr/0052-a-source-moves-its-own-feed.md`, sections 1, 2 and 6, and
  "Guards"
- `docs/adr/0051-feed-content-comes-from-its-source-url.md`, section 2
- `migrations/0038_feed_blocks.sql` and the `MIGRATIONS` array in
  `src/db.rs`, as the pattern
- `src/api.rs`: `handle_ingest_feed`, the whole function, and
  `build_source_entity_links` (the `self_feed` link type)
- `src/db.rs`: `classify_submission`, `ingest_transaction`, `get_feed`
- `src/proof.rs`: `revoke_tokens_for_feed`
- `src/ingest.rs`: `IngestLink`
- `tests/adr0051_source_url_tests.rs`: the helpers and the mirror test
- `tests/migration_tests.rs`: the tests that name a migration watermark

## Files Likely To Change

- `migrations/0039_feed_declared_self_url.sql`, new
- `src/schema.sql`
- `src/db.rs`: the `MIGRATIONS` array, and small read and write functions for
  the column
- `src/api.rs`: `handle_ingest_feed`
- `tests/migration_tests.rs`: watermark numbers only
- `docs/API.md`: the ingest section
- `tests/adr0052_self_link_tests.rs`, new

## Do Not Touch

- `classify_submission` and `SubmissionClass`. The move is a handler step
  after the classification.
- `src/model.rs`: the `Feed` struct does not get the field.
- `src/event.rs`, `src/apply.rs`. No event type.
- `stophammer-parser`, `stophammer-crawler`

## Constraints

- The migration and the column of plan decision 1.
- `db::set_declared_self_url(conn, feed_guid, Option<&str>)` and
  `db::get_declared_self_url(conn, feed_guid) -> Result<Option<String>>`.
- The write of plan decision 2. It happens in the same transaction as the
  content. `ingest_transaction` owns that transaction. Add the write in it
  through a new parameter, or write it right after it under the same writer
  lock. Report which one you chose and why.
- The move of plan decision 3. The comparison is exact string equality with
  `req.source_url` or `req.canonical_url`.
- After a move, the rest of the handler runs as an update. Step 7b uses the
  new URL. The hash cache is written for `req.canonical_url`, as for an
  update.
- The proof tokens of the record are revoked in the move. Use
  `proof::revoke_tokens_for_feed`.
- A conflict case of ADR 0051 never moves.
- The log of plan decision 5. Short comments name ADR 0052 section 2.
- `docs/API.md` gives the move and its warning text.

## Implementation Steps

1. Add the migration, the schema column and the two functions.
2. Add the write in the update and new-feed cases.
3. Add the move in the mirror case.
4. Update the migration watermark tests.
5. Add the tests below.
6. Run the gate, and `cargo test --test migration_tests`.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0052_self_link_tests.rs`, through the router,
with a chain of `["content_hash"]`. `UA` is
`https://wavlake.example/feed/a`. `UM` is
`https://wavlake.example/feed/music/a`.

- **Record.** A feed admitted at `UA` with a self link `UM` has
  `declared_self_url` `UM`.
- **Move.** Then a submission at `UM` with the same GUID and a new title:
  `accepted: true`, the stored `feed_url` is `UM`, the title is new, the
  warning names both URLs, and one `feed_upserted` event carries `feed_url`
  `UM`.
- **After the move.** A submission at `UA` is a mirror: `source_conflict`
  with `source_url` `UM`.
- **No record, no move.** A feed admitted at `UA` with no self link, then a
  submission at `UM`: `source_conflict`, and `feed_url` stays `UA`.
- **A different URL.** A self link `UM`, then a submission at
  `https://wavlake.example/feed/other`: `source_conflict`.
- **Old links do not count.** A feed with a `self_feed` row in
  `source_entity_links` and a null `declared_self_url`, then a submission at
  that URL: `source_conflict`.
- **A mirror body does not write the column.** A mirror submission with a
  self link `UX` leaves `declared_self_url` unchanged.
- **Tokens.** A proof token for the record exists before a move and does not
  exist after it.
- The gate is green, and `cargo test --test migration_tests` passes.

## Test Commands

```bash
cargo build
cargo test --test adr0052_self_link_tests
cargo test --test migration_tests
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

- An existing test fails for a reason other than a migration watermark.
- The mirror case writes a row before the place of the move check.
- A test token cannot be made with the existing proof helpers. Report it and
  leave out only that test.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0052-task-001-self-link-move.md`, all of it
- `docs/plans/adr-0052-self-link-phase-plan.md`, decisions 1 to 5
- The files in "Files To Inspect"

Goal:
- Add `feeds.declared_self_url`, written only by a source ingest. A mirror
  submission at exactly that URL moves the record there, revokes its proof
  tokens, and applies the content as an update.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `classify_submission`, `SubmissionClass`, `src/model.rs`, `src/event.rs`,
  `src/apply.rs`
- `stophammer-parser`, `stophammer-crawler`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0052_self_link_tests.rs`.
- The gate is green. `cargo test --test migration_tests` passes.

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
