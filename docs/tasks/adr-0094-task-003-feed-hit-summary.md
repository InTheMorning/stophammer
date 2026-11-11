# ADR 0094 Task 003: A Feed Hit Gives Its Medium, Kind And Date

Owner: [ADR 0094](../adr/0094-a-feed-hit-states-its-medium-kind-and-date.md) §2, §3
and the capability `search_feed_summary` of §4.

Repository: `stophammer`. This task has no dependency. Task 002 needs it.

## Goal

Each hit with the `entity_type` `feed` or `publisher` gives `medium`,
`release_kind` and `newest_item_at`. A track hit gives none of them.

## Files To Inspect

- `src/query.rs`: `SearchSummary`, `feed_search_summary`, `SearchResponseItem`, the
  map of each hit in `handle_search`, `handle_capabilities`, the read of
  `GET /v1/feeds/recent` that gives `release_kind` and `newest_item_at`
- `src/openapi.rs`: `SearchResponseItem`
- `docs/API.md`: `GET /v1/search` and `GET /v1/node/capabilities`
- The ADR 0044 guards of the response fields
- ADR 0092 §1, for a key that is present with a null value

## Files Likely To Change

- `src/query.rs`
- `src/openapi.rs`
- `docs/API.md`
- New: `tests/adr0094_feed_hit_summary_tests.rs`

## Do Not Touch

- The search parameters and the search index. Tasks 001 and 002 own them.
- `pub_date`. It stays empty on a feed hit (ADR 0094 §3).
- The ADR files, the index and `AGENTS.md`.

## Constraints

- `feed_search_summary` reads the three values in its present query of `feeds`.
  Add no second query for each hit.
- `medium` is the stored `raw_medium`, with no change of case.
- On a feed hit and on a publisher hit, each of the three keys is present also
  when its value is null. On a track hit, the three keys are absent. Find how
  ADR 0092 and ADR 0091 §1a give a present null key, and use the same method.
- Add `search_feed_summary` to the end of the capability list.
- Regenerate the OpenAPI document with `cargo run --bin gen_openapi`.

## Steps

1. Read the three columns in `feed_search_summary`.
2. Add the three fields to the response of a feed hit and a publisher hit.
3. Add the capability.
4. Update the contract and the documents.
5. Write the tests.

## Acceptance Criteria

Mechanical, in `tests/adr0094_feed_hit_summary_tests.rs`:

- ADR 0094 criterion 6: a feed hit and a publisher hit give `medium`,
  `release_kind` and `newest_item_at` with the values of `GET /v1/feeds/recent`
  for the same feed. A feed with no `newest_item_at` gives a present null key.
- A track hit gives none of the three keys.
- ADR 0094 criterion 7, the part of this task: the capabilities list
  `search_feed_summary`.

The ADR 0044 guards pass with the new fields.

Visual: none.

## Test Commands

```bash
cargo test --test adr0094_feed_hit_summary_tests
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi
```

## Final Report

State the files changed and the gate result. Give the response of one feed hit
and one publisher hit from a copy of the production database.

## Escalation Triggers

- A guard or a present test requires an edit for a reason that ADR 0094 does not
  give.
- `GET /v1/feeds/recent` and `feeds` give different values for the same feed.
