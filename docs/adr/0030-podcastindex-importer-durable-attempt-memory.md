# ADR 0030: PodcastIndex Importer Durable Attempt Memory

## Status
Accepted. Amended on 2026-10-02 with the section "A failure that repeats
waits 7 days".

Date: 2026-03-25

Supersedes [ADR 0013: PodcastIndex Bulk Importer](archive/0013-bulk-importer.md)

## Context
ADR 0013 established the PodcastIndex bulk importer, but two parts of that
decision no longer match the codebase:

- the importer now lives in `stophammer-crawler/src/modes/import.rs`, not in a
  separate Bun/TypeScript package
- the importer shares `stophammer-crawler/src/crawl.rs` and
  `stophammer-parser` with the other crawler modes, so importer behavior now
  depends on the same Rust fetch/parse/POST pipeline as the rest of the crawler
  stack

The broad scan, live-fetch requirement, and batch resume cursor from ADR 0013
are still sound. The gap is state durability.

Today `import_state.db` stores only `import_progress.last_processed_id`. That is
enough to resume batch scanning, but it loses importer-visible memory for each
PodcastIndex row that was attempted. After a run completes, operators cannot ask
which specific snapshot entries:

- returned `404`, `429`, or another fetch status
- parsed successfully and declared `podcast:medium="music"` or another medium
- were rejected by stophammer after a successful fetch
- were retried on later runs

The current importer already has most of the required inputs:

- `CandidateRow.id` is the durable PodcastIndex row key
- `crawl_feed()` sees the HTTP response status
- `stophammer-parser` already extracts `raw_medium`
- import mode already persists a separate SQLite state file

What is missing is a durable per-entry memory model and an importer-facing
result type that preserves the fetch and parse metadata long enough to write it.

## Decision
The PodcastIndex importer remains a Rust mode inside `stophammer-crawler` and
extends `import_state.db` with durable per-entry attempt memory.

### Importer runtime and package boundary

The importer stays in `stophammer-crawler`. We do not reintroduce a separate
importer package or runtime. This supersedes ADR 0013's package/runtime choice
and aligns the architecture with the current shared Rust crawler pipeline.

### State database contract

`import_state.db` remains the single operator-facing state file for PodcastIndex
imports. It now contains:

- `import_progress` for the batch resume cursor
- `import_feed_memory` for latest-known per-entry attempt state

`import_feed_memory` is keyed by `podcastindex_id` and records the latest known
attempt result, including:

- `feed_url`
- `podcastindex_guid`
- `fetch_http_status`
- `fetch_outcome`
- `outcome_reason`
- `retryable`
- `raw_medium`
- `parsed_feed_guid`
- `first_attempted_at`
- `last_attempted_at`
- `attempt_count`

This table is a latest-known-state table, not an append-only attempt log.

### Importer-facing crawl result

The shared crawl/import path must return an importer-facing report that carries:

- the final `CrawlOutcome`
- the fetch HTTP status when available
- the parsed `raw_medium` when a `200 OK` body was successfully parsed into
  `IngestFeedData`
- the parsed feed GUID when available

This metadata must come from the canonical fetch/parse path. The importer must
not reparse XML solely to populate state memory.

### Write ordering and durability

Import-state writes are serialized through a single writer that owns the
`import_state.db` connection. Workers do not write directly to SQLite.

The same writer is responsible for:

- upserting `import_feed_memory` after each completed candidate attempt
- persisting `import_progress.last_processed_id` after a batch completes

Cursor updates must be ordered after all per-candidate memory writes for that
batch on the same connection. This preserves the existing batch-resume behavior
while preventing the cursor from advancing past candidate memory rows that were
still in flight.

### Optional skip mode

Once durable memory exists, import mode may add an optional
`--skip-known-non-music` mode that consults `import_feed_memory` before
scheduling a fetch.

This mode is optional and conservative:

- the default importer behavior continues to fetch every candidate
- skip decisions only apply to entries already known to have
  `fetch_http_status = 200`
- skip eligibility may come from either a known non-`music`/non-`publisher`
  `raw_medium` or a prior `[medium_music]` rejection such as
  `podcast:medium absent`
- the accepted set for v1 remains conservative so feeds relevant to current
  policy are not skipped accidentally
- skipped rows still update memory with a machine-readable outcome such as
  `skipped_known_irrelevant`

### A failure that repeats waits 7 days

Amended on 2026-10-02. Each mode that skips known feeds also reads the shared
skip list, `feed_skip.db`. The list also skips a feed URL for 7 days after
one of these results of a fetch that gave `200`:

- a parse error of the body.
- an ingest answer `413` from the node, for a body that states no
  `podcast:medium`.

The same body gives the same failure at each fetch. The node refuses a request
body over 2 MiB with `413`, and the crawler accepts a feed body of a maximum of 16 MiB.
A feed with no `podcast:medium` also fails the default `medium_music` gate. So
each fetch of such a feed transfers a large body and changes nothing.

After 7 days, one fetch tries the URL again. When the failure repeats, the URL
waits 7 more days. When the publisher corrects the feed, the next fetch after
the 7 days ingests it. A shorter `--skip-ttl-days` applies first.

A `413` for a body that states a medium is not skipped. A larger body limit of
the node can accept such a feed, and the operator must see each failure. A
`429` from the node and each fetch error are also not skipped.

The skip list of 2026-10-02 held 179,592 fetches of `413` feeds and 105,666
fetches of parse errors. A 7-day wait allows at most 12,199 and 33,174 of
them.

## Consequences
- ADR 0013's high-level seed-source and live-fetch decisions remain valid, but
  its runtime/package decision is superseded by the existing Rust crawler
  architecture.
- `import_state.db` becomes materially more useful to operators and auditors,
  because importer-visible attempt results survive process restarts and finished
  runs.
- The importer implementation must change in `stophammer-crawler/src/crawl.rs`
  and `stophammer-crawler/src/modes/import.rs`, plus CLI/docs/tests.
- The state file will grow with the number of attempted snapshot rows, so the
  schema should keep indexes minimal and oriented around operator queries.
- This ADR does not change stophammer ingest semantics, does not change the
  current batch cursor granularity, and does not require an append-only history
  log.
- A feed that gives a parse error, or a node `413` with no medium, is fetched
  at most once in 7 days. The guards are the `feed_skip` unit tests of
  `stophammer-crawler`: a new failure skips, a failure older than 7 days does
  not, and a `413` for a music feed or a `429` never skips.
