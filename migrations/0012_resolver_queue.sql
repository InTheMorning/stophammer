CREATE TABLE IF NOT EXISTS resolver_queue (
    feed_guid        TEXT PRIMARY KEY REFERENCES feeds(feed_guid) ON DELETE CASCADE,
    dirty_mask       INTEGER NOT NULL,
    first_marked_at  INTEGER NOT NULL,
    last_marked_at   INTEGER NOT NULL,
    locked_at        INTEGER,
    locked_by        TEXT,
    attempt_count    INTEGER NOT NULL DEFAULT 0,
    last_error       TEXT
) STRICT;

CREATE INDEX IF NOT EXISTS idx_resolver_queue_locked_at
    ON resolver_queue(locked_at);
CREATE INDEX IF NOT EXISTS idx_resolver_queue_last_marked
    ON resolver_queue(last_marked_at);

CREATE TABLE IF NOT EXISTS resolver_state (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
) STRICT;

