CREATE TABLE IF NOT EXISTS feed_url_observations (
    url         TEXT PRIMARY KEY,
    feed_guid   TEXT NOT NULL,
    observed_at INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS idx_feed_url_observations_guid
    ON feed_url_observations(feed_guid);
INSERT OR IGNORE INTO feed_url_observations (url, feed_guid, observed_at)
    SELECT feed_url, feed_guid, created_at FROM feeds;
