# ADR 0050 Phase Plan: The Crawler Revalidates A Feed

Owner: [ADR 0050](../adr/0050-the-crawler-revalidates-a-feed.md), Accepted
2026-09-24.

The [review checklist](../reviews/adr-0050-review-checklist.md) applies after
each task.

## Goal

The crawler keeps the validators and the last body of each feed that it
fetches. It sends a conditional GET, and after a `304` it uses the kept body.
A corrective pass then transfers almost no feed body.

## Non-Goals

- A change to the node. ADR 0050 decision 7.
- A skipped request within `max-age`. ADR 0050 leaves it open until the
  measurement of decision 6.
- The `ndjson` mode. It does not fetch, and its bodies are old.
- Removal of old rows from the cache. A row for a URL that left the index stays
  until a later task removes it.

## Current State

`stophammer-crawler` at commit `99069e7` and later.

- `crawl_feed_report` in `src/crawl.rs` sends one GET with a `User-Agent`
  header and a timeout. It reads no cache and sends no conditional header. On
  `200` it hashes the body and calls `ingest_cached_feed_report`, which parses
  the body and posts it to the node.
- Three paths call `crawl_feed_report`: `crawl_feed_with_retries` in
  `src/modes/batch.rs`, three sites in `src/modes/gossip.rs`, and one in
  `src/modes/import.rs`. The `ndjson` mode calls `ingest_cached_feed_report`
  and does not fetch.
- `src/feed_skip.rs` is the pattern for a shared SQLite store: `FeedSkipDb`
  opens the file, sets WAL mode and a 5 second busy timeout, and the modes share
  it as `Arc<std::sync::Mutex<FeedSkipDb>>`. Its `feed_outcomes` table is
  keyed by the requested URL, and it records `CrawlOutcome::label()`.
- `CrawlConfig` holds `force_reingest`, set by the global `--force` flag.
- `flate2` and `rusqlite` are already dependencies. The only dev dependency is
  `tempfile`. No HTTP stub exists in the tests.
- On 2026-09-24 four hosts answered a conditional GET with `304`. Wavlake
  sends `ETag` only. RSS Blue sends `Last-Modified` only. Fountain sends both.
  JustCast sends a weak `ETag`.

## Decisions This Plan Makes

The ADR leaves these points to the plan. A task packet does not change them.

1. **The store is `FeedCacheDb` in `src/feed_cache.rs`.** The modes share it
   as `Arc<std::sync::Mutex<FeedCacheDb>>`, as they share `FeedSkipDb`. A
   lock is held for one read or one write, never across an HTTP await.
2. **The key is the requested URL**, as in `feed_outcomes`. Two URL forms of
   one feed give two rows. That is correct, because each form has its own
   `ETag` at the host.
3. **On a `304`, the crawler parses the kept body.** The report then carries
   `parsed_feed`, `raw_medium` and `parsed_feed_guid`, so the follow step of
   ADR 0049 works on an unchanged feed. Only the ingest is skipped in a normal
   crawl.
4. **A kept body goes to the node with `http_status` 200.** The node stores
   that status for dead-feed detection, and a `304` is not a dead feed.
5. **The node answer in a row uses `CrawlOutcome::label()`**, the same
   strings as `feed_outcomes.fetch_outcome`: `accepted`, `no_change`,
   `rejected`, `parse_error`, `ingest_error`. A `fetch_error` never writes a
   row.
6. **`revalidate` is a field of `CrawlConfig`**, default true. The global flag
   `--no-revalidate` clears it, as `--force` sets `force_reingest`.
7. **The flag is `--feed-cache <path>`**, env `FEED_CACHE_DB`, binary default
   `./feed_cache.db`, as `--skip-db` defaults to `./feed_skip.db`. The compose
   files pass `/data/feed_cache.db`. The `feed`, `refresh`, `gossip` and
   `import` modes take the flag.
8. **The fetch logic is a pure function plus a thin wrapper.** The decision,
   from the cache row and the response status to an action, is a function with
   no I/O. The tests cover it without a server. The wrapper is tested against
   a local HTTP stub on `127.0.0.1` in a `tokio` test. That is no external
   network.
9. **The batch report line** is
   `fetch: ok=N not_modified=N rate_limited=N other=N`, printed after the last
   wave, with `refresh: ` in front of it in the `refresh` mode as its other
   lines have.
10. **The measurement needs two passes.** The first pass after the deploy fills
    the cache and gets `200` for each feed. The second pass gets `304` for each
    unchanged feed. The report of the second pass answers the open question.

## Affected Modules

