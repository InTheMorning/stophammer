# ADR 0098 Task 003: The Ingest, The Store And The Events Carry The Block Fields

Owner: [ADR 0098](../adr/0098-a-value-block-keeps-its-type-and-method.md) §3
and §5.

Repository: `stophammer`. Do this task after task 002.

## Goal

The three block fields go from the ingest request to the store, to the event
payload and to a community node. A change of only the routes or the value time
splits of a known track gives an event. The sum verifier examines each block.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Ingest records | extend | `src/ingest.rs` | `IngestPaymentRoute` and `IngestValueTimeSplit` get the three fields |
| Route and split records | extend | `src/model.rs` | The event payloads use these records |
| Route and split store | extend | `src/db.rs` | The existing writers and readers get the three columns |
| Change comparison | extend | `src/db.rs` | `build_changed_events` and the `*_changed` functions |
| Sum verifier | extend | `src/verifiers/payment_route_sum.rs` | It adds the splits of each block |

## Files To Inspect

- `src/ingest.rs`: `IngestPaymentRoute`, `IngestValueTimeSplit`
- `src/model.rs`: `PaymentRoute`, `FeedPaymentRoute`, `FeedListValueRaw`,
  `ValueTimeSplitRecipient`, `ValueTimeSplit`
- `src/event.rs`: `TrackUpsertedPayload`, `RoutesReplacedPayload`,
  `FeedRoutesReplacedPayload`, `FeedListValueReplacedPayload`
- `src/api.rs`: `handle_ingest_feed` where it makes the feed routes, the list
  routes and the track routes, and `build_value_time_splits`
- `src/db.rs`: `get_payment_routes_for_track`,
  `get_payment_routes_for_feed_track`, `VALUE_TIME_SPLIT_COLUMNS`,
  `value_time_split_from_row`, `attach_value_time_split_recipients`,
  `insert_value_time_split`, `replace_payment_routes`,
  `replace_payment_routes_for_feed_track`, `replace_feed_payment_routes`,
  `replace_feed_list_value_raw`, `get_feed_payment_routes_for_feed`,
  `get_feed_list_value_raw_for_feed`, `ingest_transaction`,
  `feed_routes_changed`, `feed_list_values_changed`, `build_changed_events`
- `src/apply.rs`: the arms `TrackUpserted`, `RoutesReplaced`,
  `FeedRoutesReplaced` and `FeedListValueReplaced`
- `src/verifiers/payment_route_sum.rs`
- `tests/adr0073_split_store_tests.rs` for the pattern of a signed event test
  and of an old payload test

## Files Likely To Change

- `src/ingest.rs`, `src/model.rs`, `src/api.rs`, `src/db.rs`, `src/apply.rs`
- `src/verifiers/payment_route_sum.rs`
- `src/openapi.rs`: the ingest examples
- `docs/API.md`: the ingest examples
- Each test file that makes one of the changed structs
- New: `tests/adr0098_block_fields_tests.rs`

## Do Not Touch

- The migrations and `src/schema.sql`. Task 002 owns them.
- The responses of the read API in `src/query.rs`. Task 004 owns them. Add
  only what a changed struct makes necessary for the build.
- `stophammer-parser/` and `stophammer-crawler/`.
- The ADR files, the index and `AGENTS.md`.

## Constraints

- The new fields have these types in each record: `value_type:
  Option<String>`, `value_method: Option<String>` and `block_index:
  Option<i64>`. Each has `#[serde(default)]`. A request or an event payload
  with no such field gives `None`, and the store writes `NULL`.
- Do not use `skip_serializing_if` on a record of an event payload if the
  existing fields of that record do not use it. Follow the pattern of the
  record.
- A node applies an event that a node signed before this ADR. The signature
  check uses the stored payload text, so a new optional field does not change
  an old event.
- Each writer stores the three values. Each reader gives them back. The
  readers keep their existing row order. Do not change an `ORDER BY`.
- `feed_routes_changed` and `feed_list_values_changed` compare the three
  fields.
- In `build_changed_events`, a known track with no changed track field gets a
  `TrackUpserted` event when its stored routes or its stored value time splits
  differ from the new ones. Compare the fields that the feed gives. Do not
  compare the row `id`. Read the stored rows with the existing readers.
- `payment_route_sum` groups the routes of a track by `block_index`. Routes
  with `None` are one group. The verifier fails when a group has a sum that
  is not 100, and the message names the track.
- Read the `rust-dev` skill rules that `AGENTS.md` names. Write each comment in
  Simplified Technical English. Read
  `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Acceptance Criteria (mechanical)

Each criterion is a test.

1. An ingest with two blocks on a track stores each route with its
   `value_type`, `value_method` and `block_index`, and the readers give them
   back. The same is true for the feed routes, the list routes, a value time
   split and a local split recipient.
2. An ingest request with no block field stores `NULL` in the three columns.
3. An ingest with a route of `route_type` `address` succeeds. The
   `TrackUpserted` payload holds `address`. A community node applies the
   signed event, and its store gives `address` and the three block values.
4. A `TrackUpserted` payload text from before this ADR, with no block field,
   applies and stores `NULL`.
5. A second ingest of a known feed that changes only one route of one track
   gives a `TrackUpserted` event for that track. A second ingest with no
   change gives no `TrackUpserted` event.
6. A second ingest that changes only the `value_type` of the feed routes gives
   a `FeedRoutesReplaced` event.
7. `payment_route_sum` passes a track with two blocks that each have the sum
   100. It fails a track with one block of sum 100 and one block of sum 90.

## Checks

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Report

Give the changed files, the result of each check and each point where the code
did not let you follow this packet.
