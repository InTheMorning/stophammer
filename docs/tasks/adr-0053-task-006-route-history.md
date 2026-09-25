# ADR 0053 Task 006: Route History

Owner: [ADR 0053](../adr/0053-a-correction-stays-applied.md) section 4.
Plan: [phase plan](../plans/adr-0053-durable-corrections-phase-plan.md),
decisions 10 and 11.

## Goal

A client sees each change of the payment recipients of a feed and its
tracks. The primary logs each change at ingest.

## Files To Inspect

- `src/event.rs`: `FeedRoutesReplacedPayload`, `RoutesReplacedPayload`,
  `TrackUpsertedPayload`, `EventPayload`, and the `Track` field `feed_guid`
- `src/model.rs`: `PaymentRoute`, `FeedPaymentRoute`
- `src/apply.rs`: `deserialize_verified_payload`, as the way to parse a
  stored event
- `src/query.rs`: `query_routes`, `handle_get_feed`, the `QueryResponse`
  envelope
- `src/api.rs`: `handle_ingest_feed`, from step 8b to step 11
- `src/openapi.rs`: the entry of `GET /v1/feeds/{guid}`
- `docs/API.md`: the feed routes

## Files Likely To Change

- `src/query.rs`
- `src/db.rs`: one read function for the events of a feed, when needed
- `src/api.rs`: the log in `handle_ingest_feed`
- `src/openapi.rs`
- `docs/API.md`
- `tests/adr0053_route_history_tests.rs`, new

## Do Not Touch

- `src/apply.rs`, `src/event.rs`, `src/verify.rs`, `src/main.rs`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **The route.** `GET /v1/feeds/{guid}/route-history`, in
  `query::query_routes`, so a community node serves it too.
- **The read.** Events of types `feed_routes_replaced`, `routes_replaced` and
  `track_upserted`, in `seq` order. An event belongs to the feed when one of
  these fields matches the feed GUID:
  - `feed_guid` of `feed_routes_replaced` and of `routes_replaced`,
  - `track.feed_guid` of `track_upserted`.

  Parse each payload as `apply.rs` does. Filter in SQL where the
  payload allows it, for example `subject_guid` for the feed events.
- **The recipient set.** The ordered list of `{ "address", "split" }` of the
  routes. The name, the route type and `fee` are not part of the set.
- **An entry.** Written only when the set of a subject differs from its last
  set. Fields: `subject` (`"feed"` or `"track"`), `track_guid` (null for the
  feed), `event_id`, `seq`, `changed_at` (the event `created_at`),
  `old_recipients` (null for the first set) and `new_recipients`.
- The answer uses the `QueryResponse` envelope of the other query routes, with
  at most 1,000 entries, the newest last. It answers `404` when no event names
  a route set of the feed.
- **The log.** In `handle_ingest_feed`, before the ingest transaction, read
  the stored recipient set of the feed and of each track in the submission.
  For each set that differs from the new set, log `tracing::warn!` with the
  fields `feed_guid`, `track_guid`, `old_recipients` and `new_recipients`. A
  new track or a new feed logs nothing.
- `src/openapi.rs` declares the route and the entry schema. `docs/API.md`
  gives the route.
- Short comments name ADR 0053 section 4.

## Implementation Steps

1. Add the read and the route.
2. Add the ingest log.
3. Add the OpenAPI entry and the `docs/API.md` text.
4. Add the tests below.
5. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0053_route_history_tests.rs`, through the
router:

- A feed with a feed route `a@ln.example` split 100 and one track with a route
  `t@ln.example` split 100. Then an update of the feed route to
  `b@ln.example`. The history has three entries: the first feed set, the
  first track set, and the feed change with `old_recipients` `a@` and
  `new_recipients` `b@`.
- An update that changes only a route name: no new entry.
- An update that changes only a split: a new entry.
- A GUID with no events: `404`.
- The same history from a second database that applied the events of the
  first: the answers are equal.
- The gate is green, and `gen_openapi` shows the route.

## Test Commands

```bash
cargo build
cargo test --test adr0053_route_history_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi | grep -c route-history
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- A `track_upserted` payload does not carry the routes of the track.
- The ingest diff emits a route change through an event type not in this
  packet.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0053-task-006-route-history.md`, all of it
- `docs/plans/adr-0053-durable-corrections-phase-plan.md`, decisions 10
  and 11
- The files in "Files To Inspect"

Goal:
- Add `GET /v1/feeds/{guid}/route-history` from the signed events, and a
  `warn` log at ingest for each change of a recipient set.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `src/apply.rs`, `src/event.rs`, `src/verify.rs`, `src/main.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0053_route_history_tests.rs`.
- The gate is green. `gen_openapi` shows the route.

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
