-- ADR 0079 §1: a retire that writes no block of ADR 0053 keeps a hold.
-- The table is local to the primary and makes no event.
CREATE TABLE IF NOT EXISTS retired_source_holds (
    feed_guid   TEXT PRIMARY KEY,
    source_url  TEXT NOT NULL,
    reason      TEXT NOT NULL,
    retired_at  INTEGER NOT NULL
) STRICT;
