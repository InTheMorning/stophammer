-- Indexes for the route history read of a feed (ADR 0053 §4). The read
-- selects `routes_replaced` and `track_upserted` events by a feed GUID in the
-- JSON payload. Without these indexes SQLite parsed the payload of each such
-- event on each read (musicindex.org request 8). Each index covers one event
-- type and the exact expression of the query, so the query does not change.
CREATE INDEX IF NOT EXISTS idx_events_routes_feed
    ON events(json_extract(payload_json, '$.feed_guid'))
    WHERE event_type = 'routes_replaced';
CREATE INDEX IF NOT EXISTS idx_events_track_feed
    ON events(json_extract(payload_json, '$.track.feed_guid'))
    WHERE event_type = 'track_upserted';
