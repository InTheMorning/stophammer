# ADR 0071 Task 005: Search Finds A Publisher Feed

Owner: [ADR 0071](../adr/0071-a-list-row-links-what-it-names.md) section 5,
and the amendment of 2026-10-18 of ADR 0038 Decision B. v4vmm request 8,
gap D.

Repository: `stophammer`. The operator commits.

## Goal

`GET /v1/search?type=publisher` gives the feeds with the medium `publisher`
whose text matches `q`. Each other search gives the same results as before.

## Files To Inspect

- `src/db.rs`: `sync_source_read_models_for_feed`, `open_db`,
  `ensure_search_index_track_identity` (the pattern of a repair at the start)
- `src/search.rs`: `populate_search_index`, `delete_from_search_index`,
  `search`
- `src/query.rs`: `handle_search`, `feed_search_summary`,
  `handle_capabilities`
- `src/openapi.rs`: the path `/v1/search`, and the `entity_types` of the
  capabilities
- `tests/search_tests.rs`, `tests/search_one_hit_per_entity_tests.rs`,
  `tests/client_requests_search_fields_tests.rs`

## Files Likely To Change

- `src/db.rs`, `src/search.rs`, `src/query.rs`, `src/openapi.rs`
- `tests/adr0071_publisher_search_tests.rs`, new
- `docs/API.md`

## Constraints

- **Entity type.** The index holds a publisher feed with the `entity_type`
  `publisher`, and its title, artist text and description, as a feed.
- **Medium change.** When a feed changes its medium, the sync deletes the row
  of the old type and writes the row of the new type.
- **Other mediums.** A `musicL` feed and a feed with no medium stay out of the
  index.
- **No change for other searches.** `type=feed`, `type=track` and a search with
  no `type` read only the types `feed` and `track`.
- **Row shape.** A `publisher` row has the fields of a feed row, with
  `entity_type` `publisher`.
- **Start.** Add `ensure_search_index_publisher_feeds`, called from `open_db`.
  It adds each publisher feed that has no row in `search_entities`. A database
  where each publisher feed has a row skips it. It logs the count and the time
  at `info`.
- Declare `publisher` in the `type` parameter, and in the capabilities.
- No migration, no event change and no ingest change.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- `GET /v1/publishers` and `handle_publisher_search`, which search the
  `publisher` text of the music feeds
- The ranking of search
- Each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Index a publisher feed in `sync_source_read_models_for_feed`.
2. Accept `type=publisher` in `handle_search`.
3. Add the repair at the start. Run it on a copy of the newest backup in
   `backups/`, and record the count and the time.
4. Write the tests, then `docs/API.md`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0071_publisher_search_tests.rs`.

- `type=publisher` with a word of a publisher feed title gives that feed, with
  `entity_type` `publisher`.
- `type=feed` and a search with no `type` give no publisher feed for the same
  word.
- A feed that changes its medium from `publisher` to `music` leaves the
  `publisher` results, and enters the `feed` results.
- The repair adds the publisher feeds of a database with no such rows, and a
  second open adds none.
- The existing search tests pass with no change.
- The ADR 0044 guards and the ADR 0071 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0071_publisher_search_tests
cargo test --test search_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report, with no workaround, when:

- An existing search test needs a change.
- The repair needs to clear the index.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. the count and the time of the repair on the backup
5. deviations from this task
6. unresolved concerns
