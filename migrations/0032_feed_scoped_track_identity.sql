PRAGMA foreign_keys = OFF;

DROP TRIGGER IF EXISTS trg_tracks_cleanup_before_delete;
DROP TRIGGER IF EXISTS trg_feeds_cleanup_before_delete;

ALTER TABLE tracks RENAME TO tracks_legacy_0032;

CREATE TABLE tracks (
    track_guid       TEXT NOT NULL,
    feed_guid        TEXT NOT NULL REFERENCES feeds(feed_guid),
    artist_credit_id INTEGER NOT NULL REFERENCES artist_credit(id),
    title            TEXT NOT NULL,
    title_lower      TEXT NOT NULL,
    pub_date         INTEGER,
    duration_secs    INTEGER,
    image_url        TEXT,
    publisher        TEXT,
    language         TEXT,
    enclosure_url    TEXT,
    enclosure_type   TEXT,
    enclosure_bytes  INTEGER,
    track_number     INTEGER,
    season           INTEGER,
    explicit         INTEGER NOT NULL DEFAULT 0,
    description      TEXT,
    track_artist     TEXT,
    track_artist_sort TEXT,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    PRIMARY KEY (feed_guid, track_guid)
) STRICT;

INSERT INTO tracks (
    track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date,
    duration_secs, image_url, publisher, language, enclosure_url, enclosure_type,
    enclosure_bytes, track_number, season, explicit, description, track_artist,
    track_artist_sort, created_at, updated_at
)
SELECT
    track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date,
    duration_secs, image_url, publisher, language, enclosure_url, enclosure_type,
    enclosure_bytes, track_number, season, explicit, description, track_artist,
    track_artist_sort, created_at, updated_at
FROM tracks_legacy_0032;

ALTER TABLE payment_routes RENAME TO payment_routes_legacy_0032;

CREATE TABLE payment_routes (
    id              INTEGER PRIMARY KEY,
    track_guid      TEXT NOT NULL,
    feed_guid       TEXT NOT NULL,
    recipient_name  TEXT,
    route_type      TEXT NOT NULL CHECK(route_type IN ('node','wallet','keysend','lnaddress')),
    address         TEXT NOT NULL,
    custom_key      TEXT NOT NULL DEFAULT '',
    custom_value    TEXT NOT NULL DEFAULT '',
    split           INTEGER NOT NULL CHECK(split >= 0),
    fee             INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (feed_guid, track_guid) REFERENCES tracks(feed_guid, track_guid)
) STRICT;

INSERT INTO payment_routes (
    id, track_guid, feed_guid, recipient_name, route_type, address,
    custom_key, custom_value, split, fee
)
SELECT
    id, track_guid, feed_guid, recipient_name, route_type, address,
    custom_key, custom_value, split, fee
FROM payment_routes_legacy_0032;

ALTER TABLE value_time_splits RENAME TO value_time_splits_legacy_0032;

CREATE TABLE value_time_splits (
    id                  INTEGER PRIMARY KEY,
    source_feed_guid    TEXT,
    source_track_guid   TEXT NOT NULL,
    start_time_secs     INTEGER NOT NULL,
    duration_secs       INTEGER,
    remote_feed_guid    TEXT NOT NULL,
    remote_item_guid    TEXT NOT NULL,
    split               INTEGER NOT NULL CHECK(split >= 0),
    created_at          INTEGER NOT NULL,
    FOREIGN KEY (source_feed_guid, source_track_guid) REFERENCES tracks(feed_guid, track_guid)
) STRICT;

INSERT INTO value_time_splits (
    id, source_feed_guid, source_track_guid, start_time_secs, duration_secs,
    remote_feed_guid, remote_item_guid, split, created_at
)
SELECT
    v.id, t.feed_guid, v.source_track_guid, v.start_time_secs, v.duration_secs,
    v.remote_feed_guid, v.remote_item_guid, v.split, v.created_at
