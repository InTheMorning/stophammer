-- Clear the stored sort key of each feed title and track title.
-- ADR 0090 §4: the sequence key is now the order key. SQLite cannot make it,
-- so the Rust fills in open_db write the order key into each null row after
-- this migration runs. The index on feeds(title_sort, feed_guid) follows the
-- new values.
UPDATE feeds SET title_sort = NULL;
UPDATE tracks SET title_sort = NULL;
