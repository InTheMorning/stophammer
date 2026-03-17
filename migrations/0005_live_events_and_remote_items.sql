CREATE TABLE IF NOT EXISTS feed_remote_items_raw (
    id               INTEGER PRIMARY KEY,
    feed_guid        TEXT NOT NULL REFERENCES feeds(feed_guid),
    position         INTEGER NOT NULL,
    medium           TEXT,
    remote_feed_guid TEXT NOT NULL,
    remote_feed_url  TEXT,
    source           TEXT NOT NULL DEFAULT 'podcast_remote_item',
    UNIQUE(feed_guid, position)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_feed_remote_items_feed ON feed_remote_items_raw(feed_guid);
CREATE INDEX IF NOT EXISTS idx_feed_remote_items_guid ON feed_remote_items_raw(remote_feed_guid);

CREATE TABLE IF NOT EXISTS live_events (
    live_item_guid  TEXT PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    title           TEXT NOT NULL,
    content_link    TEXT,
    status          TEXT NOT NULL CHECK(status IN ('pending', 'live', 'ended')),
    scheduled_start INTEGER,
    scheduled_end   INTEGER,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_live_events_feed   ON live_events(feed_guid);
CREATE INDEX IF NOT EXISTS idx_live_events_status ON live_events(status);
