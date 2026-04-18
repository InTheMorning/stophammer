CREATE TABLE IF NOT EXISTS track_remote_items_raw (
    id               INTEGER PRIMARY KEY,
    track_guid       TEXT NOT NULL REFERENCES tracks(track_guid),
    position         INTEGER NOT NULL,
    medium           TEXT,
    remote_feed_guid TEXT NOT NULL,
    remote_feed_url  TEXT,
    source           TEXT NOT NULL DEFAULT 'podcast_remote_item',
    UNIQUE(track_guid, position)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_track_remote_items_track ON track_remote_items_raw(track_guid);
CREATE INDEX IF NOT EXISTS idx_track_remote_items_guid  ON track_remote_items_raw(remote_feed_guid);
