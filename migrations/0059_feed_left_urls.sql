-- Store the URLs that a record left by a move.
-- ADR 0083 §4: the table is local to the primary, and no event carries it.
-- The table starts empty.
CREATE TABLE IF NOT EXISTS feed_left_urls (
    feed_guid TEXT NOT NULL,
    url       TEXT NOT NULL,
    PRIMARY KEY (feed_guid, url)
) STRICT;
