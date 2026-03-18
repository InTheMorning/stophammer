CREATE TABLE IF NOT EXISTS source_item_enclosures (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL,
    url             TEXT NOT NULL,
    mime_type       TEXT,
    bytes           INTEGER,
    rel             TEXT,
    title           TEXT,
    is_primary      INTEGER NOT NULL DEFAULT 0,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, position, url)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_item_enclosures_feed
    ON source_item_enclosures(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_item_enclosures_entity
    ON source_item_enclosures(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_source_item_enclosures_url
    ON source_item_enclosures(url);
