-- A summary row for each pair of GUID and URL that a mirror body names
-- (ADR 0058 §1). A signed FeedCopyObserved event upserts a row when the
-- summary of the pair is new or changes. A signed FeedCopyResolved event
-- writes the four resolution columns of an existing row.
--
-- last_seen is local to the primary ingest path and makes no event, so a
-- community node holds it as null.
CREATE TABLE IF NOT EXISTS feed_copies (
    feed_guid         TEXT NOT NULL,
    url               TEXT NOT NULL,
    first_seen        INTEGER NOT NULL,
    last_seen         INTEGER,
    title             TEXT NOT NULL,
    item_guids        TEXT NOT NULL,
    feed_recipients   TEXT NOT NULL,
    track_recipients  TEXT NOT NULL,
    summary_digest    TEXT NOT NULL,
    resolution        TEXT CHECK (resolution IN ('keep_source','relocate')),
    resolution_reason TEXT,
    resolved_at       INTEGER,
    resolved_digest   TEXT,
    PRIMARY KEY (feed_guid, url)
) STRICT;

-- The count of mirror bodies for a GUID that arrived at a new URL after the
-- GUID already held MAX_COPIES_PER_GUID rows (ADR 0058 §1a). Local to the
-- primary ingest path. A community node never writes it.
CREATE TABLE IF NOT EXISTS feed_copy_overflow (
    feed_guid TEXT PRIMARY KEY,
    count     INTEGER NOT NULL DEFAULT 0
) STRICT;
