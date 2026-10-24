-- ADR 0079 §1 and §3: a release of a retired source hold keeps the row.
-- The column released_at holds the time of the release. A NULL value means
-- the hold is active. The repair ensure_retired_source_hold_release_schema
-- in src/db.rs adds the column when this migration cannot run (ADR 0046).
ALTER TABLE retired_source_holds ADD COLUMN released_at INTEGER;
