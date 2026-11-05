-- Store the move state of a record: the arrival tag, the count of returns and
-- the time of the last return.
-- ADR 0083 §4 and §5: the table is local to the primary, and no event carries
-- it. The table starts empty. A record with no row has no arrival tag and a
-- count of zero.
CREATE TABLE IF NOT EXISTS feed_move_state (
    feed_guid      TEXT PRIMARY KEY,
    arrival_tag    TEXT,
    return_count   INTEGER NOT NULL DEFAULT 0,
    last_return_at INTEGER,
    arrival_pending INTEGER NOT NULL DEFAULT 0
) STRICT;
