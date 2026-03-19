#![allow(
    clippy::missing_errors_doc,
    reason = "db.rs exposes many thin Result-returning helpers; keeping per-item docs here is low-value noise"
)]
#![allow(
    clippy::too_many_lines,
    reason = "db.rs intentionally centralizes SQL-heavy flows and delete/rebuild routines"
)]

//! Database access layer for stophammer.
//!
//! All SQL operations are collected here: schema initialisation, per-entity
//! upserts, event insertion, crawl-cache management, and the single
//! `ingest_transaction` that writes an entire feed ingest atomically.
//!
//! Errors are surfaced as [`DbError`], which wraps rusqlite and `serde_json`
//! failures. `api.rs` pattern-matches on the variants to produce appropriate
//! HTTP status codes, so the typed error is intentional.

use crate::event::{Event, EventPayload, EventType};
use crate::model::{
    Artist, ArtistCredit, ArtistCreditName, Feed, FeedPaymentRoute, FeedRemoteItemRaw, LiveEvent,
    PaymentRoute, Recording, Release, ReleaseRecording, ResolvedEntitySourceByFeed,
    ResolvedExternalIdByFeed, RouteType, SourceContributorClaim, SourceEntityIdClaim,
    SourceEntityLink, SourceFeedReleaseMap, SourceItemEnclosure, SourceItemRecordingMap,
    SourcePlatformClaim, SourceReleaseClaim, Track, ValueTimeSplit,
};
use crate::signing::NodeSigner;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::Digest;
use std::fmt;
use std::sync::{Arc, Mutex}; // Issue-SEQ-INTEGRITY — 2026-03-14

pub type Db = Arc<Mutex<Connection>>;

// ── Errors ──────────────────────────────────────────────────────────────────

/// Errors returned by all database operations in this module.
// Mutex safety compliant — 2026-03-12
pub enum DbError {
    /// A rusqlite operation failed (query, execute, or schema application).
    Rusqlite(rusqlite::Error),
    /// A JSON serialisation or deserialisation step failed.
    Json(serde_json::Error),
    /// The database mutex was poisoned (a thread panicked while holding the lock).
    Poisoned,
    /// A non-SQLite, non-JSON error (e.g. connection pool failure).
    // Issue-WAL-POOL — 2026-03-14
    Other(String),
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Rusqlite(e)
    }
}

impl From<serde_json::Error> for DbError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rusqlite(e) => write!(f, "SQLite error: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::Poisoned => write!(f, "database mutex poisoned"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl fmt::Debug for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Rusqlite(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::Poisoned | Self::Other(_) => None,
        }
    }
}

// ── EventRow ─────────────────────────────────────────────────────────────────

/// A pre-assembled event ready to be written to the `events` table.
///
/// `signed_by` and `signature` are no longer stored here because the
/// signature covers the DB-assigned `seq` (Issue-SEQ-INTEGRITY). The
/// `NodeSigner` passed to [`ingest_transaction`] signs each event after
/// insertion and updates the row with the real signature.
// CRIT-03 Debug derive — 2026-03-13
// Issue-SEQ-INTEGRITY — 2026-03-14
#[derive(Debug)]
pub struct EventRow {
    /// Globally unique identifier for this event (UUID v4).
    pub event_id: String,
    /// Discriminant describing the kind of state change this event records.
    pub event_type: EventType,
    /// Canonical JSON representation of the event-specific payload.
    pub payload_json: String,
    /// GUID of the primary entity this event concerns (feed, track, etc.).
    pub subject_guid: String,
    /// Unix timestamp (seconds) at which the event was created.
    pub created_at: i64,
    /// Human-readable warnings produced by the verifier chain, if any.
    pub warnings: Vec<String>,
}

// ── ExternalIdRow ──────────────────────────────────────────────────────────────

/// A row from the `external_ids` table linking an entity to an external system.
// CRIT-03 Debug derive — 2026-03-13
#[derive(Debug)]
pub struct ExternalIdRow {
    pub id: i64,
    pub scheme: String,
    pub value: String,
}

// ── EntitySourceRow ────────────────────────────────────────────────────────────

/// A row from the `entity_source` table recording where an entity came from.
// CRIT-03 Debug derive — 2026-03-13
#[derive(Debug)]
pub struct EntitySourceRow {
    pub id: i64,
    pub source_type: String,
    pub source_url: Option<String>,
    pub trust_level: i64,
    pub created_at: i64,
}

// ── Migrations ───────────────────────────────────────────────────────────────
// Issue-MIGRATIONS — 2026-03-14

/// Ordered list of schema migrations.  Each entry is a SQL script that is
/// applied exactly once, inside its own transaction.  The `schema_migrations`
/// table tracks which versions have already been applied, so restarts never
/// re-execute earlier migrations and data is never silently dropped.
const MIGRATIONS: &[&str] = &[
    // Migration 1: baseline schema (formerly src/schema.sql, all DROPs removed)
    include_str!("../migrations/0001_baseline.sql"),
    // Migration 2: scope artist credits to feed_guid (Issue-ARTIST-IDENTITY — 2026-03-14)
    include_str!("../migrations/0002_artist_credit_feed_scope.sql"),
    // Migration 3: unique constraint on search_entities (Issue-HASH-COLLISION — 2026-03-14)
    include_str!("../migrations/0003_search_entities_unique.sql"),
    // Migration 4: add proof_level to proof_tokens (Issue-PROOF-LEVEL — 2026-03-14)
    include_str!("../migrations/0004_proof_level.sql"),
    // Migration 5: add live-events and feed-level remote-item staging tables.
    include_str!("../migrations/0005_live_events_and_remote_items.sql"),
    // Migration 6: add staged source-claim tables for contributors and IDs.
    include_str!("../migrations/0006_source_claim_staging.sql"),
    // Migration 7: add staged source-claim tables for links and release facts.
    include_str!("../migrations/0007_source_link_and_release_claims.sql"),
    // Migration 8: preserve raw contributor roles and add a normalized copy.
    include_str!("../migrations/0008_source_contributor_role_norm.sql"),
    // Migration 9: add staged source item enclosures for primary/alternate media variants.
    include_str!("../migrations/0009_source_item_enclosures.sql"),
    // Migration 10: add staged source platform claims for platform/owner provenance.
    include_str!("../migrations/0010_source_platform_claims.sql"),
    // Migration 11: add deterministic canonical release/recording derived tables.
    include_str!("../migrations/0011_canonical_release_recording.sql"),
    // Migration 12: add durable resolver queue and resolver coordination state.
    include_str!("../migrations/0012_resolver_queue.sql"),
    // Migration 13: add durable artist identity review items and operator overrides.
    include_str!("../migrations/0013_artist_identity_reviews.sql"),
    // Migration 14: add feed-scoped resolved overlay tables for authoritative replication.
    include_str!("../migrations/0014_resolved_overlay_tables.sql"),
];

/// Applies any pending schema migrations to `conn`.
///
/// On the very first run the `schema_migrations` table is created.  Each
/// migration runs inside a transaction so that a failure rolls back cleanly
/// without leaving the database in a half-migrated state.
///
/// # Errors
///
/// Returns [`DbError`] if any migration SQL fails or if the bookkeeping
/// queries fail.
fn run_migrations(conn: &mut Connection) -> Result<(), DbError> {
    // Ensure the tracker table exists (idempotent).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );",
    )?;

    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;

    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        // Migration versions are 1-indexed; the array will never have enough
        // entries for the index to overflow i64.
        let version = i64::try_from(idx).expect("migration count overflowed i64") + 1;
        if version > current {
            // Issue-CHECKED-TX — 2026-03-16: conn is freshly opened in open_db, no nesting.
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![version, unix_now()],
            )?;
            tx.commit()?;
        }
    }

    Ok(())
}

// ── open_db ──────────────────────────────────────────────────────────────────

/// Opens the `SQLite` database at `path` and runs pending schema migrations.
///
/// PRAGMAs are applied before migrations so that WAL mode, foreign keys, and
/// synchronous settings are active for all subsequent operations.
///
/// # Panics
///
/// Panics if the file cannot be opened (e.g. permission denied) or if a
/// migration fails to apply. Both are unrecoverable startup failures.
#[must_use]
// SP-01 stable FTS5 hash — 2026-03-13
// Note: The FTS5 table uses content='' (contentless), so the 'rebuild' command
// is not available. Hash stability is enforced by using SipHash-2-4 with fixed
// keys in search::rowid_for. If the hash ever changes, the index must be
// dropped and re-populated from the source tables.
// HIGH-02 impl AsRef<Path> param — 2026-03-13
pub fn open_db(path: impl AsRef<std::path::Path>) -> Connection {
    let mut conn = Connection::open(path.as_ref()).expect("failed to open database");
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;\n\
         PRAGMA foreign_keys = ON;\n\
         PRAGMA synchronous = NORMAL;",
    )
    .expect("failed to set PRAGMAs");
    run_migrations(&mut conn).expect("failed to apply migrations");
    conn
}

// ── Helper: serialize EventType to snake_case string (no quotes) ─────────────

fn event_type_str(et: &EventType) -> Result<String, DbError> {
    let s = serde_json::to_string(et)?;
    Ok(s.trim_matches('"').to_string())
}

// ── Helper: current unix timestamp ──────────────────────────────────────────

// SP-05 epoch guard — 2026-03-12
/// Returns the current Unix timestamp in seconds.
///
/// # Panics
///
/// Panics if the system clock is before the Unix epoch (1970-01-01T00:00:00Z).
/// This indicates a misconfigured system clock that would corrupt all
/// time-based operations.
#[must_use]
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before Unix epoch — check system time configuration")
        .as_secs()
        .cast_signed()
}

// ── resolve_artist ────────────────────────────────────────────────────────────
// Issue-ARTIST-IDENTITY — 2026-03-14

