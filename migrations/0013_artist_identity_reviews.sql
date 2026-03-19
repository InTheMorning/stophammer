CREATE TABLE IF NOT EXISTS artist_identity_override (
    source            TEXT NOT NULL,
    name_key          TEXT NOT NULL,
    evidence_key      TEXT NOT NULL,
    override_type     TEXT NOT NULL CHECK(override_type IN ('merge', 'do_not_merge')),
    target_artist_id  TEXT,
    note              TEXT,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    PRIMARY KEY (source, name_key, evidence_key)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_artist_identity_override_type
    ON artist_identity_override(override_type);

CREATE TABLE IF NOT EXISTS artist_identity_review (
    review_id         INTEGER PRIMARY KEY,
    feed_guid         TEXT NOT NULL REFERENCES feeds(feed_guid) ON DELETE CASCADE,
    source            TEXT NOT NULL,
    name_key          TEXT NOT NULL,
    evidence_key      TEXT NOT NULL,
    status            TEXT NOT NULL CHECK(status IN ('pending', 'merged', 'blocked', 'resolved')),
    artist_ids_json   TEXT NOT NULL,
    artist_names_json TEXT NOT NULL,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    UNIQUE(feed_guid, source, name_key, evidence_key)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_artist_identity_review_status
    ON artist_identity_review(status);
CREATE INDEX IF NOT EXISTS idx_artist_identity_review_feed
    ON artist_identity_review(feed_guid);