FROM value_time_splits_legacy_0032 v
LEFT JOIN tracks_legacy_0032 t ON t.track_guid = v.source_track_guid;

ALTER TABLE track_remote_items_raw RENAME TO track_remote_items_raw_legacy_0032;

CREATE TABLE track_remote_items_raw (
    id               INTEGER PRIMARY KEY,
    feed_guid        TEXT,
    track_guid       TEXT NOT NULL,
    position         INTEGER NOT NULL,
    medium           TEXT,
    remote_feed_guid TEXT NOT NULL,
    remote_feed_url  TEXT,
    source           TEXT NOT NULL DEFAULT 'podcast_remote_item',
    UNIQUE(feed_guid, track_guid, position),
    FOREIGN KEY (feed_guid, track_guid) REFERENCES tracks(feed_guid, track_guid)
) STRICT;

INSERT INTO track_remote_items_raw (
    id, feed_guid, track_guid, position, medium, remote_feed_guid, remote_feed_url, source
)
SELECT
    r.id, t.feed_guid, r.track_guid, r.position, r.medium, r.remote_feed_guid, r.remote_feed_url, r.source
FROM track_remote_items_raw_legacy_0032 r
LEFT JOIN tracks_legacy_0032 t ON t.track_guid = r.track_guid;

ALTER TABLE track_tag RENAME TO track_tag_legacy_0032;

CREATE TABLE track_tag (
    feed_guid  TEXT,
    track_guid TEXT NOT NULL,
    tag_id     INTEGER NOT NULL REFERENCES tags(id),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (feed_guid, track_guid, tag_id),
    FOREIGN KEY (feed_guid, track_guid) REFERENCES tracks(feed_guid, track_guid)
) STRICT;

INSERT INTO track_tag (feed_guid, track_guid, tag_id, created_at)
SELECT t.feed_guid, tt.track_guid, tt.tag_id, tt.created_at
FROM track_tag_legacy_0032 tt
LEFT JOIN tracks_legacy_0032 t ON t.track_guid = tt.track_guid;

ALTER TABLE track_rel RENAME TO track_rel_legacy_0032;

CREATE TABLE track_rel (
    id           INTEGER PRIMARY KEY,
    feed_guid_a  TEXT,
    track_guid_a TEXT NOT NULL,
    feed_guid_b  TEXT,
    track_guid_b TEXT NOT NULL,
    rel_type_id  INTEGER NOT NULL REFERENCES rel_type(id),
    created_at   INTEGER NOT NULL,
    FOREIGN KEY (feed_guid_a, track_guid_a) REFERENCES tracks(feed_guid, track_guid),
    FOREIGN KEY (feed_guid_b, track_guid_b) REFERENCES tracks(feed_guid, track_guid)
) STRICT;

INSERT INTO track_rel (
    id, feed_guid_a, track_guid_a, feed_guid_b, track_guid_b, rel_type_id, created_at
)
SELECT
    tr.id, ta.feed_guid, tr.track_guid_a, tb.feed_guid, tr.track_guid_b, tr.rel_type_id, tr.created_at
FROM track_rel_legacy_0032 tr
LEFT JOIN tracks_legacy_0032 ta ON ta.track_guid = tr.track_guid_a
LEFT JOIN tracks_legacy_0032 tb ON tb.track_guid = tr.track_guid_b;

CREATE INDEX idx_tracks_feed_0032     ON tracks(feed_guid);
CREATE INDEX idx_tracks_credit_0032   ON tracks(artist_credit_id);
CREATE INDEX idx_tracks_pub_date_0032 ON tracks(pub_date DESC);
CREATE INDEX idx_tracks_title_0032    ON tracks(title_lower);
CREATE INDEX idx_tracks_guid_0032     ON tracks(track_guid);
CREATE INDEX idx_tracks_artist_lower_0032 ON tracks(lower(track_artist));

