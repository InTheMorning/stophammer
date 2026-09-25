-- A block on one feed GUID or one exact feed URL (ADR 0053 Section 1).
-- A signed FeedBlocked event writes a row here; a signed FeedUnblocked event
-- removes one. Community nodes apply the same events, so this table replicates.
CREATE TABLE IF NOT EXISTS feed_blocks (
    block_id   TEXT PRIMARY KEY,
    kind       TEXT NOT NULL CHECK (kind IN ('guid','url')),
    value      TEXT NOT NULL,
    reason     TEXT NOT NULL,
    blocked_at INTEGER NOT NULL,
    UNIQUE (kind, value)
) STRICT;
