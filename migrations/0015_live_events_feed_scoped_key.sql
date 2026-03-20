ALTER TABLE live_events RENAME TO live_events_legacy;

CREATE TABLE IF NOT EXISTS live_events (
    live_item_guid  TEXT NOT NULL,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    title           TEXT NOT NULL,
    content_link    TEXT,
    status          TEXT NOT NULL CHECK(status IN ('pending', 'live', 'ended')),
    scheduled_start INTEGER,
    scheduled_end   INTEGER,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    PRIMARY KEY (feed_guid, live_item_guid)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_live_events_feed
    ON live_events(feed_guid);
CREATE INDEX IF NOT EXISTS idx_live_events_guid
    ON live_events(live_item_guid);
CREATE INDEX IF NOT EXISTS idx_live_events_status
    ON live_events(status);

INSERT INTO live_events (
    live_item_guid,
    feed_guid,
    title,
    content_link,
    status,
    scheduled_start,
    scheduled_end,
    created_at,
    updated_at
)
SELECT
    live_item_guid,
    feed_guid,
    title,
    content_link,
    status,
    scheduled_start,
    scheduled_end,
    created_at,
    updated_at
FROM live_events_legacy;
