# ADR 0058 Task 004: Resolve A Copy And Relocate A Record

Owner: [ADR 0058](../adr/0058-a-copy-of-a-feed-is-public.md) sections 4 and 5.
Plan: [phase plan](../plans/adr-0058-feed-copies-phase-plan.md), decisions 10
and 11.

## Goal

The operator resolves a copy with `keep_source` or `relocate`. Each relocation
clears `last_build_date` and `declared_self_url`, and needs a reason.

## Files To Inspect

- `src/api.rs`: `handle_patch_feed`, `PatchFeedRequest`, `check_admin_token`,
  `handle_create_block` (as the pattern for an admin write with one signed
  event and fan-out), `build_router`
- `src/event.rs`: `FeedUpsertedPayload`
- `src/apply.rs`: the `FeedUpserted` arm
- The functions of task 001: `get_feed_copy`, `set_feed_copy_resolution`,
  `summary_digest`
- `src/openapi.rs`: `admin_only_security`, the entry of `PATCH /v1/feeds/{guid}`
- `docs/API.md`: the `PATCH /v1/feeds/{guid}` section
- The tests that call `PATCH /v1/feeds/{guid}` with `feed_url`:
  `grep -rln "feed_url" tests | xargs grep -ln "PATCH"`

## Files Likely To Change

- `src/api.rs`, `src/event.rs`, `src/openapi.rs`, `docs/API.md`
- The existing tests of the `PATCH` relocation, only to add a `reason`
- `tests/adr0058_resolve_tests.rs`, new

## Do Not Touch

- `classify_submission`, `src/query.rs`, `src/verify.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **The payload.** `FeedUpsertedPayload` gets `reason: Option<String>`, with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`. The ingest
  path leaves it `None`. The apply step does not read it.
- **One relocation function**, `relocate_feed(tx, feed_guid, new_url, reason,
  signer, now)`. In the transaction of its caller it:
  1. Answers `409` when another record has `new_url` as its source URL. This
     keeps the rule of ADR 0052.
  2. Sets `feed_url`, and sets `last_build_date` and `declared_self_url` to
     null.
  3. Signs one `FeedUpserted` event with the updated feed and the reason.
- **`PATCH /v1/feeds/{guid}`** with `feed_url` calls `relocate_feed`. It
  answers `400` when `reason` is missing or empty after trim. A `PATCH` with
  no `feed_url` keeps its current behavior.
- **`POST /v1/feeds/{guid}/copies/resolve`**, admin token only. The body is
  `{ "url": ..., "decision": "keep_source" | "relocate", "reason": ... }`.
  - `404` when the record or the row does not exist.
  - `400` when the reason is empty, or the decision is not one of the two.
  - `keep_source`: sign one `FeedCopyResolved` with the current
    `summary_digest` of the row.
  - `relocate`: in one transaction, call `relocate_feed` with the row URL,
    then sign `FeedCopyResolved` with the decision `relocate`. The answer
    lists both event IDs.
  - Fan out each event after the commit, as `handle_create_block` does.
- `src/openapi.rs` gets the new path with `admin_only_security`, and the
  `reason` field of `PATCH`. `docs/API.md` documents the route, the decisions,
  the `reason` of `PATCH`, and the two cleared fields. It gives the four
  checks of ADR 0058 section 4 as operator guidance, and names the ADR as their
  owner.
- A short comment names ADR 0058 sections 4 and 5.

## Implementation Steps

1. Add the payload field.
2. Add `relocate_feed`, and call it from `PATCH`.
3. Add the resolve route.
4. Update the existing `PATCH` tests with a reason.
5. Update the OpenAPI document and `docs/API.md`.
6. Add the tests below.
7. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0058_resolve_tests.rs`, through the router:

- A resolve with no admin token answers `403`, and changes nothing.
- `keep_source`: the row has the resolution with the current digest. The feed
  does not change.
- `relocate`: `feed_url` is the row URL. `last_build_date` and
  `declared_self_url` are null. The `FeedUpserted` event carries the reason.
- After `relocate`, a body from the new URL with an older `last_build_date`
  than the value before the relocation is accepted as an update.
- After `relocate`, a body from the old URL is a mirror. A record whose old
  source declared the old URL as its self link does not move back.
- `relocate` to a URL that another record holds as its source answers `409`,
  and changes nothing.
- `PATCH` with `feed_url` and no reason answers `400`. With a reason, it
  clears the two fields.
- A replica that applies the events has the new `feed_url`, a null
  `last_build_date`, and the resolution.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0058_resolve_tests
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

List each existing test that you changed to add a reason.

## Escalation Triggers

Stop and report when:

- `declared_self_url` is not in the `Feed` model and a replica cannot clear
  it. That is correct, because it is local to the primary. Report it only if a
  test of a replica needs it.
- An existing test uses `PATCH` with `feed_url` for a purpose other than a
  relocation.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0058-task-004-resolve-and-relocate.md`, all of it
- `docs/adr/0058-a-copy-of-a-feed-is-public.md`, sections 4 and 5
- `docs/plans/adr-0058-feed-copies-phase-plan.md`, decisions 10 and 11
- The files in "Files To Inspect"

Goal:
- Add `POST /v1/feeds/{guid}/copies/resolve` with `keep_source` and
  `relocate`. Make each relocation clear `last_build_date` and
  `declared_self_url`, and need a reason.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `classify_submission`, `src/query.rs`, `src/verify.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0058_resolve_tests.rs`.
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
