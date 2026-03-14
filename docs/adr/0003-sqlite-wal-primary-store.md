# ADR 0003: SQLite with WAL Mode as the Primary Store

## Status
Accepted

## Context
The stophammer node stores an append-only event log plus materialized views of feeds, tracks, artists, and payment routes. The access pattern is:

- **Writes**: One primary node, serialized through `Arc<Mutex<Connection>>`, low write concurrency
- **Reads**: Many community nodes poll `/sync/events` via HTTP; reads use a shared SQLite connection
- **Scale**: ~tens of thousands of music feeds, ~hundreds of thousands of tracks — fits comfortably in a single SQLite file

Alternatives considered:
- **PostgreSQL**: Requires a separate process, significantly complicates the drop-in deployment story
- **SurrealDB / TiKV**: Distributed-native but heavy and operationally complex for single-node deployments
- **LevelDB / RocksDB**: Key-value only; would require building secondary indices manually

## Decision
We will use SQLite in WAL (Write-Ahead Log) mode with `PRAGMA synchronous = NORMAL`. All tables use `STRICT` mode to enforce type constraints at the SQLite level. Schema is applied at startup via a versioned migration system (see ADR 0023). All writes go through a single `Arc<Mutex<Connection>>`; blocking DB work runs in `tokio::task::spawn_blocking` to avoid blocking the async executor.

## Consequences
- The entire database is a single file — trivially backed up with `cp` or `rsync`.
- `STRICT` tables catch type errors that Rust's `rusqlite` params might otherwise silently coerce.
- WAL mode allows concurrent readers during writes, which is critical for the sync polling endpoints.
- Single-writer serialization via mutex is not a bottleneck at this scale; music feeds update slowly.
- If write throughput ever becomes a bottleneck, this decision will need to be revisited with ADR supersession.
