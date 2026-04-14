CREATE TABLE IF NOT EXISTS source_item_transcripts (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL,
    url             TEXT NOT NULL,
    mime_type       TEXT,
    language        TEXT,
    rel             TEXT,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, position, url)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_item_transcripts_feed
    ON source_item_transcripts(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_item_transcripts_entity
    ON source_item_transcripts(entity_type, entity_id);
