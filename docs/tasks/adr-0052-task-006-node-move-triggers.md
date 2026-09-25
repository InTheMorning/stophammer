# ADR 0052 Task 006: Node Move Triggers

Owner: [ADR 0052](../adr/0052-a-source-moves-its-own-feed.md) sections 1 and
2. Plan: [phase plan 2](../plans/adr-0052-moves-and-guid-changes-phase-plan.md),
decisions 4 to 7.

Repository: `stophammer`. Needs task 003.

## Goal

A permanent redirect from the source URL, or an `itunes:new-feed-url` that the
source URL declares, moves the record. The self link keeps working.

## Files To Inspect

- `src/ingest.rs`: `IngestFeedRequest`, `IngestFeedData`
- `src/api.rs`: `handle_ingest_feed`: the read-phase move test of task 003,
  the `Mirror` arm with `self_link_move`, step 7b, step 11a, and
  `relocate_feed`
- `src/db.rs`: `classify_submission`, `get_declared_self_url`,
  `set_declared_self_url`, the `MIGRATIONS` array
- `src/schema.sql`, `migrations/0040_feed_copies.sql`
- `tests/adr0052_self_link_tests.rs`, `tests/adr0058_resolve_tests.rs`

## Files Likely To Change

- `migrations/0041_feed_move_declarations.sql`, new
- `src/schema.sql`, `src/db.rs`, `src/ingest.rs`, `src/api.rs`
- `tests/migration_tests.rs`: the watermark tests
- `docs/API.md`: the ingest route
- `tests/adr0052_move_trigger_tests.rs`, new

## Do Not Touch

- `classify_submission`, `src/verify.rs`, `src/query.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **The contract.** `IngestFeedRequest` gets `redirects: Vec<RedirectHop>`
  with `#[serde(default)]`. `RedirectHop { url: String, status: u16 }`.
  `IngestFeedData` gets `new_feed_url`, `locked` and `locked_owner` as in the
  parser, with `#[serde(default)]`.
- **The migration.** `0041` adds `declared_new_feed_url TEXT`,
  `podcast_locked INTEGER` and `locked_owner TEXT` to `feeds`. Add the next
  entry to `MIGRATIONS` and the columns to `src/schema.sql`.
- **The writes.** Step 11a writes the three columns with `declared_self_url`,
  only in the update and new-feed cases, never on a move or a mirror.
  `relocate_feed` also clears `declared_new_feed_url`.
- **One move test.** Extend the private move function of task 003. It returns
  the move target and its trigger, or nothing:
  - `Mirror`, and `req.source_url` or `req.canonical_url` equals
    `declared_new_feed_url`: trigger `new-feed-url`.
  - `Mirror`, and one of them equals `declared_self_url`: trigger `self link`.
  - `Update`, `req.source_url` is the stored source URL, `req.redirects` is
    not empty, each hop is `301` or `308`, and `req.canonical_url` is not the
    stored source URL: trigger `permanent redirect`, target
    `req.canonical_url`.
  The read phase and the write phase use this function, as in task 003.
- **The conflict.** When the target is the source URL of a different record,
  the node answers `accepted: false`, `reason: "record_conflict"`, and writes
  nothing.
- **The move.** Each trigger uses the move path of the self link: it applies
  the body and sets `feed_url` to the target in the same transaction. The
  warning and the log name the trigger:
  `moved from <old> to <new> (ADR 0052 <trigger>)`.
- A `302`, `303` or `307` hop in the list is not a move. The node applies the
  content and keeps the source URL, as today.
- `docs/API.md` documents `redirects`, the three fields, and the three
  triggers.

## Implementation Steps

1. Add the contract fields and the migration.
2. Extend the writes of step 11a and of `relocate_feed`.
3. Extend the move function, and use its trigger in the move path.
4. Add the tests below.
5. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0052_move_trigger_tests.rs`, through the router:

- A record at A. A submission with `source_url` A, `canonical_url` B,
  `redirects` `[{A, 301}]`, and the same GUID: the record moves to B, and the
  warning names `permanent redirect`.
- The same with `[{A, 302}]`: the content applies, and `feed_url` stays A.
- The same with `[{A, 301}, {X, 302}]`: no move.
- The same with `[{A, 308}]`, when B is the source URL of a different record:
  `record_conflict`, and nothing changes.
- A record at A whose body declared `new_feed_url` N. A submission from N
  with the same GUID: the record moves to N, and the warning names
  `new-feed-url`.
- A mirror body that declares `new_feed_url` N does not write the column. A
  later submission from N does not move the record.
- A relocation clears `declared_new_feed_url`.
- A request with no `redirects` key and no new fields is accepted as before.
- The self-link tests pass unchanged.
- The gate is green, with `cargo test --test migration_tests`.

## Test Commands

```bash
cargo build
cargo test --test adr0052_move_trigger_tests
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

- `classify_submission` gives a case other than `Update` for a submission
  whose `source_url` is the stored source URL and whose `canonical_url`
  differs. Report the case, and do not change the function.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0052-task-006-node-move-triggers.md`, all of it
- `docs/adr/0052-a-source-moves-its-own-feed.md`, sections 1 and 2
- `docs/plans/adr-0052-moves-and-guid-changes-phase-plan.md`, decisions 4 to 7
- The files in "Files To Inspect"

Goal:
- Add the permanent-redirect and new-feed-url moves, with the self-link move
  in one move function.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `classify_submission`, `src/verify.rs`, `src/query.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0052_move_trigger_tests.rs`.
- The gate is green.

Test commands:
- The commands of "Test Commands".

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
