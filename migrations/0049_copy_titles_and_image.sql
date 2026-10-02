-- Add item titles and image URL to feed_copies table.
-- ADR 0058 §1c, task 006: each copy row keeps the title of each item,
-- in the order of the item GUIDs, and the channel image URL of the copy.
-- The item title is null when the item has no title. The image URL is null
-- when the copy states no image.
ALTER TABLE feed_copies
ADD COLUMN item_titles TEXT;

ALTER TABLE feed_copies
ADD COLUMN image_url TEXT;
