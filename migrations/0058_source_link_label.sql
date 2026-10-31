-- Store the label of a source link.
-- ADR 0082 §2: a funding link carries the text that the feed gives for it.
-- The column has no value for a link with no label.
ALTER TABLE source_entity_links ADD COLUMN label TEXT;