/// Returns an existing artist matched by alias or lowercased `name`, scoped to
/// a specific feed when `feed_guid` is provided, or inserts a new one and
/// auto-registers its canonical name as an alias.
///
/// When `feed_guid` is `Some`, alias and name lookups are scoped to artists
/// already linked to that feed (via `artist_aliases.feed_guid`). This prevents
/// cross-feed name collisions where two unrelated podcasts with the same
/// `owner_name` would otherwise share an artist record.
///
/// Resolution order:
/// 1. `artist_aliases.alias_lower = name.to_lowercase()` scoped by
///    `feed_guid` — alias lookup.
/// 2. Insert new artist + insert a feed-scoped canonical alias row.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL query fails.
pub fn resolve_artist(
    conn: &Connection,
    name: &str,
    feed_guid: Option<&str>,
) -> Result<Artist, DbError> {
    let name_lower = name.to_lowercase();
    let now = unix_now();

    // 1. Check alias table, scoped by feed_guid.
    let via_alias: Option<Artist> = if let Some(fg) = feed_guid {
        conn.query_row(
            "SELECT a.artist_id, a.name, a.name_lower, a.sort_name, a.type_id, a.area, \
             a.img_url, a.url, a.begin_year, a.end_year, a.created_at, a.updated_at \
             FROM artist_aliases aa \
             JOIN artists a ON a.artist_id = aa.artist_id \
             WHERE aa.alias_lower = ?1 AND aa.feed_guid = ?2 \
             LIMIT 1",
            params![name_lower, fg],
            |row| {
                Ok(Artist {
                    artist_id: row.get(0)?,
                    name: row.get(1)?,
                    name_lower: row.get(2)?,
                    sort_name: row.get(3)?,
                    type_id: row.get(4)?,
                    area: row.get(5)?,
                    img_url: row.get(6)?,
                    url: row.get(7)?,
                    begin_year: row.get(8)?,
                    end_year: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .optional()?
    } else {
        conn.query_row(
            "SELECT a.artist_id, a.name, a.name_lower, a.sort_name, a.type_id, a.area, \
             a.img_url, a.url, a.begin_year, a.end_year, a.created_at, a.updated_at \
             FROM artist_aliases aa \
             JOIN artists a ON a.artist_id = aa.artist_id \
             WHERE aa.alias_lower = ?1 \
             LIMIT 1",
            params![name_lower],
            |row| {
                Ok(Artist {
                    artist_id: row.get(0)?,
                    name: row.get(1)?,
                    name_lower: row.get(2)?,
                    sort_name: row.get(3)?,
                    type_id: row.get(4)?,
                    area: row.get(5)?,
                    img_url: row.get(6)?,
                    url: row.get(7)?,
                    begin_year: row.get(8)?,
                    end_year: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .optional()?
    };

    if let Some(a) = via_alias {
        return Ok(a);
    }

    // 2. New artist — insert artist row and its feed-scoped canonical alias.
    let artist_id = uuid::Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![artist_id, name, name_lower, now, now],
    )?;

    conn.execute(
        "INSERT INTO artist_aliases (alias_lower, artist_id, feed_guid, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![name_lower, artist_id, feed_guid, now],
    )?;

    Ok(Artist {
        artist_id,
        name: name.to_string(),
        name_lower,
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: now,
        updated_at: now,
    })
}

fn artist_feed_count(conn: &Connection, artist_id: &str) -> Result<i64, DbError> {
    Ok(conn.query_row(
        "SELECT COUNT(DISTINCT f.feed_guid) \
         FROM artist_credit_name acn \
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id \
         JOIN feeds f ON f.artist_credit_id = ac.id \
         WHERE acn.artist_id = ?1",
        params![artist_id],
        |row| row.get(0),
    )?)
}

fn canonical_artist_for_query(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Option<Artist>, DbError> {
    let mut stmt = conn.prepare(sql)?;
    let artist_ids: Vec<String> = stmt
        .query_map(params, |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let mut unique_ids = artist_ids;
    unique_ids.sort();
    unique_ids.dedup();
    if unique_ids.is_empty() {
        return Ok(None);
    }

    let mut ranked: Vec<(i64, i64, String)> = Vec::with_capacity(unique_ids.len());
    for artist_id in unique_ids {
        if let Some(artist) = get_artist_by_id(conn, &artist_id)? {
            ranked.push((
                artist_feed_count(conn, &artist_id)?,
                artist.created_at,
                artist.artist_id,
            ));
        }
    }
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    let Some((_, _, artist_id)) = ranked.into_iter().next() else {
        return Ok(None);
    };
    get_artist_by_id(conn, &artist_id)
}

fn find_existing_artist_by_npub_and_name(
    conn: &Connection,
    npub: &str,
    artist_name_lower: &str,
) -> Result<Option<Artist>, DbError> {
    canonical_artist_for_query(
        conn,
        "SELECT DISTINCT a.artist_id \
         FROM external_ids ei \
         JOIN artists a ON a.artist_id = ei.entity_id \
         WHERE ei.entity_type = 'artist' \
           AND ei.scheme = 'nostr_npub' \
           AND ei.value = ?1 \
           AND a.name_lower = ?2",
        &[&npub, &artist_name_lower],
    )
}

fn find_existing_artist_by_publisher_guid_and_name(
    conn: &Connection,
    remote_feed_guid: &str,
    artist_name_lower: &str,
) -> Result<Option<Artist>, DbError> {
    canonical_artist_for_query(
        conn,
        "SELECT DISTINCT a.artist_id \
         FROM feed_remote_items_raw fri \
         JOIN feeds f ON f.feed_guid = fri.feed_guid \
         JOIN artist_credit_name acn ON acn.artist_credit_id = f.artist_credit_id \
         JOIN artists a ON a.artist_id = acn.artist_id \
         WHERE fri.medium = 'publisher' \
           AND fri.remote_feed_guid = ?1 \
           AND a.name_lower = ?2",
        &[&remote_feed_guid, &artist_name_lower],
    )
}

fn find_existing_artist_by_website_url_and_name(
    conn: &Connection,
    url: &str,
    artist_name_lower: &str,
) -> Result<Option<Artist>, DbError> {
    canonical_artist_for_query(
        conn,
        "SELECT DISTINCT a.artist_id \
         FROM source_entity_links sel \
         JOIN feeds f ON f.feed_guid = sel.feed_guid \
         JOIN artist_credit_name acn ON acn.artist_credit_id = f.artist_credit_id \
         JOIN artists a ON a.artist_id = acn.artist_id \
         WHERE sel.entity_type = 'feed' \
           AND sel.link_type = 'website' \
           AND sel.url = ?1 \
           AND a.name_lower = ?2",
        &[&url, &artist_name_lower],
    )
}

/// Resolves a feed artist using high-confidence source claims before falling
/// back to feed-scoped alias resolution.
pub fn resolve_feed_artist_from_source_claims(
    conn: &Connection,
    name: &str,
    feed_guid: &str,
    source_entity_ids: &[SourceEntityIdClaim],
    remote_items: &[FeedRemoteItemRaw],
    source_entity_links: &[SourceEntityLink],
) -> Result<Artist, DbError> {
    let artist_name_lower = name.to_lowercase();

    let npubs: std::collections::BTreeSet<String> = source_entity_ids
        .iter()
        .filter(|claim| {
            claim.entity_type == "feed"
                && claim.entity_id == feed_guid
                && claim.scheme == "nostr_npub"
        })
        .map(|claim| claim.value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if let Some(only_npub) = (npubs.len() == 1).then(|| npubs.first()).flatten()
        && let Some(artist) =
            find_existing_artist_by_npub_and_name(conn, only_npub, &artist_name_lower)?
    {
        return Ok(artist);
    }

    let publisher_guids: std::collections::BTreeSet<String> = remote_items
        .iter()
        .filter(|item| item.medium.as_deref() == Some("publisher"))
        .map(|item| item.remote_feed_guid.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if let Some(only_publisher_guid) = (publisher_guids.len() == 1)
        .then(|| publisher_guids.first())
        .flatten()
        && let Some(artist) = find_existing_artist_by_publisher_guid_and_name(
            conn,
            only_publisher_guid,
            &artist_name_lower,
        )?
    {
        return Ok(artist);
    }

    let website_urls: std::collections::BTreeSet<String> = source_entity_links
        .iter()
        .filter(|link| {
            link.entity_type == "feed" && link.entity_id == feed_guid && link.link_type == "website"
        })
        .map(|link| link.url.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if let Some(only_website_url) = (website_urls.len() == 1)
        .then(|| website_urls.first())
        .flatten()
        && let Some(artist) = find_existing_artist_by_website_url_and_name(
            conn,
            only_website_url,
            &artist_name_lower,
        )?
    {
        return Ok(artist);
    }

    resolve_artist(conn, name, Some(feed_guid))
}

// ── get_artist_by_id ─────────────────────────────────────────────────────────
// Issue-12 PATCH emits events — 2026-03-13

/// Returns the artist row for `artist_id`, or `None` if absent.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_artist_by_id(conn: &Connection, artist_id: &str) -> Result<Option<Artist>, DbError> {
    let result = conn
        .query_row(
            "SELECT artist_id, name, name_lower, sort_name, type_id, area, \
         img_url, url, begin_year, end_year, created_at, updated_at \
         FROM artists WHERE artist_id = ?1",
            params![artist_id],
            |row| {
                Ok(Artist {
                    artist_id: row.get(0)?,
                    name: row.get(1)?,
                    name_lower: row.get(2)?,
                    sort_name: row.get(3)?,
                    type_id: row.get(4)?,
                    area: row.get(5)?,
                    img_url: row.get(6)?,
                    url: row.get(7)?,
                    begin_year: row.get(8)?,
                    end_year: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .optional()?;
    Ok(result)
}

// ── artist_exists ────────────────────────────────────────────────────────────
// Issue-SSE-EXHAUSTION — 2026-03-15

/// Returns `true` if an artist with the given `artist_id` exists in the database.
///
/// Uses a lightweight `SELECT 1` query (no row parsing) so it is cheaper than
/// [`get_artist_by_id`] for pure existence checks.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn artist_exists(conn: &Connection, artist_id: &str) -> Result<bool, DbError> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM artists WHERE artist_id = ?1)",
        params![artist_id],
        |row| row.get(0),
    )?;
    Ok(exists)
}

// ── get_payment_routes_for_track ─────────────────────────────────────────────
// Issue-12 PATCH emits events — 2026-03-13

/// Returns all payment routes for `track_guid`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_payment_routes_for_track(
    conn: &Connection,
    track_guid: &str,
) -> Result<Vec<PaymentRoute>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, track_guid, feed_guid, recipient_name, route_type, address, \
         custom_key, custom_value, split, fee \
         FROM payment_routes WHERE track_guid = ?1",
    )?;
    let rows = stmt.query_map(params![track_guid], |row| {
        let rt_str: String = row.get(4)?;
        let fee_i: i64 = row.get(9)?;
        Ok(PaymentRoute {
            id: row.get(0)?,
            track_guid: row.get(1)?,
            feed_guid: row.get(2)?,
            recipient_name: row.get(3)?,
            route_type: serde_json::from_str(&format!("\"{rt_str}\"")).unwrap_or(RouteType::Node),
            address: row.get(5)?,
            custom_key: row.get(6)?,
            custom_value: row.get(7)?,
            split: row.get(8)?,
            fee: fee_i != 0,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

// ── get_value_time_splits_for_track ──────────────────────────────────────────
// Issue-12 PATCH emits events — 2026-03-13

/// Returns all value-time splits for `source_track_guid`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_value_time_splits_for_track(
    conn: &Connection,
    track_guid: &str,
) -> Result<Vec<ValueTimeSplit>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, source_track_guid, start_time_secs, duration_secs, remote_feed_guid, \
         remote_item_guid, split, created_at \
         FROM value_time_splits WHERE source_track_guid = ?1",
    )?;
    let rows = stmt.query_map(params![track_guid], |row| {
        Ok(ValueTimeSplit {
            id: row.get(0)?,
            source_track_guid: row.get(1)?,
            start_time_secs: row.get(2)?,
            duration_secs: row.get(3)?,
            remote_feed_guid: row.get(4)?,
            remote_item_guid: row.get(5)?,
            split: row.get(6)?,
            created_at: row.get(7)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

// ── add_artist_alias ──────────────────────────────────────────────────────────

/// Registers `alias` (lowercased) as an additional lookup key for `artist_id`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL insert fails.
pub fn add_artist_alias(conn: &Connection, artist_id: &str, alias: &str) -> Result<(), DbError> {
    let now = unix_now();
    conn.execute(
        "INSERT OR IGNORE INTO artist_aliases (alias_lower, artist_id, created_at) \
         VALUES (?1, ?2, ?3)",
        params![alias.to_lowercase(), artist_id, now],
    )?;
    Ok(())
}

// ── merge_artists ──────────────────────────────────────────────────────────────

/// Merges `source_artist_id` into `target_artist_id`.
///
/// All `artist_credit_name` entries pointing to `source` are repointed to `target`.
/// All aliases of `source` that do not already exist on `target` are transferred;
/// any that would conflict are dropped. The `source` artist row is then
/// deleted. Returns the list of alias strings that were transferred.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL statement or the transaction commit fails.
pub fn merge_artists(
    conn: &mut Connection,
    source_artist_id: &str,
    target_artist_id: &str,
) -> Result<Vec<String>, DbError> {
    let tx = conn.transaction()?;
    let transferred = merge_artists_sql(&tx, source_artist_id, target_artist_id)?;
    tx.commit()?;
    Ok(transferred)
}

/// Inner implementation of artist merge: executes all SQL operations on
/// the provided connection without managing its own transaction.  Callers
/// must ensure they are already inside a transaction or savepoint.
pub(crate) fn merge_artists_sql(
    conn: &Connection,
    source_artist_id: &str,
    target_artist_id: &str,
) -> Result<Vec<String>, DbError> {
    // Finding-1 alias transfer SQL fixed — 2026-03-13
    // Collect the aliases that will be transferred.
    let mut stmt = conn.prepare(
        "SELECT aa.alias_lower FROM artist_aliases aa \
         WHERE aa.artist_id = ?1 \
           AND NOT EXISTS ( \
               SELECT 1 FROM artist_aliases existing \
               WHERE existing.alias_lower = aa.alias_lower \
                 AND existing.artist_id = ?2 \
           )",
    )?;
    let transferred: Vec<String> = stmt
        .query_map(params![source_artist_id, target_artist_id], |row| {
            row.get(0)
        })?
        .collect::<Result<_, _>>()?;
    drop(stmt);

    // Repoint artist_credit_name entries.
    conn.execute(
        "UPDATE artist_credit_name SET artist_id = ?1 WHERE artist_id = ?2",
        params![target_artist_id, source_artist_id],
    )?;

    // Transfer non-conflicting aliases (Finding-1 fix: use distinct table aliases).
    conn.execute(
        "UPDATE artist_aliases SET artist_id = ?1 \
         WHERE artist_id = ?2 \
           AND NOT EXISTS ( \
               SELECT 1 FROM artist_aliases existing \
               WHERE existing.alias_lower = artist_aliases.alias_lower \
                 AND existing.artist_id = ?1 \
           )",
        params![target_artist_id, source_artist_id],
    )?;

    conn.execute(
        "UPDATE artist_tag SET artist_id = ?1 \
         WHERE artist_id = ?2 \
           AND NOT EXISTS ( \
               SELECT 1 FROM artist_tag existing \
               WHERE existing.artist_id = ?1 \
                 AND existing.tag_id = artist_tag.tag_id \
           )",
        params![target_artist_id, source_artist_id],
    )?;
    conn.execute(
        "DELETE FROM artist_tag WHERE artist_id = ?1",
        params![source_artist_id],
    )?;

    conn.execute(
        "UPDATE external_ids SET entity_id = ?1 \
         WHERE entity_type = 'artist' AND entity_id = ?2 \
           AND NOT EXISTS ( \
               SELECT 1 FROM external_ids existing \
               WHERE existing.entity_type = 'artist' \
                 AND existing.entity_id = ?1 \
                 AND existing.scheme = external_ids.scheme \
           )",
        params![target_artist_id, source_artist_id],
    )?;
    conn.execute(
        "DELETE FROM external_ids WHERE entity_type = 'artist' AND entity_id = ?1",
        params![source_artist_id],
    )?;

    conn.execute(
        "UPDATE entity_source SET entity_id = ?1 \
         WHERE entity_type = 'artist' AND entity_id = ?2",
        params![target_artist_id, source_artist_id],
    )?;

    conn.execute(
        "UPDATE artist_artist_rel SET artist_id_a = ?1 WHERE artist_id_a = ?2",
        params![target_artist_id, source_artist_id],
    )?;
    conn.execute(
        "UPDATE artist_artist_rel SET artist_id_b = ?1 WHERE artist_id_b = ?2",
        params![target_artist_id, source_artist_id],
    )?;
    conn.execute(
        "DELETE FROM artist_artist_rel WHERE artist_id_a = artist_id_b",
        [],
    )?;

    // Drop any remaining source aliases (those that conflicted).
    conn.execute(
        "DELETE FROM artist_aliases WHERE artist_id = ?1",
        params![source_artist_id],
    )?;

    // Preserve redirect chains when merging an artist that had already absorbed
    // earlier artist IDs.
    conn.execute(
        "INSERT OR REPLACE INTO artist_id_redirect (old_artist_id, new_artist_id, merged_at) \
         SELECT old_artist_id, ?1, merged_at \
         FROM artist_id_redirect \
         WHERE new_artist_id = ?2",
        params![target_artist_id, source_artist_id],
    )?;

    // Record redirect for old ID resolution.
    let now = unix_now();
    conn.execute(
        "INSERT OR REPLACE INTO artist_id_redirect (old_artist_id, new_artist_id, merged_at) \
         VALUES (?1, ?2, ?3)",
        params![source_artist_id, target_artist_id, now],
    )?;

    // Delete the source artist row.
    conn.execute(
        "DELETE FROM artists WHERE artist_id = ?1",
        params![source_artist_id],
    )?;

    Ok(transferred)
}

// ── upsert_artist_if_absent ───────────────────────────────────────────────────

/// Inserts the artist if no row with the same `artist_id` exists yet.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL insert fails.
pub fn upsert_artist_if_absent(conn: &Connection, artist: &Artist) -> Result<(), DbError> {
    conn.execute(
        "INSERT OR IGNORE INTO artists (artist_id, name, name_lower, sort_name, type_id, area, \
         img_url, url, begin_year, end_year, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            artist.artist_id,
            artist.name,
            artist.name_lower,
            artist.sort_name,
            artist.type_id,
            artist.area,
            artist.img_url,
            artist.url,
            artist.begin_year,
            artist.end_year,
            artist.created_at,
            artist.updated_at,
        ],
    )?;
    Ok(())
}

// ── Artist credit operations ────────────────────────────────────────────────

/// Creates an artist credit with its constituent names. Returns the credit with
/// the assigned `id` populated.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL insert fails.
// Issue-ARTIST-IDENTITY — 2026-03-14
pub fn create_artist_credit(
    conn: &Connection,
    display_name: &str,
    names: &[(String, String, String)], // (artist_id, credited_name, join_phrase)
    feed_guid: Option<&str>,
) -> Result<ArtistCredit, DbError> {
    let now = unix_now();

    conn.execute(
        "INSERT INTO artist_credit (display_name, feed_guid, created_at) VALUES (?1, ?2, ?3)",
        params![display_name, feed_guid, now],
    )?;
    let credit_id = conn.last_insert_rowid();

    let mut credit_names = Vec::with_capacity(names.len());
    for (pos, (artist_id, name, join_phrase)) in names.iter().enumerate() {
        #[expect(
            clippy::cast_possible_wrap,
            reason = "artist credit position count never approaches i64::MAX"
        )]
        let position = pos as i64;
        conn.execute(
            "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![credit_id, artist_id, position, name, join_phrase],
        )?;
        let acn_id = conn.last_insert_rowid();
        credit_names.push(ArtistCreditName {
            id: acn_id,
            artist_credit_id: credit_id,
            artist_id: artist_id.clone(),
            position,
            name: name.clone(),
            join_phrase: join_phrase.clone(),
        });
    }

    Ok(ArtistCredit {
        id: credit_id,
        display_name: display_name.to_string(),
        feed_guid: feed_guid.map(String::from),
        created_at: now,
        names: credit_names,
    })
}

/// Creates a simple single-artist credit and returns it.
///
/// # Errors
///
/// Returns [`DbError`] if the underlying credit creation fails.
// Issue-ARTIST-IDENTITY — 2026-03-14
pub fn create_single_artist_credit(
    conn: &Connection,
    artist: &Artist,
    feed_guid: Option<&str>,
) -> Result<ArtistCredit, DbError> {
    create_artist_credit(
        conn,
        &artist.name,
        &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
        feed_guid,
    )
}

/// Retrieves an artist credit by ID, including its constituent names.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL query fails.
// Issue-ARTIST-IDENTITY — 2026-03-14
pub fn get_artist_credit(
    conn: &Connection,
    credit_id: i64,
) -> Result<Option<ArtistCredit>, DbError> {
    let credit: Option<(i64, String, Option<String>, i64)> = conn
        .query_row(
            "SELECT id, display_name, feed_guid, created_at FROM artist_credit WHERE id = ?1",
            params![credit_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    let Some((id, display_name, feed_guid, created_at)) = credit else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT id, artist_credit_id, artist_id, position, name, join_phrase \
         FROM artist_credit_name WHERE artist_credit_id = ?1 ORDER BY position",
    )?;
    let names: Vec<ArtistCreditName> = stmt
        .query_map(params![id], |row| {
            Ok(ArtistCreditName {
                id: row.get(0)?,
                artist_credit_id: row.get(1)?,
                artist_id: row.get(2)?,
                position: row.get(3)?,
                name: row.get(4)?,
                join_phrase: row.get(5)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    Ok(Some(ArtistCredit {
        id,
        display_name,
        feed_guid,
        created_at,
        names,
    }))
}

// Issue-6 batch credits — 2026-03-13
/// Batch-loads multiple artist credits by ID in two queries instead of 2*N.
///
/// Returns a `HashMap<credit_id, ArtistCredit>` for O(1) lookup. Credits whose
/// IDs are not found in the database are silently omitted from the map.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL query fails.
pub fn load_credits_batch(
    conn: &Connection,
    ids: &[i64],
) -> Result<std::collections::HashMap<i64, ArtistCredit>, DbError> {
    use std::collections::HashMap;

    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    // Deduplicate IDs to avoid redundant rows.
    let unique_ids: Vec<i64> = {
        let mut set = std::collections::HashSet::new();
        ids.iter().copied().filter(|id| set.insert(*id)).collect()
    };

    // Build a single parameterised placeholder string: ?,?,?
    let placeholders: String = std::iter::repeat_n("?", unique_ids.len())
        .collect::<Vec<_>>()
        .join(",");

    // Query 1: artist_credit rows.
    // Issue-ARTIST-IDENTITY — 2026-03-14
    let sql_credits = format!(
        "SELECT id, display_name, feed_guid, created_at FROM artist_credit WHERE id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql_credits)?;
    let params_credits: Vec<Box<dyn rusqlite::types::ToSql>> = unique_ids
        .iter()
        .map(|id| Box::new(*id) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let credit_rows = stmt.query_map(
        params_credits
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .as_slice(),
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
            ))
        },
    )?;

    let mut map: HashMap<i64, ArtistCredit> = HashMap::new();
    for row in credit_rows {
        let (id, display_name, feed_guid, created_at) = row?;
        map.insert(
            id,
            ArtistCredit {
                id,
                display_name,
                feed_guid,
                created_at,
                names: Vec::new(),
            },
        );
    }

    if map.is_empty() {
        return Ok(map);
    }

    // Query 2: artist_credit_name rows for all loaded credits.
    let sql_names = format!(
        "SELECT id, artist_credit_id, artist_id, position, name, join_phrase \
         FROM artist_credit_name WHERE artist_credit_id IN ({placeholders}) ORDER BY artist_credit_id, position"
    );
    let mut stmt_names = conn.prepare(&sql_names)?;
    let params_names: Vec<Box<dyn rusqlite::types::ToSql>> = unique_ids
        .iter()
        .map(|id| Box::new(*id) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let name_rows = stmt_names.query_map(
        params_names
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .as_slice(),
        |row| {
            Ok(ArtistCreditName {
                id: row.get(0)?,
                artist_credit_id: row.get(1)?,
                artist_id: row.get(2)?,
                position: row.get(3)?,
                name: row.get(4)?,
                join_phrase: row.get(5)?,
            })
        },
    )?;

    for row in name_rows {
        let acn = row?;
        if let Some(credit) = map.get_mut(&acn.artist_credit_id) {
            credit.names.push(acn);
        }
    }

    Ok(map)
}

/// Looks up an artist credit by display name (case-insensitive via `LOWER()`)
/// scoped to a specific feed when `feed_guid` is provided.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL query fails.
// Issue-ARTIST-IDENTITY — 2026-03-14
pub fn get_artist_credit_by_display_name(
    conn: &Connection,
    display_name: &str,
    feed_guid: Option<&str>,
) -> Result<Option<ArtistCredit>, DbError> {
    let lower = display_name.to_lowercase();

    let credit: Option<(i64, String, Option<String>, i64)> = if let Some(fg) = feed_guid {
        conn.query_row(
            "SELECT id, display_name, feed_guid, created_at FROM artist_credit \
             WHERE LOWER(display_name) = ?1 AND feed_guid = ?2",
            params![lower, fg],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
    } else {
        conn.query_row(
            "SELECT id, display_name, feed_guid, created_at FROM artist_credit \
             WHERE LOWER(display_name) = ?1 AND feed_guid IS NULL",
            params![lower],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
    };

    let Some((id, display_name, feed_guid_val, created_at)) = credit else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT id, artist_credit_id, artist_id, position, name, join_phrase \
         FROM artist_credit_name WHERE artist_credit_id = ?1 ORDER BY position",
    )?;
    let names: Vec<ArtistCreditName> = stmt
        .query_map(params![id], |row| {
            Ok(ArtistCreditName {
                id: row.get(0)?,
                artist_credit_id: row.get(1)?,
                artist_id: row.get(2)?,
                position: row.get(3)?,
                name: row.get(4)?,
                join_phrase: row.get(5)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    Ok(Some(ArtistCredit {
        id,
        display_name,
        feed_guid: feed_guid_val,
        created_at,
        names,
    }))
}

/// Idempotent artist credit retrieval, scoped by feed.
///
/// Returns an existing credit if one with a matching `display_name`
/// (case-insensitive) already exists within the same feed scope, otherwise
/// creates a new credit with the given `names`.
///
/// # Errors
///
/// Returns [`DbError`] if the lookup or creation query fails.
// Issue-ARTIST-IDENTITY — 2026-03-14
pub fn get_or_create_artist_credit(
    conn: &Connection,
    display_name: &str,
    names: &[(String, String, String)], // (artist_id, credited_name, join_phrase)
    feed_guid: Option<&str>,
) -> Result<ArtistCredit, DbError> {
    if let Some(existing) = get_artist_credit_by_display_name(conn, display_name, feed_guid)? {
        return Ok(existing);
    }
    create_artist_credit(conn, display_name, names, feed_guid)
}

/// Returns all artist credits in which `artist_id` participates (via
/// `artist_credit_name` JOIN).
///
/// # Errors
///
/// Returns [`DbError`] if any SQL query fails.
// Issue-ARTIST-IDENTITY — 2026-03-14
pub fn get_artist_credits_for_artist(
    conn: &Connection,
    artist_id: &str,
) -> Result<Vec<ArtistCredit>, DbError> {
    let mut credit_stmt = conn.prepare(
        "SELECT DISTINCT ac.id, ac.display_name, ac.feed_guid, ac.created_at \
         FROM artist_credit ac \
         JOIN artist_credit_name acn ON acn.artist_credit_id = ac.id \
         WHERE acn.artist_id = ?1 \
         ORDER BY ac.id",
    )?;
    let credits: Vec<(i64, String, Option<String>, i64)> = credit_stmt
        .query_map(params![artist_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<_, _>>()?;

    let mut name_stmt = conn.prepare(
        "SELECT id, artist_credit_id, artist_id, position, name, join_phrase \
         FROM artist_credit_name WHERE artist_credit_id = ?1 ORDER BY position",
    )?;

    let mut result = Vec::with_capacity(credits.len());
    for (id, display_name, feed_guid, created_at) in credits {
        let names: Vec<ArtistCreditName> = name_stmt
            .query_map(params![id], |row| {
                Ok(ArtistCreditName {
                    id: row.get(0)?,
                    artist_credit_id: row.get(1)?,
                    artist_id: row.get(2)?,
                    position: row.get(3)?,
                    name: row.get(4)?,
                    join_phrase: row.get(5)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        result.push(ArtistCredit {
            id,
            display_name,
            feed_guid,
            created_at,
            names,
        });
    }

    Ok(result)
}

// ── upsert_feed ───────────────────────────────────────────────────────────────

/// Inserts or updates a feed row keyed on `feed_guid`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL upsert fails.
pub fn upsert_feed(conn: &Connection, feed: &Feed) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, description, image_url, \
         language, explicit, itunes_type, episode_count, newest_item_at, oldest_item_at, created_at, \
         updated_at, raw_medium) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16) \
         ON CONFLICT(feed_guid) DO UPDATE SET \
           feed_url         = excluded.feed_url, \
           title            = excluded.title, \
           title_lower      = excluded.title_lower, \
           artist_credit_id = excluded.artist_credit_id, \
           description      = excluded.description, \
           image_url        = excluded.image_url, \
           language         = excluded.language, \
           explicit         = excluded.explicit, \
           itunes_type      = excluded.itunes_type, \
           episode_count    = excluded.episode_count, \
           newest_item_at   = excluded.newest_item_at, \
           oldest_item_at   = excluded.oldest_item_at, \
           updated_at       = excluded.updated_at, \
           raw_medium       = excluded.raw_medium",
        params![
            feed.feed_guid,
            feed.feed_url,
            feed.title,
            feed.title_lower,
            feed.artist_credit_id,
            feed.description,
            feed.image_url,
            feed.language,
            i64::from(feed.explicit),
            feed.itunes_type,
            feed.episode_count,
            feed.newest_item_at,
            feed.oldest_item_at,
            feed.created_at,
            feed.updated_at,
            feed.raw_medium,
        ],
    )?;
    Ok(())
}

// ── upsert_track ──────────────────────────────────────────────────────────────

/// Inserts or updates a track row keyed on `track_guid`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL upsert fails.
pub fn upsert_track(conn: &Connection, track: &Track) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, \
         duration_secs, enclosure_url, enclosure_type, enclosure_bytes, track_number, season, \
         explicit, description, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16) \
         ON CONFLICT(track_guid) DO UPDATE SET \
           feed_guid        = excluded.feed_guid, \
           artist_credit_id = excluded.artist_credit_id, \
           title            = excluded.title, \
           title_lower      = excluded.title_lower, \
           pub_date         = excluded.pub_date, \
           duration_secs    = excluded.duration_secs, \
           enclosure_url    = excluded.enclosure_url, \
           enclosure_type   = excluded.enclosure_type, \
           enclosure_bytes  = excluded.enclosure_bytes, \
           track_number     = excluded.track_number, \
           season           = excluded.season, \
           explicit         = excluded.explicit, \
           description      = excluded.description, \
           updated_at       = excluded.updated_at",
        params![
            track.track_guid,
            track.feed_guid,
            track.artist_credit_id,
            track.title,
            track.title_lower,
            track.pub_date,
            track.duration_secs,
            track.enclosure_url,
            track.enclosure_type,
            track.enclosure_bytes,
            track.track_number,
            track.season,
            i64::from(track.explicit),
            track.description,
            track.created_at,
            track.updated_at,
        ],
    )?;
    Ok(())
}

fn canonical_cluster_id(kind: &str, key: &str) -> String {
    let digest = sha2::Sha256::digest(key.as_bytes());
    format!("{kind}:{}", hex::encode(digest))
}

fn identity_release_id_for_feed_guid(feed_guid: &str) -> String {
    canonical_cluster_id(
        "release",
        &format!("identity_release_feed_guid_v1|{feed_guid}"),
    )
}

fn identity_recording_id_for_track_guid(track_guid: &str) -> String {
    canonical_cluster_id(
        "recording",
        &format!("identity_recording_track_guid_v1|{track_guid}"),
    )
}

fn track_release_sort_key(a: &Track, b: &Track) -> std::cmp::Ordering {
    a.track_number
        .is_none()
        .cmp(&b.track_number.is_none())
        .then_with(|| {
            a.track_number
                .unwrap_or(i64::MAX)
                .cmp(&b.track_number.unwrap_or(i64::MAX))
        })
        .then_with(|| {
            a.pub_date
                .unwrap_or(i64::MAX)
                .cmp(&b.pub_date.unwrap_or(i64::MAX))
        })
        .then_with(|| a.title_lower.cmp(&b.title_lower))
        .then_with(|| a.track_guid.cmp(&b.track_guid))
}

fn get_artist_credit_display_name(
    conn: &Connection,
    artist_credit_id: i64,
) -> Result<String, DbError> {
    Ok(get_artist_credit(conn, artist_credit_id)?.map_or_else(
        || format!("artist_credit_id:{artist_credit_id}"),
        |credit| credit.display_name.to_lowercase(),
    ))
}

fn get_feed_platform_keys(
    conn: &Connection,
    feed_guid: &str,
) -> Result<std::collections::BTreeSet<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT platform_key FROM source_platform_claims \
         WHERE feed_guid = ?1 ORDER BY platform_key",
    )?;
    let keys: Vec<String> = stmt
        .query_map(params![feed_guid], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(keys.into_iter().collect())
}

fn feed_artist_evidence_key(conn: &Connection, feed: &Feed) -> Result<String, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT value FROM source_entity_ids \
         WHERE feed_guid = ?1 AND entity_type = 'feed' AND entity_id = ?1 AND scheme = 'nostr_npub' \
         ORDER BY value",
    )?;
    let npubs: Vec<String> = stmt
        .query_map(params![feed.feed_guid], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    if npubs.len() == 1 {
        return Ok(format!("nostr_npub:{}", npubs[0]));
    }

    let mut stmt = conn.prepare(
        "SELECT DISTINCT remote_feed_guid FROM feed_remote_items_raw \
         WHERE feed_guid = ?1 AND medium = 'publisher' \
         ORDER BY remote_feed_guid",
    )?;
    let publisher_guids: Vec<String> = stmt
        .query_map(params![feed.feed_guid], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    if publisher_guids.len() == 1 {
        return Ok(format!("publisher_feed_guid:{}", publisher_guids[0]));
    }

    Ok(format!(
        "artist_credit_display:{}",
        get_artist_credit_display_name(conn, feed.artist_credit_id)?
    ))
}

fn cross_platform_single_track_anchor(
    conn: &Connection,
    feed: &Feed,
    track: &Track,
) -> Result<Option<(String, i64)>, DbError> {
    let Some(duration_secs) = track.duration_secs else {
        return Ok(None);
    };
    let current_platforms = get_feed_platform_keys(conn, &feed.feed_guid)?;
    if current_platforms.is_empty() {
        return Ok(None);
    }

    let artist_display_key = get_artist_credit_display_name(conn, feed.artist_credit_id)?;
    let mut corroborating_platforms = current_platforms;
    let mut found_match = false;
    let mut has_lower_neighbor = false;
    let min_duration = duration_secs.saturating_sub(1);
    let max_duration = duration_secs.saturating_add(1);

    let mut stmt = conn.prepare(
        "SELECT f.feed_guid, f.artist_credit_id, t.duration_secs \
         FROM feeds f \
         JOIN tracks t ON t.feed_guid = f.feed_guid \
         WHERE f.feed_guid <> ?1 \
           AND f.title_lower = ?2 \
           AND t.title_lower = ?3 \
           AND t.duration_secs BETWEEN ?4 AND ?5 \
           AND (SELECT COUNT(*) FROM tracks t2 WHERE t2.feed_guid = f.feed_guid) = 1",
    )?;
    let candidates: Vec<(String, i64, i64)> = stmt
        .query_map(
            params![
                feed.feed_guid,
                feed.title_lower,
                track.title_lower,
                min_duration,
                max_duration,
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?
        .collect::<Result<_, _>>()?;

    for (candidate_feed_guid, candidate_artist_credit_id, candidate_duration_secs) in candidates {
        if get_artist_credit_display_name(conn, candidate_artist_credit_id)? != artist_display_key {
            continue;
        }
        let candidate_platforms = get_feed_platform_keys(conn, &candidate_feed_guid)?;
        if candidate_platforms.is_empty() {
            continue;
        }
        found_match = true;
        if candidate_duration_secs == duration_secs - 1 {
            has_lower_neighbor = true;
        }
        corroborating_platforms.extend(candidate_platforms);
    }

    if !found_match || corroborating_platforms.len() < 2 {
        return Ok(None);
    }

    let duration_anchor = if has_lower_neighbor {
        duration_secs - 1
    } else {
        duration_secs
    };
    Ok(Some((artist_display_key, duration_anchor)))
}

fn release_cluster_target(
    conn: &Connection,
    feed: &Feed,
    tracks: &[Track],
) -> Result<(String, String, i64), DbError> {
    if tracks.len() == 1
        && let Some((artist_display_key, duration_anchor)) =
            cross_platform_single_track_anchor(conn, feed, &tracks[0])?
    {
        let key = format!(
            "single_track_cross_platform_release_v1|artist_display={artist_display_key}|release_title={}|track_title={}|duration_anchor={duration_anchor}",
            feed.title_lower, tracks[0].title_lower,
        );
        return Ok((
            canonical_cluster_id("release", &key),
            "single_track_cross_platform_release_v1".to_string(),
            92,
        ));
    }

    if tracks.is_empty() || tracks.iter().any(|track| track.duration_secs.is_none()) {
        return Ok((
            identity_release_id_for_feed_guid(&feed.feed_guid),
            "feed_guid_identity_v1".to_string(),
            100,
        ));
    }

    let artist_key = feed_artist_evidence_key(conn, feed)?;
    let track_signature = tracks
        .iter()
        .map(|track| {
            format!(
                "{}@{}",
                track.title_lower,
                track.duration_secs.unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    let key = format!(
        "exact_release_signature_v1|artist={artist_key}|title={}|tracks={track_signature}",
        feed.title_lower
    );
    Ok((
        canonical_cluster_id("release", &key),
        "exact_release_signature_v1".to_string(),
        95,
    ))
}

fn ensure_release_row(conn: &Connection, release_id: &str, feed: &Feed) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO releases \
         (release_id, title, title_lower, artist_credit_id, description, image_url, release_date, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(release_id) DO NOTHING",
        params![
            release_id,
            feed.title,
            feed.title_lower,
            feed.artist_credit_id,
            feed.description,
            feed.image_url,
            feed.oldest_item_at,
            feed.created_at,
            feed.updated_at,
        ],
    )?;
    Ok(())
}

fn upsert_release_row(conn: &Connection, release: &Release) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO releases \
         (release_id, title, title_lower, artist_credit_id, description, image_url, release_date, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(release_id) DO UPDATE SET \
           title = excluded.title, \
           title_lower = excluded.title_lower, \
           artist_credit_id = excluded.artist_credit_id, \
           description = excluded.description, \
           image_url = excluded.image_url, \
           release_date = excluded.release_date, \
           created_at = excluded.created_at, \
           updated_at = excluded.updated_at",
        params![
            release.release_id,
            release.title,
            release.title_lower,
            release.artist_credit_id,
            release.description,
            release.image_url,
            release.release_date,
            release.created_at,
            release.updated_at,
        ],
    )?;
    Ok(())
}

fn recording_cluster_target(
    conn: &Connection,
    feed: &Feed,
    track: &Track,
) -> Result<(String, String, i64), DbError> {
    if let Some((artist_display_key, duration_anchor)) =
        cross_platform_single_track_anchor(conn, feed, track)?
    {
        let key = format!(
            "single_track_cross_platform_recording_v1|artist_display={artist_display_key}|track_title={}|duration_anchor={duration_anchor}",
            track.title_lower,
        );
        return Ok((
            canonical_cluster_id("recording", &key),
            "single_track_cross_platform_recording_v1".to_string(),
            92,
        ));
    }

    let Some(duration_secs) = track.duration_secs else {
        return Ok((
            identity_recording_id_for_track_guid(&track.track_guid),
            "track_guid_identity_v1".to_string(),
            100,
        ));
    };

    let artist_key = feed_artist_evidence_key(conn, feed)?;
    let key = format!(
        "exact_recording_signature_v1|artist={artist_key}|title={}|duration={duration_secs}",
        track.title_lower
    );
    Ok((
        canonical_cluster_id("recording", &key),
        "exact_recording_signature_v1".to_string(),
        95,
    ))
}

fn ensure_recording_row(
    conn: &Connection,
    recording_id: &str,
    track: &Track,
) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO recordings \
         (recording_id, title, title_lower, artist_credit_id, duration_secs, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(recording_id) DO NOTHING",
        params![
            recording_id,
            track.title,
            track.title_lower,
            track.artist_credit_id,
            track.duration_secs,
            track.created_at,
            track.updated_at,
        ],
    )?;
    Ok(())
}

fn upsert_recording_row(conn: &Connection, recording: &Recording) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO recordings \
         (recording_id, title, title_lower, artist_credit_id, duration_secs, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(recording_id) DO UPDATE SET \
           title = excluded.title, \
           title_lower = excluded.title_lower, \
           artist_credit_id = excluded.artist_credit_id, \
           duration_secs = excluded.duration_secs, \
           created_at = excluded.created_at, \
           updated_at = excluded.updated_at",
        params![
            recording.recording_id,
            recording.title,
            recording.title_lower,
            recording.artist_credit_id,
            recording.duration_secs,
            recording.created_at,
            recording.updated_at,
        ],
    )?;
    Ok(())
}

fn has_non_blank_text(value: Option<&String>) -> i64 {
    value
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or(0, |_| 1)
}

type FeedRepresentativeRank = (i64, i64, i64, i64, i64, i64, i64, i64, i64);
type TrackRepresentativeRank = (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64);

fn feed_representative_rank(map: &SourceFeedReleaseMap, feed: &Feed) -> FeedRepresentativeRank {
    (
        map.confidence,
        has_non_blank_text(feed.description.as_ref()),
        has_non_blank_text(feed.image_url.as_ref()),
        i64::from(feed.oldest_item_at.is_some()),
        i64::from(feed.newest_item_at.is_some()),
        has_non_blank_text(feed.language.as_ref()),
        has_non_blank_text(feed.itunes_type.as_ref()),
        feed.newest_item_at.unwrap_or(i64::MIN),
        feed.updated_at.max(feed.created_at),
    )
}

fn track_representative_rank(
    map: &SourceItemRecordingMap,
    track: &Track,
    feed: &Feed,
) -> TrackRepresentativeRank {
    (
        map.confidence,
        has_non_blank_text(track.description.as_ref()),
        has_non_blank_text(track.enclosure_url.as_ref()),
        has_non_blank_text(track.enclosure_type.as_ref()),
        i64::from(track.enclosure_bytes.is_some()),
        i64::from(track.pub_date.is_some()),
        i64::from(track.duration_secs.is_some()),
        has_non_blank_text(feed.description.as_ref()),
        track.pub_date.unwrap_or(i64::MIN),
        track.updated_at.max(track.created_at).max(feed.updated_at),
    )
}

fn representative_feed_guid_for_release(
    conn: &Connection,
    release_id: &str,
) -> Result<Option<String>, DbError> {
    let maps = get_source_feed_release_maps_for_release(conn, release_id)?;
    let mut best: Option<(String, FeedRepresentativeRank)> = None;

    for map in maps {
        let Some(feed) = get_feed_by_guid(conn, &map.feed_guid)? else {
            continue;
        };
        let rank = feed_representative_rank(&map, &feed);
        match &best {
            Some((best_guid, best_rank))
                if rank < *best_rank || (rank == *best_rank && map.feed_guid > *best_guid) => {}
            _ => best = Some((map.feed_guid, rank)),
        }
    }

    Ok(best.map(|(feed_guid, _)| feed_guid))
}

fn representative_track_guid_for_recording(
    conn: &Connection,
    recording_id: &str,
) -> Result<Option<String>, DbError> {
    let maps = get_source_item_recording_maps_for_recording(conn, recording_id)?;
    let mut best: Option<(String, TrackRepresentativeRank)> = None;

    for map in maps {
        let Some(track) = get_track_by_guid(conn, &map.track_guid)? else {
            continue;
        };
        let Some(feed) = get_feed_by_guid(conn, &track.feed_guid)? else {
            continue;
        };
        let rank = track_representative_rank(&map, &track, &feed);
        match &best {
            Some((best_guid, best_rank))
                if rank < *best_rank || (rank == *best_rank && map.track_guid > *best_guid) => {}
            _ => best = Some((map.track_guid, rank)),
        }
    }

    Ok(best.map(|(track_guid, _)| track_guid))
}

fn rebuild_canonical_recording(conn: &Connection, recording_id: &str) -> Result<(), DbError> {
    let Some(track_guid) = representative_track_guid_for_recording(conn, recording_id)? else {
        conn.execute(
            "DELETE FROM recordings WHERE recording_id = ?1",
            params![recording_id],
        )?;
        return Ok(());
    };
    let Some(track) = get_track_by_guid(conn, &track_guid)? else {
        return Ok(());
    };

    conn.execute(
        "INSERT INTO recordings \
         (recording_id, title, title_lower, artist_credit_id, duration_secs, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(recording_id) DO UPDATE SET \
           title = excluded.title, \
           title_lower = excluded.title_lower, \
           artist_credit_id = excluded.artist_credit_id, \
           duration_secs = excluded.duration_secs, \
           updated_at = excluded.updated_at",
        params![
            recording_id,
            track.title,
            track.title_lower,
            track.artist_credit_id,
            track.duration_secs,
            track.created_at,
            track.updated_at,
        ],
    )?;
    Ok(())
}

fn rebuild_canonical_release(conn: &Connection, release_id: &str) -> Result<(), DbError> {
    let Some(feed_guid) = representative_feed_guid_for_release(conn, release_id)? else {
        conn.execute(
            "DELETE FROM release_recordings WHERE release_id = ?1",
            params![release_id],
        )?;
        conn.execute(
            "DELETE FROM releases WHERE release_id = ?1",
            params![release_id],
        )?;
        return Ok(());
    };
    let Some(feed) = get_feed_by_guid(conn, &feed_guid)? else {
        return Ok(());
    };

    conn.execute(
        "INSERT INTO releases \
         (release_id, title, title_lower, artist_credit_id, description, image_url, release_date, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(release_id) DO UPDATE SET \
           title = excluded.title, \
           title_lower = excluded.title_lower, \
           artist_credit_id = excluded.artist_credit_id, \
           description = excluded.description, \
           image_url = excluded.image_url, \
           release_date = excluded.release_date, \
           updated_at = excluded.updated_at",
        params![
            release_id,
            feed.title,
            feed.title_lower,
            feed.artist_credit_id,
            feed.description,
            feed.image_url,
            feed.oldest_item_at,
            feed.created_at,
            feed.updated_at,
        ],
    )?;

    conn.execute(
        "DELETE FROM release_recordings WHERE release_id = ?1",
        params![release_id],
    )?;

    let mut tracks = get_tracks_for_feed(conn, &feed_guid)?;
    tracks.sort_by(track_release_sort_key);
    for (idx, track) in tracks.iter().enumerate() {
        let recording_id: Option<String> = conn
            .query_row(
                "SELECT recording_id FROM source_item_recording_map WHERE track_guid = ?1",
                params![track.track_guid],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(recording_id) = recording_id {
            let position = i64::try_from(idx)
                .map_err(|_err| DbError::Other("release track position overflow".to_string()))?
                + 1;
            conn.execute(
                "INSERT INTO release_recordings (release_id, recording_id, position, source_track_guid) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![release_id, recording_id, position, track.track_guid],
            )?;
        }
    }
    Ok(())
}

fn cleanup_orphaned_canonical_rows(conn: &Connection) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM source_item_recording_map \
         WHERE track_guid NOT IN (SELECT track_guid FROM tracks)",
        [],
    )?;
    conn.execute(
        "DELETE FROM source_feed_release_map \
         WHERE feed_guid NOT IN (SELECT feed_guid FROM feeds)",
        [],
    )?;
    conn.execute(
        "DELETE FROM release_recordings \
         WHERE source_track_guid IS NOT NULL \
           AND source_track_guid NOT IN (SELECT track_guid FROM tracks)",
        [],
    )?;
    conn.execute(
        "DELETE FROM release_recordings \
         WHERE recording_id NOT IN (SELECT recording_id FROM source_item_recording_map)",
        [],
    )?;
    conn.execute(
        "DELETE FROM release_recordings \
         WHERE release_id NOT IN (SELECT release_id FROM source_feed_release_map)",
        [],
    )?;
    conn.execute(
        "DELETE FROM recordings \
         WHERE recording_id NOT IN (SELECT recording_id FROM source_item_recording_map)",
        [],
    )?;
    conn.execute(
        "DELETE FROM releases \
         WHERE release_id NOT IN (SELECT release_id FROM source_feed_release_map)",
        [],
    )?;
    conn.execute(
        "DELETE FROM entity_source \
         WHERE entity_type = 'recording' \
           AND entity_id NOT IN (SELECT recording_id FROM recordings)",
        [],
    )?;
    conn.execute(
        "DELETE FROM entity_source \
         WHERE entity_type = 'release' \
           AND entity_id NOT IN (SELECT release_id FROM releases)",
        [],
    )?;
    Ok(())
}

/// Rebuilds deterministic canonical release/recording rows for a source feed.
///
/// Current policy clusters only exact source matches:
/// - releases by artist evidence + release title + exact ordered tracklist
/// - recordings by artist evidence + track title + exact duration
///
/// When the evidence is incomplete, the resolver falls back to a 1:1 identity
/// mapping so no source data becomes unreachable.
pub fn sync_canonical_state_for_feed(conn: &Connection, feed_guid: &str) -> Result<(), DbError> {
    let Some(feed) = get_feed_by_guid(conn, feed_guid)? else {
        cleanup_orphaned_canonical_rows(conn)?;
        return Ok(());
    };

    let mut tracks = get_tracks_for_feed(conn, feed_guid)?;
    tracks.sort_by(track_release_sort_key);
    let previous_release_id: Option<String> = conn
        .query_row(
            "SELECT release_id FROM source_feed_release_map WHERE feed_guid = ?1",
            params![feed.feed_guid],
            |row| row.get(0),
        )
        .optional()?;
    let (release_id, release_match_type, release_confidence) =
        release_cluster_target(conn, &feed, &tracks)?;
    ensure_release_row(conn, &release_id, &feed)?;
    conn.execute(
        "INSERT INTO source_feed_release_map (feed_guid, release_id, match_type, confidence, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(feed_guid) DO UPDATE SET \
           release_id = excluded.release_id, \
           match_type = excluded.match_type, \
           confidence = excluded.confidence",
        params![
            feed.feed_guid,
            release_id,
            release_match_type,
            release_confidence,
            feed.updated_at
        ],
    )?;

    let mut affected_recording_ids = std::collections::BTreeSet::new();
    for track in &tracks {
        let previous_recording_id: Option<String> = conn
            .query_row(
                "SELECT recording_id FROM source_item_recording_map WHERE track_guid = ?1",
                params![track.track_guid],
                |row| row.get(0),
            )
            .optional()?;
        let (recording_id, recording_match_type, recording_confidence) =
            recording_cluster_target(conn, &feed, track)?;
        ensure_recording_row(conn, &recording_id, track)?;
        conn.execute(
            "INSERT INTO source_item_recording_map \
             (track_guid, recording_id, match_type, confidence, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(track_guid) DO UPDATE SET \
               recording_id = excluded.recording_id, \
               match_type = excluded.match_type, \
               confidence = excluded.confidence",
            params![
                track.track_guid,
                recording_id,
                recording_match_type,
                recording_confidence,
                track.updated_at
            ],
        )?;
        if let Some(previous_recording_id) = previous_recording_id {
            affected_recording_ids.insert(previous_recording_id);
        }
        affected_recording_ids.insert(recording_id);
    }

    let mut affected_release_ids = std::collections::BTreeSet::new();
    if let Some(previous_release_id) = previous_release_id {
        affected_release_ids.insert(previous_release_id);
    }
    affected_release_ids.insert(release_id.clone());

    for release_id in &affected_release_ids {
        rebuild_canonical_release(conn, release_id)?;
    }
    for recording_id in &affected_recording_ids {
        rebuild_canonical_recording(conn, recording_id)?;
    }

    cleanup_orphaned_canonical_rows(conn)?;
    Ok(())
}

/// Returns metadata for the primary-resolved source read-model completion
/// currently attributed to `feed_guid`.
pub fn build_source_feed_read_models_resolved_payload(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Option<crate::event::SourceFeedReadModelsResolvedPayload>, DbError> {
    let feed_exists = get_feed_by_guid(conn, feed_guid)?.is_some();
    if !feed_exists {
        return Ok(None);
    }

    let track_rows: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tracks WHERE feed_guid = ?1",
        params![feed_guid],
        |row| row.get(0),
    )?;
    let artist_rows = count_source_artist_rows_for_feed(conn, feed_guid)?;

    Ok(Some(crate::event::SourceFeedReadModelsResolvedPayload {
        feed_guid: feed_guid.to_string(),
        feed_rows: 1,
        track_rows: usize::try_from(track_rows).map_err(|err| {
            DbError::Other(format!("track row count overflow for {feed_guid}: {err}"))
        })?,
        artist_rows,
    }))
}

/// Emits a signed feed-scoped source read-model completion event after
/// primary-side resolver work has converged for `feed_guid`.
pub fn emit_source_feed_read_models_event(
    conn: &Connection,
    feed_guid: &str,
    signer: &NodeSigner,
) -> Result<Option<Event>, DbError> {
    let Some(payload) = build_source_feed_read_models_resolved_payload(conn, feed_guid)? else {
        return Ok(None);
    };

    let payload_json = serde_json::to_string(&payload)?;
    let event_id = uuid::Uuid::new_v4().to_string();
    let created_at = unix_now();
    let (seq, signed_by, signature) = insert_event(
        conn,
        &event_id,
        &EventType::SourceFeedReadModelsResolved,
        &payload_json,
        feed_guid,
        signer,
        created_at,
        &[],
    )?;

    Ok(Some(Event {
        event_id,
        event_type: EventType::SourceFeedReadModelsResolved,
        payload: EventPayload::SourceFeedReadModelsResolved(payload),
        subject_guid: feed_guid.to_string(),
        signed_by,
        signature,
        seq,
        created_at,
        warnings: Vec::new(),
        payload_json,
    }))
}

/// Returns the primary-resolved canonical-state snapshot currently mapped from
/// `feed_guid`.
pub fn build_canonical_feed_state_snapshot(
    conn: &Connection,
    feed_guid: &str,
) -> Result<crate::event::CanonicalFeedStateReplacedPayload, DbError> {
    let release_maps = get_source_feed_release_maps_for_feed(conn, feed_guid)?;
    let mut releases = Vec::with_capacity(release_maps.len());
    let mut release_recordings = Vec::new();
    for map in &release_maps {
        if let Some(release) = get_release(conn, &map.release_id)? {
            releases.push(release);
        }
        release_recordings.extend(get_release_recordings(conn, &map.release_id)?);
    }

    let recording_maps = get_source_item_recording_maps_for_feed(conn, feed_guid)?;
    let mut recordings = Vec::with_capacity(recording_maps.len());
    for map in &recording_maps {
        if let Some(recording) = get_recording(conn, &map.recording_id)? {
            recordings.push(recording);
        }
    }

    Ok(crate::event::CanonicalFeedStateReplacedPayload {
        feed_guid: feed_guid.to_string(),
        releases,
        recordings,
        release_recordings,
        release_maps,
        recording_maps,
    })
}

/// Replaces feed-scoped canonical release/recording state from a primary-owned
/// resolved snapshot.
pub fn replace_canonical_feed_state_from_snapshot(
    conn: &Connection,
    payload: &crate::event::CanonicalFeedStateReplacedPayload,
) -> Result<(), DbError> {
    let previous_release_ids: Vec<String> = conn
        .prepare(
            "SELECT release_id FROM source_feed_release_map
             WHERE feed_guid = ?1
             ORDER BY release_id",
        )?
        .query_map(params![payload.feed_guid], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    let previous_recording_ids: Vec<String> = conn
        .prepare(
            "SELECT sirm.recording_id
             FROM source_item_recording_map sirm
             JOIN tracks t ON t.track_guid = sirm.track_guid
             WHERE t.feed_guid = ?1
             ORDER BY sirm.recording_id",
        )?
        .query_map(params![payload.feed_guid], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    for release in &payload.releases {
        upsert_release_row(conn, release)?;
    }
    for recording in &payload.recordings {
        upsert_recording_row(conn, recording)?;
    }

    conn.execute(
        "DELETE FROM source_feed_release_map WHERE feed_guid = ?1",
        params![payload.feed_guid],
    )?;
    for map in &payload.release_maps {
        conn.execute(
            "INSERT INTO source_feed_release_map (feed_guid, release_id, match_type, confidence, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                map.feed_guid,
                map.release_id,
                map.match_type,
                map.confidence,
                map.created_at
            ],
        )?;
    }

    conn.execute(
        "DELETE FROM source_item_recording_map
         WHERE track_guid IN (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![payload.feed_guid],
    )?;
    for map in &payload.recording_maps {
        conn.execute(
            "INSERT INTO source_item_recording_map (track_guid, recording_id, match_type, confidence, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                map.track_guid,
                map.recording_id,
                map.match_type,
                map.confidence,
                map.created_at
            ],
        )?;
    }

    let mut affected_release_ids: std::collections::BTreeSet<String> =
        previous_release_ids.into_iter().collect();
    affected_release_ids.extend(
        payload
            .release_maps
            .iter()
            .map(|map| map.release_id.clone()),
    );
    for release_id in &affected_release_ids {
        conn.execute(
            "DELETE FROM release_recordings WHERE release_id = ?1",
            params![release_id],
        )?;
    }
    for row in &payload.release_recordings {
        conn.execute(
            "INSERT INTO release_recordings (release_id, recording_id, position, source_track_guid)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                row.release_id,
                row.recording_id,
                row.position,
                row.source_track_guid
            ],
        )?;
    }

    let mut affected_recording_ids: std::collections::BTreeSet<String> =
        previous_recording_ids.into_iter().collect();
    affected_recording_ids.extend(
        payload
            .recording_maps
            .iter()
            .map(|map| map.recording_id.clone()),
    );
    for recording_id in &affected_recording_ids {
        if payload
            .recordings
            .iter()
            .any(|recording| &recording.recording_id == recording_id)
        {
            continue;
        }
        conn.execute(
            "DELETE FROM recordings WHERE recording_id = ?1",
            params![recording_id],
        )?;
    }
    for release_id in &affected_release_ids {
        if payload
            .releases
            .iter()
            .any(|release| &release.release_id == release_id)
        {
            continue;
        }
        conn.execute(
            "DELETE FROM releases WHERE release_id = ?1",
            params![release_id],
        )?;
    }

    cleanup_orphaned_canonical_rows(conn)?;
    Ok(())
}

/// Emits a signed feed-scoped canonical-state snapshot event after primary-side
/// resolver work has converged for `feed_guid`.
pub fn emit_canonical_feed_state_event(
    conn: &Connection,
    feed_guid: &str,
    signer: &NodeSigner,
) -> Result<Option<Event>, DbError> {
    let payload = build_canonical_feed_state_snapshot(conn, feed_guid)?;
    if payload.release_maps.is_empty() && payload.recording_maps.is_empty() {
        return Ok(None);
    }

    let payload_json = serde_json::to_string(&payload)?;
    let event_id = uuid::Uuid::new_v4().to_string();
    let created_at = unix_now();
    let (seq, signed_by, signature) = insert_event(
        conn,
        &event_id,
        &EventType::CanonicalFeedStateReplaced,
        &payload_json,
        feed_guid,
        signer,
        created_at,
        &[],
    )?;

    Ok(Some(Event {
        event_id,
        event_type: EventType::CanonicalFeedStateReplaced,
        payload: EventPayload::CanonicalFeedStateReplaced(payload),
        subject_guid: feed_guid.to_string(),
        signed_by,
        signature,
        seq,
        created_at,
        warnings: Vec::new(),
        payload_json,
    }))
}

/// Returns the primary-resolved promotions snapshot currently attributed to
/// `feed_guid`.
pub fn build_canonical_feed_promotions_snapshot(
    conn: &Connection,
    feed_guid: &str,
) -> Result<crate::event::CanonicalFeedPromotionsReplacedPayload, DbError> {
    Ok(crate::event::CanonicalFeedPromotionsReplacedPayload {
        feed_guid: feed_guid.to_string(),
        external_ids: get_resolved_external_ids_for_feed(conn, feed_guid)?,
        entity_sources: get_resolved_entity_sources_for_feed(conn, feed_guid)?,
    })
}

/// Replaces feed-scoped promoted IDs and provenance from a primary-owned
/// resolved snapshot.
pub fn replace_canonical_feed_promotions_from_snapshot(
    conn: &Connection,
    payload: &crate::event::CanonicalFeedPromotionsReplacedPayload,
) -> Result<(), DbError> {
    replace_materialized_canonical_promotions_for_feed(
        conn,
        &payload.feed_guid,
        &payload.external_ids,
        &payload.entity_sources,
    )
}

/// Emits a signed feed-scoped promotions snapshot event after primary-side
/// resolver work has converged for `feed_guid`.
pub fn emit_canonical_feed_promotions_event(
    conn: &Connection,
    feed_guid: &str,
    signer: &NodeSigner,
) -> Result<Option<Event>, DbError> {
    let payload = build_canonical_feed_promotions_snapshot(conn, feed_guid)?;
    if payload.external_ids.is_empty() && payload.entity_sources.is_empty() {
        return Ok(None);
    }

    let payload_json = serde_json::to_string(&payload)?;
    let event_id = uuid::Uuid::new_v4().to_string();
    let created_at = unix_now();
    let (seq, signed_by, signature) = insert_event(
        conn,
        &event_id,
        &EventType::CanonicalFeedPromotionsReplaced,
        &payload_json,
        feed_guid,
        signer,
        created_at,
        &[],
    )?;

    Ok(Some(Event {
        event_id,
        event_type: EventType::CanonicalFeedPromotionsReplaced,
        payload: EventPayload::CanonicalFeedPromotionsReplaced(payload),
        subject_guid: feed_guid.to_string(),
        signed_by,
        signature,
        seq,
        created_at,
        warnings: Vec::new(),
        payload_json,
    }))
}

fn single_artist_id_for_credit(
    conn: &Connection,
    artist_credit_id: i64,
) -> Result<Option<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT artist_id FROM artist_credit_name \
         WHERE artist_credit_id = ?1 ORDER BY position",
    )?;
    let artist_ids: Vec<String> = stmt
        .query_map(params![artist_credit_id], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    if artist_ids.len() == 1 {
        Ok(artist_ids.into_iter().next())
    } else {
        Ok(None)
    }
}

fn delete_promoted_entity_sources(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM entity_source \
         WHERE entity_type = ?1 AND entity_id = ?2 \
           AND source_type IN ('source_feed', 'source_release_page', 'source_recording_page', 'source_primary_enclosure')",
        params![entity_type, entity_id],
    )?;
    Ok(())
}

fn record_promoted_entity_source(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    source_type: &str,
    source_url: &str,
) -> Result<(), DbError> {
    record_entity_source(
        conn,
        entity_type,
        entity_id,
        source_type,
        Some(source_url),
        1,
    )?;
    Ok(())
}

fn release_id_for_feed_map(conn: &Connection, feed_guid: &str) -> Result<Option<String>, DbError> {
    conn.query_row(
        "SELECT release_id FROM source_feed_release_map WHERE feed_guid = ?1",
        params![feed_guid],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn rebuild_release_sources(conn: &Connection, release_id: &str) -> Result<(), DbError> {
    delete_promoted_entity_sources(conn, "release", release_id)?;
    let mut stmt = conn.prepare(
        "SELECT f.feed_url, f.feed_guid \
         FROM source_feed_release_map sfr
         JOIN feeds f ON f.feed_guid = sfr.feed_guid \
         WHERE sfr.release_id = ?1
         ORDER BY f.feed_guid",
    )?;
    let mapped_feeds: Vec<(String, String)> = stmt
        .query_map(params![release_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;

    let mut seen = std::collections::HashSet::new();
    for (feed_url, feed_guid) in mapped_feeds {
        if seen.insert(feed_url.clone()) {
            record_promoted_entity_source(conn, "release", release_id, "source_feed", &feed_url)?;
        }
        let mut link_stmt = conn.prepare(
            "SELECT DISTINCT url FROM source_entity_links \
             WHERE feed_guid = ?1 AND entity_type = 'feed' AND entity_id = ?1 AND link_type = 'website' \
             ORDER BY position, url",
        )?;
        let urls: Vec<String> = link_stmt
            .query_map(params![feed_guid], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        for url in urls {
            if seen.insert(url.clone()) {
                record_promoted_entity_source(
                    conn,
                    "release",
                    release_id,
                    "source_release_page",
                    &url,
                )?;
            }
        }
    }
    Ok(())
}

fn rebuild_recording_sources(conn: &Connection, recording_id: &str) -> Result<(), DbError> {
    delete_promoted_entity_sources(conn, "recording", recording_id)?;
    let mut seen = std::collections::HashSet::new();
    let mut track_stmt = conn.prepare(
        "SELECT t.track_guid, t.feed_guid, t.enclosure_url FROM source_item_recording_map sirm
         JOIN tracks t ON t.track_guid = sirm.track_guid
         WHERE sirm.recording_id = ?1
         ORDER BY t.track_guid",
    )?;
    let mapped_tracks: Vec<(String, String, Option<String>)> = track_stmt
        .query_map(params![recording_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<_, _>>()?;

    for (track_guid, feed_guid, enclosure_url) in mapped_tracks {
        let mut enclosure_stmt = conn.prepare(
            "SELECT DISTINCT url FROM source_item_enclosures \
             WHERE feed_guid = ?1 AND entity_type = 'track' AND entity_id = ?2 AND is_primary = 1 \
             ORDER BY position, url",
        )?;
        let enclosure_urls: Vec<String> = enclosure_stmt
            .query_map(params![feed_guid, track_guid], |row| row.get(0))?
            .collect::<Result<_, _>>()?;

        if enclosure_urls.is_empty() {
            if let Some(url) = enclosure_url {
                seen.insert(url.clone());
                record_promoted_entity_source(
                    conn,
                    "recording",
                    recording_id,
                    "source_primary_enclosure",
                    &url,
                )?;
            }
        } else {
            for url in enclosure_urls {
                if seen.insert(url.clone()) {
                    record_promoted_entity_source(
                        conn,
                        "recording",
                        recording_id,
                        "source_primary_enclosure",
                        &url,
                    )?;
                }
            }
        }

        let mut link_stmt = conn.prepare(
            "SELECT DISTINCT url FROM source_entity_links \
             WHERE feed_guid = ?1 AND entity_type = 'track' AND entity_id = ?2 AND link_type = 'web_page' \
             ORDER BY position, url",
        )?;
        let link_urls: Vec<String> = link_stmt
            .query_map(params![feed_guid, track_guid], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        for url in link_urls {
            if seen.insert(url.clone()) {
                record_promoted_entity_source(
                    conn,
                    "recording",
                    recording_id,
                    "source_recording_page",
                    &url,
                )?;
            }
        }
    }
    Ok(())
}

/// Promotes a narrow set of high-confidence source claims onto canonical rows.
///
/// Current promotion policy is intentionally conservative:
/// - a feed-level `nostr_npub` is promoted only when the feed resolves to a
///   single canonical artist and there is no conflicting owner for that npub
/// - release and recording provenance is promoted into `entity_source` rows so
///   canonical entities retain stable page/feed/media URLs without flattening
///   the underlying source claim tables
pub fn sync_canonical_promotions_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<(), DbError> {
    let Some(feed) = get_feed_by_guid(conn, feed_guid)? else {
        cleanup_orphaned_canonical_rows(conn)?;
        return Ok(());
    };

    let external_ids = collect_high_confidence_artist_external_ids_for_feed(conn, &feed)?;
    let mut entity_sources = collect_release_source_overlays_for_feed(conn, &feed)?;
    entity_sources.extend(collect_recording_source_overlays_for_feed(conn, &feed)?);
    replace_materialized_canonical_promotions_for_feed(
        conn,
        feed_guid,
        &external_ids,
        &entity_sources,
    )?;
    Ok(())
}

fn cleanup_canonical_search_entities(conn: &Connection) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM search_entities \
         WHERE entity_type = 'release' \
           AND entity_id NOT IN (SELECT release_id FROM releases)",
        [],
    )?;
    conn.execute(
        "DELETE FROM search_entities \
         WHERE entity_type = 'recording' \
           AND entity_id NOT IN (SELECT recording_id FROM recordings)",
        [],
    )?;
    Ok(())
}

/// Updates canonical search index rows for the release/recording objects
/// currently mapped from one feed.
///
/// Search stays canonical-first at the API layer. Source `feed`/`track`
/// search rows are maintained separately via [`sync_source_read_models_for_feed`].
pub fn sync_canonical_search_index_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<(), DbError> {
    cleanup_canonical_search_entities(conn)?;

    if let Some(release_id) = release_id_for_feed_map(conn, feed_guid)?
        && let Some(release) = get_release(conn, &release_id)?
    {
        crate::search::populate_search_index(
            conn,
            "release",
            &release.release_id,
            "",
            &release.title,
            release.description.as_deref().unwrap_or(""),
            "",
        )?;
    }

    let mut stmt = conn.prepare(
        "SELECT DISTINCT sirm.recording_id \
         FROM source_item_recording_map sirm \
         JOIN tracks t ON t.track_guid = sirm.track_guid \
         WHERE t.feed_guid = ?1 \
         ORDER BY sirm.recording_id",
    )?;
    let recording_ids: Vec<String> = stmt
        .query_map(params![feed_guid], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for recording_id in recording_ids {
        if let Some(recording) = get_recording(conn, &recording_id)? {
            crate::search::populate_search_index(
                conn,
                "recording",
                &recording.recording_id,
                "",
                &recording.title,
                "",
                "",
            )?;
        }
    }

    Ok(())
}

/// Rebuilds source-layer `feed`/`track` search rows and quality scores for one
/// feed without touching canonical tables.
pub fn sync_source_read_models_for_feed(conn: &Connection, feed_guid: &str) -> Result<(), DbError> {
    let Some(feed) = get_feed_by_guid(conn, feed_guid)? else {
        return Ok(());
    };

    crate::search::populate_search_index(
        conn,
        "feed",
        &feed.feed_guid,
        "",
        &feed.title,
        feed.description.as_deref().unwrap_or(""),
        feed.raw_medium.as_deref().unwrap_or(""),
    )?;
    let feed_score = crate::quality::compute_feed_quality(conn, &feed.feed_guid)?;
    crate::quality::store_quality(conn, "feed", &feed.feed_guid, feed_score)?;

    for track in get_tracks_for_feed(conn, feed_guid)? {
        crate::search::populate_search_index(
            conn,
            "track",
            &track.track_guid,
            "",
            &track.title,
            track.description.as_deref().unwrap_or(""),
            "",
        )?;
        let track_score = crate::quality::compute_track_quality(conn, &track.track_guid)?;
        crate::quality::store_quality(conn, "track", &track.track_guid, track_score)?;
    }

    let mut stmt = conn.prepare(
        "SELECT DISTINCT a.artist_id, a.name \
         FROM artists a \
         JOIN artist_credit_name acn ON acn.artist_id = a.artist_id \
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id \
         JOIN feeds f ON f.artist_credit_id = ac.id \
         WHERE f.feed_guid = ?1 \
         UNION \
         SELECT DISTINCT a.artist_id, a.name \
         FROM artists a \
         JOIN artist_credit_name acn ON acn.artist_id = a.artist_id \
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id \
         JOIN tracks t ON t.artist_credit_id = ac.id \
         WHERE t.feed_guid = ?1",
    )?;
    let artists: Vec<(String, String)> = stmt
        .query_map(params![feed_guid], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (artist_id, artist_name) in artists {
        crate::search::populate_search_index(conn, "artist", &artist_id, &artist_name, "", "", "")?;
        let artist_score = crate::quality::compute_artist_quality(conn, &artist_id)?;
        crate::quality::store_quality(conn, "artist", &artist_id, artist_score)?;
    }

    Ok(())
}

fn count_source_artist_rows_for_feed(conn: &Connection, feed_guid: &str) -> Result<usize, DbError> {
    let artist_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM (
             SELECT DISTINCT a.artist_id
             FROM artists a
             JOIN artist_credit_name acn ON acn.artist_id = a.artist_id
             JOIN artist_credit ac ON ac.id = acn.artist_credit_id
             JOIN feeds f ON f.artist_credit_id = ac.id
             WHERE f.feed_guid = ?1
             UNION
             SELECT DISTINCT a.artist_id
             FROM artists a
             JOIN artist_credit_name acn ON acn.artist_id = a.artist_id
             JOIN artist_credit ac ON ac.id = acn.artist_credit_id
             JOIN tracks t ON t.artist_credit_id = ac.id
             WHERE t.feed_guid = ?1
         )",
        params![feed_guid],
        |row| row.get(0),
    )?;
    usize::try_from(artist_count)
        .map_err(|err| DbError::Other(format!("artist row count overflow for {feed_guid}: {err}")))
}

// ── replace_payment_routes ────────────────────────────────────────────────────

/// Deletes all payment routes for `track_guid` and inserts the new `routes`.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL delete/insert or JSON serialisation fails.
pub fn replace_payment_routes(
    conn: &Connection,
    track_guid: &str,
    routes: &[PaymentRoute],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM payment_routes WHERE track_guid = ?1",
        params![track_guid],
    )?;
    for r in routes {
        let route_type = serde_json::to_string(&r.route_type)?;
        let route_type = route_type.trim_matches('"');
        conn.execute(
            "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, \
             custom_key, custom_value, split, fee) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                r.track_guid,
                r.feed_guid,
                r.recipient_name,
                route_type,
                r.address,
                r.custom_key,
                r.custom_value,
                r.split,
                i64::from(r.fee),
            ],
        )?;
    }
    Ok(())
}

// ── replace_feed_payment_routes ─────────────────────────────────────────────

/// Deletes all feed-level payment routes for `feed_guid` and inserts `routes`.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL delete/insert or JSON serialisation fails.
pub fn replace_feed_payment_routes(
    conn: &Connection,
    feed_guid: &str,
    routes: &[FeedPaymentRoute],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM feed_payment_routes WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for r in routes {
        let route_type = serde_json::to_string(&r.route_type)?;
        let route_type = route_type.trim_matches('"');
        conn.execute(
            "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, \
             custom_key, custom_value, split, fee) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                r.feed_guid,
                r.recipient_name,
                route_type,
                r.address,
                r.custom_key,
                r.custom_value,
                r.split,
                i64::from(r.fee),
            ],
        )?;
    }
    Ok(())
}

// ── replace_value_time_splits ─────────────────────────────────────────────────

/// Deletes all value-time splits for `source_track_guid` and inserts `splits`.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL delete or insert fails.
pub fn replace_value_time_splits(
    conn: &Connection,
    source_track_guid: &str,
    splits: &[ValueTimeSplit],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM value_time_splits WHERE source_track_guid = ?1",
        params![source_track_guid],
    )?;
    for s in splits {
        conn.execute(
            "INSERT INTO value_time_splits (source_track_guid, start_time_secs, duration_secs, \
             remote_feed_guid, remote_item_guid, split, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                s.source_track_guid,
                s.start_time_secs,
                s.duration_secs,
                s.remote_feed_guid,
                s.remote_item_guid,
                s.split,
                s.created_at,
            ],
        )?;
    }
    Ok(())
}

// ── feed_remote_items_raw ───────────────────────────────────────────────────

/// Returns the raw feed-level `podcast:remoteItem` refs for a feed ordered by position.
pub fn get_feed_remote_items_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<FeedRemoteItemRaw>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, position, medium, remote_feed_guid, remote_feed_url, source \
         FROM feed_remote_items_raw WHERE feed_guid = ?1 ORDER BY position",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(FeedRemoteItemRaw {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            position: row.get(2)?,
            medium: row.get(3)?,
            remote_feed_guid: row.get(4)?,
            remote_feed_url: row.get(5)?,
            source: row.get(6)?,
        })
    })?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

/// Replaces the raw feed-level `podcast:remoteItem` refs for a feed.
pub fn replace_feed_remote_items_raw(
    conn: &Connection,
    feed_guid: &str,
    remote_items: &[FeedRemoteItemRaw],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM feed_remote_items_raw WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for item in remote_items {
        conn.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &item.feed_guid,
                item.position,
                &item.medium,
                &item.remote_feed_guid,
                &item.remote_feed_url,
                &item.source,
            ],
        )?;
    }
    Ok(())
}

// ── live_events ─────────────────────────────────────────────────────────────

/// Returns the current ephemeral live-event rows for a feed.
pub fn get_live_events_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<LiveEvent>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT live_item_guid, feed_guid, title, content_link, status, scheduled_start, \
         scheduled_end, created_at, updated_at \
         FROM live_events WHERE feed_guid = ?1 ORDER BY COALESCE(scheduled_start, created_at), live_item_guid",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(LiveEvent {
            live_item_guid: row.get(0)?,
            feed_guid: row.get(1)?,
            title: row.get(2)?,
            content_link: row.get(3)?,
            status: row.get(4)?,
            scheduled_start: row.get(5)?,
            scheduled_end: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })?;

    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    Ok(events)
}

/// Replaces the current ephemeral live-event rows for a feed.
pub fn replace_live_events_for_feed(
    conn: &Connection,
    feed_guid: &str,
    live_events: &[LiveEvent],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM live_events WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for live_event in live_events {
        conn.execute(
            "INSERT INTO live_events \
             (live_item_guid, feed_guid, title, content_link, status, scheduled_start, scheduled_end, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                &live_event.live_item_guid,
                &live_event.feed_guid,
                &live_event.title,
                &live_event.content_link,
                &live_event.status,
                live_event.scheduled_start,
                live_event.scheduled_end,
                live_event.created_at,
                live_event.updated_at,
            ],
        )?;
    }
    Ok(())
}

// ── source_contributor_claims ───────────────────────────────────────────────

/// Returns the staged contributor claims for a feed ordered by entity + position.
pub fn get_source_contributor_claims_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<SourceContributorClaim>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, name, role, role_norm, group_name, href, \
         img, source, extraction_path, observed_at \
         FROM source_contributor_claims WHERE feed_guid = ?1 \
         ORDER BY entity_type, entity_id, position, id",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(SourceContributorClaim {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            name: row.get(5)?,
            role: row.get(6)?,
            role_norm: row.get(7)?,
            group_name: row.get(8)?,
            href: row.get(9)?,
            img: row.get(10)?,
            source: row.get(11)?,
            extraction_path: row.get(12)?,
            observed_at: row.get(13)?,
        })
    })?;

    let mut claims = Vec::new();
    for row in rows {
        claims.push(row?);
    }
    Ok(claims)
}

/// Replaces the staged contributor claims for a feed.
pub fn replace_source_contributor_claims_for_feed(
    conn: &Connection,
    feed_guid: &str,
    claims: &[SourceContributorClaim],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM source_contributor_claims WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for claim in claims {
        conn.execute(
            "INSERT INTO source_contributor_claims \
             (feed_guid, entity_type, entity_id, position, name, role, role_norm, group_name, href, img, \
              source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                &claim.feed_guid,
                &claim.entity_type,
                &claim.entity_id,
                claim.position,
                &claim.name,
                &claim.role,
                &claim.role_norm,
                &claim.group_name,
                &claim.href,
                &claim.img,
                &claim.source,
                &claim.extraction_path,
                claim.observed_at,
            ],
        )?;
    }
    Ok(())
}

// ── source_entity_ids ───────────────────────────────────────────────────────

/// Returns the staged entity-ID claims for a feed ordered by entity + position.
pub fn get_source_entity_ids_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<SourceEntityIdClaim>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, scheme, value, source, \
         extraction_path, observed_at \
         FROM source_entity_ids WHERE feed_guid = ?1 \
         ORDER BY entity_type, entity_id, position, scheme, value, id",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(SourceEntityIdClaim {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            scheme: row.get(5)?,
            value: row.get(6)?,
            source: row.get(7)?,
            extraction_path: row.get(8)?,
            observed_at: row.get(9)?,
        })
    })?;

    let mut claims = Vec::new();
    for row in rows {
        claims.push(row?);
    }
    Ok(claims)
}

/// Replaces the staged entity-ID claims for a feed.
pub fn replace_source_entity_ids_for_feed(
    conn: &Connection,
    feed_guid: &str,
    claims: &[SourceEntityIdClaim],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM source_entity_ids WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for claim in claims {
        conn.execute(
            "INSERT INTO source_entity_ids \
             (feed_guid, entity_type, entity_id, position, scheme, value, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                &claim.feed_guid,
                &claim.entity_type,
                &claim.entity_id,
                claim.position,
                &claim.scheme,
                &claim.value,
                &claim.source,
                &claim.extraction_path,
                claim.observed_at,
            ],
        )?;
    }
    Ok(())
}

