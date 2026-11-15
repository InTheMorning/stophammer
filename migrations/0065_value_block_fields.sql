-- stophammer: foreign_keys=off
-- Keep route type text and reserve columns for value block metadata (ADR 0098 §2).
DROP TRIGGER IF EXISTS trg_tracks_cleanup_before_delete;
DROP TRIGGER IF EXISTS trg_feeds_cleanup_before_delete;

ALTER TABLE payment_routes RENAME TO payment_routes_legacy_0065;
CREATE TABLE payment_routes (
    id INTEGER PRIMARY KEY,
    track_guid TEXT NOT NULL,
    feed_guid TEXT NOT NULL,
    recipient_name TEXT,
    route_type TEXT NOT NULL,
    address TEXT NOT NULL,
    custom_key TEXT NOT NULL DEFAULT '',
    custom_value TEXT NOT NULL DEFAULT '',
    split INTEGER NOT NULL CHECK(split >= 0),
    fee INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (feed_guid, track_guid) REFERENCES tracks(feed_guid, track_guid)
) STRICT;
INSERT INTO payment_routes SELECT * FROM payment_routes_legacy_0065;
DROP TABLE payment_routes_legacy_0065;

ALTER TABLE feed_payment_routes RENAME TO feed_payment_routes_legacy_0065;
CREATE TABLE feed_payment_routes (
    id INTEGER PRIMARY KEY,
    feed_guid TEXT NOT NULL REFERENCES feeds(feed_guid),
    recipient_name TEXT,
    route_type TEXT NOT NULL,
    address TEXT NOT NULL,
    custom_key TEXT,
    custom_value TEXT,
    split INTEGER NOT NULL CHECK(split >= 0),
    fee INTEGER NOT NULL DEFAULT 0
) STRICT;
INSERT INTO feed_payment_routes SELECT * FROM feed_payment_routes_legacy_0065;
DROP TABLE feed_payment_routes_legacy_0065;

ALTER TABLE value_time_split_recipients RENAME TO value_time_split_recipients_legacy_0065;
CREATE TABLE value_time_split_recipients (
    id INTEGER PRIMARY KEY,
    split_id INTEGER NOT NULL REFERENCES value_time_splits(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    recipient_name TEXT,
    route_type TEXT NOT NULL,
    address TEXT NOT NULL,
    custom_key TEXT NOT NULL DEFAULT '',
    custom_value TEXT NOT NULL DEFAULT '',
    split INTEGER NOT NULL CHECK(split >= 0),
    fee INTEGER NOT NULL DEFAULT 0
) STRICT;
INSERT INTO value_time_split_recipients SELECT * FROM value_time_split_recipients_legacy_0065;
DROP TABLE value_time_split_recipients_legacy_0065;

ALTER TABLE feed_list_value_raw RENAME TO feed_list_value_raw_legacy_0065;
CREATE TABLE feed_list_value_raw (
    id INTEGER PRIMARY KEY,
    feed_guid TEXT NOT NULL REFERENCES feeds(feed_guid),
    recipient_name TEXT,
    route_type TEXT NOT NULL,
    address TEXT NOT NULL,
    custom_key TEXT NOT NULL DEFAULT '',
    custom_value TEXT NOT NULL DEFAULT '',
    split INTEGER NOT NULL CHECK(split >= 0),
    fee INTEGER NOT NULL DEFAULT 0,
    position INTEGER NOT NULL
) STRICT;
INSERT INTO feed_list_value_raw SELECT * FROM feed_list_value_raw_legacy_0065;
DROP TABLE feed_list_value_raw_legacy_0065;

ALTER TABLE payment_routes ADD COLUMN value_type TEXT;
ALTER TABLE payment_routes ADD COLUMN value_method TEXT;
ALTER TABLE payment_routes ADD COLUMN block_index INTEGER;
ALTER TABLE feed_payment_routes ADD COLUMN value_type TEXT;
ALTER TABLE feed_payment_routes ADD COLUMN value_method TEXT;
ALTER TABLE feed_payment_routes ADD COLUMN block_index INTEGER;
ALTER TABLE value_time_split_recipients ADD COLUMN value_type TEXT;
ALTER TABLE value_time_split_recipients ADD COLUMN value_method TEXT;
ALTER TABLE value_time_split_recipients ADD COLUMN block_index INTEGER;
ALTER TABLE feed_list_value_raw ADD COLUMN value_type TEXT;
ALTER TABLE feed_list_value_raw ADD COLUMN value_method TEXT;
ALTER TABLE feed_list_value_raw ADD COLUMN block_index INTEGER;
ALTER TABLE value_time_splits ADD COLUMN value_type TEXT;
ALTER TABLE value_time_splits ADD COLUMN value_method TEXT;
ALTER TABLE value_time_splits ADD COLUMN block_index INTEGER;

CREATE INDEX idx_routes_track_0032 ON payment_routes(track_guid);
CREATE INDEX idx_routes_feed_0032 ON payment_routes(feed_guid);
CREATE INDEX idx_routes_feed_track_0032 ON payment_routes(feed_guid, track_guid);
CREATE INDEX idx_feed_routes_guid ON feed_payment_routes(feed_guid);
CREATE INDEX idx_feed_list_value_guid ON feed_list_value_raw(feed_guid);
CREATE INDEX idx_vts_recipients_split ON value_time_split_recipients(split_id);

CREATE TRIGGER trg_feeds_cleanup_before_delete
BEFORE DELETE ON feeds
FOR EACH ROW
BEGIN
    DELETE FROM feed_payment_routes
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM feed_list_value_raw
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM entity_quality
    WHERE entity_type = 'feed'
      AND entity_id = OLD.feed_guid;

    DELETE FROM feed_remote_items_raw
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM live_events
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

CREATE TRIGGER trg_tracks_cleanup_before_delete
BEFORE DELETE ON tracks
FOR EACH ROW
BEGIN
    DELETE FROM value_time_split_recipients
    WHERE split_id IN (
        SELECT id FROM value_time_splits
        WHERE source_track_guid = OLD.track_guid
          AND (source_feed_guid = OLD.feed_guid OR source_feed_guid IS NULL)
    );

    DELETE FROM value_time_splits
    WHERE source_track_guid = OLD.track_guid
      AND (source_feed_guid = OLD.feed_guid OR source_feed_guid IS NULL);

    DELETE FROM payment_routes
    WHERE track_guid = OLD.track_guid
      AND feed_guid = OLD.feed_guid;

    DELETE FROM track_remote_items_raw
    WHERE track_guid = OLD.track_guid
      AND (feed_guid = OLD.feed_guid OR feed_guid IS NULL);

    DELETE FROM entity_quality
    WHERE entity_type = 'track'
      AND (entity_id = OLD.track_guid OR entity_id = json_array(OLD.feed_guid, OLD.track_guid));
END;
