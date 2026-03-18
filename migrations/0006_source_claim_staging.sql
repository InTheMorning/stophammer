CREATE TABLE IF NOT EXISTS source_contributor_claims (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('feed', 'track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL,
    name            TEXT NOT NULL,
    role            TEXT,
    group_name      TEXT,
    href            TEXT,
    img             TEXT,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, position, source)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_contrib_feed
    ON source_contributor_claims(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_contrib_entity
    ON source_contributor_claims(entity_type, entity_id);

CREATE TABLE IF NOT EXISTS source_entity_ids (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('feed', 'track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL DEFAULT 0,
    scheme          TEXT NOT NULL,
    value           TEXT NOT NULL,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, scheme, value)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_ids_feed
    ON source_entity_ids(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_ids_entity
    ON source_entity_ids(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_source_ids_scheme_value
    ON source_entity_ids(scheme, value);
