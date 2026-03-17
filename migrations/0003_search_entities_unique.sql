-- Issue-HASH-COLLISION — 2026-03-14
-- Prevent the same entity from appearing twice with different rowids.
-- The idx_search_entities_entity index already exists but is non-unique;
-- this adds a UNIQUE constraint so duplicate (entity_type, entity_id) pairs
-- are rejected at the database level.
CREATE UNIQUE INDEX IF NOT EXISTS idx_search_entities_type_id
    ON search_entities(entity_type, entity_id);
