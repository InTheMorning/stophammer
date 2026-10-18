-- Add item facts to feed_copies table.
-- ADR 0058 §1e, task 008: each copy row keeps the duration, the enclosure
-- URL and the publication date of each item, in the order of the item
-- GUIDs. Each row also keeps the channel author. An item with no value
-- gives null in its position. A copy with no author keeps author null.
ALTER TABLE feed_copies
ADD COLUMN item_durations TEXT;

ALTER TABLE feed_copies
ADD COLUMN item_enclosure_urls TEXT;

ALTER TABLE feed_copies
ADD COLUMN item_pub_dates TEXT;

ALTER TABLE feed_copies
ADD COLUMN author TEXT;
