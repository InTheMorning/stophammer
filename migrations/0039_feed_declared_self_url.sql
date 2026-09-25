-- The self link a source body declares for its own feed (ADR 0052 §2).
-- Only an ingest in the update or new-feed case writes this column, from
-- the first `self_feed` link in the body. A mirror submission reads the
-- column to decide a move; it never writes it.
ALTER TABLE feeds ADD COLUMN declared_self_url TEXT;
