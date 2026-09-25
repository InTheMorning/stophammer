-- A pending GUID change at a source URL, and the link from a GUID the
-- transition retired to the GUID that replaced it (ADR 0052 sections 4 and
-- 5, task 007).
--
-- feed_guid_changes: one row for each source URL that currently declares a
-- GUID different from the record it used to name. A signed
-- FeedGuidChangeObserved event upserts the row when it is new or its
-- new_guid changes. The same event, with new_guid equal to old_guid, is the
-- delete marker: the source URL returned to its old GUID, and the row goes
-- away. A signed FeedGuidChangeDecided event writes the three decision
-- columns.
--
-- last_seen is local to the primary ingest path and makes no event, so a
-- community node holds it as null.
CREATE TABLE IF NOT EXISTS feed_guid_changes (
    source_url      TEXT PRIMARY KEY,
    old_guid        TEXT NOT NULL,
    new_guid        TEXT NOT NULL,
    first_seen      INTEGER NOT NULL,
    last_seen       INTEGER,
    decision        TEXT CHECK (decision IN ('approve','reject')),
    decision_reason TEXT,
    decided_at      INTEGER
) STRICT;

-- The link from a GUID the transition retired to the GUID that replaced it
-- (ADR 0052 section 5). A signed FeedGuidSuperseded event writes a row here.
-- GET /v1/feeds/{old_guid} reads this table to answer 404 with
-- superseded_by. The link is for navigation only: it does not resolve a
-- payment route, a track or a publisher relationship.
CREATE TABLE IF NOT EXISTS feed_guid_supersessions (
    old_guid      TEXT PRIMARY KEY,
    new_guid      TEXT NOT NULL,
    source_url    TEXT NOT NULL,
    superseded_at INTEGER NOT NULL
) STRICT;
