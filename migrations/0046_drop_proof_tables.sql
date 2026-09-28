-- Drop the two proof-of-possession tables (ADR 0056 section 3, task 002).
--
-- ADR 0056 removed the code that read or wrote proof_challenges and
-- proof_tokens. It kept the tables for one release, because a rollback to
-- the prior binary still needed them. That release has passed. The
-- production copy holds no row in either table.
--
-- This migration rebuilds the feed delete trigger first, from its form in
-- migration 0043, with the two DELETE statements for the proof tables
-- removed. It then drops the two tables. No table refers to either proof
-- table, so the drops need no change of the foreign key setting. A PRAGMA
-- foreign_keys line would have no effect here: the runner runs each
-- migration inside a transaction.

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

DROP TABLE IF EXISTS proof_challenges;
DROP TABLE IF EXISTS proof_tokens;
