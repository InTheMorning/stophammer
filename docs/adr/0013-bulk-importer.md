# ADR 0013: PodcastIndex Bulk Importer

## Status
Accepted

## Context

Stophammer needs an initial corpus of music podcast feeds to be useful.
Manual curation is impractical at scale.  The PodcastIndex maintains a
snapshot SQLite database of ~4.65 million known podcast feeds, available
locally at `/Volumes/T7/hey-v4v/podcastindex-snapshots/podcastindex_feeds.db`.
This snapshot provides a large, pre-curated seed set with known-alive status
(`dead = 0`), hosting metadata, and rough category classification.

Key constraints:

- The `podcast:medium` namespace tag (which would directly identify music feeds)
  is embedded in each feed's RSS XML and is not captured in the snapshot DB
  columns.  Only denormalized flat strings (`category1`–`category10`) and
  `itunesType` are available as filters in the snapshot.
- The snapshot `chash` column holds a stale MD5 content hash; it cannot be
  used as the `content_hash` submitted to stophammer because
  `ContentHashVerifier` uses the hash of the live-fetched body to detect
  changes.  A fresh fetch is always required.
- Importing millions of feeds takes hours to days.  The process must survive
  restarts without re-processing already-attempted feeds.
- The importer is an offline administrative tool, not a production service.
  Bun/TypeScript is chosen for rapid iteration, consistent with the existing
  crawler tooling in this monorepo.

## Decision

### Seed source: PodcastIndex snapshot

The snapshot provides 4.65 million feeds with `dead`, category, and hosting
metadata already scraped.  Filtering `dead = 0` and using category columns as
a pre-filter yields a manageable candidate set without requiring a full-table
scan or network request per row to check aliveness.

### Category-based pre-filter

```sql
WHERE dead = 0
  AND (
    category1 LIKE '%music%' OR category2 LIKE '%music%'
    OR itunesType = 'serial'
  )
  AND id > :last_id
ORDER BY id ASC
LIMIT :batch_size
```

`category1 LIKE '%music%'` catches feeds that publishers tagged as "Music" in
their iTunes/Apple Podcasts category.  `itunesType = 'serial'` catches
narrative and album-style feeds that may not be tagged music but are structured
like music albums.  This is deliberately over-inclusive: stophammer's own
verifier chain (particularly `MediumVerifier` and `TrackCountVerifier`) will
reject false positives when the live feed is parsed.

### Resume cursor in a separate SQLite file (`import_state.db`)

A single-row `import_progress` table keyed by `last_processed_id` persists the
highest PodcastIndex `id` attempted in a previous run.  On restart the importer
queries `id > last_processed_id`, skipping already-attempted feeds.

Cursor advancement happens after each batch completes, not per-feed.  A crash
mid-batch causes that batch to be re-tried on the next run.  Re-submission is
safe because stophammer deduplicates on `content_hash`; a re-submitted
unchanged feed returns `no_change = true` without writing a new event.

### Live RSS fetch per candidate

Each candidate URL is fetched live (10 s timeout) to obtain:

1. A fresh SHA-256 `content_hash` for `ContentHashVerifier`.
2. Up-to-date `podcast:medium` in the raw XML for `MediumVerifier`.
3. Current track list for `TrackCountVerifier` and `ValueVerifier`.

Fetch errors are caught and logged; the cursor still advances so a single
unreachable host does not stall the entire run.

### Concurrency model

Feeds within a batch are processed by a fixed-size worker pool (`--concurrency`,
default 5).  This limits open file descriptors and outbound connections without
requiring a queue library.  The pool size should be tuned down if rate-limiting
responses are observed from hosting providers.

### Importer location: `stophammer-importer/`

The importer lives in its own package directory rather than inside
`stophammer/` (Rust) or `stophammer-crawler/` because it has a different
runtime (Bun vs. Node/Cloudflare Workers), a heavier SQLite dependency, and a
different deployment lifecycle (run once, not continuously).

## Consequences

- The candidate set is large.  Even with the category filter, millions of rows
  may match.  A full run will take many hours to days depending on concurrency,
  network latency, and stophammer write throughput.
- Rate limiting from RSS hosting providers is likely at sustained concurrency.
  Operators should monitor error rates and reduce `--concurrency` if needed.
  Future work may add per-host rate limiting inside `crawl.ts`.
- The category pre-filter includes `itunesType = 'serial'` as a broad catch for
  album-structured feeds.  This increases candidate volume but keeps recall
  high for music feeds that omit category tags.
- `podcast:medium` is absent from snapshot columns, so non-music feeds tagged
  only with `podcast:medium = music` (and no iTunes category) are missed by
  this importer.  A future pass using a full XML scan of the snapshot blobs
  (if available) could close this gap.
- Re-running the importer after stophammer indexes new feeds from other sources
  will produce mostly `no_change` responses, which are cheap.  Periodic
  re-runs are safe and will pick up newly published tracks on already-indexed
  feeds.
