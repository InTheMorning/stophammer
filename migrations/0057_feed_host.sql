-- Store the host of each feed URL.
-- ADR 0080 §5: the host and not_host filters of GET /v1/search read this
-- column. The migration adds it with no value and no index. The Rust fill in
-- open_db sets each null value after this migration runs.
ALTER TABLE feeds ADD COLUMN feed_host TEXT;
