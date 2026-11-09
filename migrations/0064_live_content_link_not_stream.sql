-- Clear each stored content_link that is the stream of its live item.
-- ADR 0093 §4: the parser used the enclosure URL when a live item had no
-- contentLink attribute. The enclosure stays in source_item_enclosures, and
-- the next changed body of the feed gives the content link.
UPDATE live_events
SET content_link = NULL
WHERE content_link IS NOT NULL
  AND EXISTS (
      SELECT 1
      FROM source_item_enclosures e
      WHERE e.feed_guid = live_events.feed_guid
        AND e.entity_type = 'live_item'
        AND e.entity_id = live_events.live_item_guid
        AND e.url = live_events.content_link
  );