CREATE INDEX idx_routes_track_0032 ON payment_routes(track_guid);
CREATE INDEX idx_routes_feed_0032  ON payment_routes(feed_guid);
CREATE INDEX idx_routes_feed_track_0032 ON payment_routes(feed_guid, track_guid);

CREATE INDEX idx_vts_source_0032 ON value_time_splits(source_feed_guid, source_track_guid);
CREATE INDEX idx_vts_track_guid_only_0032 ON value_time_splits(source_track_guid);

CREATE INDEX idx_track_remote_items_track_0032 ON track_remote_items_raw(feed_guid, track_guid);
CREATE INDEX idx_track_remote_items_guid_0032  ON track_remote_items_raw(remote_feed_guid);
CREATE INDEX idx_track_remote_items_track_guid_only_0032 ON track_remote_items_raw(track_guid);

CREATE INDEX idx_track_tag_tag_0032 ON track_tag(tag_id);
CREATE INDEX idx_track_tag_track_0032 ON track_tag(feed_guid, track_guid);

CREATE INDEX idx_trel_a_0032   ON track_rel(feed_guid_a, track_guid_a);
CREATE INDEX idx_trel_b_0032   ON track_rel(feed_guid_b, track_guid_b);
CREATE INDEX idx_trel_rel_0032 ON track_rel(rel_type_id);

CREATE TRIGGER trg_tracks_cleanup_before_delete
BEFORE DELETE ON tracks
FOR EACH ROW
BEGIN
    DELETE FROM track_tag
    WHERE track_guid = OLD.track_guid
      AND (feed_guid = OLD.feed_guid OR feed_guid IS NULL);

    DELETE FROM value_time_splits
    WHERE source_track_guid = OLD.track_guid
      AND (source_feed_guid = OLD.feed_guid OR source_feed_guid IS NULL);

    DELETE FROM payment_routes
    WHERE track_guid = OLD.track_guid
      AND feed_guid = OLD.feed_guid;

    DELETE FROM track_remote_items_raw
    WHERE track_guid = OLD.track_guid
      AND (feed_guid = OLD.feed_guid OR feed_guid IS NULL);

    DELETE FROM track_rel
    WHERE (track_guid_a = OLD.track_guid AND (feed_guid_a = OLD.feed_guid OR feed_guid_a IS NULL))
       OR (track_guid_b = OLD.track_guid AND (feed_guid_b = OLD.feed_guid OR feed_guid_b IS NULL));

    DELETE FROM entity_quality
    WHERE entity_type = 'track'
      AND (entity_id = OLD.track_guid OR entity_id = json_array(OLD.feed_guid, OLD.track_guid));

    DELETE FROM entity_field_status
    WHERE entity_type = 'track'
      AND (entity_id = OLD.track_guid OR entity_id = json_array(OLD.feed_guid, OLD.track_guid));
END;

CREATE TRIGGER trg_feeds_cleanup_before_delete
BEFORE DELETE ON feeds
FOR EACH ROW
BEGIN
    DELETE FROM feed_tag
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM feed_payment_routes
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM entity_quality
    WHERE entity_type = 'feed'
      AND entity_id = OLD.feed_guid;

    DELETE FROM entity_field_status
    WHERE entity_type = 'feed'
      AND entity_id = OLD.feed_guid;

    DELETE FROM proof_tokens
    WHERE subject_feed_guid = OLD.feed_guid;

    DELETE FROM proof_challenges
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM feed_rel
    WHERE feed_guid_a = OLD.feed_guid
       OR feed_guid_b = OLD.feed_guid;

    DELETE FROM feed_remote_items_raw
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM live_events
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM live_events_legacy
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_contributor_claims
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_entity_ids
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_entity_links
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_release_claims
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_item_enclosures
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_item_transcripts
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_platform_claims
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM tracks
    WHERE feed_guid = OLD.feed_guid;
END;

PRAGMA foreign_keys = ON;
