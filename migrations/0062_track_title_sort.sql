-- Store the sort key of each track title.
-- ADR 0084 §8, task 001: a later task sorts a search and a list by this
-- column. SQLite cannot make the key, so the Rust fill in open_db sets each
-- null value after this migration runs. The column has no index.
ALTER TABLE tracks ADD COLUMN title_sort TEXT;
