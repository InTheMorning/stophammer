-- The move declarations a source body makes for itself, and the
-- podcast:locked fact it carries (ADR 0052 section 2, task 006).
--
-- declared_new_feed_url: the value of a source body's itunes:new-feed-url.
-- Only an ingest in the update or new-feed case writes it, as for
-- declared_self_url. A mirror submission reads it to decide a move; it
-- never writes it.
--
-- podcast_locked and locked_owner: the podcast:locked fact and its owner
-- email. The node stores them as source facts. They grant nothing in this
-- index (ADR 0052 section 3).
ALTER TABLE feeds ADD COLUMN declared_new_feed_url TEXT;
ALTER TABLE feeds ADD COLUMN podcast_locked INTEGER;
ALTER TABLE feeds ADD COLUMN locked_owner TEXT;
