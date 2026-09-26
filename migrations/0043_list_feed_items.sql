-- Store itemGuid and title of channel remote items (ADR 0060 §2).
-- Also create feed_list_value_raw to store channel payment routes of
-- musicL feeds as source data (ADR 0060 §4).

-- Add itemGuid and title fields to feed remote items.
ALTER TABLE feed_remote_items_raw ADD COLUMN remote_item_guid TEXT;
ALTER TABLE feed_remote_items_raw ADD COLUMN remote_item_title TEXT;

-- A list feed keeps its value block as source data (ADR 0060 §4).
-- Same columns as feed_payment_routes, plus position.
CREATE TABLE IF NOT EXISTS feed_list_value_raw (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    recipient_name  TEXT,
    route_type      TEXT NOT NULL CHECK(route_type IN ('node','wallet','keysend','lnaddress')),
    address         TEXT NOT NULL,
    custom_key      TEXT NOT NULL DEFAULT '',
    custom_value    TEXT NOT NULL DEFAULT '',
    split           INTEGER NOT NULL CHECK(split >= 0),
    fee             INTEGER NOT NULL DEFAULT 0,
    position        INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_feed_list_value_guid ON feed_list_value_raw(feed_guid);

-- Update the feed deletion trigger to also clean up feed_list_value_raw rows.
PRAGMA foreign_keys = OFF;

DROP TRIGGER IF EXISTS trg_feeds_cleanup_before_delete;

CREATE TRIGGER trg_feeds_cleanup_before_delete
BEFORE DELETE ON feeds
FOR EACH ROW
BEGIN
    DELETE FROM feed_tag
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM feed_payment_routes
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM feed_list_value_raw
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
