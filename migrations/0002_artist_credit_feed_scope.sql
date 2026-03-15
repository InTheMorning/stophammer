-- Migration 0002: scope artist credits to feed_guid
-- Issue-ARTIST-IDENTITY — 2026-03-14
--
-- Artist credits were previously deduplicated on LOWER(display_name) alone,
-- which caused cross-feed name collisions at scale (two unrelated podcasts
-- with owner_name = "John Smith" shared an artist record).  This migration
-- adds a feed_guid column so the dedup key becomes (display_name, feed_guid).

ALTER TABLE artist_credit ADD COLUMN feed_guid TEXT;

-- New composite unique index for feed-scoped dedup.
CREATE UNIQUE INDEX IF NOT EXISTS idx_artist_credit_display_feed
    ON artist_credit(LOWER(display_name), feed_guid);

-- Scope artist_aliases by feed_guid so that the same lowercased name on
-- different feeds resolves to different artist rows.
ALTER TABLE artist_aliases ADD COLUMN feed_guid TEXT;

CREATE INDEX IF NOT EXISTS idx_aliases_feed ON artist_aliases(feed_guid);
