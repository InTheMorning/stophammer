-- Store both forms of a value time split (ADR 0073 §2, task 001).
-- A split is in the remote form or in the local form. The table keeps the
-- stated values: decimal times, the remote start time and the remote
-- percentage. A local split keeps its recipients in a child table. Each row
-- from before this migration becomes a remote split at position 0, with a
-- null remote percentage. The old `split` column is dropped.

-- Drop the track trigger first. The rename below can fail when a trigger
-- body names a table that is dropped.
DROP TRIGGER IF EXISTS trg_tracks_cleanup_before_delete;

DROP INDEX IF EXISTS idx_vts_source;
DROP INDEX IF EXISTS idx_vts_track_guid_only;

ALTER TABLE value_time_splits RENAME TO value_time_splits_legacy_0053;

CREATE TABLE value_time_splits (
    id                     INTEGER PRIMARY KEY,
    source_feed_guid       TEXT,
    source_track_guid      TEXT NOT NULL,
    position               INTEGER NOT NULL DEFAULT 0,
    form                   TEXT NOT NULL CHECK(form IN ('remote','local')),
    start_time_secs        REAL NOT NULL,
    duration_secs          REAL,
    remote_feed_guid       TEXT,
    remote_item_guid       TEXT,
    remote_start_time_secs REAL,
    remote_percentage      REAL,
    created_at             INTEGER NOT NULL,
    CHECK(form = 'local' OR remote_feed_guid IS NOT NULL),
    FOREIGN KEY (source_feed_guid, source_track_guid) REFERENCES tracks(feed_guid, track_guid)
) STRICT;

INSERT INTO value_time_splits (
    id, source_feed_guid, source_track_guid, position, form,
    start_time_secs, duration_secs, remote_feed_guid, remote_item_guid,
    remote_start_time_secs, remote_percentage, created_at
)
SELECT
    id, source_feed_guid, source_track_guid, 0, 'remote',
    CAST(start_time_secs AS REAL), CAST(duration_secs AS REAL),
    remote_feed_guid, remote_item_guid,
    NULL, NULL, created_at
FROM value_time_splits_legacy_0053;

DROP TABLE value_time_splits_legacy_0053;

CREATE INDEX IF NOT EXISTS idx_vts_source ON value_time_splits(source_feed_guid, source_track_guid);
CREATE INDEX IF NOT EXISTS idx_vts_track_guid_only ON value_time_splits(source_track_guid);

-- The recipients of a local split, in source order (ADR 0073 §2). The
-- recipient columns have the types and checks of `payment_routes`.
CREATE TABLE IF NOT EXISTS value_time_split_recipients (
    id              INTEGER PRIMARY KEY,
    split_id        INTEGER NOT NULL REFERENCES value_time_splits(id) ON DELETE CASCADE,
    position        INTEGER NOT NULL,
    recipient_name  TEXT,
    route_type      TEXT NOT NULL CHECK(route_type IN ('node','wallet','keysend','lnaddress')),
    address         TEXT NOT NULL,
    custom_key      TEXT NOT NULL DEFAULT '',
    custom_value    TEXT NOT NULL DEFAULT '',
    split           INTEGER NOT NULL CHECK(split >= 0),
    fee             INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX IF NOT EXISTS idx_vts_recipients_split ON value_time_split_recipients(split_id);

-- The body of migration 0047, with one first statement: delete the
-- recipients of each split of the track (ADR 0073 §2).
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
