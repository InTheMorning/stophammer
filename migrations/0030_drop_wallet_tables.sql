-- Rebuild delete triggers without wallet_* references before dropping the
-- wallet tables themselves, so no trigger references a missing table.
DROP TRIGGER IF EXISTS trg_tracks_cleanup_before_delete;
CREATE TRIGGER trg_tracks_cleanup_before_delete
BEFORE DELETE ON tracks
FOR EACH ROW
BEGIN
    DELETE FROM track_tag
    WHERE track_guid = OLD.track_guid;

    DELETE FROM value_time_splits
    WHERE source_track_guid = OLD.track_guid;

    DELETE FROM payment_routes
    WHERE track_guid = OLD.track_guid;

    DELETE FROM track_rel
    WHERE track_guid_a = OLD.track_guid
       OR track_guid_b = OLD.track_guid;

    DELETE FROM entity_quality
    WHERE entity_type = 'track'
      AND entity_id = OLD.track_guid;

    DELETE FROM entity_field_status
    WHERE entity_type = 'track'
      AND entity_id = OLD.track_guid;
END;

DROP TRIGGER IF EXISTS trg_feeds_cleanup_before_delete;
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

    DELETE FROM source_platform_claims
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM tracks
    WHERE feed_guid = OLD.feed_guid;
END;

DROP TABLE IF EXISTS wallet_merge_apply_entry;
DROP TABLE IF EXISTS wallet_merge_apply_batch;
DROP TABLE IF EXISTS wallet_identity_override;
DROP TABLE IF EXISTS wallet_identity_review;
DROP TABLE IF EXISTS wallet_identity_review_legacy_0023;
DROP TABLE IF EXISTS wallet_identity_review_legacy_0024;
DROP TABLE IF EXISTS wallet_artist_links;
DROP TABLE IF EXISTS wallet_id_redirect;
DROP TABLE IF EXISTS wallet_feed_route_map;
DROP TABLE IF EXISTS wallet_track_route_map;
DROP TABLE IF EXISTS wallet_aliases;
DROP TABLE IF EXISTS wallet_endpoints;
DROP TABLE IF EXISTS wallets;
