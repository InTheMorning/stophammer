CREATE TABLE IF NOT EXISTS resolved_external_ids_by_feed (
    feed_guid    TEXT NOT NULL REFERENCES feeds(feed_guid) ON DELETE CASCADE,
    entity_type  TEXT NOT NULL,
    entity_id    TEXT NOT NULL,
    scheme       TEXT NOT NULL,
    value        TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (feed_guid, entity_type, entity_id, scheme, value)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_resolved_extid_feed
    ON resolved_external_ids_by_feed(feed_guid);
CREATE INDEX IF NOT EXISTS idx_resolved_extid_entity
    ON resolved_external_ids_by_feed(entity_type, entity_id);

CREATE TABLE IF NOT EXISTS resolved_entity_sources_by_feed (
    feed_guid    TEXT NOT NULL REFERENCES feeds(feed_guid) ON DELETE CASCADE,
    entity_type  TEXT NOT NULL,
    entity_id    TEXT NOT NULL,
    source_type  TEXT NOT NULL,
    source_url   TEXT,
    trust_level  INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (feed_guid, entity_type, entity_id, source_type, source_url)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_resolved_esrc_feed
    ON resolved_entity_sources_by_feed(feed_guid);
CREATE INDEX IF NOT EXISTS idx_resolved_esrc_entity
    ON resolved_entity_sources_by_feed(entity_type, entity_id);