// ── source_entity_links ─────────────────────────────────────────────────────

/// Returns the staged entity-link claims for a feed ordered by entity + position.
pub fn get_source_entity_links_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<SourceEntityLink>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, link_type, url, source, \
         extraction_path, observed_at \
         FROM source_entity_links WHERE feed_guid = ?1 \
         ORDER BY entity_type, entity_id, position, link_type, url, id",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(SourceEntityLink {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            link_type: row.get(5)?,
            url: row.get(6)?,
            source: row.get(7)?,
            extraction_path: row.get(8)?,
            observed_at: row.get(9)?,
        })
    })?;

    let mut links = Vec::new();
    for row in rows {
        links.push(row?);
    }
    Ok(links)
}

/// Replaces the staged entity-link claims for a feed.
pub fn replace_source_entity_links_for_feed(
    conn: &Connection,
    feed_guid: &str,
    links: &[SourceEntityLink],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM source_entity_links WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for link in links {
        conn.execute(
            "INSERT INTO source_entity_links \
             (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                &link.feed_guid,
                &link.entity_type,
                &link.entity_id,
                link.position,
                &link.link_type,
                &link.url,
                &link.source,
                &link.extraction_path,
                link.observed_at,
            ],
        )?;
    }
    Ok(())
}

// ── source_release_claims ───────────────────────────────────────────────────

/// Returns the staged release claims for a feed ordered by entity + position.
pub fn get_source_release_claims_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<SourceReleaseClaim>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, claim_type, claim_value, source, \
         extraction_path, observed_at \
         FROM source_release_claims WHERE feed_guid = ?1 \
         ORDER BY entity_type, entity_id, claim_type, position, id",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(SourceReleaseClaim {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            claim_type: row.get(5)?,
            claim_value: row.get(6)?,
            source: row.get(7)?,
            extraction_path: row.get(8)?,
            observed_at: row.get(9)?,
        })
    })?;

    let mut claims = Vec::new();
    for row in rows {
        claims.push(row?);
    }
    Ok(claims)
}

