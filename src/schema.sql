PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;

-- Migration: drop legacy unused tables
-- Dead schema removed — 2026-03-13
DROP TABLE IF EXISTS feed_type;
DROP TABLE IF EXISTS artist_location;
DROP TABLE IF EXISTS manifest_source;

-- ---------------------------------------------------------------------------
-- LOOKUP TABLES
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS artist_type (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE IF NOT EXISTS rel_type (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    entity_pair TEXT NOT NULL,
    description TEXT
) STRICT;

-- ---------------------------------------------------------------------------
-- CORE ENTITY TABLES
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS artists (
    artist_id   TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    name_lower  TEXT NOT NULL,
    sort_name   TEXT,
    type_id     INTEGER REFERENCES artist_type(id),
    area        TEXT,
    img_url     TEXT,
    url         TEXT,
    begin_year  INTEGER,
    end_year    INTEGER,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_artists_name_lower ON artists(name_lower);

CREATE TABLE IF NOT EXISTS artist_aliases (
    alias_lower  TEXT NOT NULL,
    artist_id    TEXT NOT NULL REFERENCES artists(artist_id),
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (alias_lower, artist_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_aliases_lower ON artist_aliases(alias_lower);

-- MusicBrainz-style artist credits
CREATE TABLE IF NOT EXISTS artist_credit (
    id           INTEGER PRIMARY KEY,
    display_name TEXT NOT NULL,
    created_at   INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS artist_credit_name (
    id               INTEGER PRIMARY KEY,
    artist_credit_id INTEGER NOT NULL REFERENCES artist_credit(id),
    artist_id        TEXT NOT NULL REFERENCES artists(artist_id),
    position         INTEGER NOT NULL,
    name             TEXT NOT NULL,
    join_phrase      TEXT NOT NULL DEFAULT '',
    UNIQUE(artist_credit_id, position)
) STRICT;

-- Issue-7 missing indexes — 2026-03-13
CREATE INDEX IF NOT EXISTS idx_ac_display_lower ON artist_credit(LOWER(display_name));

CREATE INDEX IF NOT EXISTS idx_acn_credit ON artist_credit_name(artist_credit_id);
CREATE INDEX IF NOT EXISTS idx_acn_artist ON artist_credit_name(artist_id);

CREATE TABLE IF NOT EXISTS feeds (
    feed_guid        TEXT PRIMARY KEY,
    feed_url         TEXT NOT NULL UNIQUE,
    title            TEXT NOT NULL,
    title_lower      TEXT NOT NULL,
    artist_credit_id INTEGER NOT NULL REFERENCES artist_credit(id),
    description      TEXT,
    image_url        TEXT,
    language         TEXT,
    explicit         INTEGER NOT NULL DEFAULT 0,
    itunes_type      TEXT,
    episode_count    INTEGER NOT NULL DEFAULT 0,
    newest_item_at   INTEGER,
    oldest_item_at   INTEGER,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    raw_medium       TEXT
) STRICT;

CREATE INDEX IF NOT EXISTS idx_feeds_credit ON feeds(artist_credit_id);
CREATE INDEX IF NOT EXISTS idx_feeds_newest ON feeds(newest_item_at DESC);
CREATE INDEX IF NOT EXISTS idx_feeds_title  ON feeds(title_lower);
CREATE INDEX IF NOT EXISTS idx_feeds_title_guid ON feeds(title_lower, feed_guid);

CREATE TABLE IF NOT EXISTS tracks (
    track_guid       TEXT PRIMARY KEY,
    feed_guid        TEXT NOT NULL REFERENCES feeds(feed_guid),
    artist_credit_id INTEGER NOT NULL REFERENCES artist_credit(id),
    title            TEXT NOT NULL,
    title_lower      TEXT NOT NULL,
    pub_date         INTEGER,
    duration_secs    INTEGER,
    enclosure_url    TEXT,
    enclosure_type   TEXT,
    enclosure_bytes  INTEGER,
    track_number     INTEGER,
    season           INTEGER,
    explicit         INTEGER NOT NULL DEFAULT 0,
    description      TEXT,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_tracks_feed     ON tracks(feed_guid);
CREATE INDEX IF NOT EXISTS idx_tracks_credit   ON tracks(artist_credit_id);
CREATE INDEX IF NOT EXISTS idx_tracks_pub_date ON tracks(pub_date DESC);
CREATE INDEX IF NOT EXISTS idx_tracks_title    ON tracks(title_lower);

-- Track-level payment routes
-- NOTE (SG-04/SG-05): CHECK constraints apply to new inserts only on existing
-- databases (SQLite does not retroactively validate existing rows when using
-- CREATE TABLE IF NOT EXISTS). For a full migration on an existing database,
-- recreate the table via: CREATE new -> INSERT INTO new SELECT * FROM old ->
-- DROP old -> ALTER TABLE new RENAME TO old.
CREATE TABLE IF NOT EXISTS payment_routes (
    id              INTEGER PRIMARY KEY,
    track_guid      TEXT NOT NULL REFERENCES tracks(track_guid),
    feed_guid       TEXT NOT NULL,
    recipient_name  TEXT,
    route_type      TEXT NOT NULL CHECK(route_type IN ('node','wallet','keysend','lnaddress')),
    address         TEXT NOT NULL,
    custom_key      TEXT,
    custom_value    TEXT,
    split           INTEGER NOT NULL CHECK(split >= 0),
    fee             INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX IF NOT EXISTS idx_routes_track ON payment_routes(track_guid);
CREATE INDEX IF NOT EXISTS idx_routes_feed  ON payment_routes(feed_guid);

-- Feed-level payment routes (same CHECK migration note as payment_routes above)
CREATE TABLE IF NOT EXISTS feed_payment_routes (
    id              INTEGER PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    recipient_name  TEXT,
    route_type      TEXT NOT NULL CHECK(route_type IN ('node','wallet','keysend','lnaddress')),
    address         TEXT NOT NULL,
    custom_key      TEXT,
    custom_value    TEXT,
    split           INTEGER NOT NULL CHECK(split >= 0),
    fee             INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX IF NOT EXISTS idx_feed_routes_guid ON feed_payment_routes(feed_guid);

CREATE TABLE IF NOT EXISTS value_time_splits (
    id                  INTEGER PRIMARY KEY,
    source_track_guid   TEXT NOT NULL REFERENCES tracks(track_guid),
    start_time_secs     INTEGER NOT NULL,
    duration_secs       INTEGER,
    remote_feed_guid    TEXT NOT NULL,
    remote_item_guid    TEXT NOT NULL,
    split               INTEGER NOT NULL CHECK(split >= 0),
    created_at          INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_vts_source ON value_time_splits(source_track_guid);

CREATE TABLE IF NOT EXISTS feed_remote_items_raw (
    id               INTEGER PRIMARY KEY,
    feed_guid        TEXT NOT NULL REFERENCES feeds(feed_guid),
    position         INTEGER NOT NULL,
    medium           TEXT,
    remote_feed_guid TEXT NOT NULL,
    remote_feed_url  TEXT,
    source           TEXT NOT NULL DEFAULT 'podcast_remote_item',
    UNIQUE(feed_guid, position)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_feed_remote_items_feed ON feed_remote_items_raw(feed_guid);
CREATE INDEX IF NOT EXISTS idx_feed_remote_items_guid ON feed_remote_items_raw(remote_feed_guid);

CREATE TABLE IF NOT EXISTS live_events (
    live_item_guid  TEXT PRIMARY KEY,
    feed_guid       TEXT NOT NULL REFERENCES feeds(feed_guid),
    title           TEXT NOT NULL,
    content_link    TEXT,
    status          TEXT NOT NULL CHECK(status IN ('pending', 'live', 'ended')),
    scheduled_start INTEGER,
    scheduled_end   INTEGER,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_live_events_feed   ON live_events(feed_guid);
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
CREATE INDEX IF NOT EXISTS idx_events_created ON events(created_at DESC);

CREATE TABLE IF NOT EXISTS feed_crawl_cache (
    feed_url     TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    crawled_at   INTEGER NOT NULL
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

-- Artist-to-artist relationships (member_of, collaboration, etc.)
CREATE TABLE IF NOT EXISTS artist_artist_rel (
    id          INTEGER PRIMARY KEY,
    artist_id_a TEXT NOT NULL REFERENCES artists(artist_id),
    artist_id_b TEXT NOT NULL REFERENCES artists(artist_id),
    rel_type_id INTEGER NOT NULL REFERENCES rel_type(id),
    begin_year  INTEGER,
    end_year    INTEGER,
    created_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_aar_a   ON artist_artist_rel(artist_id_a);
CREATE INDEX IF NOT EXISTS idx_aar_b   ON artist_artist_rel(artist_id_b);
CREATE INDEX IF NOT EXISTS idx_aar_rel ON artist_artist_rel(rel_type_id);

-- Artist ID redirect (when artists are merged, old ID -> new ID)
CREATE TABLE IF NOT EXISTS artist_id_redirect (
    old_artist_id TEXT PRIMARY KEY,
    new_artist_id TEXT NOT NULL REFERENCES artists(artist_id),
    merged_at     INTEGER NOT NULL
) STRICT;

-- Track relationships (featuring, remix_of, cover_of)
CREATE TABLE IF NOT EXISTS track_rel (
    id           INTEGER PRIMARY KEY,
    track_guid_a TEXT NOT NULL REFERENCES tracks(track_guid),
    track_guid_b TEXT NOT NULL REFERENCES tracks(track_guid),
    rel_type_id  INTEGER NOT NULL REFERENCES rel_type(id),
    created_at   INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_trel_a   ON track_rel(track_guid_a);
CREATE INDEX IF NOT EXISTS idx_trel_b   ON track_rel(track_guid_b);
CREATE INDEX IF NOT EXISTS idx_trel_rel ON track_rel(rel_type_id);

-- Feed relationships
CREATE TABLE IF NOT EXISTS feed_rel (
    id           INTEGER PRIMARY KEY,
    feed_guid_a  TEXT NOT NULL REFERENCES feeds(feed_guid),
    feed_guid_b  TEXT NOT NULL REFERENCES feeds(feed_guid),
    rel_type_id  INTEGER NOT NULL REFERENCES rel_type(id),
    created_at   INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_frel_a   ON feed_rel(feed_guid_a);
CREATE INDEX IF NOT EXISTS idx_frel_b   ON feed_rel(feed_guid_b);
CREATE INDEX IF NOT EXISTS idx_frel_rel ON feed_rel(rel_type_id);

-- ---------------------------------------------------------------------------
-- TAGS
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS tags (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS artist_tag (
    artist_id  TEXT NOT NULL REFERENCES artists(artist_id),
    tag_id     INTEGER NOT NULL REFERENCES tags(id),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (artist_id, tag_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_artist_tag_tag ON artist_tag(tag_id);

CREATE TABLE IF NOT EXISTS feed_tag (
    feed_guid  TEXT NOT NULL REFERENCES feeds(feed_guid),
    tag_id     INTEGER NOT NULL REFERENCES tags(id),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (feed_guid, tag_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_feed_tag_tag ON feed_tag(tag_id);

CREATE TABLE IF NOT EXISTS track_tag (
    track_guid TEXT NOT NULL REFERENCES tracks(track_guid),
    tag_id     INTEGER NOT NULL REFERENCES tags(id),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (track_guid, tag_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_track_tag_tag ON track_tag(tag_id);

-- ---------------------------------------------------------------------------
-- EXTERNAL IDS & PROVENANCE
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS external_ids (
    id          INTEGER PRIMARY KEY,
    entity_type TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    scheme      TEXT NOT NULL,
    value       TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    UNIQUE(entity_type, entity_id, scheme)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_extid_entity ON external_ids(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_extid_scheme ON external_ids(scheme, value);

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

CREATE TABLE IF NOT EXISTS entity_field_status (
    entity_type TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    field_name  TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'present',
    PRIMARY KEY (entity_type, entity_id, field_name)
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

-- ---------------------------------------------------------------------------
-- SEED DATA
-- ---------------------------------------------------------------------------

-- Seed artist_type
INSERT OR IGNORE INTO artist_type (id, name) VALUES (1, 'person');
INSERT OR IGNORE INTO artist_type (id, name) VALUES (2, 'group');
INSERT OR IGNORE INTO artist_type (id, name) VALUES (3, 'orchestra');
INSERT OR IGNORE INTO artist_type (id, name) VALUES (4, 'choir');
INSERT OR IGNORE INTO artist_type (id, name) VALUES (5, 'character');
INSERT OR IGNORE INTO artist_type (id, name) VALUES (6, 'other');

-- Seed rel_type
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (1, 'performer', 'artist-track', 'Primary performing artist');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (2, 'songwriter', 'artist-track', 'Writer of the song');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (3, 'producer', 'artist-track', 'Music producer');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (4, 'engineer', 'artist-track', 'Sound/recording engineer');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (5, 'mixer', 'artist-track', 'Mixing engineer');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (6, 'mastering', 'artist-track', 'Mastering engineer');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (7, 'composer', 'artist-track', 'Composer of the music');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (8, 'lyricist', 'artist-track', 'Writer of the lyrics');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (9, 'arranger', 'artist-track', 'Musical arranger');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (10, 'conductor', 'artist-track', 'Orchestra/ensemble conductor');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (11, 'dj', 'artist-track', 'DJ / selector');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (12, 'remixer', 'artist-track', 'Created a remix');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (13, 'featuring', 'artist-track', 'Featured artist');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (14, 'vocal', 'artist-track', 'Vocal performance');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (15, 'instrument', 'artist-track', 'Instrument performance');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (16, 'programming', 'artist-track', 'Electronic programming');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (17, 'recording', 'artist-track', 'Recording engineer');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (18, 'mixing', 'artist-track', 'Mixing');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (19, 'live_sound', 'artist-track', 'Live sound engineer');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (20, 'member_of', 'artist-artist', 'Member of a group');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (21, 'collaboration', 'artist-artist', 'Collaborative project');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (22, 'cover_art', 'artist-feed', 'Cover art creator');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (23, 'photographer', 'artist-feed', 'Photographer');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (24, 'liner_notes', 'artist-feed', 'Liner notes author');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (25, 'translator', 'artist-feed', 'Translator');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (26, 'promoter', 'artist-feed', 'Promoter');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (27, 'booking', 'artist-artist', 'Booking agent');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (28, 'management', 'artist-artist', 'Artist management');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (29, 'label', 'artist-feed', 'Record label');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (30, 'publisher', 'artist-feed', 'Music publisher');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (31, 'distributor', 'artist-feed', 'Distributor');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (32, 'legal', 'artist-feed', 'Legal representation');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (33, 'marketing', 'artist-feed', 'Marketing');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (34, 'a_and_r', 'artist-feed', 'A&R representative');
INSERT OR IGNORE INTO rel_type (id, name, entity_pair, description) VALUES (35, 'other', 'artist-track', 'Other role');

-- ---------------------------------------------------------------------------
-- PROOF-OF-POSSESSION (Sprint 3)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS proof_challenges (
    challenge_id     TEXT PRIMARY KEY,
    feed_guid        TEXT NOT NULL,
    scope            TEXT NOT NULL,
    token_binding    TEXT NOT NULL,
    state            TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','valid','invalid')),
    expires_at       INTEGER NOT NULL,
    created_at       INTEGER NOT NULL
) STRICT;
-- Issue-7 missing indexes — 2026-03-13
DROP INDEX IF EXISTS idx_proof_challenges_feed;
CREATE INDEX IF NOT EXISTS idx_proof_challenges_feed_state ON proof_challenges(feed_guid, state);
CREATE INDEX IF NOT EXISTS idx_proof_challenges_expires ON proof_challenges(expires_at);

CREATE TABLE IF NOT EXISTS proof_tokens (
    access_token      TEXT PRIMARY KEY,
    scope             TEXT NOT NULL,
    subject_feed_guid TEXT NOT NULL,
    expires_at        INTEGER NOT NULL,
    created_at        INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS idx_proof_tokens_expires ON proof_tokens(expires_at);
