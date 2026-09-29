-- stophammer: foreign_keys=off
--
-- ADR 0034 section 11: release B stops the compatibility artist credit. This
-- migration rebuilds `feeds` and `tracks` with no `artist_credit_id` column,
-- and drops every table section 11 names: the artist credit, the
-- relationships and tags, the resolver and the review, the wallets, and the
-- leftover copies of migration 0032. Each one is empty, holds fixed seed
-- values, or is a legacy copy on the production database of 2026-09-26; no
-- code reads or writes any of them.
--
-- The operator widened this migration from the seven artist-credit tables to
-- the full list on 2026-09-28, after a first build of this task found that
-- `feed_rel`, `track_rel`, and five more tables hold a foreign key to
-- `rel_type` or `artists` — a table outside the original seven-table list,
-- naming a table inside it. Dropping only the seven would leave those six
-- tables with a dangling reference, and the two delete triggers below touch
-- two of them, so an ordinary feed or track delete would fail once foreign
-- keys are back on.
--
-- SQLite cannot drop a column that a foreign key references, so the rebuild
-- of `feeds` and `tracks` makes a new table, copies the surviving columns,
-- drops the old table, and renames the new table into place (the phase plan,
-- plan decision 3). Neither old table is renamed first: a rename also changes
-- each foreign key in another table that names it, and many other tables keep
-- referring to `feeds` and `tracks` by name across this migration.
--
-- The rebuild needs foreign keys off. With them on, `DROP TABLE feeds` fails
-- because `tracks` still refers to it. `PRAGMA defer_foreign_keys` is not a
-- fix either: the implicit delete of the drop then runs the `ON DELETE
-- CASCADE` of `source_gone_answers` (migration 0045) and empties it. A test
-- on 2026-09-28 showed both failures. The marker line above this comment
-- tells the runner to set `PRAGMA foreign_keys = OFF` before this migration's
-- transaction, and to run `PRAGMA foreign_key_check` before the commit.

DROP TRIGGER IF EXISTS trg_feeds_cleanup_before_delete;
DROP TRIGGER IF EXISTS trg_tracks_cleanup_before_delete;

CREATE TABLE feeds_new (
    feed_guid        TEXT PRIMARY KEY,
    feed_url         TEXT NOT NULL UNIQUE,
    title            TEXT NOT NULL,
    title_lower      TEXT NOT NULL,
    description      TEXT,
    image_url        TEXT,
    publisher        TEXT,
    language         TEXT,
    explicit         INTEGER NOT NULL DEFAULT 0,
    itunes_type      TEXT,
    release_artist   TEXT,
    release_artist_sort TEXT,
    release_date     INTEGER,
    release_kind     TEXT,
    episode_count    INTEGER NOT NULL DEFAULT 0,
    newest_item_at   INTEGER,
    oldest_item_at   INTEGER,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    raw_medium       TEXT,
    last_build_date  INTEGER,
    release_artist_source TEXT,
    declared_self_url TEXT,
    declared_new_feed_url TEXT,
    podcast_locked INTEGER,
    locked_owner TEXT
) STRICT;

INSERT INTO feeds_new (
    feed_guid, feed_url, title, title_lower, description, image_url,
    publisher, language, explicit, itunes_type, release_artist,
    release_artist_sort, release_date, release_kind, episode_count,
    newest_item_at, oldest_item_at, created_at, updated_at, raw_medium,
    last_build_date, release_artist_source, declared_self_url,
    declared_new_feed_url, podcast_locked, locked_owner
)
SELECT
    feed_guid, feed_url, title, title_lower, description, image_url,
    publisher, language, explicit, itunes_type, release_artist,
    release_artist_sort, release_date, release_kind, episode_count,
    newest_item_at, oldest_item_at, created_at, updated_at, raw_medium,
    last_build_date, release_artist_source, declared_self_url,
    declared_new_feed_url, podcast_locked, locked_owner
FROM feeds;

DROP TABLE feeds;

ALTER TABLE feeds_new RENAME TO feeds;

CREATE INDEX IF NOT EXISTS idx_feeds_newest ON feeds(newest_item_at DESC);
CREATE INDEX IF NOT EXISTS idx_feeds_title  ON feeds(title_lower);
CREATE INDEX IF NOT EXISTS idx_feeds_title_guid ON feeds(title_lower, feed_guid);

CREATE TABLE tracks_new (
    track_guid       TEXT NOT NULL,
    feed_guid        TEXT NOT NULL REFERENCES feeds(feed_guid),
    title            TEXT NOT NULL,
    title_lower      TEXT NOT NULL,
    pub_date         INTEGER,
    duration_secs    INTEGER,
    image_url        TEXT,
    publisher        TEXT,
    language         TEXT,
    enclosure_url    TEXT,
    enclosure_type   TEXT,
    enclosure_bytes  INTEGER,
    track_number     INTEGER,
    season           INTEGER,
    explicit         INTEGER NOT NULL DEFAULT 0,
    description      TEXT,
    track_artist     TEXT,
    track_artist_sort TEXT,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    PRIMARY KEY (feed_guid, track_guid)
) STRICT;

INSERT INTO tracks_new (
    track_guid, feed_guid, title, title_lower, pub_date, duration_secs,
    image_url, publisher, language, enclosure_url, enclosure_type,
    enclosure_bytes, track_number, season, explicit, description,
    track_artist, track_artist_sort, created_at, updated_at
)
SELECT
    track_guid, feed_guid, title, title_lower, pub_date, duration_secs,
    image_url, publisher, language, enclosure_url, enclosure_type,
    enclosure_bytes, track_number, season, explicit, description,
    track_artist, track_artist_sort, created_at, updated_at