/// Replaces the staged release claims for a feed.
pub fn replace_source_release_claims_for_feed(
    conn: &Connection,
    feed_guid: &str,
    claims: &[SourceReleaseClaim],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM source_release_claims WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for claim in claims {
        conn.execute(
            "INSERT INTO source_release_claims \
             (feed_guid, entity_type, entity_id, position, claim_type, claim_value, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                &claim.feed_guid,
                &claim.entity_type,
                &claim.entity_id,
                claim.position,
                &claim.claim_type,
                &claim.claim_value,
                &claim.source,
                &claim.extraction_path,
                claim.observed_at,
            ],
        )?;
    }
    Ok(())
}

// ── source_item_enclosures ──────────────────────────────────────────────────

/// Returns the staged item-enclosure rows for a feed ordered by entity + position.
pub fn get_source_item_enclosures_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<SourceItemEnclosure>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, url, mime_type, bytes, rel, \
         title, is_primary, source, extraction_path, observed_at \
         FROM source_item_enclosures WHERE feed_guid = ?1 \
         ORDER BY entity_type, entity_id, position, url, id",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(SourceItemEnclosure {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            url: row.get(5)?,
            mime_type: row.get(6)?,
            bytes: row.get(7)?,
            rel: row.get(8)?,
            title: row.get(9)?,
            is_primary: row.get(10)?,
            source: row.get(11)?,
            extraction_path: row.get(12)?,
            observed_at: row.get(13)?,
        })
    })?;

    let mut enclosures = Vec::new();
    for row in rows {
        enclosures.push(row?);
    }
    Ok(enclosures)
}

/// Replaces the staged item-enclosure rows for a feed.
pub fn replace_source_item_enclosures_for_feed(
    conn: &Connection,
    feed_guid: &str,
    enclosures: &[SourceItemEnclosure],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM source_item_enclosures WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for enclosure in enclosures {
        conn.execute(
            "INSERT INTO source_item_enclosures \
             (feed_guid, entity_type, entity_id, position, url, mime_type, bytes, rel, title, is_primary, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                &enclosure.feed_guid,
                &enclosure.entity_type,
                &enclosure.entity_id,
                enclosure.position,
                &enclosure.url,
                &enclosure.mime_type,
                enclosure.bytes,
                &enclosure.rel,
                &enclosure.title,
                enclosure.is_primary,
                &enclosure.source,
                &enclosure.extraction_path,
                enclosure.observed_at,
            ],
        )?;
    }
    Ok(())
}

// ── source_platform_claims ──────────────────────────────────────────────────

/// Returns the staged platform claims for a feed.
pub fn get_source_platform_claims_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<SourcePlatformClaim>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at \
         FROM source_platform_claims WHERE feed_guid = ?1 \
         ORDER BY platform_key, extraction_path, url, owner_name, id",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(SourcePlatformClaim {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            platform_key: row.get(2)?,
            url: row.get(3)?,
            owner_name: row.get(4)?,
            source: row.get(5)?,
            extraction_path: row.get(6)?,
            observed_at: row.get(7)?,
        })
    })?;

    let mut claims = Vec::new();
    for row in rows {
        claims.push(row?);
    }
    Ok(claims)
}

/// Replaces the staged platform claims for a feed.
pub fn replace_source_platform_claims_for_feed(
    conn: &Connection,
    feed_guid: &str,
    claims: &[SourcePlatformClaim],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM source_platform_claims WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for claim in claims {
        conn.execute(
            "INSERT INTO source_platform_claims \
             (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &claim.feed_guid,
                &claim.platform_key,
                &claim.url,
                &claim.owner_name,
                &claim.source,
                &claim.extraction_path,
                claim.observed_at,
            ],
        )?;
    }
    Ok(())
}

// ── delete_track ────────────────────────────────────────────────────────────

/// Cascade-deletes a track and all child rows, respecting FK constraints.
///
/// Deletes in order: `track_tag`, `value_time_splits`, `payment_routes`,
/// `entity_quality`, `entity_field_status`, then the `tracks` row itself.
///
/// Idempotent: calling with a non-existent `track_guid` is a no-op.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL delete fails.
pub fn delete_track(conn: &mut Connection, track_guid: &str) -> Result<(), DbError> {
    let sp = conn.savepoint()?;
    delete_track_sql(&sp, track_guid)?;
    sp.commit()?;
    Ok(())
}

/// Inner implementation of track cascade-delete: executes all SQL deletes on
/// the provided connection without managing its own transaction.  Callers
/// must ensure they are already inside a transaction or savepoint.
pub(crate) fn delete_track_sql(conn: &Connection, track_guid: &str) -> Result<(), DbError> {
    let recording_id: Option<String> = conn
        .query_row(
            "SELECT recording_id FROM source_item_recording_map WHERE track_guid = ?1",
            params![track_guid],
            |row| row.get(0),
        )
        .optional()?;
    conn.execute(
        "DELETE FROM source_item_recording_map WHERE track_guid = ?1",
        params![track_guid],
    )?;
    if let Some(recording_id) = &recording_id {
        conn.execute(
            "DELETE FROM release_recordings WHERE source_track_guid = ?1 OR recording_id = ?2",
            params![track_guid, recording_id],
        )?;
        rebuild_canonical_recording(conn, recording_id)?;
        rebuild_recording_sources(conn, recording_id)?;
    } else {
        conn.execute(
            "DELETE FROM release_recordings WHERE source_track_guid = ?1",
            params![track_guid],
        )?;
    }
    conn.execute(
        "DELETE FROM track_tag WHERE track_guid = ?1",
        params![track_guid],
    )?;
    conn.execute(
        "DELETE FROM value_time_splits WHERE source_track_guid = ?1",
        params![track_guid],
    )?;
    conn.execute(
        "DELETE FROM payment_routes WHERE track_guid = ?1",
        params![track_guid],
    )?;
    conn.execute(
        "DELETE FROM entity_quality WHERE entity_type = 'track' AND entity_id = ?1",
        params![track_guid],
    )?;
    conn.execute(
        "DELETE FROM entity_field_status WHERE entity_type = 'track' AND entity_id = ?1",
        params![track_guid],
    )?;
    conn.execute(
        "DELETE FROM tracks WHERE track_guid = ?1",
        params![track_guid],
    )?;
    Ok(())
}

// ── delete_feed ─────────────────────────────────────────────────────────────

/// Cascade-deletes a feed and all child rows, respecting FK constraints.
///
/// Uses correlated subqueries (`WHERE col IN (SELECT track_guid FROM tracks
/// WHERE feed_guid = ?1)`) so that child-row deletion is O(1) SQL operations
/// regardless of the number of tracks. Deletes in dependency order: track-level
/// children, feed-level children, tracks, then the feed itself.
///
/// Idempotent: calling with a non-existent `feed_guid` is a no-op.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL query or delete fails.
// DB performance compliant (subqueries) — 2026-03-12
pub fn delete_feed(conn: &mut Connection, feed_guid: &str) -> Result<(), DbError> {
    let sp = conn.savepoint()?;
    delete_feed_sql(&sp, feed_guid)?;
    sp.commit()?;
    Ok(())
}

