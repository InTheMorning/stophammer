-- Issue-ARTIST-CREDIT-NULL-SCOPE — 2026-03-28
-- Legacy artist_credit rows created before feed scoping have feed_guid IS NULL.
-- SQLite treats NULLs as distinct in UNIQUE indexes, so the existing
-- idx_artist_credit_display_feed index does not prevent duplicate
-- (LOWER(display_name), NULL) rows. Deduplicate legacy rows, repoint
-- references, then enforce uniqueness with a COALESCE() expression index.

WITH canonical AS (
    SELECT LOWER(display_name) AS display_name_lower, MIN(id) AS keep_id
    FROM artist_credit
    WHERE feed_guid IS NULL
    GROUP BY LOWER(display_name)
),
duplicates AS (
    SELECT ac.id AS drop_id, canonical.keep_id
    FROM artist_credit ac
    JOIN canonical
      ON canonical.display_name_lower = LOWER(ac.display_name)
    WHERE ac.feed_guid IS NULL
      AND ac.id <> canonical.keep_id
)
UPDATE feeds
SET artist_credit_id = (
    SELECT keep_id
    FROM duplicates
    WHERE drop_id = feeds.artist_credit_id
)
WHERE artist_credit_id IN (SELECT drop_id FROM duplicates);

WITH canonical AS (
    SELECT LOWER(display_name) AS display_name_lower, MIN(id) AS keep_id
    FROM artist_credit
    WHERE feed_guid IS NULL
    GROUP BY LOWER(display_name)
),
duplicates AS (
    SELECT ac.id AS drop_id, canonical.keep_id
    FROM artist_credit ac
    JOIN canonical
      ON canonical.display_name_lower = LOWER(ac.display_name)
    WHERE ac.feed_guid IS NULL
      AND ac.id <> canonical.keep_id
)
UPDATE tracks
SET artist_credit_id = (
    SELECT keep_id
    FROM duplicates
    WHERE drop_id = tracks.artist_credit_id
)
WHERE artist_credit_id IN (SELECT drop_id FROM duplicates);

WITH canonical AS (
    SELECT LOWER(display_name) AS display_name_lower, MIN(id) AS keep_id
    FROM artist_credit
    WHERE feed_guid IS NULL
    GROUP BY LOWER(display_name)
),
duplicates AS (
    SELECT ac.id AS drop_id, canonical.keep_id
    FROM artist_credit ac
    JOIN canonical
      ON canonical.display_name_lower = LOWER(ac.display_name)
    WHERE ac.feed_guid IS NULL
      AND ac.id <> canonical.keep_id
)
UPDATE releases
SET artist_credit_id = (
    SELECT keep_id
    FROM duplicates
    WHERE drop_id = releases.artist_credit_id
)
WHERE artist_credit_id IN (SELECT drop_id FROM duplicates);

WITH canonical AS (
    SELECT LOWER(display_name) AS display_name_lower, MIN(id) AS keep_id
    FROM artist_credit
    WHERE feed_guid IS NULL
    GROUP BY LOWER(display_name)
),
duplicates AS (
    SELECT ac.id AS drop_id, canonical.keep_id
    FROM artist_credit ac
    JOIN canonical
      ON canonical.display_name_lower = LOWER(ac.display_name)
    WHERE ac.feed_guid IS NULL
      AND ac.id <> canonical.keep_id
)
UPDATE recordings
SET artist_credit_id = (
    SELECT keep_id
    FROM duplicates
    WHERE drop_id = recordings.artist_credit_id
)
WHERE artist_credit_id IN (SELECT drop_id FROM duplicates);

WITH canonical AS (
    SELECT LOWER(display_name) AS display_name_lower, MIN(id) AS keep_id
    FROM artist_credit
    WHERE feed_guid IS NULL
    GROUP BY LOWER(display_name)
),
duplicates AS (
    SELECT ac.id AS drop_id, canonical.keep_id
    FROM artist_credit ac
    JOIN canonical
      ON canonical.display_name_lower = LOWER(ac.display_name)
    WHERE ac.feed_guid IS NULL
      AND ac.id <> canonical.keep_id
)
INSERT OR IGNORE INTO artist_credit_name
    (artist_credit_id, artist_id, position, name, join_phrase)
SELECT duplicates.keep_id,
       acn.artist_id,
       acn.position,
       acn.name,
       acn.join_phrase
FROM artist_credit_name acn
JOIN duplicates
  ON duplicates.drop_id = acn.artist_credit_id;

WITH canonical AS (
    SELECT LOWER(display_name) AS display_name_lower, MIN(id) AS keep_id
    FROM artist_credit
    WHERE feed_guid IS NULL
    GROUP BY LOWER(display_name)
),
duplicates AS (
    SELECT ac.id AS drop_id, canonical.keep_id
    FROM artist_credit ac
    JOIN canonical
      ON canonical.display_name_lower = LOWER(ac.display_name)
    WHERE ac.feed_guid IS NULL
      AND ac.id <> canonical.keep_id
)
DELETE FROM artist_credit_name
WHERE artist_credit_id IN (SELECT drop_id FROM duplicates);

WITH canonical AS (
    SELECT LOWER(display_name) AS display_name_lower, MIN(id) AS keep_id
    FROM artist_credit
    WHERE feed_guid IS NULL
    GROUP BY LOWER(display_name)
),
duplicates AS (
    SELECT ac.id AS drop_id, canonical.keep_id
    FROM artist_credit ac
    JOIN canonical
      ON canonical.display_name_lower = LOWER(ac.display_name)
    WHERE ac.feed_guid IS NULL
      AND ac.id <> canonical.keep_id
)
DELETE FROM artist_credit
WHERE id IN (SELECT drop_id FROM duplicates);

CREATE UNIQUE INDEX IF NOT EXISTS idx_artist_credit_display_feed_norm
    ON artist_credit(LOWER(display_name), COALESCE(feed_guid, ''));