FROM tracks;

DROP TABLE tracks;

ALTER TABLE tracks_new RENAME TO tracks;

CREATE INDEX IF NOT EXISTS idx_tracks_feed     ON tracks(feed_guid);
CREATE INDEX IF NOT EXISTS idx_tracks_pub_date ON tracks(pub_date DESC);
CREATE INDEX IF NOT EXISTS idx_tracks_title    ON tracks(title_lower);
CREATE INDEX IF NOT EXISTS idx_tracks_guid     ON tracks(track_guid);

CREATE TRIGGER trg_feeds_cleanup_before_delete
BEFORE DELETE ON feeds
FOR EACH ROW
BEGIN
    DELETE FROM feed_payment_routes
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM feed_list_value_raw
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM entity_quality
    WHERE entity_type = 'feed'
      AND entity_id = OLD.feed_guid;

    DELETE FROM feed_remote_items_raw
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM live_events
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_contributor_claims
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_entity_ids
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_entity_links
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_release_claims
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_item_enclosures
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_item_transcripts
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM source_platform_claims
    WHERE feed_guid = OLD.feed_guid;

    DELETE FROM tracks
    WHERE feed_guid = OLD.feed_guid;
END;

CREATE TRIGGER trg_tracks_cleanup_before_delete
BEFORE DELETE ON tracks
FOR EACH ROW
BEGIN
    DELETE FROM value_time_splits
    WHERE source_track_guid = OLD.track_guid
      AND (source_feed_guid = OLD.feed_guid OR source_feed_guid IS NULL);

    DELETE FROM payment_routes
    WHERE track_guid = OLD.track_guid
      AND feed_guid = OLD.feed_guid;

    DELETE FROM track_remote_items_raw
    WHERE track_guid = OLD.track_guid
      AND (feed_guid = OLD.feed_guid OR feed_guid IS NULL);

    DELETE FROM entity_quality
    WHERE entity_type = 'track'
      AND (entity_id = OLD.track_guid OR entity_id = json_array(OLD.feed_guid, OLD.track_guid));
END;

-- The artist credit (ADR 0034 §11, first group). Children first.
DROP TABLE IF EXISTS artist_credit_name;
DROP TABLE IF EXISTS artist_aliases;
DROP TABLE IF EXISTS artist_credit;
DROP TABLE IF EXISTS artists;
DROP TABLE IF EXISTS artist_type;
DROP TABLE IF EXISTS rel_type;
DROP TABLE IF EXISTS external_ids;

-- Relationships and tags. `feed_rel` and `track_rel` are derived links with
-- the closed role list of `rel_type`; the `rel` role of a publisher link
-- stays as RSS gives it, in `feed_remote_items_raw.rel` (ADR 0049 §6).
DROP TABLE IF EXISTS feed_rel;
DROP TABLE IF EXISTS track_rel;
DROP TABLE IF EXISTS artist_artist_rel;
DROP TABLE IF EXISTS artist_tag;
DROP TABLE IF EXISTS artist_id_redirect;
DROP TABLE IF EXISTS feed_tag;
DROP TABLE IF EXISTS track_tag;
DROP TABLE IF EXISTS tags;

-- The resolver and the review.
DROP TABLE IF EXISTS resolver_queue;
DROP TABLE IF EXISTS resolver_state;
DROP TABLE IF EXISTS resolved_entity_sources_by_feed;
DROP TABLE IF EXISTS resolved_external_ids_by_feed;
DROP TABLE IF EXISTS artist_identity_override;
DROP TABLE IF EXISTS artist_identity_review;
DROP TABLE IF EXISTS entity_field_status;
DROP TABLE IF EXISTS entity_source;

-- The wallets. `wallet_artist_links` refers to `artists`, dropped above, and
-- to `wallets`, dropped in this group; both references end together.
DROP TABLE IF EXISTS wallet_aliases;
DROP TABLE IF EXISTS wallet_artist_links;
DROP TABLE IF EXISTS wallet_endpoints;
DROP TABLE IF EXISTS wallet_feed_route_map;
DROP TABLE IF EXISTS wallet_id_redirect;
DROP TABLE IF EXISTS wallet_identity_override;
DROP TABLE IF EXISTS wallet_identity_review;
DROP TABLE IF EXISTS wallet_identity_review_legacy_0023;
DROP TABLE IF EXISTS wallet_identity_review_legacy_0024;
DROP TABLE IF EXISTS wallet_merge_apply_entry;
DROP TABLE IF EXISTS wallet_merge_apply_batch;
DROP TABLE IF EXISTS wallet_track_route_map;
DROP TABLE IF EXISTS wallets;

-- Leftover copies: each table migration 0032 renamed instead of dropping,
-- and the live-item table its predecessor left behind.
DROP TABLE IF EXISTS payment_routes_legacy_0032;
DROP TABLE IF EXISTS track_rel_legacy_0032;
DROP TABLE IF EXISTS track_tag_legacy_0032;
DROP TABLE IF EXISTS track_remote_items_raw_legacy_0032;
DROP TABLE IF EXISTS value_time_splits_legacy_0032;
DROP TABLE IF EXISTS tracks_legacy_0032;
DROP TABLE IF EXISTS live_events_legacy;
