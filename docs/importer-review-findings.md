# Importer Review Findings

Review of the import and wavlake-only crawling paths in `stophammer-crawler`,
conducted 2026-03-27.

## Summary

The import mode design is sound. The batch cursor + per-row feed memory pattern
is correct by construction: the cursor advances after each batch, and
`import_feed_memory` tracks per-row outcomes independently, so no work is lost
on restart. The wavlake-only mode adds appropriate single-flight throttling
with 429-aware backoff.

The findings below are ranked by severity. All issues are now **resolved**.

## High Severity

### ~~State writer panics crash the entire importer~~ (resolved)

Fixed in `e9f16d8`. The state writer now uses `if let Err(e)` + `eprintln!` for
upsert failures (`import.rs:373-377`) and channel send failures
(`import.rs:533-542`). A transient SQLite error logs a warning and skips the
row instead of crashing.

### ~~Audit lock file persists after ungraceful shutdown~~ (resolved)

Fixed in `e9f16d8`. `acquire_audit_lock` (`import.rs:477-531`) now writes the
PID into the lock file. On startup, if the lock exists, it reads the PID and
checks `/proc/<pid>`. Dead process locks are automatically reclaimed with a
warning; live process locks still panic.

### ~~301 permanent redirects are not propagated~~ (resolved)

Fixed: `ImportMemoryRow::from_crawl_report` now uses `report.final_url` when it
differs from the original URL, so `import_feed_memory.feed_url` tracks the
resolved target after redirects.

## Medium Severity

### ~~Wavlake backoff only triggers on HTTP 429~~ (resolved)

Fixed: `record_attempt` and `wavlake_throttle_delay` now back off on both 429
and 503 responses.

### ~~Hard timeout rows retry without strategy change~~ (resolved)

Fixed: `import_feed_memory` tracks `consecutive_timeout_count`. Rows with 3+
consecutive timeouts are skipped with a `skip_timeout_exhausted` log. The
counter resets when a non-timeout result is recorded.

### ~~No process-level lock on state DB~~ (resolved)

Fixed: `run()` acquires a PID-based lock on `import_state.db` before
processing (skipped in dry-run mode). Uses the same `acquire_pid_lock` helper
as the audit writer.
