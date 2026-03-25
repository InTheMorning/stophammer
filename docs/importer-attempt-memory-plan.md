# Importer Attempt Memory Plan

This plan has now been accepted in
[ADR 0030](/home/citizen/build/stophammer/docs/adr/0030-podcastindex-importer-durable-attempt-memory.md).
Treat this file as rollout notes and implementation guidance for that ADR.

This document translates the proposal in `importer_memory.md` into a phased
implementation plan that matches the current repo structure:

- the importer lives in
  [stophammer-crawler/src/modes/import.rs](/home/citizen/build/stophammer/stophammer-crawler/src/modes/import.rs)
- the shared fetch/parse/POST pipeline lives in
  [stophammer-crawler/src/crawl.rs](/home/citizen/build/stophammer/stophammer-crawler/src/crawl.rs)
- RSS parsing lives in
  [stophammer-parser](/home/citizen/build/stophammer/stophammer-parser)
- GitHub CI currently validates only the top-level crate in
  [.github/workflows/ci.yml](/home/citizen/build/stophammer/.github/workflows/ci.yml)

## Why

The importer currently remembers only one value:

- `import_progress.last_processed_id`

That is enough to resume scanning the PodcastIndex snapshot, but it loses all
candidate-level memory. Operators cannot answer which PodcastIndex rows were
attempted, what HTTP status they returned, or what `podcast:medium` was
published by feeds that fetched successfully.

The proposal is directionally correct, but it needs to be grounded in the
actual codebase:

- there is no separate `stophammer-importer` package to modify
- the right seam is the shared Rust crawl pipeline, not importer-only reparsing
- the current CI does not automatically protect `stophammer-crawler` or
  `stophammer-parser`, so importer work can regress outside the top-level
  pipeline unless we fix that first

## Invariants

- Live feed fetch remains the source of truth for importer acceptance unless
  the operator explicitly opts into skip mode.
- `import_state.db` remains the single state file for import cursor and
  importer memory.
- `import_feed_memory` is latest-known-state, not append-only history.
- Every completed importer attempt records memory, including accepted,
  rejected, unchanged, fetch-error, parse-error, ingest-error, and skipped
  outcomes.
- State writes serialize through a single owner connection; workers do not
  write directly to SQLite.
- Batch cursor semantics stay unchanged in the first pass: resume remains
  batch-based, not per-row.
- The shared parse path remains canonical; the importer must not add a second
  XML parsing path just to capture `raw_medium`.
- `audit_import` keeps its own NDJSON-row cursor contract unless a later change
  explicitly decides to unify those tools.

## Code Touchpoints

Primary implementation files:

- [stophammer-crawler/src/crawl.rs](/home/citizen/build/stophammer/stophammer-crawler/src/crawl.rs)
- [stophammer-crawler/src/modes/import.rs](/home/citizen/build/stophammer/stophammer-crawler/src/modes/import.rs)
- [stophammer-crawler/src/main.rs](/home/citizen/build/stophammer/stophammer-crawler/src/main.rs)
- [stophammer-crawler/README.md](/home/citizen/build/stophammer/stophammer-crawler/README.md)

Verification and release guard rails:

- [.github/workflows/ci.yml](/home/citizen/build/stophammer/.github/workflows/ci.yml)
- [docs/adr/0013-bulk-importer.md](/home/citizen/build/stophammer/docs/adr/0013-bulk-importer.md)
- [docs/adr/0030-podcastindex-importer-durable-attempt-memory.md](/home/citizen/build/stophammer/docs/adr/0030-podcastindex-importer-durable-attempt-memory.md)

Adjacent but out-of-scope unless implementation reveals a shared abstraction:

- [stophammer-crawler/analysis/bin/audit_import.rs](/home/citizen/build/stophammer/stophammer-crawler/analysis/bin/audit_import.rs)

## Phase Plan

### Phase 1: Guard Rails and Result Contract

Scope:

- Extend CI to cover `stophammer-crawler` and `stophammer-parser` explicitly.
  The repo should not be restructured into a Cargo workspace just for this
  feature; add explicit `cargo ... --manifest-path ...` steps so structure stays
  stable.
- Introduce an importer-facing `CrawlReport` (or equivalent) in
  `stophammer-crawler/src/crawl.rs`.
