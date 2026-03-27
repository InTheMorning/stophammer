# Targeted `musicL` Backfill Plan

This document captures the plan for filling `stophammer.db` with newly
accepted `musicL` feeds without rerunning the full PodcastIndex importer.

The intended model is:

- use crawler state DBs as a discovery index
- identify `musicL` feeds that were already fetched successfully
- re-fetch only those missing feeds through the normal ingest path
- avoid any importer rerun or synthetic direct DB writes

## Why

The crawler state DBs already preserve enough metadata to discover likely
`musicL` candidates:

- `feed_url`
- `fetch_http_status`
- `raw_medium`
- `parsed_feed_guid`

However, they do not preserve enough data to reconstruct full ingest from state
alone:

- no parsed feed payload
- no feed title/description snapshot
- no source claims
- no remote item snapshot
- no raw XML body

That means a state-driven backfill is viable only as a narrow targeted re-fetch
tool, not as a metadata-only database reconstruction.

## Invariants

- Do not rerun the full importer.
- Do not mutate importer progress state as part of the backfill.
- Network fetch remains the source of truth for the final ingest.
- `musicL` support in the main ingest path must land before this backfill runs.
- `musicL` feeds must not enter resolver-driven canonical or identity work.
- Backfill scope is `musicL` only in the first pass.

## Code Touchpoints

Primary implementation files:

- [stophammer-crawler/src/main.rs](/home/citizen/build/stophammer/stophammer-crawler/src/main.rs)
- [stophammer-crawler/src/crawl.rs](/home/citizen/build/stophammer/stophammer-crawler/src/crawl.rs)
- [stophammer-crawler/src/modes/mod.rs](/home/citizen/build/stophammer/stophammer-crawler/src/modes/mod.rs)
- [stophammer-crawler/src/modes/import.rs](/home/citizen/build/stophammer/stophammer-crawler/src/modes/import.rs)
- [stophammer-crawler/README.md](/home/citizen/build/stophammer/stophammer-crawler/README.md)

Primary data sources:

- [import_state.db](/home/citizen/build/stophammer/import_state.db)
- [import_state_wavlake.db](/home/citizen/build/stophammer/import_state_wavlake.db)
- [stophammer.db](/home/citizen/build/stophammer/stophammer.db)

Current limitation to preserve:

- [gossip_state.db](/home/citizen/build/stophammer/gossip_state.db) currently has
  no importer-like per-feed memory, so it is not a useful source for this
  backfill pass.

## Phase Plan

### Phase 1: Candidate Discovery From Crawler State

Scope:

- Add a new `stophammer-crawler` operational subcommand for state-driven
  backfill.
- Read candidates from:
  - `./import_state.db`
  - `./import_state_wavlake.db`
- Ignore missing state DB files.
- Ignore DBs that do not expose `import_feed_memory`.

Candidate selection rule:

- `fetch_http_status = 200`
- `lower(raw_medium) = 'musicl'`
- `parsed_feed_guid IS NOT NULL`
- feed does not already exist in `stophammer.db`

Implementation notes:

- Deduplicate by `parsed_feed_guid`.
- Use `feed_url` as fallback identity only if a candidate lacks a GUID in a
  future extension; v1 should require `parsed_feed_guid`.
- Discovery logic should emit a clear summary before any fetches start.

Acceptance:

- Dry-run lists candidate totals and sample URLs.
- Duplicate rows across state DBs collapse to one candidate.
- Missing or non-import state DBs do not fail the run.

### Phase 2: Narrow Re-Fetch and Ingest

Scope:

- Re-fetch only the selected candidate URLs using the shared crawl pipeline.
- Submit them through the normal `/ingest/feed` endpoint.
- Do not rerun batch import logic.

Implementation notes:

- Reuse `crawl_feed()` / shared fetch-parse-post flow rather than special-case
  direct ingestion code.
- The new command is a replay helper, not a second importer.
- It should not write to `import_progress` or `import_feed_memory`.

Acceptance:

- Only selected `musicL` candidate URLs are fetched.
- Successful ingests populate `stophammer.db` through the canonical path.
- Importer state DBs remain unchanged.

### Phase 3: Resolver Isolation Verification

Scope:

- Ensure the backfilled `musicL` feeds stay out of resolver work.
- Verify that accepted `musicL` feeds do not pollute canonical, promotion, or
  identity layers.

Implementation notes:

- This phase depends on the separate `musicL` ingest design landing first:
  - accepted medium
  - resolver exclusion
  - container-only semantics
- Backfill should fail fast or warn clearly if run against a binary that still
  treats `musicL` as irrelevant or resolver-eligible.

Acceptance:

- Post-backfill `feeds.raw_medium = 'musicL'` rows exist.
- `resolver_queue` contains no `musicL` feeds.
- No canonical-state side effects are introduced by the backfill itself.

### Phase 4: Operator Docs and Verification Queries

Scope:

- Document the backfill command and expected workflow in crawler docs.
- Add SQL verification examples for discovery and post-run validation.

Operator workflow:

1. Land `musicL` ingest support first.
2. Run the backfill in dry-run mode.
3. Inspect candidate count and sample URLs.
4. Run the real backfill.
5. Verify `musicL` feed presence in `stophammer.db`.
6. Verify resolver exclusion.

Acceptance:

- README shows a dry-run example and a real-run example.
- Operators have SQL examples for:
  - counting `musicL` candidates in state DBs
  - counting `musicL` feeds in `stophammer.db`
  - confirming `resolver_queue` exclusion

## Recommended Delivery Shape

Split the work into small reviewable PRs:

1. `musicL` ingest support and resolver exclusion.
2. State-driven `musicL` backfill command.
3. Docs and operator verification queries.

This keeps the new medium semantics separate from the operational replay tool.

## Explicit Non-Goals

- No full PodcastIndex importer rerun.
- No synthetic direct writes into `stophammer.db` from crawler metadata alone.
- No use of `gossip_state.db` in the first pass.
- No generalized “replay all crawler state” framework in v1.
- No append-only history import from crawler state.
