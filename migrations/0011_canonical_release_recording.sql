CREATE TABLE IF NOT EXISTS releases (
    release_id        TEXT PRIMARY KEY,
    title             TEXT NOT NULL,
    title_lower       TEXT NOT NULL,
    artist_credit_id  INTEGER NOT NULL REFERENCES artist_credit(id),
    description       TEXT,
    image_url         TEXT,
    release_date      INTEGER,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_releases_credit ON releases(artist_credit_id);
CREATE INDEX IF NOT EXISTS idx_releases_title ON releases(title_lower);

CREATE TABLE IF NOT EXISTS recordings (
    recording_id      TEXT PRIMARY KEY,
    title             TEXT NOT NULL,
    title_lower       TEXT NOT NULL,
    artist_credit_id  INTEGER NOT NULL REFERENCES artist_credit(id),
    duration_secs     INTEGER,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_recordings_credit ON recordings(artist_credit_id);
CREATE INDEX IF NOT EXISTS idx_recordings_title ON recordings(title_lower);

CREATE TABLE IF NOT EXISTS release_recordings (
    release_id        TEXT NOT NULL REFERENCES releases(release_id),
    recording_id      TEXT NOT NULL REFERENCES recordings(recording_id),
    position          INTEGER NOT NULL,
    source_track_guid TEXT REFERENCES tracks(track_guid),
    PRIMARY KEY (release_id, position),
    UNIQUE(release_id, recording_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_release_recordings_recording ON release_recordings(recording_id);
CREATE INDEX IF NOT EXISTS idx_release_recordings_source_track ON release_recordings(source_track_guid);

CREATE TABLE IF NOT EXISTS source_feed_release_map (
    feed_guid    TEXT PRIMARY KEY REFERENCES feeds(feed_guid),
    release_id   TEXT NOT NULL REFERENCES releases(release_id),
    match_type   TEXT NOT NULL,
    confidence   INTEGER NOT NULL CHECK(confidence BETWEEN 0 AND 100),
    created_at   INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_feed_release_map_release
    ON source_feed_release_map(release_id);

CREATE TABLE IF NOT EXISTS source_item_recording_map (
    track_guid    TEXT PRIMARY KEY REFERENCES tracks(track_guid),
    recording_id  TEXT NOT NULL REFERENCES recordings(recording_id),
    match_type    TEXT NOT NULL,
    confidence    INTEGER NOT NULL CHECK(confidence BETWEEN 0 AND 100),
    created_at    INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_item_recording_map_recording
    ON source_item_recording_map(recording_id);
