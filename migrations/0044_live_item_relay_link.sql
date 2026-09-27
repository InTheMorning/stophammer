-- Store the relay link of a live item: the `uri` and `protocol` of its
-- `podcast:liveValue` element (ADR 0064 section 3).
ALTER TABLE live_events ADD COLUMN live_value_uri TEXT;
ALTER TABLE live_events ADD COLUMN live_value_protocol TEXT;
