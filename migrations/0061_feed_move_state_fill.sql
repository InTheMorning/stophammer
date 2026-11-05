-- Give an arrival tag to each record that a candidate of release 0.18.1 moved.
-- ADR 0083 §4: such a record has left URLs and no state row, so a stale tag at
-- its source URL has no protection. The stored tag is the best available value.
INSERT OR IGNORE INTO feed_move_state (feed_guid, arrival_tag)
SELECT feed_guid, declared_new_feed_url
FROM feeds
WHERE feed_guid IN (SELECT feed_guid FROM feed_left_urls);
