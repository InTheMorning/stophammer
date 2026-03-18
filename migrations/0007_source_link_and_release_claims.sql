CREATE TABLE IF NOT EXISTS source_entity_links (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('feed', 'track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL,
    link_type       TEXT NOT NULL,
    url             TEXT NOT NULL,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, link_type, url)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_links_feed
    ON source_entity_links(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_links_entity
    ON source_entity_links(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_source_links_type
    ON source_entity_links(link_type);

CREATE TABLE IF NOT EXISTS source_release_claims (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('feed', 'track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL DEFAULT 0,
    claim_type      TEXT NOT NULL,
    claim_value     TEXT NOT NULL,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, claim_type, position)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_release_claims_feed
    ON source_release_claims(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_release_claims_entity
    ON source_release_claims(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_source_release_claims_type
    ON source_release_claims(claim_type);
