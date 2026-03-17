# ADR 0023: Versioned Schema Migrations

## Status
Accepted

## Context
The original schema management approach (documented in ADR 0003) embedded the full schema as `include_str!("schema.sql")` and applied it via `execute_batch` on every startup. This had several problems:

1. **Destructive resets**: `schema.sql` contained `DROP TABLE IF EXISTS` statements for several tables. On a restart, any data in those tables was silently destroyed. This caused production data loss on community nodes that had accumulated state between restarts.
2. **No incremental evolution**: adding a new table or column required editing the monolithic `schema.sql` file. There was no mechanism to apply only the delta to an existing database, and no record of which version of the schema a given database was running.
3. **Multi-node divergence**: with community nodes (ADR 0009) and push-gossip replication (ADR 0016), multiple independent SQLite databases exist across the network. Without version tracking, there was no way to verify that a node's schema matched the expected version after a binary upgrade.

All `CREATE TABLE` and `CREATE INDEX` statements already used `IF NOT EXISTS`, so idempotent re-application was safe for additive changes. The problem was exclusively with destructive statements (`DROP TABLE`, `DROP INDEX`) and the absence of version tracking.

## Decision

### Migration table

A `schema_migrations` table tracks applied versions:

```sql
CREATE TABLE IF NOT EXISTS schema_migrations (
    version    INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);
```

This table is created unconditionally before any migration runs, so it bootstraps itself on both fresh and existing databases.

### Compile-time migration array

Migrations are defined as a compile-time constant in `src/db.rs`:

```rust
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_baseline.sql"),
];
```

Each entry is a SQL script stored under `migrations/` and pulled into the binary at compile time via `include_str!`. The array is append-only: new migrations are added to the end and existing entries are never modified. The array index (1-based) is the migration version number.

### Transactional application

`run_migrations(conn)` iterates over `MIGRATIONS`, skipping any version already recorded in `schema_migrations`. Each pending migration runs inside a single `unchecked_transaction`:

1. `execute_batch(sql)` applies the migration DDL/DML.
2. An `INSERT INTO schema_migrations` records the version and timestamp.
3. `tx.commit()` makes both the schema change and the version record atomic.

If a migration fails, the transaction rolls back and the version is not recorded, so the next startup will retry from the same point.

### Baseline migration

`migrations/0001_baseline.sql` contains the full schema as of the migration system introduction. It is derived from the former `src/schema.sql` with all `DROP TABLE` and `DROP INDEX` statements removed. Every statement uses `IF NOT EXISTS` / `INSERT OR IGNORE`, making it safe to apply against both empty and pre-existing databases.

### Startup order

`open_db` applies PRAGMAs (WAL, foreign keys, synchronous) before calling `run_migrations`. This ensures WAL mode and foreign key enforcement are active during migration execution.

## Consequences

- Restarts no longer destroy data. The `DROP TABLE` statements that caused production data loss are eliminated from the codebase.
- Schema changes are additive and auditable: each migration file is a permanent record in version control of what changed and when.
- Community nodes upgrading to a new binary will automatically apply any new migrations on next startup, without operator intervention.
- The `schema_migrations` table provides a runtime-queryable record of the database's schema version, useful for debugging version skew across nodes.
- `src/schema.sql` is retained as a reference but is no longer executed at startup. Future schema changes must be added as new migration files appended to the `MIGRATIONS` array.
- SQLite does not support transactional DDL for all operations (e.g., `ALTER TABLE` with column renames has limitations). Future migrations that require non-transactional DDL will need to document their failure-recovery strategy in comments within the migration file.
- Rollback (downgrade) migrations are not supported. If a migration must be reversed, a new forward migration must be written. This is consistent with the append-only event log philosophy (ADR 0004).
