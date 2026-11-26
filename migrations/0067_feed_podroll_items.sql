CREATE TABLE feed_podroll_items (
    id               INTEGER PRIMARY KEY,
    feed_guid        TEXT NOT NULL REFERENCES feeds(feed_guid),
    position         INTEGER NOT NULL,
    medium           TEXT,
    remote_feed_guid TEXT NOT NULL,
    remote_feed_url  TEXT,
    UNIQUE(feed_guid, position)
) STRICT;

CREATE INDEX idx_feed_podroll_items_guid ON feed_podroll_items(remote_feed_guid);

DROP TRIGGER IF EXISTS trg_feeds_cleanup_before_delete;
CREATE TRIGGER trg_feeds_cleanup_before_delete
BEFORE DELETE ON feeds
FOR EACH ROW
BEGIN
    DELETE FROM feed_payment_routes WHERE feed_guid = OLD.feed_guid;
    DELETE FROM feed_list_value_raw WHERE feed_guid = OLD.feed_guid;
    DELETE FROM entity_quality
    WHERE entity_type = 'feed' AND entity_id = OLD.feed_guid;
    DELETE FROM feed_remote_items_raw WHERE feed_guid = OLD.feed_guid;
    DELETE FROM feed_podroll_items WHERE feed_guid = OLD.feed_guid;
    DELETE FROM live_events WHERE feed_guid = OLD.feed_guid;
    DELETE FROM source_contributor_claims WHERE feed_guid = OLD.feed_guid;
    DELETE FROM source_entity_ids WHERE feed_guid = OLD.feed_guid;
    DELETE FROM source_entity_links WHERE feed_guid = OLD.feed_guid;
    DELETE FROM source_release_claims WHERE feed_guid = OLD.feed_guid;
    DELETE FROM source_item_enclosures WHERE feed_guid = OLD.feed_guid;
    DELETE FROM source_item_transcripts WHERE feed_guid = OLD.feed_guid;
    DELETE FROM source_platform_claims WHERE feed_guid = OLD.feed_guid;
    DELETE FROM tracks WHERE feed_guid = OLD.feed_guid;
END;
