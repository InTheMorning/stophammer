# ADR 0024: SQLite WAL Connection Pool

## Status
Accepted

Date: 2026-03-14

## Context

The original design used a single `Arc<Mutex<Connection>>` for all database
access. While simple, this serialises every request — readers block writers
and vice-versa — defeating the concurrency that SQLite's WAL journal mode is
designed to provide.

Under load, GET endpoints (search, entity lookups, event listing) queue behind
write operations (event ingestion, entity upserts), adding unnecessary latency
to read-heavy workloads.

## Decision

Replace the single shared connection with a two-tier pool:

1. **Writer** — a single `Mutex<Connection>` (SQLite permits only one
   concurrent writer regardless of connection count).
2. **Reader pool** — an `r2d2` pool of up to 8 read-only connections, each
   initialised with `PRAGMA query_only=ON` so they cannot accidentally
   mutate state.

Both tiers open in WAL mode (`PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON`). The pool is wrapped in a `DbPool` struct that is
cheaply cloneable (both fields are `Arc`-wrapped).

All read-only API handlers (`GET /v1/search`, `GET /v1/feeds/*`,
`GET /v1/tracks/*`, `GET /sync/events`, etc.) obtain a connection from the
reader pool. All mutating handlers (`POST /ingest/feed`, `POST /sync/register`,
`POST /sync/reconcile`, `PATCH /v1/*`, `DELETE /v1/*`, etc.) lock the writer
mutex.

For tests that use in-memory databases (`:memory:`), a `from_writer_only`
constructor routes both reads and writes through the single writer connection,
since separate r2d2 reader connections would each open a distinct in-memory
database. This constructor is gated behind `#[cfg(feature = "test-util")]`.

## Trade-offs

| Aspect | Pro | Con |
|--------|-----|-----|
| Read concurrency | Multiple GET requests proceed in parallel without blocking on writes | Pool overhead: 8 open file handles, ~8× memory for page caches |
| Write serialisation | Unchanged — single writer matches SQLite semantics | No improvement for write-heavy bursts |
| `PRAGMA query_only` | Prevents accidental writes through reader connections | Adds a per-connection pragma that must be maintained |
| Connection limit | Bounded at 8 readers; prevents fd exhaustion | Could become a bottleneck under extreme read load (adjustable) |
| Test complexity | `from_writer_only` keeps in-memory test ergonomics | Two code paths (pooled vs writer-only) to maintain |

## Consequences

- `AppState.db` changes type from `Arc<Mutex<Connection>>` to `DbPool`.
- All handler code must be classified as read or write and routed accordingly.
- `spawn_db` (read path) and `spawn_db_mut` / `spawn_db_write` (write path)
  provide the async-to-blocking bridge.
- New dependencies: `r2d2 = "0.8"`, `r2d2_sqlite = "0.24"`.
