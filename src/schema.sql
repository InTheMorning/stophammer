PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;

-- Migration: drop legacy unused tables
-- Dead schema removed — 2026-03-13
DROP TABLE IF EXISTS feed_type;
DROP TABLE IF EXISTS artist_location;
DROP TABLE IF EXISTS manifest_source;

-- ---------------------------------------------------------------------------
-- CORE ENTITY TABLES
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS feeds (
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

CREATE INDEX IF NOT EXISTS idx_feeds_newest ON feeds(newest_item_at DESC);
CREATE INDEX IF NOT EXISTS idx_feeds_title  ON feeds(title_lower);
CREATE INDEX IF NOT EXISTS idx_feeds_title_guid ON feeds(title_lower, feed_guid);

CREATE TABLE IF NOT EXISTS tracks (
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

CREATE INDEX IF NOT EXISTS idx_tracks_feed     ON tracks(feed_guid);
CREATE INDEX IF NOT EXISTS idx_tracks_pub_date ON tracks(pub_date DESC);
CREATE INDEX IF NOT EXISTS idx_tracks_title    ON tracks(title_lower);
CREATE INDEX IF NOT EXISTS idx_tracks_guid     ON tracks(track_guid);

-- Track-level payment routes
-- NOTE (SG-04/SG-05): CHECK constraints apply to new inserts only on existing
-- databases (SQLite does not retroactively validate existing rows when using
-- CREATE TABLE IF NOT EXISTS). For a full migration on an existing database,
-- recreate the table via: CREATE new -> INSERT INTO new SELECT * FROM old ->
-- DROP old -> ALTER TABLE new RENAME TO old.
CREATE TABLE IF NOT EXISTS payment_routes (
    id              INTEGER PRIMARY KEY,
    track_guid      TEXT NOT NULL,
    feed_guid       TEXT NOT NULL,
    recipient_name  TEXT,
    route_type      TEXT NOT NULL CHECK(route_type IN ('node','wallet','keysend','lnaddress')),
    address         TEXT NOT NULL,
    custom_key      TEXT NOT NULL DEFAULT '',
    custom_value    TEXT NOT NULL DEFAULT '',
    split           INTEGER NOT NULL CHECK(split >= 0),
    fee             INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (feed_guid, track_guid) REFERENCES tracks(feed_guid, track_guid)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_routes_track ON payment_routes(track_guid);
CREATE INDEX IF NOT EXISTS idx_routes_feed  ON payment_routes(feed_guid);
CREATE INDEX IF NOT EXISTS idx_routes_feed_track ON payment_routes(feed_guid, track_guid);

-- Feed-level payment routes (same CHECK migration note as payment_routes above)
CREATE TABLE IF NOT EXISTS feed_payment_routes (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    recipient_name  TEXT,
    route_type      TEXT NOT NULL CHECK(route_type IN ('node','wallet','keysend','lnaddress')),
    address         TEXT NOT NULL,
    custom_key      TEXT NOT NULL DEFAULT '',
    custom_value    TEXT NOT NULL DEFAULT '',
    split           INTEGER NOT NULL CHECK(split >= 0),
    fee             INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX IF NOT EXISTS idx_feed_routes_guid ON feed_payment_routes(feed_guid);

CREATE TABLE IF NOT EXISTS feed_list_value_raw (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    recipient_name  TEXT,
    route_type      TEXT NOT NULL CHECK(route_type IN ('node','wallet','keysend','lnaddress')),
    address         TEXT NOT NULL,
    custom_key      TEXT NOT NULL DEFAULT '',
    custom_value    TEXT NOT NULL DEFAULT '',
    split           INTEGER NOT NULL CHECK(split >= 0),
    fee             INTEGER NOT NULL DEFAULT 0,
    position        INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_feed_list_value_guid ON feed_list_value_raw(feed_guid);

CREATE TABLE IF NOT EXISTS value_time_splits (
    id                  INTEGER PRIMARY KEY,
    source_feed_guid    TEXT,
    source_track_guid   TEXT NOT NULL,
    start_time_secs     INTEGER NOT NULL,
    duration_secs       INTEGER,
    remote_feed_guid    TEXT NOT NULL,
    remote_item_guid    TEXT NOT NULL,
    split               INTEGER NOT NULL CHECK(split >= 0),
    created_at          INTEGER NOT NULL,
    FOREIGN KEY (source_feed_guid, source_track_guid) REFERENCES tracks(feed_guid, track_guid)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_vts_source ON value_time_splits(source_feed_guid, source_track_guid);
CREATE INDEX IF NOT EXISTS idx_vts_track_guid_only ON value_time_splits(source_track_guid);

CREATE TABLE IF NOT EXISTS feed_remote_items_raw (
    id               INTEGER PRIMARY KEY,
    feed_guid        TEXT NOT NULL REFERENCES feeds(feed_guid),
    position         INTEGER NOT NULL,
    medium           TEXT,
    remote_feed_guid TEXT NOT NULL,
    remote_feed_url  TEXT,
    rel              TEXT,
    source           TEXT NOT NULL DEFAULT 'podcast_remote_item',
    remote_item_guid TEXT,
    remote_item_title TEXT,
    UNIQUE(feed_guid, position)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_feed_remote_items_feed ON feed_remote_items_raw(feed_guid);
CREATE INDEX IF NOT EXISTS idx_feed_remote_items_guid ON feed_remote_items_raw(remote_feed_guid);

CREATE TABLE IF NOT EXISTS track_remote_items_raw (
    id               INTEGER PRIMARY KEY,
    feed_guid        TEXT,
    track_guid       TEXT NOT NULL,
    position         INTEGER NOT NULL,
    medium           TEXT,
    remote_feed_guid TEXT NOT NULL,
    remote_feed_url  TEXT,
    rel              TEXT,
    source           TEXT NOT NULL DEFAULT 'podcast_remote_item',
    UNIQUE(feed_guid, track_guid, position),
    FOREIGN KEY (feed_guid, track_guid) REFERENCES tracks(feed_guid, track_guid)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_track_remote_items_track ON track_remote_items_raw(feed_guid, track_guid);
CREATE INDEX IF NOT EXISTS idx_track_remote_items_guid  ON track_remote_items_raw(remote_feed_guid);
CREATE INDEX IF NOT EXISTS idx_track_remote_items_track_guid_only ON track_remote_items_raw(track_guid);

CREATE TABLE IF NOT EXISTS live_events (
    live_item_guid  TEXT NOT NULL,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    title           TEXT NOT NULL,
    content_link    TEXT,
    status          TEXT NOT NULL CHECK(status IN ('pending', 'live', 'ended')),
    scheduled_start INTEGER,
    scheduled_end   INTEGER,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    live_value_uri      TEXT,
    live_value_protocol TEXT,
    PRIMARY KEY (feed_guid, live_item_guid)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_live_events_feed   ON live_events(feed_guid);
CREATE INDEX IF NOT EXISTS idx_live_events_guid   ON live_events(live_item_guid);
CREATE INDEX IF NOT EXISTS idx_live_events_status ON live_events(status);

CREATE TABLE IF NOT EXISTS source_contributor_claims (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('feed', 'track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL,
    name            TEXT NOT NULL,
    role            TEXT,
    role_norm       TEXT,
    group_name      TEXT,
    href            TEXT,
    img             TEXT,
    npub            TEXT,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, position, source)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_contrib_feed   ON source_contributor_claims(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_contrib_entity ON source_contributor_claims(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_source_contrib_role_norm ON source_contributor_claims(role_norm);

CREATE TABLE IF NOT EXISTS source_entity_ids (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('feed', 'track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL DEFAULT 0,
    scheme          TEXT NOT NULL,
    value           TEXT NOT NULL,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, scheme, value)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_ids_feed         ON source_entity_ids(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_ids_entity       ON source_entity_ids(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_source_ids_scheme_value ON source_entity_ids(scheme, value);

CREATE TABLE IF NOT EXISTS source_entity_links (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('feed', 'track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL,
    link_type       TEXT NOT NULL,
    url             TEXT NOT NULL,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, link_type, url)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_links_feed   ON source_entity_links(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_links_entity ON source_entity_links(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_source_links_type   ON source_entity_links(link_type);

CREATE TABLE IF NOT EXISTS source_release_claims (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('feed', 'track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL DEFAULT 0,
    claim_type      TEXT NOT NULL,
    claim_value     TEXT NOT NULL,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, claim_type, position)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_release_claims_feed   ON source_release_claims(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_release_claims_entity ON source_release_claims(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_source_release_claims_type   ON source_release_claims(claim_type);

CREATE TABLE IF NOT EXISTS source_item_enclosures (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL,
    url             TEXT NOT NULL,
    mime_type       TEXT,
    bytes           INTEGER,
    rel             TEXT,
    title           TEXT,
    is_primary      INTEGER NOT NULL DEFAULT 0,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, position, url)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_item_enclosures_feed   ON source_item_enclosures(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_item_enclosures_entity ON source_item_enclosures(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_source_item_enclosures_url    ON source_item_enclosures(url);

CREATE TABLE IF NOT EXISTS source_item_transcripts (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    entity_type     TEXT NOT NULL CHECK(entity_type IN ('track', 'live_item')),
    entity_id       TEXT NOT NULL,
    position        INTEGER NOT NULL,
    url             TEXT NOT NULL,
    mime_type       TEXT,
    language        TEXT,
    rel             TEXT,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL,
    UNIQUE(feed_guid, entity_type, entity_id, position, url)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_item_transcripts_feed
    ON source_item_transcripts(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_item_transcripts_entity
    ON source_item_transcripts(entity_type, entity_id);

CREATE TABLE IF NOT EXISTS source_platform_claims (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    platform_key    TEXT NOT NULL,
    url             TEXT,
    owner_name      TEXT,
    source          TEXT NOT NULL,
    extraction_path TEXT NOT NULL,
    observed_at     INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_source_platform_claims_feed     ON source_platform_claims(feed_guid);
CREATE INDEX IF NOT EXISTS idx_source_platform_claims_platform ON source_platform_claims(platform_key);

-- ---------------------------------------------------------------------------
-- EVENTS & SYNC
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS events (
    event_id        TEXT PRIMARY KEY,
    event_type      TEXT NOT NULL,
    payload_json    TEXT NOT NULL,
    subject_guid    TEXT NOT NULL,
    signed_by       TEXT NOT NULL,
    signature       TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    created_at      INTEGER NOT NULL,
    warnings_json   TEXT NOT NULL DEFAULT '[]'
) STRICT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_events_seq_unique ON events(seq);
CREATE INDEX IF NOT EXISTS idx_events_subject ON events(subject_guid);
CREATE INDEX IF NOT EXISTS idx_events_type    ON events(event_type);
-- The route history read of a feed (ADR 0053 §4, migration 0048).
CREATE INDEX IF NOT EXISTS idx_events_routes_feed
    ON events(json_extract(payload_json, '$.feed_guid'))
    WHERE event_type = 'routes_replaced';
CREATE INDEX IF NOT EXISTS idx_events_track_feed
    ON events(json_extract(payload_json, '$.track.feed_guid'))
    WHERE event_type = 'track_upserted';
CREATE INDEX IF NOT EXISTS idx_events_created ON events(created_at DESC);

CREATE TABLE IF NOT EXISTS feed_crawl_cache (
    feed_url     TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    crawled_at   INTEGER NOT NULL
) STRICT;

-- Which URL gave which podcast:guid (ADR 0049 Section 1). A fresh database
-- has no feeds yet, so this table starts empty here; migration 0036 seeds it
-- on an existing database from the stored feeds.feed_url.
CREATE TABLE IF NOT EXISTS feed_url_observations (
    url         TEXT PRIMARY KEY,
    feed_guid   TEXT NOT NULL,
    observed_at INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS idx_feed_url_observations_guid
    ON feed_url_observations(feed_guid);

-- The count of a gone answer from the stored source URL of a record (ADR
-- 0067 section 2). Local to the primary ingest path and makes no event, so a
-- community node holds no row. `ON DELETE CASCADE` deletes the row with its
-- feed, so the feed delete trigger does not change.
CREATE TABLE IF NOT EXISTS source_gone_answers (
    feed_guid     TEXT PRIMARY KEY REFERENCES feeds(feed_guid) ON DELETE CASCADE,
    source_url    TEXT NOT NULL,
    first_gone_at INTEGER NOT NULL,
    last_gone_at  INTEGER NOT NULL,
    last_status   INTEGER NOT NULL
) STRICT;

-- A block on one feed GUID or one exact feed URL (ADR 0053 Section 1). A
-- signed FeedBlocked event writes a row here; a signed FeedUnblocked event
-- removes one.
CREATE TABLE IF NOT EXISTS feed_blocks (
    block_id   TEXT PRIMARY KEY,
    kind       TEXT NOT NULL CHECK (kind IN ('guid','url')),
    value      TEXT NOT NULL,
    reason     TEXT NOT NULL,
    blocked_at INTEGER NOT NULL,
    UNIQUE (kind, value)
) STRICT;

-- A summary row for each pair of GUID and URL that a mirror body names
-- (ADR 0058 §1). last_seen is local to the primary ingest path and makes no
-- event, so a community node holds it as null.
CREATE TABLE IF NOT EXISTS feed_copies (
    feed_guid         TEXT NOT NULL,
    url               TEXT NOT NULL,
    first_seen        INTEGER NOT NULL,
    last_seen         INTEGER,
    title             TEXT NOT NULL,
    item_guids        TEXT NOT NULL,
    feed_recipients   TEXT NOT NULL,
    track_recipients  TEXT NOT NULL,
    summary_digest    TEXT NOT NULL,
    resolution        TEXT CHECK (resolution IN ('keep_source','relocate')),
    resolution_reason TEXT,
    resolved_at       INTEGER,
    resolved_digest   TEXT,
    PRIMARY KEY (feed_guid, url)
) STRICT;

-- The count of mirror bodies for a GUID that arrived at a new URL after the
-- GUID already held MAX_COPIES_PER_GUID rows (ADR 0058 §1a). Local to the
-- primary ingest path. A community node never writes it.
CREATE TABLE IF NOT EXISTS feed_copy_overflow (
    feed_guid TEXT PRIMARY KEY,
    count     INTEGER NOT NULL DEFAULT 0
) STRICT;

-- A pending GUID change at a source URL (ADR 0052 section 4). last_seen is
-- local to the primary ingest path and makes no event, so a community node
-- holds it as null.
CREATE TABLE IF NOT EXISTS feed_guid_changes (
    source_url      TEXT PRIMARY KEY,
    old_guid        TEXT NOT NULL,
    new_guid        TEXT NOT NULL,
    first_seen      INTEGER NOT NULL,
    last_seen       INTEGER,
    decision        TEXT CHECK (decision IN ('approve','reject')),
    decision_reason TEXT,
    decided_at      INTEGER
) STRICT;

-- The link from a GUID a transition retired to the GUID that replaced it
-- (ADR 0052 section 5). The link is for navigation only.
CREATE TABLE IF NOT EXISTS feed_guid_supersessions (
    old_guid      TEXT PRIMARY KEY,
    new_guid      TEXT NOT NULL,
    source_url    TEXT NOT NULL,
    superseded_at INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS node_sync_state (
    node_pubkey  TEXT PRIMARY KEY,
    last_seq     INTEGER NOT NULL DEFAULT 0,
    last_seen_at INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS peer_nodes (
    node_pubkey          TEXT NOT NULL PRIMARY KEY,
    node_url             TEXT NOT NULL,
    discovered_at        INTEGER NOT NULL,
    last_push_at         INTEGER,
    consecutive_failures INTEGER NOT NULL DEFAULT 0
) STRICT;

-- ---------------------------------------------------------------------------
-- METADATA TABLES
-- ---------------------------------------------------------------------------

-- ---------------------------------------------------------------------------
-- PROVENANCE
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS entity_source (
    id          INTEGER PRIMARY KEY,
    entity_type TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    source_type TEXT NOT NULL,
    source_url  TEXT,
    trust_level INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_esrc_entity ON entity_source(entity_type, entity_id);

-- ---------------------------------------------------------------------------
-- QUALITY
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS entity_quality (
    entity_type TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    score       INTEGER NOT NULL DEFAULT 0,
    computed_at INTEGER NOT NULL,
    PRIMARY KEY (entity_type, entity_id)
) STRICT;

-- ---------------------------------------------------------------------------
-- FTS5 SEARCH (virtual table, no STRICT mode)
-- ---------------------------------------------------------------------------

CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
    entity_type,
    entity_id,
    name,
    title,
    description,
    tags,
    content='',
    tokenize='unicode61'
);

-- Issue-FTS5-CONTENT — 2026-03-14
-- Companion table for the contentless FTS5 index.  Because search_index uses
-- content='', column values cannot be read back via SELECT.  This table maps
-- each FTS5 rowid to the (entity_type, entity_id) pair so that search results
-- can be resolved through a JOIN on rowid.
CREATE TABLE IF NOT EXISTS search_entities (
    rowid       INTEGER PRIMARY KEY,
    entity_type TEXT NOT NULL,
    entity_id   TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_search_entities_entity
    ON search_entities(entity_type, entity_id);
