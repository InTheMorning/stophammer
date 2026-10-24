-- ADR 0078 §1 and §6: the search index holds only searchable text.
-- This migration drops the old table, creates a normal FTS5 table with the
-- columns name, title and description, and deletes each row of
-- search_entities. It does not fill the table.
-- The repair ensure_search_index_text_columns in src/db.rs fills it from
-- the source tables (ADR 0046).
DROP TABLE IF EXISTS search_index;

CREATE VIRTUAL TABLE search_index USING fts5(
    name,
    title,
    description,
    tokenize='unicode61'
);

DELETE FROM search_entities;
