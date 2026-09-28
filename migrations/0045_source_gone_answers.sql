-- The count of a gone answer from the stored source URL of a record (ADR
-- 0067 Section 2). The primary writes a row on the first accepted `404` or
-- `410` from the source URL, and updates it on each accepted answer after.
-- Local to the primary ingest path and makes no event, so a community node
-- holds no row. The cascade deletes the row with its feed, so the feed
-- delete trigger does not change.
CREATE TABLE IF NOT EXISTS source_gone_answers (
    feed_guid     TEXT PRIMARY KEY REFERENCES feeds(feed_guid) ON DELETE CASCADE,
    source_url    TEXT NOT NULL,
    first_gone_at INTEGER NOT NULL,
    last_gone_at  INTEGER NOT NULL,
    last_status   INTEGER NOT NULL
) STRICT;
