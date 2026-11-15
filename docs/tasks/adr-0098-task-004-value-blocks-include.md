# ADR 0098 Task 004: The API Gives The Value Blocks

Owner: [ADR 0098](../adr/0098-a-value-block-keeps-its-type-and-method.md) §4.

Repository: `stophammer`. Do this task after task 003.

## Goal

`payment_routes` gives only the Lightning routes that a client can pay. The
new include `value_blocks` of a feed and of a track gives each block. The two
split includes give the block fields.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Include lists | extend | `src/query.rs` | `FEED_INCLUDES` and `TRACK_INCLUDES` get `value_blocks` |
| Route response | reuse | `src/query.rs` | A recipient of a block has the fields of `RouteResponse` |
| Route readers | reuse | `src/db.rs` | The readers of task 003 give the block fields |
| OpenAPI document | extend | `src/openapi.rs` | It declares each new name (ADR 0071 §1c) |

## Files To Inspect

- `src/query.rs`: `FEED_INCLUDES`, `TRACK_INCLUDES`, `FeedResponse`,
  `TrackResponse`, `RouteResponse`, `VtsResponse`, `LocalVtsResponse`,
  `vts_response_from_split`, `local_vts_response_from_split`,
  `build_feed_response`, `build_track_response`, `handle_capabilities`,
  `response_schemas`
- `src/openapi.rs`: `include_query_param` and the path items of a feed and a
  track
- `docs/API.md`: the includes of a feed and of a track, the split contracts and
  the capabilities
- `tests/client_requests_capabilities_tests.rs`,
  `tests/adr0071_row_link_guard_tests.rs`,
  `tests/adr0044_field_removal_tests.rs`,
  `tests/adr0044_schema_refs_tests.rs`, `tests/fixtures/v1-schema-fields.json`,
  `tests/adr0073_split_response_tests.rs`, `tests/adr0060_list_items_tests.rs`

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`
- `src/db.rs`: only if a reader for the feed routes with the block fields is
  necessary for `build_feed_response`
- `docs/API.md`
- `tests/fixtures/v1-schema-fields.json` and each guard test that lists the
  includes
- New: `tests/adr0098_value_blocks_tests.rs`

## Do Not Touch

- The name and the type of each existing field of `payment_routes`,
  `value_time_splits` and `local_value_time_splits` (ADR 0044 §4).
- The writers of the store, the events and the migrations.
- `stophammer-parser/` and `stophammer-crawler/`.
- The ADR files, the index and `AGENTS.md`.

## Constraints

- `payment_routes` of a feed and of a track gives a route only when all these
  conditions are true:
  - `value_type` is `lightning` or `NULL`.
  - `route_type` is `node` or `lnaddress`.
  - A `node` route has an address of 66 hexadecimal characters. Capital and
    small letters are permitted.
- Put the three conditions in one function. The feed path, the track path and
  the feed routes that a track gets use that function.
- A track with no stored route gets the routes of its feed, as before. A track
  with one or more stored routes does not get them, also when the conditions
  remove each route of the track.
- `value_blocks` is an include of a feed and of a track. It is a list of
  blocks. Each block has `block_index` (integer or `null`), `value_type` (text
  or `null`), `value_method` (text or `null`) and `recipients`. Each recipient
  has the seven fields of `RouteResponse`, and `route_type` is the stored text.
- `value_blocks` applies none of the three conditions. It reads the same rows
  as `payment_routes`. A track with no stored route gets the blocks of its
  feed.
- The blocks are in the order of `block_index`. The rows with `NULL` in
  `block_index` give one block with `null` in the three fields, and that block
  is the first one. The recipients of a block keep the stored row order.
- With no `include=value_blocks`, the response has no `value_blocks` key, as
  for the other includes.
- `VtsResponse` and `LocalVtsResponse` get `block_index`, `value_type` and
  `value_method`, each `null` for a row from before this ADR.
- `handle_capabilities` gives `value_blocks` through the two include lists.
  Add no capability name.
- Regenerate the OpenAPI document with `cargo run --bin gen_openapi`, if the
  repository holds a generated copy.
- `docs/API.md` describes `value_blocks`, the three conditions of
  `payment_routes` and the new split fields.
- Read the `rust-dev` skill rules that `AGENTS.md` names. Write each comment
  and each document line in Simplified Technical English. Read
  `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Acceptance Criteria (mechanical)

Each criterion is a test on the HTTP router.

1. A feed with a Lightning block and a `usdc` block: `include=payment_routes`
   gives only the Lightning routes. `include=value_blocks` gives two blocks in
   source order with their types, their methods and each recipient, and the
   `usdc` recipient has `route_type` `address`.
2. The same is true for a track with the two blocks.
3. `payment_routes` gives no route with an unknown `route_type` in a Lightning
   block, and no `node` route with an address that is not 66 hexadecimal
   characters. `value_blocks` gives the two routes.
4. Stored rows with `NULL` block columns: `payment_routes` gives the valid
   Lightning routes, and `value_blocks` gives one block with `null` in
   `block_index`, `value_type` and `value_method`.
5. A track with no stored route gives the filtered routes of its feed in
   `payment_routes` and the blocks of its feed in `value_blocks`. A track with
   only a `usdc` block gives an empty `payment_routes`.
6. `value_time_splits` and `local_value_time_splits` give the three block
   fields of a split in a `usdc` block.
7. `GET /v1/node/capabilities` lists `value_blocks` in the feed includes and
   in the track includes, and the OpenAPI document declares it.
8. `tests/adr0044_field_removal_tests.rs` passes with no removed field.

## Checks

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Report

Give the changed files, the result of each check and each point where the code
did not let you follow this packet.
