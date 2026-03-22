-- ============================================================
-- DERIVED LAYER: owner entities (must precede wallet_endpoints
-- so the FK reference resolves)
-- ============================================================

CREATE TABLE IF NOT EXISTS wallets (
    wallet_id          TEXT PRIMARY KEY,
    display_name       TEXT NOT NULL,
    display_name_lower TEXT NOT NULL,
    wallet_class       TEXT NOT NULL DEFAULT 'unknown'
                       CHECK(wallet_class IN ('person_artist','organization_platform',
                                              'bot_service','unknown')),
    class_confidence   TEXT NOT NULL DEFAULT 'provisional'
                       CHECK(class_confidence IN ('provisional','high_confidence',
                                                  'reviewed','blocked')),
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
) STRICT;

-- ============================================================
-- FACT LAYER: endpoint identity, aliases, and route mappings
-- ============================================================

CREATE TABLE IF NOT EXISTS wallet_endpoints (
    id                 INTEGER PRIMARY KEY,
    route_type         TEXT NOT NULL
                       CHECK(route_type IN ('node','wallet','keysend','lnaddress')),
    normalized_address TEXT NOT NULL,
    custom_key         TEXT NOT NULL DEFAULT '',
    custom_value       TEXT NOT NULL DEFAULT '',
    wallet_id          TEXT REFERENCES wallets(wallet_id),
    created_at         INTEGER NOT NULL,
    UNIQUE(route_type, normalized_address, custom_key, custom_value)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_wallet_endpoints_wallet
    ON wallet_endpoints(wallet_id);

CREATE TABLE IF NOT EXISTS wallet_aliases (
    id              INTEGER PRIMARY KEY,
    endpoint_id     INTEGER NOT NULL REFERENCES wallet_endpoints(id),
    alias           TEXT NOT NULL,
    alias_lower     TEXT NOT NULL,
    first_seen_at   INTEGER NOT NULL,
    last_seen_at    INTEGER NOT NULL,
    UNIQUE(endpoint_id, alias_lower)
) STRICT;

CREATE TABLE IF NOT EXISTS wallet_track_route_map (
    route_id    INTEGER PRIMARY KEY REFERENCES payment_routes(id),
    endpoint_id INTEGER NOT NULL REFERENCES wallet_endpoints(id),
    created_at  INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS wallet_feed_route_map (
    route_id    INTEGER PRIMARY KEY REFERENCES feed_payment_routes(id),
    endpoint_id INTEGER NOT NULL REFERENCES wallet_endpoints(id),
    created_at  INTEGER NOT NULL
) STRICT;

-- ============================================================
-- DERIVED LAYER: merge redirects, artist links
-- ============================================================

CREATE TABLE IF NOT EXISTS wallet_id_redirect (
    old_wallet_id   TEXT PRIMARY KEY,
    new_wallet_id   TEXT NOT NULL,
    created_at      INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS wallet_artist_links (
    id                   INTEGER PRIMARY KEY,
    wallet_id            TEXT NOT NULL REFERENCES wallets(wallet_id),
    artist_id            TEXT NOT NULL REFERENCES artists(artist_id),
    evidence_entity_type TEXT NOT NULL,
    evidence_entity_id   TEXT NOT NULL,
    confidence           TEXT NOT NULL DEFAULT 'provisional'
                         CHECK(confidence IN ('provisional','high_confidence',
                                              'reviewed','blocked')),
    created_at           INTEGER NOT NULL,
    UNIQUE(wallet_id, artist_id)
) STRICT;

-- ============================================================
-- REVIEW AND OVERRIDE LAYER
-- ============================================================

CREATE TABLE IF NOT EXISTS wallet_identity_review (
    id          INTEGER PRIMARY KEY,
    wallet_id   TEXT NOT NULL REFERENCES wallets(wallet_id),
    review_type TEXT NOT NULL,
    details     TEXT,
    status      TEXT NOT NULL DEFAULT 'pending'
                CHECK(status IN ('pending','resolved','blocked')),
    created_at  INTEGER NOT NULL,
    resolved_at INTEGER
) STRICT;

CREATE TABLE IF NOT EXISTS wallet_identity_override (
    id              INTEGER PRIMARY KEY,
    override_type   TEXT NOT NULL
                    CHECK(override_type IN ('merge','do_not_merge','force_class',
                                            'force_artist_link','block_artist_link')),
    wallet_id       TEXT NOT NULL,
    target_id       TEXT,
    value           TEXT,
    created_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_wallet_override_wallet
    ON wallet_identity_override(wallet_id);
