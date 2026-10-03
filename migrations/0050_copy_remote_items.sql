-- Add channel-level remote items to feed_copies table.
-- ADR 0058 §1d, task 007: each copy row keeps its channel-level
-- podcast:remoteItem entries in the sequence of the body. Each entry keeps
-- the raw medium, feedGuid, feedUrl and itemGuid. The list has its own digest
-- to track changes independent of the summary digest.
ALTER TABLE feed_copies
ADD COLUMN remote_items TEXT;

ALTER TABLE feed_copies
ADD COLUMN remote_items_digest TEXT;

ALTER TABLE feed_copies
ADD COLUMN resolved_remote_items_digest TEXT;