| Repository | Module | Change |
|---|---|---|
| `stophammer-crawler` | `src/feed_cache.rs`, new | the store |
| `stophammer-crawler` | `src/crawl.rs` | conditional GET, `304` handling, `CrawlConfig.revalidate` |
| `stophammer-crawler` | `src/main.rs`, `src/modes/batch.rs`, `src/modes/gossip.rs`, `src/modes/import.rs` | flags and wiring |
| `stophammer-crawler` | `src/modes/batch.rs` | the report line |
| `stophammer-crawler` | `README.md`, `AGENTS.md` | the flags and the file |
| `stophammer` | `docker-compose.yml`, `packaging/env/*.compose.env.example`, `docs/operations.md`, `AGENTS.md` | deployment and operations |

## Sequence

Each task is one commit in one repository. Each task ends green.

| Task | Repository | Result | Needs |
|---|---|---|---|
| [001](../tasks/adr-0050-task-001-feed-cache-store.md) | `stophammer-crawler` | `FeedCacheDb`: the file, the table, get and put | none |
| [002](../tasks/adr-0050-task-002-conditional-fetch.md) | `stophammer-crawler` | `crawl_feed_report` sends a conditional GET and handles `304` | 001 |
| [003](../tasks/adr-0050-task-003-flags-and-wiring.md) | `stophammer-crawler` | `--feed-cache`, `--no-revalidate`, the four modes use the cache, the README | 002 |
| [004](../tasks/adr-0050-task-004-batch-fetch-report.md) | `stophammer-crawler` | The batch pass reports `200`, `304`, `429` and other counts | 003 |
| [005](../tasks/adr-0050-task-005-deployment.md) | `stophammer` | Compose passes the cache path, env examples, `docs/operations.md`, `AGENTS.md` | 003 |

Task 004 and task 005 can run at the same time. Deploy the crawler after
task 004. No node deploy is needed.

## Schema And API Implications

None for the node. The crawler gains one SQLite file in its volume:

```sql
CREATE TABLE IF NOT EXISTS feed_cache (
    url            TEXT PRIMARY KEY,
    final_url      TEXT NOT NULL,
    etag           TEXT,
    last_modified  TEXT,
    content_sha256 TEXT NOT NULL,
    body_gzip      BLOB NOT NULL,
    fetched_at     INTEGER NOT NULL,
    node_answer    TEXT,
    node_reason    TEXT,
    answered_at    INTEGER
) STRICT;
```

`node_answer` is null until the node answers. A `fetch_error` never writes a
row, so `body_gzip` is never null.

## Risk Areas

- **A host that keeps its `ETag` for changed content.** The change is hidden
  until a pass with `--no-revalidate`. ADR 0050 decision 5 gives that flag.
- **A CDN edge with a different `ETag` for the same body.** The crawler then
  gets `200` instead of `304`, which costs a body and nothing else. Wavlake's
  `ETag` looked like a content hash on 2026-09-24, so this is probably rare.
- **Two containers write the file at once.** WAL mode and the busy timeout
  cover it, as they do for `feed_skip.db`. A lock is never held across an
  await.
- **Disk.** About 30 to 50 MB for 10,000 bodies. The file has no removal rule
  yet.
- **A wrong skip.** A normal crawl skips the ingest after a `304`. The row's
  node answer must be reliable, so decision 5 uses the same strings as
  `feed_skip`, and an ingest error makes the next `304` submit the body.

## Test Strategy

- Each task keeps the suite green. Tests are inline `#[cfg(test)]` modules.
- Task 001 tests the store with `tempfile`: put, get, replace, gzip round
  trip, two connections on one file.
- Task 002 tests the decision function for each row of the ADR §3 table, and
  the wrapper against a local HTTP stub for `200`, `304` and `429`.
- Task 003 tests the flags with `clap` introspection, as task 012 of ADR 0049
  did, and each mode's use of the cache through its existing seams.
- Task 004 tests the report line from a list of reports.
- Task 005 has no test. Its check is `docker compose config` on the VPS.
- No test sends a request to an external host.

## Rollback

Each task is one commit and reverts alone. A revert of the crawler leaves
`/data/feed_cache.db` in the volume, and the old code ignores it. To remove it:

```bash
docker compose --profile tools run --rm --entrypoint sh stophammer-crawler -c 'rm -f /data/feed_cache.db /data/feed_cache.db-wal /data/feed_cache.db-shm'
```

## Open Questions

None for the tasks. The ADR's open question, whether a `304` counts against
the Wavlake `429` limit, is answered by the report of the second pass after
the deploy.