- Refactor the shared crawl path so import mode can observe:
  - final `CrawlOutcome`
  - fetch HTTP status
  - parsed `raw_medium`
  - parsed feed GUID
- Keep import behavior unchanged in this phase: no new state table yet, no skip
  mode yet.

Implementation notes:

- Prefer a small internal refactor in `crawl.rs` over importer-specific
  branching scattered across `import.rs`.
- Preserve the single canonical parse path via `stophammer-parser`.
- Keep the existing public crawl behavior stable for `crawl`, `podping`, and
  `gossip` modes; only import mode needs the richer report initially.

Acceptance:

- Root CI still passes.
- CI additionally runs build/test/clippy/fmt for `stophammer-crawler`, and
  build/test/clippy/fmt for `stophammer-parser` if that crate gains testable
  changes in the same PR.
- Unit tests cover the report mapping from fetch/parse/ingest outcomes to the
  importer-facing metadata.

### Phase 2: Durable Per-Entry Import Memory

Scope:

- Extend the import state schema with `import_feed_memory`.
- Add an importer-only row model such as `ImportMemoryRow`.
- Introduce a single writer thread/task that owns the `import_state.db`
  connection.
- Have workers send completed `ImportMemoryRow` values to that writer after each
  candidate finishes.
- Move cursor persistence onto the same writer so batch cursor updates are
  ordered after the batch's memory writes.

Implementation notes:

- The writer should own a prepared UPSERT for `import_feed_memory`.
- `fetch_outcome` should be a compact machine-readable enum/string aligned with
  importer-visible terminal states:
  `accepted`, `rejected`, `no_change`, `fetch_error`, `parse_error`,
  `ingest_error`, and later `skipped_known_irrelevant`.
- `outcome_reason` should store the rejection or error detail already produced
  by `CrawlOutcome`.
- `raw_medium` should be populated only when a `200 OK` response produced parsed
  `IngestFeedData`.
- Do not change `import_progress.last_processed_id` semantics in this phase.

Acceptance:

- State tests cover:
  - `404` persisted with `fetch_http_status = 404` and `raw_medium IS NULL`
  - `429` persisted with `retryable = 1`
  - `200` plus successful parse persisted with `raw_medium`
  - repeated attempts increment `attempt_count`
  - batch cursor persistence stays unchanged from the operator's point of view
- Importer logs and README remain accurate.

### Phase 3: Optional Skip Mode

Scope:

- Add `--skip-known-non-music` to import mode.
- Before scheduling a fetch, consult `import_feed_memory`.
- Skip only rows already known to have:
  - `fetch_http_status = 200`
  - a non-NULL `raw_medium`
  - a `raw_medium` outside the accepted set
- Persist a new memory row for the skip event so `last_attempted_at` and
  `attempt_count` still move forward.

Implementation notes:

- For the first slice, the accepted set should be conservative:
  `music` and `publisher`.
- Default behavior must remain a full fetch/import pass.
- Skip mode should remain easy to disable after parser or policy changes.

Acceptance:

- Default import runs remain behaviorally unchanged.
- Skip mode bypasses the network fetch for eligible rows and records
  `skipped_known_irrelevant`.
- Tests prove that known `music` and `publisher` rows are not skipped.

### Phase 4: Operational Polish

Scope:

- Update operator docs and query examples for `import_feed_memory`.
- Document when to use a full re-crawl instead of skip mode.
- Decide whether any analysis tools should consume the new memory table or stay
  separate.
- Add rollout notes for large existing state files and index growth.

Acceptance:

- README and docs explain the new table, new flag, and expected state-file
  growth.
- Operators have direct SQL examples for row lookup, 429 discovery, and
  medium-based inspection.

## Recommended Delivery Shape

Split the work into small reviewable PRs:

1. CI coverage plus `CrawlReport` refactor.
2. Import-state schema plus writer path and tests.
3. Skip mode plus operator docs.

This keeps the highest-risk concurrency and durability change isolated from the
policy choice around skip mode.

## Explicit Non-Goals

- No append-only attempt history in the first pass.
- No raw XML storage in `import_state.db`.
- No change to stophammer ingest semantics.
- No change to `audit_import` state format in the first pass.
- No repo-wide Cargo workspace conversion solely to cover this feature.
