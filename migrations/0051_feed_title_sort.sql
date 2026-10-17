-- Store the sort key of each feed title.
-- ADR 0070 §1, task 001: GET /v1/feeds/by-name orders by this column, then
-- by feed_guid. SQLite cannot give the Unicode lowercase that the key
-- needs, so the Rust fill in open_db sets each null value after this
-- migration runs.
ALTER TABLE feeds ADD COLUMN title_sort TEXT;

CREATE INDEX IF NOT EXISTS idx_feeds_title_sort_guid ON feeds(title_sort, feed_guid);
