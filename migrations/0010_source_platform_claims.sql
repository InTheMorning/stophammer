CREATE TABLE IF NOT EXISTS source_platform_claims (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    platform_key    TEXT NOT NULL,
    url             TEXT,
    owner_name      TEXT,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_platform_claims_feed
    ON source_platform_claims(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_platform_claims_platform
    ON source_platform_claims(platform_key);