/// Inner implementation of feed cascade-delete: executes all SQL deletes on
/// the provided connection without managing its own transaction.  Callers
/// must ensure they are already inside a transaction or savepoint.
// DB performance compliant (subqueries) — 2026-03-12
pub(crate) fn delete_feed_sql(conn: &Connection, feed_guid: &str) -> Result<(), DbError> {
    let release_id: Option<String> = conn
        .query_row(
            "SELECT release_id FROM source_feed_release_map WHERE feed_guid = ?1",
            params![feed_guid],
            |row| row.get(0),
        )
        .optional()?;
    let recording_ids: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT recording_id FROM source_item_recording_map \
             WHERE track_guid IN (SELECT track_guid FROM tracks WHERE feed_guid = ?1) \
             ORDER BY recording_id",
        )?;
        stmt.query_map(params![feed_guid], |row| row.get(0))?
            .collect::<Result<_, _>>()?
    };

    // 1. track_tag for all tracks in the feed (subquery)
    conn.execute(
        "DELETE FROM track_tag WHERE track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;

    // 2. feed_tag
    conn.execute(
        "DELETE FROM feed_tag WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    // 3. value_time_splits for all tracks (subquery)
    conn.execute(
        "DELETE FROM value_time_splits WHERE source_track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;

    // 4. payment_routes for all tracks (subquery)
    conn.execute(
        "DELETE FROM payment_routes WHERE track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;

    // 5. feed_payment_routes
    conn.execute(
        "DELETE FROM feed_payment_routes WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    // 6. entity_quality for all tracks (subquery) and the feed
    conn.execute(
        "DELETE FROM entity_quality WHERE entity_type = 'track' AND entity_id IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM entity_quality WHERE entity_type = 'feed' AND entity_id = ?1",
        params![feed_guid],
    )?;

    // 7. entity_field_status for all tracks (subquery) and the feed
    conn.execute(
        "DELETE FROM entity_field_status WHERE entity_type = 'track' AND entity_id IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM entity_field_status WHERE entity_type = 'feed' AND entity_id = ?1",
        params![feed_guid],
    )?;

    // 8. proof_tokens & proof_challenges (SG-07)
    conn.execute(
        "DELETE FROM proof_tokens WHERE subject_feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM proof_challenges WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    // 9. Feed-scoped staged/source rows
    conn.execute(
        "DELETE FROM feed_remote_items_raw WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM live_events WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM source_contributor_claims WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM source_entity_ids WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM source_entity_links WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM source_release_claims WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM source_item_enclosures WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM source_platform_claims WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    // 9b. Derived canonical release/recording mappings for this feed
    conn.execute(
        "DELETE FROM source_item_recording_map WHERE track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM source_feed_release_map WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    // 10. tracks
    conn.execute(
        "DELETE FROM tracks WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    // 11. feeds
    conn.execute("DELETE FROM feeds WHERE feed_guid = ?1", params![feed_guid])?;

    for recording_id in &recording_ids {
        rebuild_canonical_recording(conn, recording_id)?;
        rebuild_recording_sources(conn, recording_id)?;
    }
    if let Some(release_id) = &release_id {
        rebuild_canonical_release(conn, release_id)?;
        rebuild_release_sources(conn, release_id)?;
    }
    cleanup_orphaned_canonical_rows(conn)?;

    Ok(())
}

// ── delete_feed_with_event ───────────────────────────────────────────────────

/// Cascade-deletes a feed and records a `FeedRetired` event in a single atomic
/// transaction, returning the assigned event `seq`.
///
/// Uses correlated subqueries for track-level child deletion, matching the
/// strategy in [`delete_feed`].
///
/// # Errors
///
/// Returns [`DbError`] if any SQL statement, JSON serialisation, or the
/// transaction commit fails.
// DB performance compliant (subqueries) — 2026-03-12
// Issue-SEQ-INTEGRITY — 2026-03-14
#[expect(
    clippy::too_many_arguments,
    reason = "all event fields are required for a complete atomic delete+event"
)]
pub fn delete_feed_with_event(
    conn: &mut Connection,
    feed_guid: &str,
    event_id: &str,
    payload_json: &str,
    subject_guid: &str,
    signer: &NodeSigner,
    created_at: i64,
    warnings: &[String],
) -> Result<(i64, String, String), DbError> {
    let tx = conn.transaction()?;

    tx.execute(
        "DELETE FROM track_tag WHERE track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM feed_tag WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM value_time_splits WHERE source_track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM payment_routes WHERE track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM feed_payment_routes WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM entity_quality WHERE entity_type = 'track' AND entity_id IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM entity_quality WHERE entity_type = 'feed' AND entity_id = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM entity_field_status WHERE entity_type = 'track' AND entity_id IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM entity_field_status WHERE entity_type = 'feed' AND entity_id = ?1",
        params![feed_guid],
    )?;
    // proof_tokens & proof_challenges (SG-07)
    tx.execute(
        "DELETE FROM proof_tokens WHERE subject_feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM proof_challenges WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    tx.execute(
        "DELETE FROM tracks WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute("DELETE FROM feeds WHERE feed_guid = ?1", params![feed_guid])?;

    let et_str = event_type_str(&crate::event::EventType::FeedRetired)?;
    let warnings_json = serde_json::to_string(warnings)?;
    // Issue-SEQ-INTEGRITY — 2026-03-14: insert with placeholder, sign with seq, update.
    let seq = tx.query_row(
        "INSERT INTO events \
         (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at, warnings_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(seq),0)+1 FROM events), ?7, ?8) \
         RETURNING seq",
        params![event_id, et_str, payload_json, subject_guid, signer.pubkey_hex(), "", created_at, warnings_json],
        |row| row.get::<_, i64>(0),
    )?;
    let (signed_by, signature) = signer.sign_event(
        event_id,
        &crate::event::EventType::FeedRetired,
        payload_json,
        subject_guid,
        created_at,
        seq,
    );
    update_event_signature(&tx, event_id, &signed_by, &signature)?;

    tx.commit()?;
    Ok((seq, signed_by, signature))
}

// ── delete_track_with_event ──────────────────────────────────────────────────

/// Cascade-deletes a track and records a `TrackRemoved` event in a single
/// atomic transaction, returning the assigned event `seq`.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL statement, JSON serialisation, or the
/// transaction commit fails.
// Issue-SEQ-INTEGRITY — 2026-03-14
#[expect(
    clippy::too_many_arguments,
    reason = "all event fields are required for a complete atomic delete+event"
)]
pub fn delete_track_with_event(
    conn: &mut Connection,
    track_guid: &str,
    event_id: &str,
    payload_json: &str,
    subject_guid: &str,
    signer: &NodeSigner,
    created_at: i64,
    warnings: &[String],
) -> Result<(i64, String, String), DbError> {
    let tx = conn.transaction()?;

    tx.execute(
        "DELETE FROM track_tag WHERE track_guid = ?1",
        params![track_guid],
    )?;
    tx.execute(
        "DELETE FROM value_time_splits WHERE source_track_guid = ?1",
        params![track_guid],
    )?;
    tx.execute(
        "DELETE FROM payment_routes WHERE track_guid = ?1",
        params![track_guid],
    )?;
    tx.execute(
        "DELETE FROM entity_quality WHERE entity_type = 'track' AND entity_id = ?1",
        params![track_guid],
    )?;
    tx.execute(
        "DELETE FROM entity_field_status WHERE entity_type = 'track' AND entity_id = ?1",
        params![track_guid],
    )?;
    tx.execute(
        "DELETE FROM tracks WHERE track_guid = ?1",
        params![track_guid],
    )?;

    let et_str = event_type_str(&crate::event::EventType::TrackRemoved)?;
    let warnings_json = serde_json::to_string(warnings)?;
    // Issue-SEQ-INTEGRITY — 2026-03-14: insert with placeholder, sign with seq, update.
    let seq = tx.query_row(
        "INSERT INTO events \
         (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at, warnings_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(seq),0)+1 FROM events), ?7, ?8) \
         RETURNING seq",
        params![event_id, et_str, payload_json, subject_guid, signer.pubkey_hex(), "", created_at, warnings_json],
        |row| row.get::<_, i64>(0),
    )?;
    let (signed_by, signature) = signer.sign_event(
        event_id,
        &crate::event::EventType::TrackRemoved,
        payload_json,
        subject_guid,
        created_at,
        seq,
    );
    update_event_signature(&tx, event_id, &signed_by, &signature)?;

    tx.commit()?;
    Ok((seq, signed_by, signature))
}

// ── merge_artists_with_event ──────────────────────────────────────────────────

/// Merges two artists AND records the signed event in a single atomic
/// transaction.  Returns `(transferred_aliases, seq)`.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL statement or the transaction commit fails.
// Finding-2 atomic mutation+event — 2026-03-13
// Issue-SEQ-INTEGRITY — 2026-03-14
#[expect(
    clippy::too_many_arguments,
    reason = "all event fields are required for a complete atomic merge+event"
)]
pub fn merge_artists_with_event(
    conn: &mut Connection,
    source_artist_id: &str,
    target_artist_id: &str,
    event_id: &str,
    event_type: &EventType,
    payload_json: &str,
    subject_guid: &str,
    signer: &NodeSigner,
    created_at: i64,
    warnings: &[String],
) -> Result<(Vec<String>, i64, String, String), DbError> {
    let tx = conn.transaction()?;

    let transferred = merge_artists_sql(&tx, source_artist_id, target_artist_id)?;

    let (seq, signed_by, signature) = insert_event(
        &tx,
        event_id,
        event_type,
        payload_json,
        subject_guid,
        signer,
        created_at,
        warnings,
    )?;

    tx.commit()?;
    Ok((transferred, seq, signed_by, signature))
}

fn merge_artists_sql_with_event(
    conn: &Connection,
    source_artist_id: &str,
    target_artist_id: &str,
    signer: &NodeSigner,
) -> Result<Event, DbError> {
    let transferred = merge_artists_sql(conn, source_artist_id, target_artist_id)?;
    let payload = crate::event::ArtistMergedPayload {
        source_artist_id: source_artist_id.to_string(),
        target_artist_id: target_artist_id.to_string(),
        aliases_transferred: transferred,
    };
    let payload_json = serde_json::to_string(&payload)?;
    let event_id = uuid::Uuid::new_v4().to_string();
    let created_at = unix_now();
    let (seq, signed_by, signature) = insert_event(
        conn,
        &event_id,
        &EventType::ArtistMerged,
        &payload_json,
        target_artist_id,
        signer,
        created_at,
        &[],
    )?;

    Ok(Event {
        event_id,
        event_type: EventType::ArtistMerged,
        payload: EventPayload::ArtistMerged(payload),
        subject_guid: target_artist_id.to_string(),
        signed_by,
        signature,
        seq,
        created_at,
        warnings: Vec::new(),
        payload_json,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtistIdentityBackfillStats {
    pub groups_processed: usize,
    pub merges_applied: usize,
    pub merge_events_emitted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtistIdentityResolveStats {
    pub seed_artists: usize,
    pub candidate_groups: usize,
    pub groups_processed: usize,
    pub merges_applied: usize,
    pub merge_events_emitted: usize,
    pub pending_reviews: usize,
    pub blocked_reviews: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArtistIdentitySeedArtist {
    pub artist_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArtistIdentityCandidateGroup {
    pub source: String,
    pub name_key: String,
    pub evidence_key: String,
    pub artist_ids: Vec<String>,
    pub artist_names: Vec<String>,
    pub review_id: Option<i64>,
    pub review_status: Option<String>,
    pub override_type: Option<String>,
    pub target_artist_id: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArtistIdentityFeedPlan {
    pub feed_guid: String,
    pub seed_artists: Vec<ArtistIdentitySeedArtist>,
    pub candidate_groups: Vec<ArtistIdentityCandidateGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArtistIdentityPendingFeed {
    pub feed_guid: String,
    pub title: String,
    pub feed_url: String,
    pub seed_artists: usize,
    pub candidate_groups: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArtistIdentityReviewItem {
    pub review_id: i64,
    pub feed_guid: String,
    pub source: String,
    pub name_key: String,
    pub evidence_key: String,
    pub status: String,
    pub artist_ids: Vec<String>,
    pub artist_names: Vec<String>,
    pub override_type: Option<String>,
    pub target_artist_id: Option<String>,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArtistIdentityPendingReview {
    pub review_id: i64,
    pub feed_guid: String,
    pub title: String,
    pub source: String,
    pub name_key: String,
    pub evidence_key: String,
    pub artist_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtistIdentityEvidenceGroup {
    source: String,
    name_key: String,
    evidence_key: String,
    artist_ids: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtistIdentityOverrideRow {
    override_type: String,
    target_artist_id: Option<String>,
    note: Option<String>,
}

fn apply_artist_identity_groups(
    conn: &Connection,
    groups: Vec<ArtistIdentityEvidenceGroup>,
    review_feed_guid: Option<&str>,
    signer: Option<&NodeSigner>,
) -> Result<ArtistIdentityBackfillStats, DbError> {
    let mut stats = ArtistIdentityBackfillStats {
        groups_processed: 0,
        merges_applied: 0,
        merge_events_emitted: 0,
    };
    let mut active_review_keys = std::collections::BTreeSet::new();

    for group in groups {
        if let Some(feed_guid) = review_feed_guid {
            active_review_keys.insert((
                group.source.clone(),
                group.name_key.clone(),
                group.evidence_key.clone(),
            ));
            sync_artist_identity_review_item(conn, feed_guid, &group, "pending", None, None, None)?;
        }

        let mut current_ids = std::collections::BTreeSet::new();
        for artist_id in &group.artist_ids {
            if let Some(current_id) = current_artist_id(conn, artist_id)? {
                current_ids.insert(current_id);
            }
        }
        if current_ids.len() <= 1 {
            if let Some(feed_guid) = review_feed_guid {
                sync_artist_identity_review_item(
                    conn, feed_guid, &group, "resolved", None, None, None,
                )?;
            }
            continue;
        }

        let override_row = artist_identity_override_for_group(
            conn,
            &group.source,
            &group.name_key,
            &group.evidence_key,
        )?;
        if let Some(override_row) = &override_row
            && override_row.override_type == "do_not_merge"
        {
            if let Some(feed_guid) = review_feed_guid {
                sync_artist_identity_review_item(
                    conn,
                    feed_guid,
                    &group,
                    "blocked",
                    Some("do_not_merge"),
                    None,
                    override_row.note.as_deref(),
                )?;
            }
            continue;
        }

        let target_artist_id = match override_row.as_ref() {
            Some(override_row) if override_row.override_type == "merge" => {
                let Some(target_artist_id) = override_row.target_artist_id.as_deref() else {
                    return Err(DbError::Other(
                        "artist identity merge override is missing target_artist_id".into(),
                    ));
                };
                current_artist_id(conn, target_artist_id)?.ok_or_else(|| {
                    DbError::Other(format!(
                        "artist identity merge override target does not exist: {target_artist_id}"
                    ))
                })?
            }
            _ => {
                if let Some(target_artist_id) = preferred_artist_target(conn, &current_ids)? {
                    target_artist_id
                } else {
                    if let Some(feed_guid) = review_feed_guid {
                        sync_artist_identity_review_item(
                            conn, feed_guid, &group, "pending", None, None, None,
                        )?;
                    }
                    continue;
                }
            }
        };

        stats.groups_processed += 1;
        let mut merges_applied = 0usize;
        for source_artist_id in current_ids {
            if source_artist_id == target_artist_id {
                continue;
            }
            if current_artist_id(conn, &source_artist_id)?.as_deref()
                != Some(source_artist_id.as_str())
            {
                continue;
            }
            if let Some(signer) = signer {
                let _event = merge_artists_sql_with_event(
                    conn,
                    &source_artist_id,
                    &target_artist_id,
                    signer,
                )?;
                stats.merge_events_emitted += 1;
            } else {
                merge_artists_sql(conn, &source_artist_id, &target_artist_id)?;
            }
            merges_applied += 1;
            stats.merges_applied += 1;
        }

        if let Some(feed_guid) = review_feed_guid {
            let (override_type, note) = override_row.as_ref().map_or((None, None), |row| {
                (Some(row.override_type.as_str()), row.note.as_deref())
            });
            sync_artist_identity_review_item(
                conn,
                feed_guid,
                &group,
                if merges_applied > 0 || current_ids_for_review(conn, &group.artist_ids)?.len() <= 1
                {
                    "merged"
                } else {
                    "pending"
                },
                override_type,
                Some(target_artist_id.as_str()),
                note,
            )?;
        }
    }

    if let Some(feed_guid) = review_feed_guid {
        resolve_missing_artist_identity_reviews(conn, feed_guid, &active_review_keys)?;
    }

    Ok(stats)
}

fn count_feed_artist_identity_review_statuses(
    conn: &Connection,
    feed_guid: &str,
) -> Result<(usize, usize), DbError> {
    let reviews = list_artist_identity_reviews_for_feed(conn, feed_guid)?;
    let pending_reviews = reviews
        .iter()
        .filter(|review| review.status == "pending")
        .count();
    let blocked_reviews = reviews
        .iter()
        .filter(|review| review.status == "blocked")
        .count();
    Ok((pending_reviews, blocked_reviews))
}

pub fn emit_artist_identity_feed_resolved_event(
    conn: &Connection,
    feed_guid: &str,
    stats: &ArtistIdentityResolveStats,
    signer: &NodeSigner,
) -> Result<Event, DbError> {
    let payload = crate::event::ArtistIdentityFeedResolvedPayload {
        feed_guid: feed_guid.to_string(),
        seed_artists: stats.seed_artists,
        candidate_groups: stats.candidate_groups,
        groups_processed: stats.groups_processed,
        merges_applied: stats.merges_applied,
        pending_reviews: stats.pending_reviews,
        blocked_reviews: stats.blocked_reviews,
    };
    let payload_json = serde_json::to_string(&payload)?;
    let event_id = uuid::Uuid::new_v4().to_string();
    let created_at = unix_now();
    let (seq, signed_by, signature) = insert_event(
        conn,
        &event_id,
        &EventType::ArtistIdentityFeedResolved,
        &payload_json,
        feed_guid,
        signer,
        created_at,
        &[],
    )?;

    Ok(Event {
        event_id,
        event_type: EventType::ArtistIdentityFeedResolved,
        payload: EventPayload::ArtistIdentityFeedResolved(payload),
        subject_guid: feed_guid.to_string(),
        signed_by,
        signature,
        seq,
        created_at,
        warnings: Vec::new(),
        payload_json,
    })
}

fn artist_ids_for_feed_scope(
    conn: &Connection,
    feed_guid: &str,
) -> Result<std::collections::BTreeSet<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT acn.artist_id
         FROM artist_credit_name acn
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id
         LEFT JOIN feeds f ON f.artist_credit_id = ac.id
         LEFT JOIN tracks t ON t.artist_credit_id = ac.id
         WHERE f.feed_guid = ?1 OR t.feed_guid = ?1
         ORDER BY acn.artist_id",
    )?;
    let rows = stmt
        .query_map(params![feed_guid], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut artist_ids = std::collections::BTreeSet::new();
    for artist_id in rows {
        if let Some(current_id) = current_artist_id(conn, &artist_id)? {
            artist_ids.insert(current_id);
        }
    }
    Ok(artist_ids)
}

fn filter_artist_groups_for_seed_ids(
    conn: &Connection,
    groups: Vec<ArtistIdentityEvidenceGroup>,
    seed_ids: &std::collections::BTreeSet<String>,
) -> Result<Vec<ArtistIdentityEvidenceGroup>, DbError> {
    let mut filtered = Vec::new();
    for group in groups {
        let mut current_group_ids = std::collections::BTreeSet::new();
        for artist_id in &group.artist_ids {
            if let Some(current_id) = current_artist_id(conn, artist_id)? {
                current_group_ids.insert(current_id);
            }
        }
        if !current_group_ids.is_disjoint(seed_ids) {
            filtered.push(group);
        }
    }
    Ok(filtered)
}

fn collect_artist_identity_groups_for_seed_ids(
    conn: &Connection,
    seed_ids: &std::collections::BTreeSet<String>,
) -> Result<Vec<ArtistIdentityEvidenceGroup>, DbError> {
    collect_labeled_artist_identity_groups_for_seed_ids(conn, seed_ids)
}

fn collect_labeled_artist_identity_groups_for_seed_ids(
    conn: &Connection,
    seed_ids: &std::collections::BTreeSet<String>,
) -> Result<Vec<ArtistIdentityEvidenceGroup>, DbError> {
    let mut groups = Vec::new();
    groups.extend(filter_artist_groups_for_seed_ids(
        conn,
        collect_artist_groups_by_npub(conn)?,
        seed_ids,
    )?);
    groups.extend(filter_artist_groups_for_seed_ids(
        conn,
        collect_artist_groups_by_publisher_guid(conn)?,
        seed_ids,
    )?);
    groups.extend(filter_artist_groups_for_seed_ids(
        conn,
        collect_artist_groups_by_website(conn)?,
        seed_ids,
    )?);
    groups.extend(filter_artist_groups_for_seed_ids(
        conn,
        collect_artist_groups_by_normalized_website(conn)?,
        seed_ids,
    )?);
    groups.extend(filter_artist_groups_for_seed_ids(
        conn,
        collect_artist_groups_by_release_cluster(conn)?,
        seed_ids,
    )?);
    groups.extend(filter_artist_groups_for_seed_ids(
        conn,
        collect_artist_groups_by_anchored_name(conn)?,
        seed_ids,
    )?);
    Ok(groups)
}

fn current_ids_for_review(
    conn: &Connection,
    artist_ids: &std::collections::BTreeSet<String>,
) -> Result<std::collections::BTreeSet<String>, DbError> {
    let mut current_ids = std::collections::BTreeSet::new();
    for artist_id in artist_ids {
        if let Some(current_id) = current_artist_id(conn, artist_id)? {
            current_ids.insert(current_id);
        }
    }
    Ok(current_ids)
}

fn artist_names_for_review_group(
    conn: &Connection,
    artist_ids: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    artist_ids
        .iter()
        .filter_map(|artist_id| get_artist_by_id(conn, artist_id).ok().flatten())
        .map(|artist| artist.name)
        .collect()
}

fn artist_identity_override_for_group(
    conn: &Connection,
    source: &str,
    name_key: &str,
    evidence_key: &str,
) -> Result<Option<ArtistIdentityOverrideRow>, DbError> {
    conn.query_row(
        "SELECT override_type, target_artist_id, note
         FROM artist_identity_override
         WHERE source = ?1 AND name_key = ?2 AND evidence_key = ?3",
        params![source, name_key, evidence_key],
        |row| {
            Ok(ArtistIdentityOverrideRow {
                override_type: row.get(0)?,
                target_artist_id: row.get(1)?,
                note: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn sync_artist_identity_review_item(
    conn: &Connection,
    feed_guid: &str,
    group: &ArtistIdentityEvidenceGroup,
    status: &str,
    override_type: Option<&str>,
    target_artist_id: Option<&str>,
    note: Option<&str>,
) -> Result<i64, DbError> {
    let now = unix_now();
    let current_ids = current_ids_for_review(conn, &group.artist_ids)?;
    let artist_ids = current_ids.into_iter().collect::<Vec<_>>();
    let artist_names = artist_names_for_review_group(conn, &group.artist_ids);
    let artist_ids_json = serde_json::to_string(&artist_ids)?;
    let artist_names_json = serde_json::to_string(&artist_names)?;

    conn.execute(
        "INSERT INTO artist_identity_review (
             feed_guid, source, name_key, evidence_key, status,
             artist_ids_json, artist_names_json, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
         ON CONFLICT(feed_guid, source, name_key, evidence_key) DO UPDATE SET
             status = excluded.status,
             artist_ids_json = excluded.artist_ids_json,
             artist_names_json = excluded.artist_names_json,
             updated_at = excluded.updated_at",
        params![
            feed_guid,
            group.source,
            group.name_key,
            group.evidence_key,
            status,
            artist_ids_json,
            artist_names_json,
            now
        ],
    )?;

    if let Some(override_type) = override_type {
        set_artist_identity_override(
            conn,
            &group.source,
            &group.name_key,
            &group.evidence_key,
            override_type,
            target_artist_id,
            note,
        )?;
    }

    conn.query_row(
        "SELECT review_id
         FROM artist_identity_review
         WHERE feed_guid = ?1 AND source = ?2 AND name_key = ?3 AND evidence_key = ?4",
        params![feed_guid, group.source, group.name_key, group.evidence_key],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn resolve_missing_artist_identity_reviews(
    conn: &Connection,
    feed_guid: &str,
    active_keys: &std::collections::BTreeSet<(String, String, String)>,
) -> Result<(), DbError> {
    let mut stmt = conn.prepare(
        "SELECT source, name_key, evidence_key
         FROM artist_identity_review
         WHERE feed_guid = ?1",
    )?;
    let existing = stmt
        .query_map(params![feed_guid], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let now = unix_now();
    for key in existing {
        if active_keys.contains(&key) {
            continue;
        }
        conn.execute(
            "UPDATE artist_identity_review
             SET status = 'resolved', updated_at = ?5
             WHERE feed_guid = ?1 AND source = ?2 AND name_key = ?3 AND evidence_key = ?4",
            params![feed_guid, key.0, key.1, key.2, now],
        )?;
    }
    Ok(())
}

fn seed_artist_rows_for_feed_scope(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<ArtistIdentitySeedArtist>, DbError> {
    let mut rows = Vec::new();
    for artist_id in artist_ids_for_feed_scope(conn, feed_guid)? {
        if let Some(artist) = get_artist_by_id(conn, &artist_id)? {
            rows.push(ArtistIdentitySeedArtist {
                artist_id: artist.artist_id,
                name: artist.name,
            });
        }
    }
    Ok(rows)
}

fn current_artist_id(conn: &Connection, artist_id: &str) -> Result<Option<String>, DbError> {
    let mut current = artist_id.to_string();
    for _ in 0..32 {
        let redirect: Option<String> = conn
            .query_row(
                "SELECT new_artist_id FROM artist_id_redirect WHERE old_artist_id = ?1",
                params![current],
                |row| row.get(0),
            )
            .optional()?;
        match redirect {
            Some(next) if next != current => current = next,
            _ => break,
        }
    }
    if get_artist_by_id(conn, &current)?.is_some() {
        Ok(Some(current))
    } else {
        Ok(None)
    }
}

fn collect_artist_groups_from_rows(
    source: &str,
    rows: Vec<(String, String, String)>,
) -> Vec<ArtistIdentityEvidenceGroup> {
    let mut grouped: std::collections::BTreeMap<
        (String, String),
        std::collections::BTreeSet<String>,
    > = std::collections::BTreeMap::new();
    for (name_key, evidence_key, artist_id) in rows {
        grouped
            .entry((name_key, evidence_key))
            .or_default()
            .insert(artist_id);
    }
    grouped
        .into_iter()
        .filter_map(|((name_key, evidence_key), artist_ids)| {
            (artist_ids.len() > 1).then_some(ArtistIdentityEvidenceGroup {
                source: source.to_string(),
                name_key,
                evidence_key,
                artist_ids,
            })
        })
        .collect()
}

fn collect_artist_groups_by_npub(
    conn: &Connection,
) -> Result<Vec<ArtistIdentityEvidenceGroup>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT LOWER(ac.display_name), sid.value, acn.artist_id \
         FROM source_entity_ids sid \
         JOIN feeds f ON f.feed_guid = sid.feed_guid \
         JOIN artist_credit ac ON ac.id = f.artist_credit_id \
         JOIN artist_credit_name acn ON acn.artist_credit_id = ac.id \
         WHERE sid.entity_type = 'feed' \
           AND sid.scheme = 'nostr_npub' \
           AND TRIM(sid.value) <> '' \
         ORDER BY LOWER(ac.display_name), sid.value, acn.artist_id",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<Vec<(String, String, String)>, _>>()?;
    Ok(collect_artist_groups_from_rows("npub", rows))
}

fn collect_artist_groups_by_publisher_guid(
    conn: &Connection,
) -> Result<Vec<ArtistIdentityEvidenceGroup>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT LOWER(ac.display_name), fri.remote_feed_guid, acn.artist_id \
         FROM feed_remote_items_raw fri \
         JOIN feeds f ON f.feed_guid = fri.feed_guid \
         JOIN artist_credit ac ON ac.id = f.artist_credit_id \
         JOIN artist_credit_name acn ON acn.artist_credit_id = ac.id \
         WHERE fri.medium = 'publisher' \
           AND TRIM(fri.remote_feed_guid) <> '' \
         ORDER BY LOWER(ac.display_name), fri.remote_feed_guid, acn.artist_id",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<Vec<(String, String, String)>, _>>()?;
    Ok(collect_artist_groups_from_rows("publisher_guid", rows))
}

fn collect_artist_groups_by_website(
    conn: &Connection,
) -> Result<Vec<ArtistIdentityEvidenceGroup>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT LOWER(ac.display_name), sel.url, acn.artist_id \
         FROM source_entity_links sel \
         JOIN feeds f ON f.feed_guid = sel.feed_guid \
         JOIN artist_credit ac ON ac.id = f.artist_credit_id \
         JOIN artist_credit_name acn ON acn.artist_credit_id = ac.id \
         WHERE sel.entity_type = 'feed' \
           AND sel.link_type = 'website' \
           AND TRIM(sel.url) <> '' \
         ORDER BY LOWER(ac.display_name), sel.url, acn.artist_id",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<Vec<(String, String, String)>, _>>()?;
    Ok(collect_artist_groups_from_rows("website", rows))
}

fn collect_artist_groups_by_release_cluster(
    conn: &Connection,
) -> Result<Vec<ArtistIdentityEvidenceGroup>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT LOWER(ac.display_name), sfr.release_id, acn.artist_id \
         FROM source_feed_release_map sfr \
         JOIN feeds f ON f.feed_guid = sfr.feed_guid \
         JOIN artist_credit ac ON ac.id = f.artist_credit_id \
         JOIN artist_credit_name acn ON acn.artist_credit_id = ac.id \
         WHERE sfr.match_type IN ('exact_release_signature_v1', 'single_track_cross_platform_release_v1') \
         ORDER BY LOWER(ac.display_name), sfr.release_id, acn.artist_id",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<Vec<(String, String, String)>, _>>()?;
    Ok(collect_artist_groups_from_rows("release_cluster", rows))
}

fn normalize_artist_website_key(raw_url: &str) -> Option<String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed = url::Url::parse(trimmed)
        .or_else(|_| url::Url::parse(&format!("https://{trimmed}")))
        .ok()?;
    let mut host = parsed.host_str()?.trim().to_ascii_lowercase();
    if let Some(stripped) = host.strip_prefix("www.") {
        host = stripped.to_string();
    }

    let segments = parsed
        .path_segments()
        .map(|parts| {
            parts
                .filter(|part| !part.is_empty())
                .map(|part| part.trim().to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if host.ends_with(".bandcamp.com") {
        return Some(host);
    }

    if host == "instagram.com" {
        if let Some(profile) = segments.first()
            && !matches!(profile.as_str(), "p" | "reel" | "reels" | "tv" | "stories")
        {
            return Some(format!("{host}/{profile}"));
        }
        return Some(host);
    }

    if segments.is_empty() {
        return Some(host);
    }

    Some(format!("{host}/{}", segments.join("/")))
}

fn collect_artist_groups_by_normalized_website(
    conn: &Connection,
) -> Result<Vec<ArtistIdentityEvidenceGroup>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT LOWER(ac.display_name), sel.url, acn.artist_id \
         FROM source_entity_links sel \
         JOIN feeds f ON f.feed_guid = sel.feed_guid \
         JOIN artist_credit ac ON ac.id = f.artist_credit_id \
         JOIN artist_credit_name acn ON acn.artist_credit_id = ac.id \
         WHERE sel.entity_type = 'feed' \
           AND sel.link_type = 'website' \
           AND TRIM(sel.url) <> '' \
         ORDER BY LOWER(ac.display_name), sel.url, acn.artist_id",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<(String, String, String)>, _>>()?;

    let normalized = rows
        .into_iter()
        .filter_map(|(name_key, raw_url, artist_id)| {
            let site_key = normalize_artist_website_key(&raw_url)?;
            Some((name_key, site_key, artist_id))
        })
        .collect::<Vec<_>>();
    Ok(collect_artist_groups_from_rows(
        "normalized_website",
        normalized,
    ))
}

fn artist_has_strong_identity_claims(conn: &Connection, artist_id: &str) -> Result<bool, DbError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) \
         FROM artist_credit_name acn \
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id \
         JOIN feeds f ON f.artist_credit_id = ac.id \
         WHERE acn.artist_id = ?1 \
           AND ( \
                EXISTS(SELECT 1 FROM source_entity_ids sid \
                       WHERE sid.entity_type = 'feed' \
                         AND sid.entity_id = f.feed_guid \
                         AND sid.scheme = 'nostr_npub') \
             OR EXISTS(SELECT 1 FROM feed_remote_items_raw fri \
                       WHERE fri.feed_guid = f.feed_guid \
                         AND fri.medium = 'publisher' \
                         AND TRIM(fri.remote_feed_guid) <> '') \
             OR EXISTS(SELECT 1 FROM source_entity_links sel \
                       WHERE sel.entity_type = 'feed' \
                         AND sel.entity_id = f.feed_guid \
                         AND sel.link_type = 'website' \
                         AND TRIM(sel.url) <> '') \
           )",
        params![artist_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn artist_platforms(
    conn: &Connection,
    artist_id: &str,
) -> Result<std::collections::BTreeSet<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT spc.platform_key \
         FROM artist_credit_name acn \
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id \
         JOIN feeds f ON f.artist_credit_id = ac.id \
         JOIN source_platform_claims spc ON spc.feed_guid = f.feed_guid \
         WHERE acn.artist_id = ?1 \
           AND TRIM(spc.platform_key) <> ''",
    )?;
    let rows = stmt
        .query_map(params![artist_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().collect())
}

fn collect_artist_groups_by_anchored_name(
    conn: &Connection,
) -> Result<Vec<ArtistIdentityEvidenceGroup>, DbError> {
    let mut stmt =
        conn.prepare("SELECT LOWER(name), artist_id FROM artists ORDER BY LOWER(name), artist_id")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut names_to_artists: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (name_key, artist_id) in rows {
        names_to_artists
            .entry(name_key)
            .or_default()
            .push(artist_id);
    }

    let mut groups = Vec::new();
    for artist_ids in names_to_artists.into_values() {
        if artist_ids.len() <= 1 {
            continue;
        }

        let mut anchored = Vec::new();
        let mut weak = Vec::new();
        for artist_id in artist_ids {
            let feed_count = artist_feed_count(conn, &artist_id)?;
            let strong = artist_has_strong_identity_claims(conn, &artist_id)?;
            if strong && feed_count >= 2 {
                anchored.push(artist_id);
                continue;
            }

            if strong || feed_count != 1 {
                continue;
            }

            let platforms = artist_platforms(conn, &artist_id)?;
            if platforms
                .iter()
                .all(|platform| matches!(platform.as_str(), "fountain" | "rss_blue"))
            {
                weak.push(artist_id);
            }
        }

        if anchored.len() == 1 && !weak.is_empty() {
            let mut group_artist_ids = std::collections::BTreeSet::new();
            group_artist_ids.insert(anchored.remove(0));
            group_artist_ids.extend(weak);
            let Some(name_key) = group_artist_ids
                .iter()
                .find_map(|artist_id| get_artist_by_id(conn, artist_id).ok().flatten())
                .map(|artist| artist.name.to_lowercase())
            else {
                continue;
            };
            groups.push(ArtistIdentityEvidenceGroup {
                source: "anchored_name".to_string(),
                name_key: name_key.clone(),
                evidence_key: name_key,
                artist_ids: group_artist_ids,
            });
        }
    }

    Ok(groups)
}

fn preferred_artist_target(
    conn: &Connection,
    artist_ids: &std::collections::BTreeSet<String>,
) -> Result<Option<String>, DbError> {
    let mut ranked = Vec::new();
    for artist_id in artist_ids {
        let Some(current_id) = current_artist_id(conn, artist_id)? else {
            continue;
        };
        let Some(artist) = get_artist_by_id(conn, &current_id)? else {
            continue;
        };
        ranked.push((
            artist_feed_count(conn, &current_id)?,
            artist.created_at,
            current_id,
        ));
    }
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    Ok(ranked.into_iter().next().map(|(_, _, artist_id)| artist_id))
}

pub fn backfill_artist_identity(
    conn: &mut Connection,
) -> Result<ArtistIdentityBackfillStats, DbError> {
    let tx = conn.transaction()?;
    let mut groups = Vec::new();
    groups.extend(collect_artist_groups_by_npub(&tx)?);
    groups.extend(collect_artist_groups_by_publisher_guid(&tx)?);
    groups.extend(collect_artist_groups_by_website(&tx)?);
    groups.extend(collect_artist_groups_by_normalized_website(&tx)?);
    groups.extend(collect_artist_groups_by_release_cluster(&tx)?);
    groups.extend(collect_artist_groups_by_anchored_name(&tx)?);
    let stats = apply_artist_identity_groups(&tx, groups, None, None)?;
    tx.commit()?;
    Ok(stats)
}

pub fn resolve_artist_identity_for_feed_with_signer(
    conn: &mut Connection,
    feed_guid: &str,
    signer: Option<&NodeSigner>,
) -> Result<ArtistIdentityResolveStats, DbError> {
    let tx = conn.transaction()?;
    let seed_ids = artist_ids_for_feed_scope(&tx, feed_guid)?;
    if seed_ids.is_empty() {
        tx.commit()?;
        return Ok(ArtistIdentityResolveStats {
            seed_artists: 0,
            candidate_groups: 0,
            groups_processed: 0,
            merges_applied: 0,
            merge_events_emitted: 0,
            pending_reviews: 0,
            blocked_reviews: 0,
        });
    }

    let groups = collect_artist_identity_groups_for_seed_ids(&tx, &seed_ids)?;
    let candidate_groups = groups.len();
    let backfill_stats = apply_artist_identity_groups(&tx, groups, Some(feed_guid), signer)?;
    let (pending_reviews, blocked_reviews) =
        count_feed_artist_identity_review_statuses(&tx, feed_guid)?;
    tx.commit()?;
    Ok(ArtistIdentityResolveStats {
        seed_artists: seed_ids.len(),
        candidate_groups,
        groups_processed: backfill_stats.groups_processed,
        merges_applied: backfill_stats.merges_applied,
        merge_events_emitted: backfill_stats.merge_events_emitted,
        pending_reviews,
        blocked_reviews,
    })
}

pub fn resolve_artist_identity_for_feed(
    conn: &mut Connection,
    feed_guid: &str,
) -> Result<ArtistIdentityResolveStats, DbError> {
    resolve_artist_identity_for_feed_with_signer(conn, feed_guid, None)
}

/// Explains the current feed-scoped artist identity plan for one feed.
///
/// # Errors
///
/// Returns [`DbError`] if the feed-scoped seed artists or candidate groups
/// cannot be loaded from `SQLite`.
pub fn explain_artist_identity_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<ArtistIdentityFeedPlan, DbError> {
    let seed_artists = seed_artist_rows_for_feed_scope(conn, feed_guid)?;
    let seed_ids = seed_artists
        .iter()
        .map(|artist| artist.artist_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let candidate_groups = collect_labeled_artist_identity_groups_for_seed_ids(conn, &seed_ids)?
        .into_iter()
        .map(|group| {
            let artist_ids = current_ids_for_review(conn, &group.artist_ids)
                .unwrap_or_else(|_err| group.artist_ids.clone())
                .into_iter()
                .collect::<Vec<_>>();
            let artist_names = artist_names_for_review_group(conn, &group.artist_ids);
            let review = get_artist_identity_review_for_subject(
                conn,
                feed_guid,
                &group.source,
                &group.name_key,
                &group.evidence_key,
            )
            .ok()
            .flatten();
            ArtistIdentityCandidateGroup {
                source: group.source,
                name_key: group.name_key,
                evidence_key: group.evidence_key,
                artist_ids,
                artist_names,
                review_id: review.as_ref().map(|item| item.review_id),
                review_status: review.as_ref().map(|item| item.status.clone()),
                override_type: review.as_ref().and_then(|item| item.override_type.clone()),
                target_artist_id: review
                    .as_ref()
                    .and_then(|item| item.target_artist_id.clone()),
                note: review.and_then(|item| item.note),
            }
        })
        .collect::<Vec<_>>();

    Ok(ArtistIdentityFeedPlan {
        feed_guid: feed_guid.to_string(),
        seed_artists,
        candidate_groups,
    })
}

/// Lists feeds whose current targeted artist-identity plan still has
/// candidate groups to review.
///
/// # Errors
///
/// Returns [`DbError`] if feed rows or feed-scoped artist identity plans
/// cannot be loaded from `SQLite`.
pub fn list_pending_artist_identity_feeds(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<ArtistIdentityPendingFeed>, DbError> {
    let mut pending = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT feed_guid, title, feed_url
         FROM feeds
         ORDER BY title_lower, feed_guid",
    )?;
    let feed_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (feed_guid, title, feed_url) in feed_rows {
        let plan = explain_artist_identity_for_feed(conn, &feed_guid)?;
        if !plan.candidate_groups.is_empty() {
            pending.push(ArtistIdentityPendingFeed {
                feed_guid,
                title,
                feed_url,
                seed_artists: plan.seed_artists.len(),
                candidate_groups: plan.candidate_groups.len(),
            });
            if pending.len() >= limit {
                break;
            }
        }
    }
    Ok(pending)
}

/// Returns the stored review items for one feed-scoped artist identity plan.
///
/// # Errors
///
/// Returns [`DbError`] if the review rows or joined override metadata cannot be
/// loaded from `SQLite`.
pub fn list_artist_identity_reviews_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<ArtistIdentityReviewItem>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT
             r.review_id,
             r.feed_guid,
             r.source,
             r.name_key,
             r.evidence_key,
             r.status,
             r.artist_ids_json,
             r.artist_names_json,
             o.override_type,
             o.target_artist_id,
             o.note,
             r.created_at,
             r.updated_at
         FROM artist_identity_review r
         LEFT JOIN artist_identity_override o
           ON o.source = r.source
          AND o.name_key = r.name_key
          AND o.evidence_key = r.evidence_key
         WHERE r.feed_guid = ?1
         ORDER BY r.updated_at DESC, r.review_id DESC",
    )?;
    stmt.query_map(params![feed_guid], artist_identity_review_row)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Returns one review item by `review_id`.
///
/// # Errors
///
/// Returns [`DbError`] if the review row or joined override metadata cannot be
/// loaded from `SQLite`.
pub fn get_artist_identity_review(
    conn: &Connection,
    review_id: i64,
) -> Result<Option<ArtistIdentityReviewItem>, DbError> {
    conn.query_row(
        "SELECT
             r.review_id,
             r.feed_guid,
             r.source,
             r.name_key,
             r.evidence_key,
             r.status,
             r.artist_ids_json,
             r.artist_names_json,
             o.override_type,
             o.target_artist_id,
             o.note,
             r.created_at,
             r.updated_at
         FROM artist_identity_review r
         LEFT JOIN artist_identity_override o
           ON o.source = r.source
          AND o.name_key = r.name_key
          AND o.evidence_key = r.evidence_key
         WHERE r.review_id = ?1",
        params![review_id],
        artist_identity_review_row,
    )
    .optional()
    .map_err(Into::into)
}

/// Returns one review item for a specific feed and subject triple.
///
/// # Errors
///
/// Returns [`DbError`] if the review row or joined override metadata cannot be
/// loaded from `SQLite`.
pub fn get_artist_identity_review_for_subject(
    conn: &Connection,
    feed_guid: &str,
    source: &str,
    name_key: &str,
    evidence_key: &str,
) -> Result<Option<ArtistIdentityReviewItem>, DbError> {
    conn.query_row(
        "SELECT
             r.review_id,
             r.feed_guid,
             r.source,
             r.name_key,
             r.evidence_key,
             r.status,
             r.artist_ids_json,
             r.artist_names_json,
             o.override_type,
             o.target_artist_id,
             o.note,
             r.created_at,
             r.updated_at
         FROM artist_identity_review r
         LEFT JOIN artist_identity_override o
           ON o.source = r.source
          AND o.name_key = r.name_key
          AND o.evidence_key = r.evidence_key
         WHERE r.feed_guid = ?1
           AND r.source = ?2
           AND r.name_key = ?3
           AND r.evidence_key = ?4",
        params![feed_guid, source, name_key, evidence_key],
        artist_identity_review_row,
    )
    .optional()
    .map_err(Into::into)
}

/// Lists unresolved artist-identity review items that still need an operator
/// decision.
///
/// # Errors
///
/// Returns [`DbError`] if the pending review rows cannot be loaded.
pub fn list_pending_artist_identity_reviews(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<ArtistIdentityPendingReview>, DbError> {
    let limit_i64 = i64::try_from(limit).map_err(|_err| {
        DbError::Other("pending review limit exceeded supported SQLite integer range".into())
    })?;
    let mut stmt = conn.prepare(
        "SELECT
             r.review_id,
             r.feed_guid,
             f.title,
             r.source,
             r.name_key,
             r.evidence_key,
             r.artist_ids_json
         FROM artist_identity_review r
         JOIN feeds f ON f.feed_guid = r.feed_guid
         WHERE r.status = 'pending'
         ORDER BY r.updated_at DESC, r.review_id DESC
         LIMIT ?1",
    )?;
    stmt.query_map(params![limit_i64], |row| {
        let artist_ids_json: String = row.get(6)?;
        let artist_ids = serde_json::from_str::<Vec<String>>(&artist_ids_json).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, err.into())
        })?;
        Ok(ArtistIdentityPendingReview {
            review_id: row.get(0)?,
            feed_guid: row.get(1)?,
            title: row.get(2)?,
            source: row.get(3)?,
            name_key: row.get(4)?,
            evidence_key: row.get(5)?,
            artist_count: artist_ids.len(),
        })
    })?
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

/// Stores a merge override for one artist-identity review item.
///
/// # Errors
///
/// Returns [`DbError`] if the review item does not exist, the target artist
/// cannot be resolved, or the override write fails.
pub fn set_artist_identity_merge_override_for_review(
    conn: &Connection,
    review_id: i64,
    target_artist_id: &str,
    note: Option<&str>,
) -> Result<(), DbError> {
    let review = get_artist_identity_review(conn, review_id)?
        .ok_or_else(|| DbError::Other(format!("artist identity review not found: {review_id}")))?;
    let resolved_target = current_artist_id(conn, target_artist_id)?.ok_or_else(|| {
        DbError::Other(format!(
            "artist identity merge target does not exist: {target_artist_id}"
        ))
    })?;
    set_artist_identity_override(
        conn,
        &review.source,
        &review.name_key,
        &review.evidence_key,
        "merge",
        Some(resolved_target.as_str()),
        note,
    )
}

/// Stores a do-not-merge override for one artist-identity review item.
///
/// # Errors
///
/// Returns [`DbError`] if the review item does not exist or the override write
/// fails.
pub fn set_artist_identity_do_not_merge_override_for_review(
    conn: &Connection,
    review_id: i64,
    note: Option<&str>,
) -> Result<(), DbError> {
    let review = get_artist_identity_review(conn, review_id)?
        .ok_or_else(|| DbError::Other(format!("artist identity review not found: {review_id}")))?;
    set_artist_identity_override(
        conn,
        &review.source,
        &review.name_key,
        &review.evidence_key,
        "do_not_merge",
        None,
        note,
    )
}

fn set_artist_identity_override(
    conn: &Connection,
    source: &str,
    name_key: &str,
    evidence_key: &str,
    override_type: &str,
    target_artist_id: Option<&str>,
    note: Option<&str>,
) -> Result<(), DbError> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO artist_identity_override (
             source, name_key, evidence_key, override_type,
             target_artist_id, note, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(source, name_key, evidence_key) DO UPDATE SET
             override_type = excluded.override_type,
             target_artist_id = excluded.target_artist_id,
             note = excluded.note,
             updated_at = excluded.updated_at",
        params![
            source,
            name_key,
            evidence_key,
            override_type,
            target_artist_id,
            note,
            now
        ],
    )?;
    Ok(())
}

fn artist_identity_review_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ArtistIdentityReviewItem> {
    let artist_ids_json: String = row.get(6)?;
    let artist_names_json: String = row.get(7)?;
    let artist_ids = serde_json::from_str::<Vec<String>>(&artist_ids_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, err.into())
    })?;
    let artist_names = serde_json::from_str::<Vec<String>>(&artist_names_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, err.into())
    })?;
    Ok(ArtistIdentityReviewItem {
        review_id: row.get(0)?,
        feed_guid: row.get(1)?,
        source: row.get(2)?,
        name_key: row.get(3)?,
        evidence_key: row.get(4)?,
        status: row.get(5)?,
        artist_ids,
        artist_names,
        override_type: row.get(8)?,
        target_artist_id: row.get(9)?,
        note: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

// ── get_feed_by_guid ────────────────────────────────────────────────────────

/// Looks up the feed row by `feed_guid`, returning `None` if absent.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_feed_by_guid(conn: &Connection, feed_guid: &str) -> Result<Option<Feed>, DbError> {
    let result = conn.query_row(
        "SELECT feed_guid, feed_url, title, title_lower, artist_credit_id, description, image_url, \
         language, explicit, itunes_type, episode_count, newest_item_at, oldest_item_at, \
         created_at, updated_at, raw_medium \
         FROM feeds WHERE feed_guid = ?1",
        params![feed_guid],
        |row| {
            let explicit_i: i64 = row.get(8)?;
            Ok(Feed {
                feed_guid:        row.get(0)?,
                feed_url:         row.get(1)?,
                title:            row.get(2)?,
                title_lower:      row.get(3)?,
                artist_credit_id: row.get(4)?,
                description:      row.get(5)?,
                image_url:        row.get(6)?,
                language:         row.get(7)?,
                explicit:         explicit_i != 0,
                itunes_type:      row.get(9)?,
                episode_count:    row.get(10)?,
                newest_item_at:   row.get(11)?,
                oldest_item_at:   row.get(12)?,
                created_at:       row.get(13)?,
                updated_at:       row.get(14)?,
                raw_medium:       row.get(15)?,
            })
        },
    ).optional()?;

    Ok(result)
}

// ── get_track_by_guid ───────────────────────────────────────────────────────

/// Looks up the track row by `track_guid`, returning `None` if absent.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_track_by_guid(conn: &Connection, track_guid: &str) -> Result<Option<Track>, DbError> {
    let result = conn
        .query_row(
            "SELECT track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, \
         duration_secs, enclosure_url, enclosure_type, enclosure_bytes, track_number, \
         season, explicit, description, created_at, updated_at \
         FROM tracks WHERE track_guid = ?1",
            params![track_guid],
            |row| {
                let explicit_i: i64 = row.get(12)?;
                Ok(Track {
                    track_guid: row.get(0)?,
                    feed_guid: row.get(1)?,
                    artist_credit_id: row.get(2)?,
                    title: row.get(3)?,
                    title_lower: row.get(4)?,
                    pub_date: row.get(5)?,
                    duration_secs: row.get(6)?,
                    enclosure_url: row.get(7)?,
                    enclosure_type: row.get(8)?,
                    enclosure_bytes: row.get(9)?,
                    track_number: row.get(10)?,
                    season: row.get(11)?,
                    explicit: explicit_i != 0,
                    description: row.get(13)?,
                    created_at: row.get(14)?,
                    updated_at: row.get(15)?,
                })
            },
        )
        .optional()?;

    Ok(result)
}

// ── get_tracks_for_feed ─────────────────────────────────────────────────────

/// Returns all tracks belonging to the given `feed_guid`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_tracks_for_feed(conn: &Connection, feed_guid: &str) -> Result<Vec<Track>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, \
         duration_secs, enclosure_url, enclosure_type, enclosure_bytes, track_number, \
         season, explicit, description, created_at, updated_at \
         FROM tracks WHERE feed_guid = ?1",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        let explicit_i: i64 = row.get(12)?;
        Ok(Track {
            track_guid: row.get(0)?,
            feed_guid: row.get(1)?,
            artist_credit_id: row.get(2)?,
            title: row.get(3)?,
            title_lower: row.get(4)?,
            pub_date: row.get(5)?,
            duration_secs: row.get(6)?,
            enclosure_url: row.get(7)?,
            enclosure_type: row.get(8)?,
            enclosure_bytes: row.get(9)?,
            track_number: row.get(10)?,
            season: row.get(11)?,
            explicit: explicit_i != 0,
            description: row.get(13)?,
            created_at: row.get(14)?,
            updated_at: row.get(15)?,
        })
    })?;

    let mut tracks = Vec::new();
    for row in rows {
        tracks.push(row?);
    }
    Ok(tracks)
}

// ── get_feed_payment_routes_for_feed ────────────────────────────────────────
// Issue-WRITE-AMP — 2026-03-14

/// Returns all feed-level payment routes for the given `feed_guid`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_feed_payment_routes_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<FeedPaymentRoute>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, recipient_name, route_type, address, \
         custom_key, custom_value, split, fee \
         FROM feed_payment_routes WHERE feed_guid = ?1",
    )?;
    let rows = stmt.query_map(params![feed_guid], |row| {
        let rt_str: String = row.get(3)?;
        let fee_i: i64 = row.get(8)?;
        Ok(FeedPaymentRoute {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            recipient_name: row.get(2)?,
            route_type: serde_json::from_str(&format!("\"{rt_str}\"")).unwrap_or(RouteType::Node),
            address: row.get(4)?,
            custom_key: row.get(5)?,
            custom_value: row.get(6)?,
            split: row.get(7)?,
            fee: fee_i != 0,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

// ── diff helpers ────────────────────────────────────────────────────────────
// Issue-WRITE-AMP — 2026-03-14

/// Compares two feeds by their content fields (ignoring timestamps and
/// computed fields like `episode_count`, `newest_item_at`, `oldest_item_at`).
fn feed_fields_changed(existing: &Feed, new: &Feed) -> bool {
    existing.title != new.title
        || existing.description != new.description
        || existing.image_url != new.image_url
        || existing.language != new.language
        || existing.explicit != new.explicit
        || existing.itunes_type != new.itunes_type
        || existing.raw_medium != new.raw_medium
        || existing.feed_url != new.feed_url
}

/// Compares two tracks by their content fields (ignoring timestamps).
fn track_fields_changed(existing: &Track, new: &Track) -> bool {
    existing.title != new.title
        || existing.artist_credit_id != new.artist_credit_id
        || existing.pub_date != new.pub_date
        || existing.duration_secs != new.duration_secs
        || existing.enclosure_url != new.enclosure_url
        || existing.enclosure_type != new.enclosure_type
        || existing.enclosure_bytes != new.enclosure_bytes
        || existing.track_number != new.track_number
        || existing.season != new.season
        || existing.explicit != new.explicit
        || existing.description != new.description
}

/// Compares two artists by their content fields (ignoring timestamps).
fn artist_fields_changed(existing: &Artist, new: &Artist) -> bool {
    existing.name != new.name
        || existing.sort_name != new.sort_name
        || existing.type_id != new.type_id
        || existing.area != new.area
        || existing.img_url != new.img_url
        || existing.url != new.url
        || existing.begin_year != new.begin_year
        || existing.end_year != new.end_year
}

/// Compares two sets of feed payment routes by their content fields
/// (ignoring `id` which is DB-assigned).
fn feed_routes_changed(existing: &[FeedPaymentRoute], new: &[FeedPaymentRoute]) -> bool {
    if existing.len() != new.len() {
        return true;
    }
    // Compare route-by-route; order matters.
    existing.iter().zip(new.iter()).any(|(a, b)| {
        a.recipient_name != b.recipient_name
            || a.route_type != b.route_type
            || a.address != b.address
            || a.custom_key != b.custom_key
            || a.custom_value != b.custom_value
            || a.split != b.split
            || a.fee != b.fee
    })
}

fn feed_remote_items_changed(existing: &[FeedRemoteItemRaw], new: &[FeedRemoteItemRaw]) -> bool {
    existing.len() != new.len()
        || existing.iter().zip(new.iter()).any(|(a, b)| {
            a.position != b.position
                || a.medium != b.medium
                || a.remote_feed_guid != b.remote_feed_guid
                || a.remote_feed_url != b.remote_feed_url
                || a.source != b.source
        })
}

fn live_events_changed(existing: &[LiveEvent], new: &[LiveEvent]) -> bool {
    existing.len() != new.len()
        || existing.iter().zip(new.iter()).any(|(a, b)| {
            a.live_item_guid != b.live_item_guid
                || a.title != b.title
                || a.content_link != b.content_link
                || a.status != b.status
                || a.scheduled_start != b.scheduled_start
                || a.scheduled_end != b.scheduled_end
        })
}

fn source_contributor_claims_changed(
    existing: &[SourceContributorClaim],
    new: &[SourceContributorClaim],
) -> bool {
    existing.len() != new.len()
        || existing.iter().zip(new.iter()).any(|(a, b)| {
            a.feed_guid != b.feed_guid
                || a.entity_type != b.entity_type
                || a.entity_id != b.entity_id
                || a.position != b.position
                || a.name != b.name
                || a.role != b.role
                || a.group_name != b.group_name
                || a.href != b.href
                || a.img != b.img
                || a.source != b.source
                || a.extraction_path != b.extraction_path
                || a.observed_at != b.observed_at
        })
}

fn source_entity_ids_changed(
    existing: &[SourceEntityIdClaim],
    new: &[SourceEntityIdClaim],
) -> bool {
    existing.len() != new.len()
        || existing.iter().zip(new.iter()).any(|(a, b)| {
            a.feed_guid != b.feed_guid
                || a.entity_type != b.entity_type
                || a.entity_id != b.entity_id
                || a.position != b.position
                || a.scheme != b.scheme
                || a.value != b.value
                || a.source != b.source
                || a.extraction_path != b.extraction_path
                || a.observed_at != b.observed_at
        })
}

fn source_entity_links_changed(existing: &[SourceEntityLink], new: &[SourceEntityLink]) -> bool {
    existing.len() != new.len()
        || existing.iter().zip(new.iter()).any(|(a, b)| {
            a.feed_guid != b.feed_guid
                || a.entity_type != b.entity_type
                || a.entity_id != b.entity_id
                || a.position != b.position
                || a.link_type != b.link_type
                || a.url != b.url
                || a.source != b.source
                || a.extraction_path != b.extraction_path
                || a.observed_at != b.observed_at
        })
}

fn source_release_claims_changed(
    existing: &[SourceReleaseClaim],
    new: &[SourceReleaseClaim],
) -> bool {
    existing.len() != new.len()
        || existing.iter().zip(new.iter()).any(|(a, b)| {
            a.feed_guid != b.feed_guid
                || a.entity_type != b.entity_type
                || a.entity_id != b.entity_id
                || a.position != b.position
                || a.claim_type != b.claim_type
                || a.claim_value != b.claim_value
                || a.source != b.source
                || a.extraction_path != b.extraction_path
                || a.observed_at != b.observed_at
        })
}

fn source_item_enclosures_changed(
    existing: &[SourceItemEnclosure],
    new: &[SourceItemEnclosure],
) -> bool {
    existing.len() != new.len()
        || existing.iter().zip(new.iter()).any(|(a, b)| {
            a.feed_guid != b.feed_guid
                || a.entity_type != b.entity_type
                || a.entity_id != b.entity_id
                || a.position != b.position
                || a.url != b.url
                || a.mime_type != b.mime_type
                || a.bytes != b.bytes
                || a.rel != b.rel
                || a.title != b.title
                || a.is_primary != b.is_primary
                || a.source != b.source
                || a.extraction_path != b.extraction_path
                || a.observed_at != b.observed_at
        })
}

fn source_platform_claims_changed(
    existing: &[SourcePlatformClaim],
    new: &[SourcePlatformClaim],
) -> bool {
    existing.len() != new.len()
        || existing.iter().zip(new.iter()).any(|(a, b)| {
            a.feed_guid != b.feed_guid
                || a.platform_key != b.platform_key
                || a.url != b.url
                || a.owner_name != b.owner_name
                || a.source != b.source
                || a.extraction_path != b.extraction_path
                || a.observed_at != b.observed_at
        })
}

// ── build_diff_events ───────────────────────────────────────────────────────
// Issue-WRITE-AMP — 2026-03-14

/// Queries existing DB state and builds event rows only for entities that
/// actually changed compared to what is stored.
///
/// On first ingest (feed not yet in DB), all events are emitted. On
/// re-ingest, only entities whose fields actually differ produce events.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL query or JSON serialisation fails.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the data needed to build events"
)]
pub fn build_diff_events(
    conn: &Connection,
    artist: &Artist,
    artist_credit: &ArtistCredit,
    feed: &Feed,
    remote_items: &[FeedRemoteItemRaw],
    source_contributor_claims: &[SourceContributorClaim],
    source_entity_ids: &[SourceEntityIdClaim],
    source_entity_links: &[SourceEntityLink],
    source_release_claims: &[SourceReleaseClaim],
    source_item_enclosures: &[SourceItemEnclosure],
    source_platform_claims: &[SourcePlatformClaim],
    feed_routes: &[FeedPaymentRoute],
    live_events: &[LiveEvent],
    tracks: &[(Track, Vec<PaymentRoute>, Vec<ValueTimeSplit>)],
    track_credits: &[ArtistCredit],
    now: i64,
    warnings: &[String],
) -> Result<Vec<EventRow>, DbError> {
    // Use feed existence as the primary gate: if the feed is not yet in the
    // DB, this is a first ingest and all events must be emitted. Note: the
    // artist may already exist (resolve_artist creates it before this runs),
    // so we cannot rely on artist existence alone.
    let existing_feed = get_feed_by_guid(conn, &feed.feed_guid)?;

    existing_feed.map_or_else(
        || {
            build_all_events(
                artist,
                artist_credit,
                feed,
                remote_items,
                source_contributor_claims,
                source_entity_ids,
                source_entity_links,
                source_release_claims,
                source_item_enclosures,
                source_platform_claims,
                feed_routes,
                live_events,
                tracks,
                track_credits,
                now,
                warnings,
            )
        },
        |ef| {
            build_changed_events(
                conn,
                artist,
                artist_credit,
                feed,
                remote_items,
                source_contributor_claims,
                source_entity_ids,
                source_entity_links,
                source_release_claims,
                source_item_enclosures,
                source_platform_claims,
                feed_routes,
                live_events,
                tracks,
                track_credits,
                now,
                warnings,
                &ef,
            )
        },
    )
}

/// Emits all events unconditionally (first ingest of a feed).
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors build_diff_events params"
)]
fn build_all_events(
    artist: &Artist,
    artist_credit: &ArtistCredit,
    feed: &Feed,
    remote_items: &[FeedRemoteItemRaw],
    source_contributor_claims: &[SourceContributorClaim],
    source_entity_ids: &[SourceEntityIdClaim],
    source_entity_links: &[SourceEntityLink],
    source_release_claims: &[SourceReleaseClaim],
    source_item_enclosures: &[SourceItemEnclosure],
    source_platform_claims: &[SourcePlatformClaim],
    feed_routes: &[FeedPaymentRoute],
    live_events: &[LiveEvent],
    tracks: &[(Track, Vec<PaymentRoute>, Vec<ValueTimeSplit>)],
    track_credits: &[ArtistCredit],
    now: i64,
    warnings: &[String],
) -> Result<Vec<EventRow>, DbError> {
    let mut event_rows: Vec<EventRow> = Vec::new();
    let warn_vec: Vec<String> = warnings.to_vec();

    event_rows.push(build_artist_upserted_event(artist, now, &warn_vec)?);
    event_rows.push(build_artist_credit_event(
        artist_credit,
        artist,
        now,
        &warn_vec,
    )?);
    event_rows.push(build_feed_upserted_event(
        feed,
        artist,
        artist_credit,
        now,
        &warn_vec,
    )?);

    if !feed_routes.is_empty() {
        event_rows.push(build_feed_routes_event(feed, feed_routes, now, &warn_vec)?);
    }
    if !remote_items.is_empty() {
        event_rows.push(build_feed_remote_items_event(
            feed,
            remote_items,
            now,
            &warn_vec,
        )?);
    }
    if !source_contributor_claims.is_empty() {
        event_rows.push(build_source_contributor_claims_event(
            feed,
            source_contributor_claims,
            now,
            &warn_vec,
        )?);
    }
    if !source_entity_ids.is_empty() {
        event_rows.push(build_source_entity_ids_event(
            feed,
            source_entity_ids,
            now,
            &warn_vec,
        )?);
    }
    if !source_entity_links.is_empty() {
        event_rows.push(build_source_entity_links_event(
            feed,
            source_entity_links,
            now,
            &warn_vec,
        )?);
    }
    if !source_release_claims.is_empty() {
        event_rows.push(build_source_release_claims_event(
            feed,
            source_release_claims,
            now,
            &warn_vec,
        )?);
    }
    if !source_item_enclosures.is_empty() {
        event_rows.push(build_source_item_enclosures_event(
            feed,
            source_item_enclosures,
            now,
            &warn_vec,
        )?);
    }
    if !source_platform_claims.is_empty() {
        event_rows.push(build_source_platform_claims_event(
            feed,
            source_platform_claims,
            now,
            &warn_vec,
        )?);
    }
    if !live_events.is_empty() {
        event_rows.push(build_live_events_event(feed, live_events, now, &warn_vec)?);
    }

    for (i, (track, routes, vts)) in tracks.iter().enumerate() {
        let credit = if i < track_credits.len() {
            &track_credits[i]
        } else {
            artist_credit
        };
        event_rows.push(build_track_upserted_event(
            track, routes, vts, credit, now, &warn_vec,
        )?);
    }

    Ok(event_rows)
}

/// Emits events only for entities that differ from the stored DB state.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors build_diff_events params"
)]
fn build_changed_events(
    conn: &Connection,
    artist: &Artist,
    artist_credit: &ArtistCredit,
    feed: &Feed,
    remote_items: &[FeedRemoteItemRaw],
    source_contributor_claims: &[SourceContributorClaim],
    source_entity_ids: &[SourceEntityIdClaim],
    source_entity_links: &[SourceEntityLink],
    source_release_claims: &[SourceReleaseClaim],
    source_item_enclosures: &[SourceItemEnclosure],
    source_platform_claims: &[SourcePlatformClaim],
    feed_routes: &[FeedPaymentRoute],
    live_events: &[LiveEvent],
    tracks: &[(Track, Vec<PaymentRoute>, Vec<ValueTimeSplit>)],
    track_credits: &[ArtistCredit],
    now: i64,
    warnings: &[String],
    existing_feed: &Feed,
) -> Result<Vec<EventRow>, DbError> {
    let mut event_rows: Vec<EventRow> = Vec::new();
    let warn_vec: Vec<String> = warnings.to_vec();

    // --- Artist diff ---
    let artist_changed = diff_artist(conn, artist)?;
    if artist_changed {
        event_rows.push(build_artist_upserted_event(artist, now, &warn_vec)?);
        event_rows.push(build_artist_credit_event(
            artist_credit,
            artist,
            now,
            &warn_vec,
        )?);
    }

    // --- Feed diff ---
    if feed_fields_changed(existing_feed, feed) {
        event_rows.push(build_feed_upserted_event(
            feed,
            artist,
            artist_credit,
            now,
            &warn_vec,
        )?);
    }

    // --- Feed routes diff ---
    let existing_routes = get_feed_payment_routes_for_feed(conn, &feed.feed_guid)?;
    if !feed_routes.is_empty() && feed_routes_changed(&existing_routes, feed_routes) {
        event_rows.push(build_feed_routes_event(feed, feed_routes, now, &warn_vec)?);
    }

    // --- Feed remote-item diff ---
    let existing_remote_items = get_feed_remote_items_for_feed(conn, &feed.feed_guid)?;
    if !remote_items.is_empty() && feed_remote_items_changed(&existing_remote_items, remote_items) {
        event_rows.push(build_feed_remote_items_event(
            feed,
            remote_items,
            now,
            &warn_vec,
        )?);
    }

    // --- Staged contributor-claim diff ---
    let existing_source_contributor_claims =
        get_source_contributor_claims_for_feed(conn, &feed.feed_guid)?;
    if source_contributor_claims_changed(
        &existing_source_contributor_claims,
        source_contributor_claims,
    ) {
        event_rows.push(build_source_contributor_claims_event(
            feed,
            source_contributor_claims,
            now,
            &warn_vec,
        )?);
    }

    // --- Staged entity-ID diff ---
    let existing_source_entity_ids = get_source_entity_ids_for_feed(conn, &feed.feed_guid)?;
    if source_entity_ids_changed(&existing_source_entity_ids, source_entity_ids) {
        event_rows.push(build_source_entity_ids_event(
            feed,
            source_entity_ids,
            now,
            &warn_vec,
        )?);
    }

    // --- Staged entity-link diff ---
    let existing_source_entity_links = get_source_entity_links_for_feed(conn, &feed.feed_guid)?;
    if source_entity_links_changed(&existing_source_entity_links, source_entity_links) {
        event_rows.push(build_source_entity_links_event(
            feed,
            source_entity_links,
            now,
            &warn_vec,
        )?);
    }

    // --- Staged release-claim diff ---
    let existing_source_release_claims = get_source_release_claims_for_feed(conn, &feed.feed_guid)?;
    if source_release_claims_changed(&existing_source_release_claims, source_release_claims) {
        event_rows.push(build_source_release_claims_event(
            feed,
            source_release_claims,
            now,
            &warn_vec,
        )?);
    }

    // --- Staged item-enclosure diff ---
    let existing_source_item_enclosures =
        get_source_item_enclosures_for_feed(conn, &feed.feed_guid)?;
    if source_item_enclosures_changed(&existing_source_item_enclosures, source_item_enclosures) {
        event_rows.push(build_source_item_enclosures_event(
            feed,
            source_item_enclosures,
            now,
            &warn_vec,
        )?);
    }

    // --- Staged platform-claim diff ---
    let existing_source_platform_claims =
        get_source_platform_claims_for_feed(conn, &feed.feed_guid)?;
    if source_platform_claims_changed(&existing_source_platform_claims, source_platform_claims) {
        event_rows.push(build_source_platform_claims_event(
            feed,
            source_platform_claims,
            now,
            &warn_vec,
        )?);
    }

    // --- Live-event snapshot diff ---
    let existing_live_events = get_live_events_for_feed(conn, &feed.feed_guid)?;
    if live_events_changed(&existing_live_events, live_events) {
        event_rows.push(build_live_events_event(feed, live_events, now, &warn_vec)?);
    }

    // --- Track diff ---
    let existing_tracks = get_tracks_for_feed(conn, &feed.feed_guid)?;
    let existing_map: std::collections::HashMap<&str, &Track> = existing_tracks
        .iter()
        .map(|t| (t.track_guid.as_str(), t))
        .collect();

    for (i, (track, routes, vts)) in tracks.iter().enumerate() {
        let is_new_or_changed = existing_map
            .get(track.track_guid.as_str())
            .is_none_or(|existing| track_fields_changed(existing, track));

        if is_new_or_changed {
            let credit = if i < track_credits.len() {
                &track_credits[i]
            } else {
                artist_credit
            };
            event_rows.push(build_track_upserted_event(
                track, routes, vts, credit, now, &warn_vec,
            )?);
        }
    }

    Ok(event_rows)
}

// --- private event builders (keep each under 50 lines) ---

fn diff_artist(conn: &Connection, artist: &Artist) -> Result<bool, DbError> {
    let existing = get_artist_by_id(conn, &artist.artist_id)?;
    Ok(existing.is_none_or(|e| artist_fields_changed(&e, artist)))
}

fn build_artist_upserted_event(
    artist: &Artist,
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::ArtistUpsertedPayload {
        artist: artist.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::ArtistUpserted,
        payload_json,
        subject_guid: artist.artist_id.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_artist_credit_event(
    credit: &ArtistCredit,
    artist: &Artist,
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::ArtistCreditCreatedPayload {
        artist_credit: credit.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::ArtistCreditCreated,
        payload_json,
        subject_guid: artist.artist_id.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_feed_upserted_event(
    feed: &Feed,
    artist: &Artist,
    credit: &ArtistCredit,
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::FeedUpsertedPayload {
        feed: feed.clone(),
        artist: artist.clone(),
        artist_credit: credit.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::FeedUpserted,
        payload_json,
        subject_guid: feed.feed_guid.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_feed_routes_event(
    feed: &Feed,
    routes: &[FeedPaymentRoute],
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::FeedRoutesReplacedPayload {
        feed_guid: feed.feed_guid.clone(),
        routes: routes.to_vec(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::FeedRoutesReplaced,
        payload_json,
        subject_guid: feed.feed_guid.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_feed_remote_items_event(
    feed: &Feed,
    remote_items: &[FeedRemoteItemRaw],
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::FeedRemoteItemsReplacedPayload {
        feed_guid: feed.feed_guid.clone(),
        remote_items: remote_items.to_vec(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::FeedRemoteItemsReplaced,
        payload_json,
        subject_guid: feed.feed_guid.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_live_events_event(
    feed: &Feed,
    live_events: &[LiveEvent],
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::LiveEventsReplacedPayload {
        feed_guid: feed.feed_guid.clone(),
        live_events: live_events.to_vec(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::LiveEventsReplaced,
        payload_json,
        subject_guid: feed.feed_guid.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_source_contributor_claims_event(
    feed: &Feed,
    claims: &[SourceContributorClaim],
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::SourceContributorClaimsReplacedPayload {
        feed_guid: feed.feed_guid.clone(),
        claims: claims.to_vec(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::SourceContributorClaimsReplaced,
        payload_json,
        subject_guid: feed.feed_guid.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_source_entity_ids_event(
    feed: &Feed,
    claims: &[SourceEntityIdClaim],
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::SourceEntityIdsReplacedPayload {
        feed_guid: feed.feed_guid.clone(),
        claims: claims.to_vec(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::SourceEntityIdsReplaced,
        payload_json,
        subject_guid: feed.feed_guid.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_source_entity_links_event(
    feed: &Feed,
    links: &[SourceEntityLink],
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::SourceEntityLinksReplacedPayload {
        feed_guid: feed.feed_guid.clone(),
        links: links.to_vec(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::SourceEntityLinksReplaced,
        payload_json,
        subject_guid: feed.feed_guid.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_source_release_claims_event(
    feed: &Feed,
    claims: &[SourceReleaseClaim],
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::SourceReleaseClaimsReplacedPayload {
        feed_guid: feed.feed_guid.clone(),
        claims: claims.to_vec(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::SourceReleaseClaimsReplaced,
        payload_json,
        subject_guid: feed.feed_guid.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_source_item_enclosures_event(
    feed: &Feed,
    enclosures: &[SourceItemEnclosure],
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::SourceItemEnclosuresReplacedPayload {
        feed_guid: feed.feed_guid.clone(),
        enclosures: enclosures.to_vec(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::SourceItemEnclosuresReplaced,
        payload_json,
        subject_guid: feed.feed_guid.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_source_platform_claims_event(
    feed: &Feed,
    claims: &[SourcePlatformClaim],
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::SourcePlatformClaimsReplacedPayload {
        feed_guid: feed.feed_guid.clone(),
        claims: claims.to_vec(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::SourcePlatformClaimsReplaced,
        payload_json,
        subject_guid: feed.feed_guid.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

fn build_track_upserted_event(
    track: &Track,
    routes: &[PaymentRoute],
    vts: &[ValueTimeSplit],
    credit: &ArtistCredit,
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::TrackUpsertedPayload {
        track: track.clone(),
        routes: routes.to_vec(),
        value_time_splits: vts.to_vec(),
        artist_credit: credit.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::TrackUpserted,
        payload_json,
        subject_guid: track.track_guid.clone(),
        created_at: now,
        warnings: warnings.to_vec(),
    })
}

// ── insert_event ──────────────────────────────────────────────────────────────

/// Inserts a single event row, signs it with the DB-assigned `seq`, and
/// returns `(seq, signed_by, signature)`.
///
/// The event is inserted with a placeholder signature first so the
/// DB can assign a monotonic `seq`. The signature is then computed
/// over the full signing payload (including `seq`) and written back.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL insert, update, or JSON serialisation fails.
// Issue-SEQ-INTEGRITY — 2026-03-14
#[expect(
    clippy::too_many_arguments,
    reason = "all fields are required for a complete event row"
)]
pub fn insert_event(
    conn: &Connection,
    event_id: &str,
    event_type: &EventType,
    payload_json: &str,
    subject_guid: &str,
    signer: &NodeSigner,
    created_at: i64,
    warnings: &[String],
) -> Result<(i64, String, String), DbError> {
    let et_str = event_type_str(event_type)?;
    let warnings_json = serde_json::to_string(warnings)?;

    // Issue-SEQ-INTEGRITY — 2026-03-14
    // Insert with placeholder signature to get the DB-assigned seq.
    let sql = "INSERT INTO events \
        (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at, warnings_json) \
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(seq),0)+1 FROM events), ?7, ?8) \
        RETURNING seq";

    let seq = conn.query_row(
        sql,
        params![
            event_id,
            et_str,
            payload_json,
            subject_guid,
            signer.pubkey_hex(),
            "",
            created_at,
            warnings_json
        ],
        |row| row.get::<_, i64>(0),
    )?;

    // Sign with the assigned seq and update the row.
    let (signed_by, signature) = signer.sign_event(
        event_id,
        event_type,
        payload_json,
        subject_guid,
        created_at,
        seq,
    );
    update_event_signature(conn, event_id, &signed_by, &signature)?;

    Ok((seq, signed_by, signature))
}

// ── update_event_signature ─────────────────────────────────────────────────

/// Updates the `signed_by` and `signature` columns for an existing event row.
///
/// Used by the primary after inserting an event to get the DB-assigned `seq`,
/// signing the event (including seq), and backfilling the real signature.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL update fails.
// Issue-SEQ-INTEGRITY — 2026-03-14
pub fn update_event_signature(
    conn: &Connection,
    event_id: &str,
    signed_by: &str,
    signature: &str,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE events SET signed_by = ?1, signature = ?2 WHERE event_id = ?3",
        params![signed_by, signature, event_id],
    )?;
    Ok(())
}

// ── upsert_feed_crawl_cache ───────────────────────────────────────────────────

/// Records the latest content hash and crawl timestamp for `feed_url`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL upsert fails.
pub fn upsert_feed_crawl_cache(
    conn: &Connection,
    feed_url: &str,
    content_hash: &str,
    crawled_at: i64,
) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO feed_crawl_cache (feed_url, content_hash, crawled_at) \
         VALUES (?1, ?2, ?3) \
         ON CONFLICT(feed_url) DO UPDATE SET \
           content_hash = excluded.content_hash, \
           crawled_at   = excluded.crawled_at",
        params![feed_url, content_hash, crawled_at],
    )?;
    Ok(())
}

// ── get_events_since ──────────────────────────────────────────────────────────

/// Returns up to `limit` events with `seq > after_seq`, ordered ascending.
///
/// # Errors
///
/// Returns [`DbError`] if a SQL query fails or event JSON cannot be deserialised.
pub fn get_events_since(
    conn: &Connection,
    after_seq: i64,
    limit: i64,
) -> Result<Vec<Event>, DbError> {
    // Issue-NEGATIVE-LIMIT — 2026-03-15
    let safe_limit = limit.max(1);
    let mut stmt = conn.prepare(
        "SELECT event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at, warnings_json \
         FROM events WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![after_seq, safe_limit], |row| {
        Ok((
            row.get::<_, String>(0)?, // event_id
            row.get::<_, String>(1)?, // event_type string
            row.get::<_, String>(2)?, // payload_json
            row.get::<_, String>(3)?, // subject_guid
            row.get::<_, String>(4)?, // signed_by
            row.get::<_, String>(5)?, // signature
            row.get::<_, i64>(6)?,    // seq
            row.get::<_, i64>(7)?,    // created_at
            row.get::<_, String>(8)?, // warnings_json
        ))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (
            event_id,
            et_str,
            payload_json,
            subject_guid,
            signed_by,
            signature,
            seq,
            created_at,
            warnings_json,
        ) = row?;

        let et_quoted = format!("\"{et_str}\"");
        let event_type: EventType = serde_json::from_str(&et_quoted)?;

        let tagged = format!(r#"{{"type":"{et_str}","data":{payload_json}}}"#);
        let payload: EventPayload = serde_json::from_str(&tagged)?;
        let warnings: Vec<String> = serde_json::from_str(&warnings_json)?;

        events.push(Event {
            event_id,
            event_type,
            payload,
            payload_json: payload_json.clone(),
            subject_guid,
            signed_by,
            signature,
            seq,
            created_at,
            warnings,
        });
    }

    Ok(events)
}

// ── get_event_refs_since ──────────────────────────────────────────────────────

// Finding-5 reconcile pagination — 2026-03-13
/// Returns lightweight `(event_id, seq)` references for events with `seq >= since_seq`,
/// bounded by `limit` to prevent unbounded memory usage.
///
/// Returns a tuple of `(refs, truncated)` where `truncated` is `true` when
/// more rows exist beyond the limit.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_event_refs_since(
    conn: &Connection,
    since_seq: i64,
    limit: i64,
) -> Result<(Vec<crate::sync::EventRef>, bool), DbError> {
    // Issue-NEGATIVE-LIMIT — 2026-03-15
    let limit = limit.max(1);
    // Fetch limit + 1 to detect truncation without a separate COUNT query.
    let fetch_limit = limit.saturating_add(1);
    let mut stmt =
        conn.prepare("SELECT event_id, seq FROM events WHERE seq >= ?1 ORDER BY seq ASC LIMIT ?2")?;

    let rows = stmt.query_map(params![since_seq, fetch_limit], |row| {
        Ok(crate::sync::EventRef {
            event_id: row.get(0)?,
            seq: row.get(1)?,
        })
    })?;

    let mut refs = Vec::new();
    for row in rows {
        refs.push(row?);
    }

    let truncated = i64::try_from(refs.len()).unwrap_or(i64::MAX) > limit;
    if truncated {
        refs.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    }

    Ok((refs, truncated))
}

// ── upsert_node_sync_state ────────────────────────────────────────────────────

/// Records or updates the last-seen sequence number for a peer node.
///
/// The cursor is monotonic: the stored `last_seq` can only increase.
/// `MAX(last_seq, excluded.last_seq)` prevents regression when events
/// are applied out of order (e.g. seq=15 then seq=10).
///
/// # Errors
///
/// Returns [`DbError`] if the SQL upsert fails.
// Issue-CURSOR-MONOTONIC — 2026-03-14
pub fn upsert_node_sync_state(
    conn: &Connection,
    node_pubkey: &str,
    last_seq: i64,
    last_seen_at: i64,
) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO node_sync_state (node_pubkey, last_seq, last_seen_at) \
         VALUES (?1, ?2, ?3) \
         ON CONFLICT(node_pubkey) DO UPDATE SET \
           last_seq     = MAX(last_seq, excluded.last_seq), \
           last_seen_at = excluded.last_seen_at",
        params![node_pubkey, last_seq, last_seen_at],
    )?;
    Ok(())
}

// ── peer_nodes ────────────────────────────────────────────────────────────────

/// Maximum consecutive push failures before a peer is considered unhealthy.
/// Used for both startup reload and runtime eviction.
// Issue-PEER-THRESHOLD — 2026-03-16
pub const MAX_PEER_FAILURES: i64 = 10;

/// A peer node registered for push fan-out.
#[derive(Debug)]
pub struct PeerNode {
    pub node_pubkey: String,
    pub node_url: String,
    pub discovered_at: i64,
    pub last_push_at: Option<i64>,
    pub consecutive_failures: i64,
}

/// Returns all peers with `consecutive_failures < MAX_PEER_FAILURES`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
// Issue-PEER-THRESHOLD — 2026-03-16
pub fn get_push_peers(conn: &Connection) -> Result<Vec<PeerNode>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT node_pubkey, node_url, discovered_at, last_push_at, consecutive_failures \
         FROM peer_nodes WHERE consecutive_failures < ?1",
    )?;

    let rows = stmt.query_map(rusqlite::params![MAX_PEER_FAILURES], |row| {
        Ok(PeerNode {
            node_pubkey: row.get(0)?,
            node_url: row.get(1)?,
            discovered_at: row.get(2)?,
            last_push_at: row.get(3)?,
            consecutive_failures: row.get(4)?,
        })
    })?;

    let mut peers = Vec::new();
    for row in rows {
        peers.push(row?);
    }
    Ok(peers)
}

/// Upserts a peer node record.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL upsert fails.
pub fn upsert_peer_node(
    conn: &Connection,
    node_pubkey: &str,
    node_url: &str,
    now: i64,
) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO peer_nodes (node_pubkey, node_url, discovered_at) \
         VALUES (?1, ?2, ?3) \
         ON CONFLICT(node_pubkey) DO UPDATE SET node_url = excluded.node_url",
        rusqlite::params![node_pubkey, node_url, now],
    )?;
    Ok(())
}

/// Records a successful push delivery.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL update fails.
pub fn record_push_success(conn: &Connection, node_pubkey: &str, now: i64) -> Result<(), DbError> {
    conn.execute(
        "UPDATE peer_nodes SET last_push_at = ?1, consecutive_failures = 0 \
         WHERE node_pubkey = ?2",
        rusqlite::params![now, node_pubkey],
    )?;
    Ok(())
}

/// Increments `consecutive_failures` by 1 for the given peer.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL update fails.
pub fn increment_peer_failures(conn: &Connection, node_pubkey: &str) -> Result<(), DbError> {
    conn.execute(
        "UPDATE peer_nodes SET consecutive_failures = consecutive_failures + 1 \
         WHERE node_pubkey = ?1",
        rusqlite::params![node_pubkey],
    )?;
    Ok(())
}

/// Resets `consecutive_failures` to 0 for the given peer.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL update fails.
pub fn reset_peer_failures(conn: &Connection, node_pubkey: &str) -> Result<(), DbError> {
    conn.execute(
        "UPDATE peer_nodes SET consecutive_failures = 0 WHERE node_pubkey = ?1",
        rusqlite::params![node_pubkey],
    )?;
    Ok(())
}

/// Inserts a single event row using `INSERT OR IGNORE`.
///
/// Returns `Some(seq)` if the event was newly inserted, or `None` if a row
/// with the same `event_id` already existed (idempotent community-side apply).
///
/// # Errors
///
/// Returns [`DbError`] if the SQL insert or JSON serialisation fails.
#[expect(
    clippy::too_many_arguments,
    reason = "all fields are required for a complete event row"
)]
pub fn insert_event_idempotent(
    conn: &Connection,
    event_id: &str,
    event_type: &crate::event::EventType,
    payload_json: &str,
    subject_guid: &str,
    signed_by: &str,
    signature: &str,
    created_at: i64,
    warnings: &[String],
) -> Result<Option<i64>, DbError> {
    // Issue-3 RETURNING seq — 2026-03-13
    let et_str = event_type_str(event_type)?;
    let warnings_json = serde_json::to_string(warnings)?;

    let sql = "INSERT OR IGNORE INTO events \
        (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at, warnings_json) \
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(seq),0)+1 FROM events), ?7, ?8) \
        RETURNING seq";

    let seq: Option<i64> = conn
        .query_row(
            sql,
            rusqlite::params![
                event_id,
                et_str,
                payload_json,
                subject_guid,
                signed_by,
                signature,
                created_at,
                warnings_json
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;

    Ok(seq)
}

// ── get_node_sync_cursor ──────────────────────────────────────────────────────

/// Returns the `last_seq` cursor stored for `node_pubkey`, or `0` if none exists.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_node_sync_cursor(conn: &Connection, node_pubkey: &str) -> Result<i64, DbError> {
    let seq: Option<i64> = conn
        .query_row(
            "SELECT last_seq FROM node_sync_state WHERE node_pubkey = ?1",
            params![node_pubkey],
            |row| row.get(0),
        )
        .optional()?;
    Ok(seq.unwrap_or(0))
}

// ── resolver queue ───────────────────────────────────────────────────────────

/// Dirty bit for canonical release/recording rebuilds.
pub const RESOLVER_DIRTY_CANONICAL_STATE: i64 = 1;
/// Dirty bit for canonical promotion rows.
pub const RESOLVER_DIRTY_CANONICAL_PROMOTIONS: i64 = 1 << 1;
/// Dirty bit for canonical search rows.
pub const RESOLVER_DIRTY_CANONICAL_SEARCH: i64 = 1 << 2;
/// Reserved dirty bit for incremental artist identity work.
pub const RESOLVER_DIRTY_ARTIST_IDENTITY: i64 = 1 << 3;
/// Dirty bit for source-layer search and quality read models.
pub const RESOLVER_DIRTY_SOURCE_READ_MODELS: i64 = 1 << 4;

const RESOLVER_LOCK_STALE_AFTER_SECS: i64 = 15 * 60;
const RESOLVER_IMPORT_HEARTBEAT_STALE_AFTER_SECS: i64 = 10 * 60;

/// A claimed resolver queue row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolverQueueEntry {
    pub feed_guid: String,
    pub dirty_mask: i64,
    pub first_marked_at: i64,
    pub last_marked_at: i64,
    pub locked_at: Option<i64>,
    pub locked_by: Option<String>,
    pub attempt_count: i64,
    pub last_error: Option<String>,
}

/// Aggregate counts for the resolver queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolverQueueCounts {
    pub total: i64,
    pub ready: i64,
    pub locked: i64,
    pub failed: i64,
}

/// Import pause state for the resolver worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolverImportState {
    pub active: bool,
    pub stale: bool,
    pub heartbeat_at: Option<i64>,
}

/// Inserts or updates a dirty-feed row in the resolver queue.
pub fn mark_feed_dirty(conn: &Connection, feed_guid: &str, dirty_mask: i64) -> Result<(), DbError> {
    let feed_exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM feeds WHERE feed_guid = ?1",
            params![feed_guid],
            |row| row.get(0),
        )
        .optional()?;
    if feed_exists.is_none() {
        return Ok(());
    }

    let now = unix_now();
    conn.execute(
        "INSERT INTO resolver_queue (
             feed_guid, dirty_mask, first_marked_at, last_marked_at
         ) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(feed_guid) DO UPDATE SET
             dirty_mask = resolver_queue.dirty_mask | excluded.dirty_mask,
             last_marked_at = excluded.last_marked_at,
             last_error = NULL",
        params![feed_guid, dirty_mask, now, now],
    )?;
    Ok(())
}

/// Claims up to `limit` dirty feeds for `worker_id`.
pub fn claim_dirty_feeds(
    conn: &mut Connection,
    worker_id: &str,
    limit: i64,
    now: i64,
) -> Result<Vec<ResolverQueueEntry>, DbError> {
    let safe_limit = limit.max(1);
    let stale_before = now - RESOLVER_LOCK_STALE_AFTER_SECS;
    let tx = conn.transaction()?;

    let claimed: Vec<ResolverQueueEntry> = {
        let mut stmt = tx.prepare(
            "SELECT
                 feed_guid,
                 dirty_mask,
                 first_marked_at,
                 last_marked_at,
                 locked_at,
                 locked_by,
                 attempt_count,
                 last_error
             FROM resolver_queue
             WHERE dirty_mask != 0
               AND (locked_at IS NULL OR locked_at < ?1)
             ORDER BY last_marked_at ASC, first_marked_at ASC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![stale_before, safe_limit], |row| {
            Ok(ResolverQueueEntry {
                feed_guid: row.get(0)?,
                dirty_mask: row.get(1)?,
                first_marked_at: row.get(2)?,
                last_marked_at: row.get(3)?,
                locked_at: row.get(4)?,
                locked_by: row.get(5)?,
                attempt_count: row.get(6)?,
                last_error: row.get(7)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        entries
    };

    for entry in &claimed {
        tx.execute(
            "UPDATE resolver_queue
             SET locked_at = ?1, locked_by = ?2
             WHERE feed_guid = ?3",
            params![now, worker_id, entry.feed_guid],
        )?;
    }

    tx.commit()?;
    Ok(claimed)
}

/// Completes a claimed dirty-feed row.
pub fn complete_dirty_feed(
    conn: &Connection,
    feed_guid: &str,
    worker_id: &str,
) -> Result<(), DbError> {
    let deleted = conn.execute(
        "DELETE FROM resolver_queue
         WHERE feed_guid = ?1
           AND locked_by = ?2
           AND COALESCE(last_marked_at, 0) <= COALESCE(locked_at, 0)",
        params![feed_guid, worker_id],
    )?;

    if deleted == 0 {
        conn.execute(
            "UPDATE resolver_queue
             SET locked_at = NULL,
                 locked_by = NULL,
                 attempt_count = 0,
                 last_error = NULL
             WHERE feed_guid = ?1
               AND locked_by = ?2",
            params![feed_guid, worker_id],
        )?;
    }

    Ok(())
}

/// Unlocks a claimed dirty-feed row and records an error.
pub fn fail_dirty_feed(
    conn: &Connection,
    feed_guid: &str,
    worker_id: &str,
    error: &str,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE resolver_queue
         SET locked_at = NULL,
             locked_by = NULL,
             attempt_count = attempt_count + 1,
             last_error = ?3
         WHERE feed_guid = ?1
           AND locked_by = ?2",
        params![feed_guid, worker_id, error],
    )?;
    Ok(())
}

/// Clears a subset of dirty bits for one queued feed, deleting the queue row
/// when no work remains.
pub fn clear_feed_dirty_bits(conn: &Connection, feed_guid: &str, mask: i64) -> Result<(), DbError> {
    conn.execute(
        "UPDATE resolver_queue
         SET dirty_mask = dirty_mask & ~?2
         WHERE feed_guid = ?1",
        params![feed_guid, mask],
    )?;
    conn.execute(
        "DELETE FROM resolver_queue
         WHERE feed_guid = ?1 AND dirty_mask = 0",
        params![feed_guid],
    )?;
    Ok(())
}

/// Stores a resolver coordination state value.
pub fn set_resolver_state(conn: &Connection, key: &str, value: &str) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO resolver_state (key, value)
         VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Returns the stored resolver coordination state value for `key`.
pub fn get_resolver_state(conn: &Connection, key: &str) -> Result<Option<String>, DbError> {
    conn.query_row(
        "SELECT value FROM resolver_state WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

/// Sets the `import_active` resolver coordination flag.
pub fn set_resolver_import_active(conn: &Connection, active: bool) -> Result<(), DbError> {
    set_resolver_import_active_with_now(conn, active, unix_now())
}

/// Sets the `import_active` resolver coordination flag at a specific timestamp.
pub fn set_resolver_import_active_with_now(
    conn: &Connection,
    active: bool,
    now: i64,
) -> Result<(), DbError> {
    set_resolver_state(conn, "import_active", if active { "true" } else { "false" })?;
    if active {
        set_resolver_state(conn, "import_heartbeat_at", &now.to_string())?;
    } else {
        conn.execute(
            "DELETE FROM resolver_state WHERE key = 'import_heartbeat_at'",
            [],
        )?;
    }
    Ok(())
}

/// Refreshes the import heartbeat while a bulk import is active.
pub fn touch_resolver_import_active(conn: &Connection) -> Result<(), DbError> {
    touch_resolver_import_active_with_now(conn, unix_now())
}

/// Refreshes the import heartbeat at a specific timestamp.
pub fn touch_resolver_import_active_with_now(conn: &Connection, now: i64) -> Result<(), DbError> {
    let active_flag = matches!(
        get_resolver_state(conn, "import_active")?.as_deref(),
        Some("1" | "true" | "yes" | "on")
    );
    if active_flag {
        set_resolver_state(conn, "import_heartbeat_at", &now.to_string())?;
    }
    Ok(())
}

/// Returns whether bulk import is currently marked active.
pub fn resolver_import_active(conn: &Connection) -> Result<bool, DbError> {
    Ok(resolver_import_state(conn)?.active)
}

/// Returns bulk-import pause state, including whether the heartbeat is stale.
pub fn resolver_import_state(conn: &Connection) -> Result<ResolverImportState, DbError> {
    let flag = matches!(
        get_resolver_state(conn, "import_active")?.as_deref(),
        Some("1" | "true" | "yes" | "on")
    );
    let heartbeat_at = get_resolver_state(conn, "import_heartbeat_at")?
        .and_then(|value| value.parse::<i64>().ok());
    let stale = flag
        && heartbeat_at
            .is_none_or(|ts| ts < unix_now() - RESOLVER_IMPORT_HEARTBEAT_STALE_AFTER_SECS);
    Ok(ResolverImportState {
        active: flag && !stale,
        stale,
        heartbeat_at,
    })
}

/// Returns aggregate queue counts for operator inspection.
pub fn get_resolver_queue_counts(conn: &Connection) -> Result<ResolverQueueCounts, DbError> {
    conn.query_row(
        "SELECT
             COUNT(*),
             COALESCE(SUM(CASE WHEN locked_at IS NULL THEN 1 ELSE 0 END), 0),
             COALESCE(SUM(CASE WHEN locked_at IS NOT NULL THEN 1 ELSE 0 END), 0),
             COALESCE(SUM(CASE WHEN last_error IS NOT NULL THEN 1 ELSE 0 END), 0)
         FROM resolver_queue",
        [],
        |row| {
            Ok(ResolverQueueCounts {
                total: row.get(0)?,
                ready: row.get(1)?,
                locked: row.get(2)?,
                failed: row.get(3)?,
            })
        },
    )
    .map_err(Into::into)
}

// ── Tags ─────────────────────────────────────────────────────────────────────

/// Returns the id of an existing tag with the given (lowercased) name, or
/// creates a new one and returns its id.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL insert or query fails.
pub fn get_or_create_tag(conn: &Connection, name: &str) -> Result<i64, DbError> {
    let lower = name.to_lowercase();
    let now = unix_now();

    conn.execute(
        "INSERT OR IGNORE INTO tags (name, created_at) VALUES (?1, ?2)",
        params![lower, now],
    )?;

    let id: i64 = conn.query_row(
        "SELECT id FROM tags WHERE name = ?1",
        params![lower],
        |row| row.get(0),
    )?;

    Ok(id)
}

/// Inserts a tag association into the appropriate junction table based on
/// `entity_type` ("artist", "feed", or "track").
///
/// # Errors
///
/// Returns [`DbError`] if the SQL insert fails.
pub fn apply_tag(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    tag_id: i64,
) -> Result<(), DbError> {
    let now = unix_now();
    match entity_type {
        "artist" => {
            conn.execute(
                "INSERT OR IGNORE INTO artist_tag (artist_id, tag_id, created_at) VALUES (?1, ?2, ?3)",
                params![entity_id, tag_id, now],
            )?;
        }
        "feed" => {
            conn.execute(
                "INSERT OR IGNORE INTO feed_tag (feed_guid, tag_id, created_at) VALUES (?1, ?2, ?3)",
                params![entity_id, tag_id, now],
            )?;
        }
        "track" => {
            conn.execute(
                "INSERT OR IGNORE INTO track_tag (track_guid, tag_id, created_at) VALUES (?1, ?2, ?3)",
                params![entity_id, tag_id, now],
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// Removes a tag association from the appropriate junction table.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL delete fails.
pub fn remove_tag(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    tag_id: i64,
) -> Result<(), DbError> {
    match entity_type {
        "artist" => {
            conn.execute(
                "DELETE FROM artist_tag WHERE artist_id = ?1 AND tag_id = ?2",
                params![entity_id, tag_id],
            )?;
        }
        "feed" => {
            conn.execute(
                "DELETE FROM feed_tag WHERE feed_guid = ?1 AND tag_id = ?2",
                params![entity_id, tag_id],
            )?;
        }
        "track" => {
            conn.execute(
                "DELETE FROM track_tag WHERE track_guid = ?1 AND tag_id = ?2",
                params![entity_id, tag_id],
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// Returns `(tag_id, name)` pairs for all tags associated with an entity.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL query fails.
pub fn get_tags_for_entity(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<(i64, String)>, DbError> {
    let result = match entity_type {
        "artist" => {
            let mut stmt = conn.prepare(
                "SELECT t.id, t.name FROM tags t \
                 JOIN artist_tag at ON at.tag_id = t.id \
                 WHERE at.artist_id = ?1 ORDER BY t.name",
            )?;
            stmt.query_map(params![entity_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?
        }
        "feed" => {
            let mut stmt = conn.prepare(
                "SELECT t.id, t.name FROM tags t \
                 JOIN feed_tag ft ON ft.tag_id = t.id \
                 WHERE ft.feed_guid = ?1 ORDER BY t.name",
            )?;
            stmt.query_map(params![entity_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?
        }
        "track" => {
            let mut stmt = conn.prepare(
                "SELECT t.id, t.name FROM tags t \
                 JOIN track_tag tt ON tt.tag_id = t.id \
                 WHERE tt.track_guid = ?1 ORDER BY t.name",
            )?;
            stmt.query_map(params![entity_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => Vec::new(),
    };
    Ok(result)
}

// ── Relationships ────────────────────────────────────────────────────────────

/// Row returned by [`get_artist_rels`].
#[derive(Debug)]
pub struct ArtistRelRow {
    pub id: i64,
    pub artist_id_a: String,
    pub artist_id_b: String,
    pub rel_type_name: String,
    pub begin_year: Option<i64>,
    pub end_year: Option<i64>,
}

/// Checks whether a `rel_type_id` exists in the `rel_type` lookup table.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn validate_rel_type(conn: &Connection, rel_type_id: i64) -> Result<bool, DbError> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM rel_type WHERE id = ?1",
            params![rel_type_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

/// Creates an artist-to-artist relationship. Returns the new row id.
///
/// Validates `rel_type_id` before inserting.
///
/// # Errors
///
/// Returns [`DbError`] if the rel type is invalid or the SQL insert fails.
pub fn create_artist_artist_rel(
    conn: &Connection,
    artist_id_a: &str,
    artist_id_b: &str,
    rel_type_id: i64,
    begin_year: Option<i64>,
    end_year: Option<i64>,
) -> Result<i64, DbError> {
    // Validate rel_type_id exists.
    let valid = validate_rel_type(conn, rel_type_id)?;
    if !valid {
        return Err(DbError::Rusqlite(rusqlite::Error::QueryReturnedNoRows));
    }

    let now = unix_now();
    conn.execute(
        "INSERT INTO artist_artist_rel (artist_id_a, artist_id_b, rel_type_id, begin_year, end_year, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![artist_id_a, artist_id_b, rel_type_id, begin_year, end_year, now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Returns all artist-artist relationships where `artist_id` appears on
/// either side (as `artist_id_a` or `artist_id_b`), joined with the
/// `rel_type` name.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_artist_rels(conn: &Connection, artist_id: &str) -> Result<Vec<ArtistRelRow>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT aar.id, aar.artist_id_a, aar.artist_id_b, rt.name, aar.begin_year, aar.end_year \
         FROM artist_artist_rel aar \
         JOIN rel_type rt ON rt.id = aar.rel_type_id \
         WHERE aar.artist_id_a = ?1 OR aar.artist_id_b = ?1 \
         ORDER BY aar.id",
    )?;

    let rows: Vec<ArtistRelRow> = stmt
        .query_map(params![artist_id], |row| {
            Ok(ArtistRelRow {
                id: row.get(0)?,
                artist_id_a: row.get(1)?,
                artist_id_b: row.get(2)?,
                rel_type_name: row.get(3)?,
                begin_year: row.get(4)?,
                end_year: row.get(5)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    Ok(rows)
}

/// Creates a track-to-track relationship. Returns the new row id.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL insert fails.
pub fn create_track_rel(
    conn: &Connection,
    track_guid_a: &str,
    track_guid_b: &str,
    rel_type_id: i64,
) -> Result<i64, DbError> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO track_rel (track_guid_a, track_guid_b, rel_type_id, created_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![track_guid_a, track_guid_b, rel_type_id, now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Creates a feed-to-feed relationship. Returns the new row id.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL insert fails.
pub fn create_feed_rel(
    conn: &Connection,
    feed_guid_a: &str,
    feed_guid_b: &str,
    rel_type_id: i64,
) -> Result<i64, DbError> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO feed_rel (feed_guid_a, feed_guid_b, rel_type_id, created_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![feed_guid_a, feed_guid_b, rel_type_id, now],
    )?;
    Ok(conn.last_insert_rowid())
}

// ── get_existing_feed ─────────────────────────────────────────────────────────

/// Looks up the feed row whose `feed_url` matches, returning `None` if absent.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_existing_feed(conn: &Connection, feed_url: &str) -> Result<Option<Feed>, DbError> {
    let result = conn.query_row(
        "SELECT feed_guid, feed_url, title, title_lower, artist_credit_id, description, image_url, \
         language, explicit, itunes_type, episode_count, newest_item_at, oldest_item_at, \
         created_at, updated_at, raw_medium \
         FROM feeds WHERE feed_url = ?1",
        params![feed_url],
        |row| {
            let explicit_i: i64 = row.get(8)?;
            Ok(Feed {
                feed_guid:        row.get(0)?,
                feed_url:         row.get(1)?,
                title:            row.get(2)?,
                title_lower:      row.get(3)?,
                artist_credit_id: row.get(4)?,
                description:      row.get(5)?,
                image_url:        row.get(6)?,
                language:         row.get(7)?,
                explicit:         explicit_i != 0,
                itunes_type:      row.get(9)?,
                episode_count:    row.get(10)?,
                newest_item_at:   row.get(11)?,
                oldest_item_at:   row.get(12)?,
                created_at:       row.get(13)?,
                updated_at:       row.get(14)?,
                raw_medium:       row.get(15)?,
            })
        },
    ).optional()?;

    Ok(result)
}

// ── ingest_transaction ────────────────────────────────────────────────────────

// NOTE: The feed and track upsert SQL below duplicates the standalone
// `upsert_feed` and `upsert_track` functions. This is intentional: those
// functions take `&Connection`, but inside a transaction we must use the
// `&Transaction` handle so all writes participate in the same atomic commit.
/// Writes a complete feed ingest atomically and returns the new event `seq` values.
///
/// Upserts the artist, creates the artist credit, upserts the feed (with feed
/// payment routes), all tracks (with payment routes and value-time splits),
/// and inserts the supplied event rows — all inside one `SQLite` transaction.
///
/// Tracks that existed in the DB for this feed but are absent from the new
/// crawl are removed: their search-index and quality rows are cleaned up,
/// the track row is cascade-deleted, and a signed `TrackRemoved` event is
/// emitted — all within the same transaction (Issue-STALE-TRACKS).
///
/// # Errors
///
/// Returns [`DbError`] if any SQL statement, JSON serialisation, or the
/// transaction commit fails.
// Issue-SEQ-INTEGRITY — 2026-03-14
// Issue-STALE-TRACKS — 2026-03-14
#[expect(
    clippy::too_many_lines,
    reason = "single atomic transaction — splitting would obscure the transactional boundary"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "takes ownership to make the transaction boundary clear at call sites"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "Issue-SEQ-INTEGRITY added signer param; grouping into a struct would obscure the call-site types"
)]
pub fn ingest_transaction(
    conn: &mut Connection,
    artist: Artist,
    artist_credit: ArtistCredit,
    feed: Feed,
    remote_items: Vec<FeedRemoteItemRaw>,
    source_contributor_claims: Vec<SourceContributorClaim>,
    source_entity_ids: Vec<SourceEntityIdClaim>,
    source_entity_links: Vec<SourceEntityLink>,
    source_release_claims: Vec<SourceReleaseClaim>,
    source_item_enclosures: Vec<SourceItemEnclosure>,
    source_platform_claims: Vec<SourcePlatformClaim>,
    feed_routes: Vec<FeedPaymentRoute>,
    live_events: Vec<LiveEvent>,
    tracks: Vec<(Track, Vec<PaymentRoute>, Vec<ValueTimeSplit>)>,
    event_rows: Vec<EventRow>,
    signer: &NodeSigner,
) -> Result<Vec<(i64, String, String)>, DbError> {
    let tx = conn.transaction()?;

    // 1. Resolve/insert artist (and ensure a feed-scoped canonical alias row exists)
    // Issue-ARTIST-IDENTITY — 2026-03-14
    {
        let name_lower = artist.name.to_lowercase();
        let feed_guid_ref = Some(feed.feed_guid.as_str());

        // Check if this artist already exists (by artist_id PK, not by name).
        let existing: Option<String> = tx
            .query_row(
                "SELECT artist_id FROM artists WHERE artist_id = ?1",
                params![artist.artist_id],
                |row| row.get(0),
            )
            .optional()?;

        if existing.is_none() {
            tx.execute(
                "INSERT INTO artists (artist_id, name, name_lower, sort_name, type_id, area, \
                 img_url, url, begin_year, end_year, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    artist.artist_id,
                    artist.name,
                    name_lower,
                    artist.sort_name,
                    artist.type_id,
                    artist.area,
                    artist.img_url,
                    artist.url,
                    artist.begin_year,
                    artist.end_year,
                    artist.created_at,
                    artist.updated_at,
                ],
            )?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO artist_aliases (alias_lower, artist_id, feed_guid, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                name_lower,
                artist.artist_id,
                feed_guid_ref,
                artist.created_at
            ],
        )?;
    }

    // 2. Insert artist credit (idempotent via INSERT OR IGNORE on PK)
    // Issue-ARTIST-IDENTITY — 2026-03-14
    {
        tx.execute(
            "INSERT OR IGNORE INTO artist_credit (id, display_name, feed_guid, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                artist_credit.id,
                artist_credit.display_name,
                artist_credit.feed_guid,
                artist_credit.created_at
            ],
        )?;
        for acn in &artist_credit.names {
            tx.execute(
                "INSERT OR IGNORE INTO artist_credit_name \
                 (artist_credit_id, artist_id, position, name, join_phrase) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    acn.artist_credit_id,
                    acn.artist_id,
                    acn.position,
                    acn.name,
                    acn.join_phrase
                ],
            )?;
        }
    }

    // 3. Upsert feed
    tx.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, description, image_url, \
         language, explicit, itunes_type, episode_count, newest_item_at, oldest_item_at, created_at, \
         updated_at, raw_medium) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16) \
         ON CONFLICT(feed_guid) DO UPDATE SET \
           feed_url         = excluded.feed_url, \
           title            = excluded.title, \
           title_lower      = excluded.title_lower, \
           artist_credit_id = excluded.artist_credit_id, \
           description      = excluded.description, \
           image_url        = excluded.image_url, \
           language         = excluded.language, \
           explicit         = excluded.explicit, \
           itunes_type      = excluded.itunes_type, \
           episode_count    = excluded.episode_count, \
           newest_item_at   = excluded.newest_item_at, \
           oldest_item_at   = excluded.oldest_item_at, \
           updated_at       = excluded.updated_at, \
           raw_medium       = excluded.raw_medium",
        params![
            feed.feed_guid,
            feed.feed_url,
            feed.title,
            feed.title_lower,
            feed.artist_credit_id,
            feed.description,
            feed.image_url,
            feed.language,
            i64::from(feed.explicit),
            feed.itunes_type,
            feed.episode_count,
            feed.newest_item_at,
            feed.oldest_item_at,
            feed.created_at,
            feed.updated_at,
            feed.raw_medium,
        ],
    )?;

    // 3b. Replace feed-level payment routes
    tx.execute(
        "DELETE FROM feed_payment_routes WHERE feed_guid = ?1",
        params![feed.feed_guid],
    )?;
    for r in &feed_routes {
        let route_type = serde_json::to_string(&r.route_type)?;
        let route_type = route_type.trim_matches('"');
        tx.execute(
            "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, \
             custom_key, custom_value, split, fee) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                r.feed_guid,
                r.recipient_name,
                route_type,
                r.address,
                r.custom_key,
                r.custom_value,
                r.split,
                i64::from(r.fee),
            ],
        )?;
    }

    // 3c. Replace feed-level raw remote-item refs
    tx.execute(
        "DELETE FROM feed_remote_items_raw WHERE feed_guid = ?1",
        params![feed.feed_guid],
    )?;
    for item in &remote_items {
        tx.execute(
            "INSERT INTO feed_remote_items_raw \
             (feed_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &item.feed_guid,
                item.position,
                &item.medium,
                &item.remote_feed_guid,
                &item.remote_feed_url,
                &item.source,
            ],
        )?;
    }

    // 3d. Replace live-event snapshot rows for this feed
    tx.execute(
        "DELETE FROM live_events WHERE feed_guid = ?1",
        params![feed.feed_guid],
    )?;
    for live_event in &live_events {
        tx.execute(
            "INSERT INTO live_events \
             (live_item_guid, feed_guid, title, content_link, status, scheduled_start, scheduled_end, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                &live_event.live_item_guid,
                &live_event.feed_guid,
                &live_event.title,
                &live_event.content_link,
                &live_event.status,
                live_event.scheduled_start,
                live_event.scheduled_end,
                live_event.created_at,
                live_event.updated_at,
            ],
        )?;
    }

    // 3e. Replace staged source contributor claims for this feed
    tx.execute(
        "DELETE FROM source_contributor_claims WHERE feed_guid = ?1",
        params![feed.feed_guid],
    )?;
    for claim in &source_contributor_claims {
        tx.execute(
            "INSERT INTO source_contributor_claims \
             (feed_guid, entity_type, entity_id, position, name, role, role_norm, group_name, href, img, \
              source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                &claim.feed_guid,
                &claim.entity_type,
                &claim.entity_id,
                claim.position,
                &claim.name,
                &claim.role,
                &claim.role_norm,
                &claim.group_name,
                &claim.href,
                &claim.img,
                &claim.source,
                &claim.extraction_path,
                claim.observed_at,
            ],
        )?;
    }

    // 3f. Replace staged source identity claims for this feed
    tx.execute(
        "DELETE FROM source_entity_ids WHERE feed_guid = ?1",
        params![feed.feed_guid],
    )?;
    for claim in &source_entity_ids {
        tx.execute(
            "INSERT INTO source_entity_ids \
             (feed_guid, entity_type, entity_id, position, scheme, value, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                &claim.feed_guid,
                &claim.entity_type,
                &claim.entity_id,
                claim.position,
                &claim.scheme,
                &claim.value,
                &claim.source,
                &claim.extraction_path,
                claim.observed_at,
            ],
        )?;
    }

    // 3g. Replace staged source entity links for this feed
    tx.execute(
        "DELETE FROM source_entity_links WHERE feed_guid = ?1",
        params![feed.feed_guid],
    )?;
    for link in &source_entity_links {
        tx.execute(
            "INSERT INTO source_entity_links \
             (feed_guid, entity_type, entity_id, position, link_type, url, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                &link.feed_guid,
                &link.entity_type,
                &link.entity_id,
                link.position,
                &link.link_type,
                &link.url,
                &link.source,
                &link.extraction_path,
                link.observed_at,
            ],
        )?;
    }

    // 3h. Replace staged release claims for this feed
    tx.execute(
        "DELETE FROM source_release_claims WHERE feed_guid = ?1",
        params![feed.feed_guid],
    )?;
    for claim in &source_release_claims {
        tx.execute(
            "INSERT INTO source_release_claims \
             (feed_guid, entity_type, entity_id, position, claim_type, claim_value, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                &claim.feed_guid,
                &claim.entity_type,
                &claim.entity_id,
                claim.position,
                &claim.claim_type,
                &claim.claim_value,
                &claim.source,
                &claim.extraction_path,
                claim.observed_at,
            ],
        )?;
    }

    // 3i. Replace staged item enclosures for this feed
    tx.execute(
        "DELETE FROM source_item_enclosures WHERE feed_guid = ?1",
        params![feed.feed_guid],
    )?;
    for enclosure in &source_item_enclosures {
        tx.execute(
            "INSERT INTO source_item_enclosures \
             (feed_guid, entity_type, entity_id, position, url, mime_type, bytes, rel, title, is_primary, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                &enclosure.feed_guid,
                &enclosure.entity_type,
                &enclosure.entity_id,
                enclosure.position,
                &enclosure.url,
                &enclosure.mime_type,
                enclosure.bytes,
                &enclosure.rel,
                &enclosure.title,
                enclosure.is_primary,
                &enclosure.source,
                &enclosure.extraction_path,
                enclosure.observed_at,
            ],
        )?;
    }

    // 3j. Replace staged platform claims for this feed
    tx.execute(
        "DELETE FROM source_platform_claims WHERE feed_guid = ?1",
        params![feed.feed_guid],
    )?;
    for claim in &source_platform_claims {
        tx.execute(
            "INSERT INTO source_platform_claims \
             (feed_guid, platform_key, url, owner_name, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &claim.feed_guid,
                &claim.platform_key,
                &claim.url,
                &claim.owner_name,
                &claim.source,
                &claim.extraction_path,
                claim.observed_at,
            ],
        )?;
    }

    // 4a. Collect existing track GUIDs for this feed before upserting.
    // Issue-STALE-TRACKS — 2026-03-14
    let existing_guids: std::collections::HashSet<String> = {
        let mut stmt = tx.prepare("SELECT track_guid FROM tracks WHERE feed_guid = ?1")?;
        let rows = stmt.query_map(params![feed.feed_guid], |row| row.get::<_, String>(0))?;
        let mut set = std::collections::HashSet::new();
        for row in rows {
            set.insert(row?);
        }
        set
    };

    // 4. Tracks, routes, splits
    for (track, routes, splits) in &tracks {
        tx.execute(
            "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, \
             duration_secs, enclosure_url, enclosure_type, enclosure_bytes, track_number, season, \
             explicit, description, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16) \
             ON CONFLICT(track_guid) DO UPDATE SET \
               feed_guid        = excluded.feed_guid, \
               artist_credit_id = excluded.artist_credit_id, \
               title            = excluded.title, \
               title_lower      = excluded.title_lower, \
               pub_date         = excluded.pub_date, \
               duration_secs    = excluded.duration_secs, \
               enclosure_url    = excluded.enclosure_url, \
               enclosure_type   = excluded.enclosure_type, \
               enclosure_bytes  = excluded.enclosure_bytes, \
               track_number     = excluded.track_number, \
               season           = excluded.season, \
               explicit         = excluded.explicit, \
               description      = excluded.description, \
               updated_at       = excluded.updated_at",
            params![
                track.track_guid,
                track.feed_guid,
                track.artist_credit_id,
                track.title,
                track.title_lower,
                track.pub_date,
                track.duration_secs,
                track.enclosure_url,
                track.enclosure_type,
                track.enclosure_bytes,
                track.track_number,
                track.season,
                i64::from(track.explicit),
                track.description,
                track.created_at,
                track.updated_at,
            ],
        )?;

        // replace payment routes
        tx.execute(
            "DELETE FROM payment_routes WHERE track_guid = ?1",
            params![track.track_guid],
        )?;
        for r in routes {
            let route_type = serde_json::to_string(&r.route_type)?;
            let route_type = route_type.trim_matches('"');
            tx.execute(
                "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, \
                 custom_key, custom_value, split, fee) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    r.track_guid,
                    r.feed_guid,
                    r.recipient_name,
                    route_type,
                    r.address,
                    r.custom_key,
                    r.custom_value,
                    r.split,
                    i64::from(r.fee),
                ],
            )?;
        }

        // replace value time splits
        tx.execute(
            "DELETE FROM value_time_splits WHERE source_track_guid = ?1",
            params![track.track_guid],
        )?;
        for s in splits {
            tx.execute(
                "INSERT INTO value_time_splits (source_track_guid, start_time_secs, duration_secs, \
                 remote_feed_guid, remote_item_guid, split, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    s.source_track_guid,
                    s.start_time_secs,
                    s.duration_secs,
                    s.remote_feed_guid,
                    s.remote_item_guid,
                    s.split,
                    s.created_at,
                ],
            )?;
        }
    }

    // 4b. Remove stale tracks that are no longer in the new crawl.
    // Issue-STALE-TRACKS — 2026-03-14
    let new_guids: std::collections::HashSet<&str> = tracks
        .iter()
        .map(|(t, _, _)| t.track_guid.as_str())
        .collect();
    let mut removal_event_rows: Vec<EventRow> = Vec::new();
    for removed_guid in &existing_guids {
        if new_guids.contains(removed_guid.as_str()) {
            continue;
        }
        // Look up the track to get search-index fields before deleting.
        let track_opt = get_track_by_guid(&tx, removed_guid)?;
        if let Some(track) = track_opt {
            // Remove the track's search index entry (best-effort).
            let _ = crate::search::delete_from_search_index(
                &tx,
                "track",
                &track.track_guid,
                "",
                &track.title,
                track.description.as_deref().unwrap_or(""),
                "",
            );
            // Cascade-delete the track and its child rows.
            delete_track_sql(&tx, removed_guid)?;
        }
        // Build a TrackRemoved event row.
        let payload = crate::event::TrackRemovedPayload {
            track_guid: removed_guid.clone(),
            feed_guid: feed.feed_guid.clone(),
        };
        let payload_json = serde_json::to_string(&payload)?;
        removal_event_rows.push(EventRow {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: EventType::TrackRemoved,
            payload_json,
            subject_guid: removed_guid.clone(),
            created_at: feed.updated_at,
            warnings: vec![],
        });
    }

    // Combine original event rows with removal event rows.
    let mut all_event_rows = event_rows;
    all_event_rows.append(&mut removal_event_rows);

    // 5. Insert events, collect seqs, sign with assigned seq, update signatures
    // Issue-SEQ-INTEGRITY — 2026-03-14
    let mut seqs = Vec::new();
    for er in &all_event_rows {
        let et_str = event_type_str(&er.event_type)?;
        let warnings_json = serde_json::to_string(&er.warnings)?;
        let sql = "INSERT INTO events \
            (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at, warnings_json) \
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(seq),0)+1 FROM events), ?7, ?8) \
            RETURNING seq";
        let seq = tx.query_row(
            sql,
            params![
                er.event_id,
                et_str,
                er.payload_json,
                er.subject_guid,
                signer.pubkey_hex(),
                "",
                er.created_at,
                warnings_json,
            ],
            |row| row.get::<_, i64>(0),
        )?;
        // Sign with the assigned seq and update the row.
        let (signed_by, signature) = signer.sign_event(
            &er.event_id,
            &er.event_type,
            &er.payload_json,
            &er.subject_guid,
            er.created_at,
            seq,
        );
        update_event_signature(&tx, &er.event_id, &signed_by, &signature)?;
        seqs.push((seq, signed_by, signature));
    }

    // Source feed/track search, quality, canonical state/search/promotions,
    // and targeted artist identity are all deferred to the durable resolver
    // queue. Ingest now focuses on preserving source facts and emitting the
    // event trail that resolverd will converge from.
    crate::resolver::queue::mark_feed_dirty_for_resolver(&tx, &feed.feed_guid)?;

    // 7. Commit
    tx.commit()?;

    Ok(seqs)
}

// ── External ID operations ──────────────────────────────────────────────────

/// Links an external identifier (e.g. `MusicBrainz`, ISRC, Spotify) to an entity.
///
/// Uses `INSERT OR REPLACE` so a second call with the same `(entity_type,
/// entity_id, scheme)` triple updates the stored `value`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL upsert fails.
pub fn link_external_id(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    scheme: &str,
    value: &str,
) -> Result<i64, DbError> {
    let now = unix_now();
    conn.execute(
        "INSERT OR REPLACE INTO external_ids (entity_type, entity_id, scheme, value, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![entity_type, entity_id, scheme, value, now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Returns all external IDs linked to the given entity.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_external_ids(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<ExternalIdRow>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, scheme, value FROM external_ids \
         WHERE entity_type = ?1 AND entity_id = ?2 \
         ORDER BY scheme",
    )?;
    let rows = stmt.query_map(params![entity_type, entity_id], |row| {
        Ok(ExternalIdRow {
            id: row.get(0)?,
            scheme: row.get(1)?,
            value: row.get(2)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Given a `(scheme, value)` pair, returns the `(entity_type, entity_id)` that
/// owns it, or `None` if no matching row exists.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn reverse_lookup_external_id(
    conn: &Connection,
    scheme: &str,
    value: &str,
) -> Result<Option<(String, String)>, DbError> {
    let result = conn
        .query_row(
            "SELECT entity_type, entity_id FROM external_ids \
         WHERE scheme = ?1 AND value = ?2",
            params![scheme, value],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(result)
}

// ── Provenance operations ───────────────────────────────────────────────────

/// Records how an entity was discovered or imported.
///
/// `source_type` should be one of: `"rss_crawl"`, `"manifest"`, `"manual"`,
/// `"bulk_import"`. `trust_level`: 0 = unknown, 1 = rss, 2 = signed manifest,
/// 3 = verified.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL insert fails.
pub fn record_entity_source(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    source_type: &str,
    source_url: Option<&str>,
    trust_level: i64,
) -> Result<i64, DbError> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO entity_source (entity_type, entity_id, source_type, source_url, trust_level, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![entity_type, entity_id, source_type, source_url, trust_level, now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Returns all provenance records for the given entity.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_entity_sources(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<EntitySourceRow>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, source_type, source_url, trust_level, created_at \
         FROM entity_source \
         WHERE entity_type = ?1 AND entity_id = ?2 \
         ORDER BY created_at",
    )?;
    let rows = stmt.query_map(params![entity_type, entity_id], |row| {
        Ok(EntitySourceRow {
            id: row.get(0)?,
            source_type: row.get(1)?,
            source_url: row.get(2)?,
            trust_level: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Returns feed-scoped authoritative external-ID overlays produced by the
/// primary resolver.
pub fn get_resolved_external_ids_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<ResolvedExternalIdByFeed>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT feed_guid, entity_type, entity_id, scheme, value, created_at
         FROM resolved_external_ids_by_feed
         WHERE feed_guid = ?1
         ORDER BY entity_type, entity_id, scheme, value",
    )?;
    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(ResolvedExternalIdByFeed {
            feed_guid: row.get(0)?,
            entity_type: row.get(1)?,
            entity_id: row.get(2)?,
            scheme: row.get(3)?,
            value: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Replaces the feed-scoped authoritative external-ID overlays for one feed.
pub fn replace_resolved_external_ids_for_feed(
    conn: &Connection,
    feed_guid: &str,
    rows: &[ResolvedExternalIdByFeed],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM resolved_external_ids_by_feed WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for row in rows {
        conn.execute(
            "INSERT INTO resolved_external_ids_by_feed
             (feed_guid, entity_type, entity_id, scheme, value, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                row.feed_guid,
                row.entity_type,
                row.entity_id,
                row.scheme,
                row.value,
                row.created_at
            ],
        )?;
    }
    Ok(())
}

/// Returns feed-scoped authoritative provenance overlays produced by the
/// primary resolver.
pub fn get_resolved_entity_sources_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<ResolvedEntitySourceByFeed>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT feed_guid, entity_type, entity_id, source_type, source_url, trust_level, created_at
         FROM resolved_entity_sources_by_feed
         WHERE feed_guid = ?1
         ORDER BY entity_type, entity_id, source_type, source_url",
    )?;
    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(ResolvedEntitySourceByFeed {
            feed_guid: row.get(0)?,
            entity_type: row.get(1)?,
            entity_id: row.get(2)?,
            source_type: row.get(3)?,
            source_url: row.get(4)?,
            trust_level: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Replaces the feed-scoped authoritative provenance overlays for one feed.
pub fn replace_resolved_entity_sources_for_feed(
    conn: &Connection,
    feed_guid: &str,
    rows: &[ResolvedEntitySourceByFeed],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM resolved_entity_sources_by_feed WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for row in rows {
        conn.execute(
            "INSERT INTO resolved_entity_sources_by_feed
             (feed_guid, entity_type, entity_id, source_type, source_url, trust_level, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                row.feed_guid,
                row.entity_type,
                row.entity_id,
                row.source_type,
                row.source_url,
                row.trust_level,
                row.created_at
            ],
        )?;
    }
    Ok(())
}

const MANAGED_PROMOTED_ENTITY_SOURCE_TYPES: &[&str] = &[
    "source_feed",
    "source_release_page",
    "source_recording_page",
    "source_primary_enclosure",
];

fn collect_high_confidence_artist_external_ids_for_feed(
    conn: &Connection,
    feed: &Feed,
) -> Result<Vec<ResolvedExternalIdByFeed>, DbError> {
    let Some(artist_id) = single_artist_id_for_credit(conn, feed.artist_credit_id)? else {
        return Ok(Vec::new());
    };

    let mut stmt = conn.prepare(
        "SELECT DISTINCT value FROM source_entity_ids \
         WHERE feed_guid = ?1 AND entity_type = 'feed' AND entity_id = ?1 AND scheme = 'nostr_npub' \
         ORDER BY value",
    )?;
    let values: Vec<String> = stmt
        .query_map(params![feed.feed_guid], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    if values.len() != 1 {
        return Ok(Vec::new());
    }
    let npub = &values[0];

    let existing_for_artist: Option<String> = conn
        .query_row(
            "SELECT value FROM external_ids \
             WHERE entity_type = 'artist' AND entity_id = ?1 AND scheme = 'nostr_npub'",
            params![artist_id],
            |row| row.get(0),
        )
        .optional()?;
    if existing_for_artist
        .as_deref()
        .is_some_and(|value| value != npub)
    {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT DISTINCT entity_type, entity_id FROM external_ids \
         WHERE scheme = 'nostr_npub' AND value = ?1 \
         ORDER BY entity_type, entity_id",
    )?;
    let existing_owners: Vec<(String, String)> = stmt
        .query_map(params![npub], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    if existing_owners
        .iter()
        .any(|(entity_type, entity_id)| entity_type != "artist" || entity_id != &artist_id)
    {
        return Ok(Vec::new());
    }

    Ok(vec![ResolvedExternalIdByFeed {
        feed_guid: feed.feed_guid.clone(),
        entity_type: "artist".to_string(),
        entity_id: artist_id,
        scheme: "nostr_npub".to_string(),
        value: npub.clone(),
        created_at: feed.updated_at,
    }])
}

fn collect_release_source_overlays_for_feed(
    conn: &Connection,
    feed: &Feed,
) -> Result<Vec<ResolvedEntitySourceByFeed>, DbError> {
    let Some(release_id) = release_id_for_feed_map(conn, &feed.feed_guid)? else {
        return Ok(Vec::new());
    };

    let mut rows = vec![ResolvedEntitySourceByFeed {
        feed_guid: feed.feed_guid.clone(),
        entity_type: "release".to_string(),
        entity_id: release_id.clone(),
        source_type: "source_feed".to_string(),
        source_url: Some(feed.feed_url.clone()),
        trust_level: 1,
        created_at: feed.updated_at,
    }];

    let mut stmt = conn.prepare(
        "SELECT DISTINCT url FROM source_entity_links \
         WHERE feed_guid = ?1 AND entity_type = 'feed' AND entity_id = ?1 AND link_type = 'website' \
         ORDER BY position, url",
    )?;
    let urls: Vec<String> = stmt
        .query_map(params![feed.feed_guid], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for url in urls {
        rows.push(ResolvedEntitySourceByFeed {
            feed_guid: feed.feed_guid.clone(),
            entity_type: "release".to_string(),
            entity_id: release_id.clone(),
            source_type: "source_release_page".to_string(),
            source_url: Some(url),
            trust_level: 1,
            created_at: feed.updated_at,
        });
    }
    Ok(rows)
}

fn collect_recording_source_overlays_for_feed(
    conn: &Connection,
    feed: &Feed,
) -> Result<Vec<ResolvedEntitySourceByFeed>, DbError> {
    let mut rows = Vec::new();
    let mut track_stmt = conn.prepare(
        "SELECT t.track_guid, sirm.recording_id, t.enclosure_url
         FROM tracks t
         JOIN source_item_recording_map sirm ON sirm.track_guid = t.track_guid
         WHERE t.feed_guid = ?1
         ORDER BY t.track_guid",
    )?;
    let mapped_tracks: Vec<(String, String, Option<String>)> = track_stmt
        .query_map(params![feed.feed_guid], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<_, _>>()?;

    for (track_guid, recording_id, enclosure_url) in mapped_tracks {
        let mut enclosure_stmt = conn.prepare(
            "SELECT DISTINCT url FROM source_item_enclosures \
             WHERE feed_guid = ?1 AND entity_type = 'track' AND entity_id = ?2 AND is_primary = 1 \
             ORDER BY position, url",
        )?;
        let enclosure_urls: Vec<String> = enclosure_stmt
            .query_map(params![feed.feed_guid, track_guid], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        if enclosure_urls.is_empty() {
            if let Some(url) = enclosure_url {
                rows.push(ResolvedEntitySourceByFeed {
                    feed_guid: feed.feed_guid.clone(),
                    entity_type: "recording".to_string(),
                    entity_id: recording_id.clone(),
                    source_type: "source_primary_enclosure".to_string(),
                    source_url: Some(url),
                    trust_level: 1,
                    created_at: feed.updated_at,
                });
            }
        } else {
            for url in enclosure_urls {
                rows.push(ResolvedEntitySourceByFeed {
                    feed_guid: feed.feed_guid.clone(),
                    entity_type: "recording".to_string(),
                    entity_id: recording_id.clone(),
                    source_type: "source_primary_enclosure".to_string(),
                    source_url: Some(url),
                    trust_level: 1,
                    created_at: feed.updated_at,
                });
            }
        }

        let mut link_stmt = conn.prepare(
            "SELECT DISTINCT url FROM source_entity_links \
             WHERE feed_guid = ?1 AND entity_type = 'track' AND entity_id = ?2 AND link_type = 'web_page' \
             ORDER BY position, url",
        )?;
        let link_urls: Vec<String> = link_stmt
            .query_map(params![feed.feed_guid, track_guid], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        for url in link_urls {
            rows.push(ResolvedEntitySourceByFeed {
                feed_guid: feed.feed_guid.clone(),
                entity_type: "recording".to_string(),
                entity_id: recording_id.clone(),
                source_type: "source_recording_page".to_string(),
                source_url: Some(url),
                trust_level: 1,
                created_at: feed.updated_at,
            });
        }
    }

    Ok(rows)
}

fn rebuild_materialized_external_ids_for_keys(
    conn: &Connection,
    keys: &std::collections::BTreeSet<(String, String, String)>,
) -> Result<(), DbError> {
    for (entity_type, entity_id, scheme) in keys {
        conn.execute(
            "DELETE FROM external_ids
             WHERE entity_type = ?1 AND entity_id = ?2 AND scheme = ?3",
            params![entity_type, entity_id, scheme],
        )?;

        let mut stmt = conn.prepare(
            "SELECT DISTINCT value, MIN(created_at)
             FROM resolved_external_ids_by_feed
             WHERE entity_type = ?1 AND entity_id = ?2 AND scheme = ?3
             GROUP BY value
             ORDER BY value",
        )?;
        let rows: Vec<(String, i64)> = stmt
            .query_map(params![entity_type, entity_id, scheme], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<Result<_, _>>()?;
        for (value, created_at) in rows {
            conn.execute(
                "INSERT INTO external_ids (entity_type, entity_id, scheme, value, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![entity_type, entity_id, scheme, value, created_at],
            )?;
        }
    }
    Ok(())
}

fn rebuild_materialized_entity_sources_for_keys(
    conn: &Connection,
    keys: &std::collections::BTreeSet<(String, String)>,
) -> Result<(), DbError> {
    for (entity_type, entity_id) in keys {
        let source_types = MANAGED_PROMOTED_ENTITY_SOURCE_TYPES
            .iter()
            .map(|_| "?".to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "DELETE FROM entity_source
             WHERE entity_type = ?1 AND entity_id = ?2
               AND source_type IN ({source_types})"
        );
        let mut params_vec: Vec<rusqlite::types::Value> =
            vec![entity_type.clone().into(), entity_id.clone().into()];
        for source_type in MANAGED_PROMOTED_ENTITY_SOURCE_TYPES {
            params_vec.push((*source_type).to_string().into());
        }
        conn.execute(&sql, rusqlite::params_from_iter(params_vec))?;

        let mut stmt = conn.prepare(
            "SELECT DISTINCT source_type, source_url, trust_level, MIN(created_at)
             FROM resolved_entity_sources_by_feed
             WHERE entity_type = ?1 AND entity_id = ?2
             GROUP BY source_type, source_url, trust_level
             ORDER BY source_type, source_url",
        )?;
        let rows: Vec<(String, Option<String>, i64, i64)> = stmt
            .query_map(params![entity_type, entity_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<_, _>>()?;
        for (source_type, source_url, trust_level, created_at) in rows {
            conn.execute(
                "INSERT INTO entity_source (entity_type, entity_id, source_type, source_url, trust_level, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    entity_type,
                    entity_id,
                    source_type,
                    source_url,
                    trust_level,
                    created_at
                ],
            )?;
        }
    }
    Ok(())
}

fn replace_materialized_canonical_promotions_for_feed(
    conn: &Connection,
    feed_guid: &str,
    external_ids: &[ResolvedExternalIdByFeed],
    entity_sources: &[ResolvedEntitySourceByFeed],
) -> Result<(), DbError> {
    let previous_ext = get_resolved_external_ids_for_feed(conn, feed_guid)?;
    let previous_sources = get_resolved_entity_sources_for_feed(conn, feed_guid)?;

    replace_resolved_external_ids_for_feed(conn, feed_guid, external_ids)?;
    replace_resolved_entity_sources_for_feed(conn, feed_guid, entity_sources)?;

    let mut ext_keys: std::collections::BTreeSet<(String, String, String)> = previous_ext
        .into_iter()
        .map(|row| (row.entity_type, row.entity_id, row.scheme))
        .collect();
    ext_keys.extend(external_ids.iter().map(|row| {
        (
            row.entity_type.clone(),
            row.entity_id.clone(),
            row.scheme.clone(),
        )
    }));
    rebuild_materialized_external_ids_for_keys(conn, &ext_keys)?;

    let mut source_keys: std::collections::BTreeSet<(String, String)> = previous_sources
        .into_iter()
        .map(|row| (row.entity_type, row.entity_id))
        .collect();
    source_keys.extend(
        entity_sources
            .iter()
            .map(|row| (row.entity_type.clone(), row.entity_id.clone())),
    );
    rebuild_materialized_entity_sources_for_keys(conn, &source_keys)?;
    Ok(())
}

// ── Canonical read helpers ──────────────────────────────────────────────────

/// Returns one canonical release by ID, or `None` if it does not exist.
pub fn get_release(conn: &Connection, release_id: &str) -> Result<Option<Release>, DbError> {
    conn.query_row(
        "SELECT release_id, title, title_lower, artist_credit_id, description, image_url, \
         release_date, created_at, updated_at \
         FROM releases WHERE release_id = ?1",
        params![release_id],
        |row| {
            Ok(Release {
                release_id: row.get(0)?,
                title: row.get(1)?,
                title_lower: row.get(2)?,
                artist_credit_id: row.get(3)?,
                description: row.get(4)?,
                image_url: row.get(5)?,
                release_date: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Returns one canonical recording by ID, or `None` if it does not exist.
pub fn get_recording(conn: &Connection, recording_id: &str) -> Result<Option<Recording>, DbError> {
    conn.query_row(
        "SELECT recording_id, title, title_lower, artist_credit_id, duration_secs, \
         created_at, updated_at \
         FROM recordings WHERE recording_id = ?1",
        params![recording_id],
        |row| {
            Ok(Recording {
                recording_id: row.get(0)?,
                title: row.get(1)?,
                title_lower: row.get(2)?,
                artist_credit_id: row.get(3)?,
                duration_secs: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Returns canonical track ordering for a release.
pub fn get_release_recordings(
    conn: &Connection,
    release_id: &str,
) -> Result<Vec<ReleaseRecording>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT release_id, recording_id, position, source_track_guid \
         FROM release_recordings WHERE release_id = ?1 ORDER BY position, recording_id",
    )?;
    let rows = stmt.query_map(params![release_id], |row| {
        Ok(ReleaseRecording {
            release_id: row.get(0)?,
            recording_id: row.get(1)?,
            position: row.get(2)?,
            source_track_guid: row.get(3)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Returns source-feed mappings for a canonical release.
pub fn get_source_feed_release_maps_for_release(
    conn: &Connection,
    release_id: &str,
) -> Result<Vec<SourceFeedReleaseMap>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT feed_guid, release_id, match_type, confidence, created_at \
         FROM source_feed_release_map WHERE release_id = ?1 \
         ORDER BY confidence DESC, feed_guid",
    )?;
    let rows = stmt.query_map(params![release_id], |row| {
        Ok(SourceFeedReleaseMap {
            feed_guid: row.get(0)?,
            release_id: row.get(1)?,
            match_type: row.get(2)?,
            confidence: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Returns source-feed mappings for one source feed.
pub fn get_source_feed_release_maps_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<SourceFeedReleaseMap>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT feed_guid, release_id, match_type, confidence, created_at \
         FROM source_feed_release_map WHERE feed_guid = ?1 \
         ORDER BY release_id",
    )?;
    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(SourceFeedReleaseMap {
            feed_guid: row.get(0)?,
            release_id: row.get(1)?,
            match_type: row.get(2)?,
            confidence: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Returns source-item mappings for a canonical recording.
pub fn get_source_item_recording_maps_for_recording(
    conn: &Connection,
    recording_id: &str,
) -> Result<Vec<SourceItemRecordingMap>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT track_guid, recording_id, match_type, confidence, created_at \
         FROM source_item_recording_map WHERE recording_id = ?1 \
         ORDER BY confidence DESC, track_guid",
    )?;
    let rows = stmt.query_map(params![recording_id], |row| {
        Ok(SourceItemRecordingMap {
            track_guid: row.get(0)?,
            recording_id: row.get(1)?,
            match_type: row.get(2)?,
            confidence: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Returns source-item mappings for the tracks currently in one feed.
pub fn get_source_item_recording_maps_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<SourceItemRecordingMap>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT sirm.track_guid, sirm.recording_id, sirm.match_type, sirm.confidence, sirm.created_at \
         FROM source_item_recording_map sirm \
         JOIN tracks t ON t.track_guid = sirm.track_guid \
         WHERE t.feed_guid = ?1 \
         ORDER BY sirm.track_guid",
    )?;
    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(SourceItemRecordingMap {
            track_guid: row.get(0)?,
            recording_id: row.get(1)?,
            match_type: row.get(2)?,
            confidence: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Returns a source feed by GUID, or `None` if it does not exist.
pub fn get_feed(conn: &Connection, feed_guid: &str) -> Result<Option<Feed>, DbError> {
    conn.query_row(
        "SELECT feed_guid, feed_url, title, title_lower, artist_credit_id, description, \
         image_url, language, explicit, itunes_type, episode_count, newest_item_at, \
         oldest_item_at, created_at, updated_at, raw_medium \
         FROM feeds WHERE feed_guid = ?1",
        params![feed_guid],
        |row| {
            Ok(Feed {
                feed_guid: row.get(0)?,
                feed_url: row.get(1)?,
                title: row.get(2)?,
                title_lower: row.get(3)?,
                artist_credit_id: row.get(4)?,
                description: row.get(5)?,
                image_url: row.get(6)?,
                language: row.get(7)?,
                explicit: row.get(8)?,
                itunes_type: row.get(9)?,
                episode_count: row.get(10)?,
                newest_item_at: row.get(11)?,
                oldest_item_at: row.get(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
                raw_medium: row.get(15)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Returns a source track by GUID, or `None` if it does not exist.
pub fn get_track(conn: &Connection, track_guid: &str) -> Result<Option<Track>, DbError> {
    conn.query_row(
        "SELECT track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, \
         duration_secs, enclosure_url, enclosure_type, enclosure_bytes, track_number, \
         season, explicit, description, created_at, updated_at \
         FROM tracks WHERE track_guid = ?1",
        params![track_guid],
        |row| {
            Ok(Track {
                track_guid: row.get(0)?,
                feed_guid: row.get(1)?,
                artist_credit_id: row.get(2)?,
                title: row.get(3)?,
                title_lower: row.get(4)?,
                pub_date: row.get(5)?,
                duration_secs: row.get(6)?,
                enclosure_url: row.get(7)?,
                enclosure_type: row.get(8)?,
                enclosure_bytes: row.get(9)?,
                track_number: row.get(10)?,
                season: row.get(11)?,
                explicit: row.get(12)?,
                description: row.get(13)?,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Returns staged contributor claims for one entity.
pub fn get_source_contributor_claims_for_entity(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceContributorClaim>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, name, role, role_norm, \
         group_name, href, img, source, extraction_path, observed_at \
         FROM source_contributor_claims \
         WHERE entity_type = ?1 AND entity_id = ?2 \
         ORDER BY position, name, id",
    )?;
    let rows = stmt.query_map(params![entity_type, entity_id], |row| {
        Ok(SourceContributorClaim {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            name: row.get(5)?,
            role: row.get(6)?,
            role_norm: row.get(7)?,
            group_name: row.get(8)?,
            href: row.get(9)?,
            img: row.get(10)?,
            source: row.get(11)?,
            extraction_path: row.get(12)?,
            observed_at: row.get(13)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Returns staged entity-ID claims for one entity.
pub fn get_source_entity_ids_for_entity(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceEntityIdClaim>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, scheme, value, source, \
         extraction_path, observed_at \
         FROM source_entity_ids \
         WHERE entity_type = ?1 AND entity_id = ?2 \
         ORDER BY position, scheme, value, id",
    )?;
    let rows = stmt.query_map(params![entity_type, entity_id], |row| {
        Ok(SourceEntityIdClaim {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            scheme: row.get(5)?,
            value: row.get(6)?,
            source: row.get(7)?,
            extraction_path: row.get(8)?,
            observed_at: row.get(9)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Returns staged link claims for one entity.
pub fn get_source_entity_links_for_entity(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceEntityLink>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, link_type, url, source, \
         extraction_path, observed_at \
         FROM source_entity_links \
         WHERE entity_type = ?1 AND entity_id = ?2 \
         ORDER BY position, link_type, url, id",
    )?;
    let rows = stmt.query_map(params![entity_type, entity_id], |row| {
        Ok(SourceEntityLink {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            link_type: row.get(5)?,
            url: row.get(6)?,
            source: row.get(7)?,
            extraction_path: row.get(8)?,
            observed_at: row.get(9)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Returns staged release claims for one entity.
pub fn get_source_release_claims_for_entity(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceReleaseClaim>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, claim_type, claim_value, \
         source, extraction_path, observed_at \
         FROM source_release_claims \
         WHERE entity_type = ?1 AND entity_id = ?2 \
         ORDER BY claim_type, position, id",
    )?;
    let rows = stmt.query_map(params![entity_type, entity_id], |row| {
        Ok(SourceReleaseClaim {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            claim_type: row.get(5)?,
            claim_value: row.get(6)?,
            source: row.get(7)?,
            extraction_path: row.get(8)?,
            observed_at: row.get(9)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Returns staged enclosure variants for one entity.
pub fn get_source_item_enclosures_for_entity(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceItemEnclosure>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, url, mime_type, bytes, rel, \
         title, is_primary, source, extraction_path, observed_at \
         FROM source_item_enclosures \
         WHERE entity_type = ?1 AND entity_id = ?2 \
         ORDER BY position, url, id",
    )?;
    let rows = stmt.query_map(params![entity_type, entity_id], |row| {
        Ok(SourceItemEnclosure {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            url: row.get(5)?,
            mime_type: row.get(6)?,
            bytes: row.get(7)?,
            rel: row.get(8)?,
            title: row.get(9)?,
            is_primary: row.get(10)?,
            source: row.get(11)?,
            extraction_path: row.get(12)?,
            observed_at: row.get(13)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}
