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
    PaymentRoute, RouteType, SourceContributorClaim, SourceEntityIdClaim, SourceEntityLink,
    SourceItemEnclosure, SourceItemTranscript, SourcePlatformClaim, SourceReleaseClaim, Track,
    TrackRemoteItemRaw, ValueTimeSplit,
};
use crate::signing::NodeSigner;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::Digest;
use std::fmt;
use std::sync::{Arc, Mutex}; // Issue-SEQ-INTEGRITY — 2026-03-14

pub type Db = Arc<Mutex<Connection>>;

/// Ingest-time track plus its child rows.
pub type TrackIngestBundle = (
    Track,
    Vec<PaymentRoute>,
    Vec<ValueTimeSplit>,
    Vec<TrackRemoteItemRaw>,
);

/// Default `SQLite` database path for local CLI tools and daemon env fallbacks.
pub const DEFAULT_DB_PATH: &str = "./stophammer.db";

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
    // Migration 15: scope live-event uniqueness by feed and preserve legacy rows.
    include_str!("../migrations/0015_live_events_feed_scoped_key.sql"),
    // Migration 19: add cleanup triggers for direct feed/track deletes on legacy tables.
    include_str!("../migrations/0019_feed_delete_cleanup_triggers.sql"),
    // Migration 20: dedupe legacy NULL-scoped artist credits and enforce normalized uniqueness.
    include_str!("../migrations/0020_artist_credit_null_scope_dedup.sql"),
    // Migration 21: normalize route custom fields to empty strings instead of NULL.
    include_str!("../migrations/0021_route_custom_value_normalization.sql"),
    // Migration 25: add source-first feed/track artist, publisher, artwork, and date fields.
    include_str!("../migrations/0025_source_first_feed_track_fields.sql"),
    // Migration 26: drop retired canonical release/recording tables and rebuild delete triggers.
    include_str!("../migrations/0026_drop_canonical_release_recording_tables.sql"),
    // Migration 27: add source-first track publisher text.
    include_str!("../migrations/0027_add_track_publisher_text.sql"),
    // Migration 28: add staged source item transcripts for podcast:transcript support.
    include_str!("../migrations/0028_source_item_transcripts.sql"),
    // Migration 29: add track-level raw podcast:remoteItem evidence.
    include_str!("../migrations/0029_track_remote_items.sql"),
    // Migration 30: drop wallet identity subsystem and rebuild delete triggers without wallet refs.
    include_str!("../migrations/0030_drop_wallet_tables.sql"),
    // Migration 31: expression index on lower(track_artist) for artist lookup endpoint.
    include_str!("../migrations/0031_track_artist_lower_index.sql"),
    // Migration 32: make source track storage feed-scoped so duplicate raw track GUIDs can coexist.
    include_str!("../migrations/0032_feed_scoped_track_identity.sql"),
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
/// # Errors
///
/// Returns [`DbError`] if the file cannot be opened, the startup PRAGMAs
/// cannot be applied, or migrations fail.
// SP-01 stable FTS5 hash — 2026-03-13
// Note: The FTS5 table uses content='' (contentless), so the 'rebuild' command
// is not available. Hash stability is enforced by using SipHash-2-4 with fixed
// keys in search::rowid_for. If the hash ever changes, the index must be
// dropped and re-populated from the source tables.
// HIGH-02 impl AsRef<Path> param — 2026-03-13
pub fn try_open_db(path: impl AsRef<std::path::Path>) -> Result<Connection, DbError> {
    let mut conn = Connection::open(path.as_ref())?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;\n\
         PRAGMA foreign_keys = ON;\n\
         PRAGMA synchronous = NORMAL;\n\
         PRAGMA cache_size = -65536;",
    )?;
    run_migrations(&mut conn)?;
    Ok(conn)
}

/// Opens the `SQLite` database at `path` and runs pending schema migrations.
///
/// This convenience wrapper preserves the existing panic-on-failure behavior
/// for short-lived tools and tests that intentionally rely on immediate aborts.
///
/// # Panics
///
/// Panics if [`try_open_db`] returns an error.
#[must_use]
pub fn open_db(path: impl AsRef<std::path::Path>) -> Connection {
    try_open_db(path).expect("failed to open database")
}

// ── Helper: serialize EventType to snake_case string (no quotes) ─────────────

fn event_type_str(et: &EventType) -> Result<String, DbError> {
    let s = serde_json::to_string(et)?;
    Ok(s.trim_matches('"').to_string())
}

fn route_type_from_db(rt_str: &str, context: &str) -> RouteType {
    match serde_json::from_str::<RouteType>(&format!("\"{rt_str}\"")) {
        Ok(route_type) => route_type,
        Err(err) => {
            tracing::warn!(
                route_type = %rt_str,
                context,
                error = %err,
                "db: invalid route_type in stored row, defaulting to node"
            );
            RouteType::Node
        }
    }
}

/// Returns the internal track entity key used for search and quality rows.
///
/// # Panics
///
/// Panics if serializing a tuple of two strings to JSON ever fails.
#[must_use]
pub fn canonical_track_entity_id(feed_guid: &str, track_guid: &str) -> String {
    serde_json::to_string(&(feed_guid, track_guid))
        .expect("serializing a pair of strings to JSON cannot fail")
}

#[must_use]
pub fn parse_canonical_track_entity_id(entity_id: &str) -> Option<(String, String)> {
    serde_json::from_str::<(String, String)>(entity_id).ok()
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

/// Returns a deterministic feed-scoped compatibility credit for published
/// artist text without invoking cross-feed resolution.
///
/// This is a transitional helper for source-first ingest paths that still need
/// `artist_credit_id` foreign keys while Phase 3 removes canonical identity
/// assumptions from feed/track writes.
pub fn get_or_create_feed_scoped_source_text_credit(
    conn: &Connection,
    display_name: &str,
    feed_guid: &str,
) -> Result<ArtistCredit, DbError> {
    let display_name = display_name.trim();
    if display_name.is_empty() {
        return Err(DbError::Other(
            "source text credit display_name must not be empty".to_string(),
        ));
    }

    if let Some(existing) = get_artist_credit_by_display_name(conn, display_name, Some(feed_guid))?
    {
        return Ok(existing);
    }

    let now = unix_now();
    let normalized = display_name.to_lowercase();
    let artist_id = canonical_cluster_id(
        "source_text_artist",
        &format!("source_text_artist_v1|{feed_guid}|{normalized}"),
    );
    let artist = Artist {
        artist_id,
        name: display_name.to_string(),
        name_lower: normalized,
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: now,
        updated_at: now,
    };
    upsert_artist_if_absent(conn, &artist)?;
    create_single_artist_credit(conn, &artist, Some(feed_guid))
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
         NULLIF(custom_key, ''), NULLIF(custom_value, ''), split, fee \
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
            route_type: route_type_from_db(&rt_str, "payment_routes"),
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

pub fn get_payment_routes_for_feed_track(
    conn: &Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<Vec<PaymentRoute>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, track_guid, feed_guid, recipient_name, route_type, address, \
         NULLIF(custom_key, ''), NULLIF(custom_value, ''), split, fee \
         FROM payment_routes WHERE feed_guid = ?1 AND track_guid = ?2",
    )?;
    let rows = stmt.query_map(params![feed_guid, track_guid], |row| {
        let rt_str: String = row.get(4)?;
        let fee_i: i64 = row.get(9)?;
        Ok(PaymentRoute {
            id: row.get(0)?,
            track_guid: row.get(1)?,
            feed_guid: row.get(2)?,
            recipient_name: row.get(3)?,
            route_type: route_type_from_db(&rt_str, "payment_routes"),
            address: row.get(5)?,
            custom_key: row.get(6)?,
            custom_value: row.get(7)?,
            split: row.get(8)?,
            fee: fee_i != 0,
        })
    })?;
    rows.collect::<Result<_, _>>().map_err(DbError::from)
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
        "SELECT id, source_feed_guid, source_track_guid, start_time_secs, duration_secs, remote_feed_guid, \
         remote_item_guid, split, created_at \
         FROM value_time_splits WHERE source_track_guid = ?1",
    )?;
    let rows = stmt.query_map(params![track_guid], |row| {
        Ok(ValueTimeSplit {
            id: row.get(0)?,
            source_feed_guid: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            source_track_guid: row.get(2)?,
            start_time_secs: row.get(3)?,
            duration_secs: row.get(4)?,
            remote_feed_guid: row.get(5)?,
            remote_item_guid: row.get(6)?,
            split: row.get(7)?,
            created_at: row.get(8)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn get_value_time_splits_for_feed_track(
    conn: &Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<Vec<ValueTimeSplit>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, source_feed_guid, source_track_guid, start_time_secs, duration_secs, remote_feed_guid, \
         remote_item_guid, split, created_at \
         FROM value_time_splits \
         WHERE source_track_guid = ?2 AND (source_feed_guid = ?1 OR source_feed_guid IS NULL)",
    )?;
    let rows = stmt.query_map(params![feed_guid, track_guid], |row| {
        Ok(ValueTimeSplit {
            id: row.get(0)?,
            source_feed_guid: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            source_track_guid: row.get(2)?,
            start_time_secs: row.get(3)?,
            duration_secs: row.get(4)?,
            remote_feed_guid: row.get(5)?,
            remote_item_guid: row.get(6)?,
            split: row.get(7)?,
            created_at: row.get(8)?,
        })
    })?;
    rows.collect::<Result<_, _>>().map_err(DbError::from)
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

fn ensure_credit_artist_exists(
    conn: &Connection,
    artist_id: &str,
    credited_name: &str,
    feed_guid: Option<&str>,
    now: i64,
) -> Result<(), DbError> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT artist_id FROM artists WHERE artist_id = ?1",
            params![artist_id],
            |row| row.get(0),
        )
        .optional()?;
    if existing.is_none() {
        let name_lower = credited_name.to_lowercase();
        conn.execute(
            "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![artist_id, credited_name, name_lower, now, now],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO artist_aliases (alias_lower, artist_id, feed_guid, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![name_lower, artist_id, feed_guid, now],
        )?;
    }
    Ok(())
}

/// Ensures the `artist_credit` row, its referenced artists, and all
/// `artist_credit_name` members exist.
///
/// # Errors
///
/// Returns [`DbError`] if any dependency lookup or insert fails.
pub(crate) fn upsert_artist_credit_sql(
    conn: &Connection,
    credit: &ArtistCredit,
) -> Result<(), DbError> {
    let now = unix_now();
    for acn in &credit.names {
        ensure_credit_artist_exists(
            conn,
            &acn.artist_id,
            &acn.name,
            credit.feed_guid.as_deref(),
            now,
        )?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO artist_credit (id, display_name, feed_guid, created_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            credit.id,
            credit.display_name,
            credit.feed_guid,
            credit.created_at
        ],
    )?;
    for acn in &credit.names {
        conn.execute(
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
    Ok(())
}

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

    // INSERT OR IGNORE guards against the SQLite LOWER()-vs-Unicode mismatch:
    // SQLite's built-in LOWER() is ASCII-only, so the pre-insert lookup in
    // get_or_create_artist_credit misses rows whose display_name contains
    // non-ASCII uppercase letters (e.g. "ZÄVODI").  If a concurrent or
    // repeated ingest hits that path we must not hard-fail; fetch the
    // existing row instead.
    conn.execute(
        "INSERT OR IGNORE INTO artist_credit (display_name, feed_guid, created_at) VALUES (?1, ?2, ?3)",
        params![display_name, feed_guid, now],
    )?;
    let credit_id = if conn.changes() == 0 {
        // Row already existed (UNIQUE conflict silenced by OR IGNORE).
        // Re-fetch by exact display_name + feed_guid.
        conn.query_row(
            "SELECT id FROM artist_credit WHERE display_name = ?1 AND \
             (feed_guid = ?2 OR (feed_guid IS NULL AND ?2 IS NULL))",
            params![display_name, feed_guid],
            |row| row.get::<_, i64>(0),
        )?
    } else {
        conn.last_insert_rowid()
    };

    let mut credit_names = Vec::with_capacity(names.len());
    for (pos, (artist_id, name, join_phrase)) in names.iter().enumerate() {
        ensure_credit_artist_exists(conn, artist_id, name, feed_guid, now)?;
        #[expect(
            clippy::cast_possible_wrap,
            reason = "artist credit position count never approaches i64::MAX"
        )]
        let position = pos as i64;
        conn.execute(
            "INSERT OR IGNORE INTO artist_credit_name \
             (artist_credit_id, artist_id, position, name, join_phrase) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![credit_id, artist_id, position, name, join_phrase],
        )?;
        let acn_id = if conn.changes() == 0 {
            conn.query_row(
                "SELECT id FROM artist_credit_name \
                 WHERE artist_credit_id = ?1 AND position = ?2",
                params![credit_id, position],
                |row| row.get::<_, i64>(0),
            )?
        } else {
            conn.last_insert_rowid()
        };
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
         publisher, language, explicit, itunes_type, release_artist, release_artist_sort, release_date, \
         release_kind, episode_count, newest_item_at, oldest_item_at, created_at, updated_at, raw_medium) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21) \
         ON CONFLICT(feed_guid) DO UPDATE SET \
           feed_url         = excluded.feed_url, \
           title            = excluded.title, \
           title_lower      = excluded.title_lower, \
           artist_credit_id = excluded.artist_credit_id, \
           description      = excluded.description, \
           image_url        = excluded.image_url, \
           publisher        = excluded.publisher, \
           language         = excluded.language, \
           explicit         = excluded.explicit, \
           itunes_type      = excluded.itunes_type, \
           release_artist   = excluded.release_artist, \
           release_artist_sort = excluded.release_artist_sort, \
           release_date     = excluded.release_date, \
           release_kind     = excluded.release_kind, \
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
            feed.publisher,
            feed.language,
            i64::from(feed.explicit),
            feed.itunes_type,
            feed.release_artist,
            feed.release_artist_sort,
            feed.release_date,
            feed.release_kind,
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

/// Inserts or updates a track row keyed on `(feed_guid, track_guid)`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL upsert fails.
pub fn upsert_track(conn: &Connection, track: &Track) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, \
         duration_secs, image_url, publisher, language, enclosure_url, enclosure_type, enclosure_bytes, track_number, season, \
         explicit, description, track_artist, track_artist_sort, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21) \
         ON CONFLICT(feed_guid, track_guid) DO UPDATE SET \
           artist_credit_id = excluded.artist_credit_id, \
           title            = excluded.title, \
           title_lower      = excluded.title_lower, \
           pub_date         = excluded.pub_date, \
           duration_secs    = excluded.duration_secs, \
           image_url        = excluded.image_url, \
           publisher        = excluded.publisher, \
           language         = excluded.language, \
           enclosure_url    = excluded.enclosure_url, \
           enclosure_type   = excluded.enclosure_type, \
           enclosure_bytes  = excluded.enclosure_bytes, \
            track_number     = excluded.track_number, \
            season           = excluded.season, \
            explicit         = excluded.explicit, \
            description      = excluded.description, \
            track_artist     = excluded.track_artist, \
            track_artist_sort = excluded.track_artist_sort, \
            updated_at       = excluded.updated_at",
        params![
            track.track_guid,
            track.feed_guid,
            track.artist_credit_id,
            track.title,
            track.title_lower,
            track.pub_date,
            track.duration_secs,
            track.image_url,
            track.publisher,
            track.language,
            track.enclosure_url,
            track.enclosure_type,
            track.enclosure_bytes,
            track.track_number,
            track.season,
            i64::from(track.explicit),
            track.description,
            track.track_artist,
            track.track_artist_sort,
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

/// Transitional no-op kept only so retired canonical tests and fixtures can
/// compile while Phase 3 removes the remaining canonical schema.
pub fn sync_canonical_state_for_feed(_conn: &Connection, _feed_guid: &str) -> Result<(), DbError> {
    Ok(())
}

/// Rebuilds source-layer `feed`/`track` search rows and quality scores for one
/// feed without touching canonical tables.
pub fn sync_source_read_models_for_feed(conn: &Connection, feed_guid: &str) -> Result<(), DbError> {
    let Some(feed) = get_feed_by_guid(conn, feed_guid)? else {
        return Ok(());
    };

    let is_music = feed.raw_medium.as_deref() == Some("music");

    if is_music {
        crate::search::populate_search_index(
            conn,
            "feed",
            &feed.feed_guid,
            feed.release_artist.as_deref().unwrap_or(""),
            &feed.title,
            feed.description.as_deref().unwrap_or(""),
            feed.raw_medium.as_deref().unwrap_or(""),
        )?;
    } else {
        crate::search::delete_from_search_index(
            conn,
            "feed",
            &feed.feed_guid,
            feed.release_artist.as_deref().unwrap_or(""),
            &feed.title,
            feed.description.as_deref().unwrap_or(""),
            feed.raw_medium.as_deref().unwrap_or(""),
        )?;
    }

    let feed_score = crate::quality::compute_feed_quality(conn, &feed.feed_guid)?;
    crate::quality::store_quality(conn, "feed", &feed.feed_guid, feed_score)?;

    for track in get_tracks_for_feed(conn, feed_guid)? {
        if is_music {
            crate::search::populate_search_index(
                conn,
                "track",
                &canonical_track_entity_id(&track.feed_guid, &track.track_guid),
                track.track_artist.as_deref().unwrap_or(""),
                &track.title,
                track.description.as_deref().unwrap_or(""),
                "",
            )?;
        } else {
            crate::search::delete_from_search_index(
                conn,
                "track",
                &canonical_track_entity_id(&track.feed_guid, &track.track_guid),
                track.track_artist.as_deref().unwrap_or(""),
                &track.title,
                track.description.as_deref().unwrap_or(""),
                "",
            )?;
        }
        let track_score = crate::quality::compute_track_quality_for_feed_track(
            conn,
            &track.feed_guid,
            &track.track_guid,
        )?;
        crate::quality::store_quality(
            conn,
            "track",
            &canonical_track_entity_id(&track.feed_guid, &track.track_guid),
            track_score,
        )?;
    }

    Ok(())
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
                r.custom_key.as_deref().unwrap_or(""),
                r.custom_value.as_deref().unwrap_or(""),
                r.split,
                i64::from(r.fee),
            ],
        )?;
    }
    Ok(())
}

pub fn replace_payment_routes_for_feed_track(
    conn: &Connection,
    feed_guid: &str,
    track_guid: &str,
    routes: &[PaymentRoute],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM payment_routes WHERE feed_guid = ?1 AND track_guid = ?2",
        params![feed_guid, track_guid],
    )?;
    for r in routes {
        let route_type = serde_json::to_string(&r.route_type)?;
        let route_type = route_type.trim_matches('"');
        conn.execute(
            "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, \
             custom_key, custom_value, split, fee) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                track_guid,
                feed_guid,
                r.recipient_name,
                route_type,
                r.address,
                r.custom_key.as_deref().unwrap_or(""),
                r.custom_value.as_deref().unwrap_or(""),
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
                r.custom_key.as_deref().unwrap_or(""),
                r.custom_value.as_deref().unwrap_or(""),
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

pub fn replace_value_time_splits_for_feed_track(
    conn: &Connection,
    source_feed_guid: &str,
    source_track_guid: &str,
    splits: &[ValueTimeSplit],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM value_time_splits WHERE source_track_guid = ?2 AND (source_feed_guid = ?1 OR source_feed_guid IS NULL)",
        params![source_feed_guid, source_track_guid],
    )?;
    for s in splits {
        conn.execute(
            "INSERT INTO value_time_splits (source_feed_guid, source_track_guid, start_time_secs, duration_secs, \
             remote_feed_guid, remote_item_guid, split, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                if s.source_feed_guid.is_empty() {
                    source_feed_guid
                } else {
                    s.source_feed_guid.as_str()
                },
                source_track_guid,
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

/// Returns the publisher feed (`raw_medium = 'publisher'`) that has declared
/// `music_feed_guid` as one of its music feeds via a `podcast:remoteItem
/// medium="music"` channel-level entry, or `None` if no such feed exists.
pub fn get_publisher_feed_for_music_feed(
    conn: &Connection,
    music_feed_guid: &str,
) -> Result<Option<Feed>, DbError> {
    conn.query_row(
        "SELECT f.feed_guid, f.feed_url, f.title, f.title_lower, f.artist_credit_id, \
         f.description, f.image_url, f.publisher, f.language, f.explicit, f.itunes_type, \
         f.release_artist, f.release_artist_sort, f.release_date, f.release_kind, \
         f.episode_count, f.newest_item_at, f.oldest_item_at, f.created_at, f.updated_at, \
         f.raw_medium \
         FROM feeds f \
         JOIN feed_remote_items_raw ri ON ri.feed_guid = f.feed_guid \
         WHERE ri.remote_feed_guid = ?1 \
           AND ri.medium = 'music' \
           AND lower(f.raw_medium) = 'publisher' \
         LIMIT 1",
        params![music_feed_guid],
        |row| {
            Ok(Feed {
                feed_guid: row.get(0)?,
                feed_url: row.get(1)?,
                title: row.get(2)?,
                title_lower: row.get(3)?,
                artist_credit_id: row.get(4)?,
                description: row.get(5)?,
                image_url: row.get(6)?,
                publisher: row.get(7)?,
                language: row.get(8)?,
                explicit: row.get(9)?,
                itunes_type: row.get(10)?,
                release_artist: row.get(11)?,
                release_artist_sort: row.get(12)?,
                release_date: row.get(13)?,
                release_kind: row.get(14)?,
                episode_count: row.get(15)?,
                newest_item_at: row.get(16)?,
                oldest_item_at: row.get(17)?,
                created_at: row.get(18)?,
                updated_at: row.get(19)?,
                raw_medium: row.get(20)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_track_remote_items_for_track(
    conn: &Connection,
    track_guid: &str,
) -> Result<Vec<TrackRemoteItemRaw>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, track_guid, position, medium, remote_feed_guid, remote_feed_url, source \
         FROM track_remote_items_raw WHERE track_guid = ?1 ORDER BY position",
    )?;
    let rows = stmt.query_map(params![track_guid], |row| {
        Ok(TrackRemoteItemRaw {
            id: row.get(0)?,
            feed_guid: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            track_guid: row.get(2)?,
            position: row.get(3)?,
            medium: row.get(4)?,
            remote_feed_guid: row.get(5)?,
            remote_feed_url: row.get(6)?,
            source: row.get(7)?,
        })
    })?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

pub fn get_track_remote_items_for_feed_track(
    conn: &Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<Vec<TrackRemoteItemRaw>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, track_guid, position, medium, remote_feed_guid, remote_feed_url, source \
         FROM track_remote_items_raw \
         WHERE track_guid = ?2 AND (feed_guid = ?1 OR feed_guid IS NULL) \
         ORDER BY position",
    )?;
    let rows = stmt.query_map(params![feed_guid, track_guid], |row| {
        Ok(TrackRemoteItemRaw {
            id: row.get(0)?,
            feed_guid: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            track_guid: row.get(2)?,
            position: row.get(3)?,
            medium: row.get(4)?,
            remote_feed_guid: row.get(5)?,
            remote_feed_url: row.get(6)?,
            source: row.get(7)?,
        })
    })?;
    rows.collect::<Result<_, _>>().map_err(DbError::from)
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

pub fn replace_track_remote_items_raw(
    conn: &Connection,
    track_guid: &str,
    remote_items: &[TrackRemoteItemRaw],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM track_remote_items_raw WHERE track_guid = ?1",
        params![track_guid],
    )?;
    for item in remote_items {
        conn.execute(
            "INSERT INTO track_remote_items_raw \
             (track_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &item.track_guid,
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

pub fn replace_track_remote_items_raw_for_feed_track(
    conn: &Connection,
    feed_guid: &str,
    track_guid: &str,
    remote_items: &[TrackRemoteItemRaw],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM track_remote_items_raw WHERE track_guid = ?2 AND (feed_guid = ?1 OR feed_guid IS NULL)",
        params![feed_guid, track_guid],
    )?;
    for item in remote_items {
        conn.execute(
            "INSERT INTO track_remote_items_raw \
             (feed_guid, track_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                if item.feed_guid.is_empty() {
                    feed_guid
                } else {
                    item.feed_guid.as_str()
                },
                track_guid,
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
fn dedupe_live_events(live_events: &[LiveEvent]) -> Vec<LiveEvent> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::with_capacity(live_events.len());
    for live_event in live_events {
        if seen.insert(live_event.live_item_guid.clone()) {
            deduped.push(live_event.clone());
        }
    }
    deduped
}

#[must_use]
pub fn dedupe_source_contributor_claims(
    claims: &[SourceContributorClaim],
) -> Vec<SourceContributorClaim> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::with_capacity(claims.len());
    for claim in claims {
        let key = (
            claim.feed_guid.clone(),
            claim.entity_type.clone(),
            claim.entity_id.clone(),
            claim.position,
            claim.source.clone(),
        );
        if seen.insert(key) {
            deduped.push(claim.clone());
        }
    }
    deduped
}

#[must_use]
pub fn dedupe_source_entity_links(links: &[SourceEntityLink]) -> Vec<SourceEntityLink> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::with_capacity(links.len());
    for link in links {
        let key = (
            link.feed_guid.clone(),
            link.entity_type.clone(),
            link.entity_id.clone(),
            link.link_type.clone(),
            link.url.clone(),
        );
        if seen.insert(key) {
            deduped.push(link.clone());
        }
    }
    deduped
}

#[must_use]
pub fn dedupe_source_entity_ids(claims: &[SourceEntityIdClaim]) -> Vec<SourceEntityIdClaim> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::with_capacity(claims.len());
    for claim in claims {
        let key = (
            claim.feed_guid.clone(),
            claim.entity_type.clone(),
            claim.entity_id.clone(),
            claim.scheme.clone(),
            claim.value.clone(),
        );
        if seen.insert(key) {
            deduped.push(claim.clone());
        }
    }
    deduped
}

#[must_use]
pub fn dedupe_source_release_claims(claims: &[SourceReleaseClaim]) -> Vec<SourceReleaseClaim> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::with_capacity(claims.len());
    for claim in claims {
        let key = (
            claim.feed_guid.clone(),
            claim.entity_type.clone(),
            claim.entity_id.clone(),
            claim.claim_type.clone(),
            claim.position,
        );
        if seen.insert(key) {
            deduped.push(claim.clone());
        }
    }
    deduped
}

#[must_use]
pub fn dedupe_source_item_enclosures(
    enclosures: &[SourceItemEnclosure],
) -> Vec<SourceItemEnclosure> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::with_capacity(enclosures.len());
    for enclosure in enclosures {
        let key = (
            enclosure.feed_guid.clone(),
            enclosure.entity_type.clone(),
            enclosure.entity_id.clone(),
            enclosure.position,
            enclosure.url.clone(),
        );
        if seen.insert(key) {
            deduped.push(enclosure.clone());
        }
    }
    deduped
}

pub fn replace_live_events_for_feed(
    conn: &Connection,
    feed_guid: &str,
    live_events: &[LiveEvent],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM live_events WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for live_event in dedupe_live_events(live_events) {
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
    for claim in dedupe_source_contributor_claims(claims) {
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
    for claim in dedupe_source_entity_ids(claims) {
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
    for link in dedupe_source_entity_links(links) {
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
    for claim in dedupe_source_release_claims(claims) {
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
    for enclosure in dedupe_source_item_enclosures(enclosures) {
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

// ── source_item_transcripts ─────────────────────────────────────────────────

/// Returns the staged item-transcript rows for a feed ordered by entity + position.
pub fn get_source_item_transcripts_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<Vec<SourceItemTranscript>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, url, mime_type, language, rel, \
         source, extraction_path, observed_at \
         FROM source_item_transcripts WHERE feed_guid = ?1 \
         ORDER BY entity_type, entity_id, position, url, id",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        Ok(SourceItemTranscript {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            url: row.get(5)?,
            mime_type: row.get(6)?,
            language: row.get(7)?,
            rel: row.get(8)?,
            source: row.get(9)?,
            extraction_path: row.get(10)?,
            observed_at: row.get(11)?,
        })
    })?;

    let mut transcripts = Vec::new();
    for row in rows {
        transcripts.push(row?);
    }
    Ok(transcripts)
}

#[must_use]
pub fn dedupe_source_item_transcripts(
    transcripts: &[SourceItemTranscript],
) -> Vec<SourceItemTranscript> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::with_capacity(transcripts.len());
    for transcript in transcripts {
        let key = (
            transcript.feed_guid.clone(),
            transcript.entity_type.clone(),
            transcript.entity_id.clone(),
            transcript.position,
            transcript.url.clone(),
        );
        if seen.insert(key) {
            deduped.push(transcript.clone());
        }
    }
    deduped
}

/// Replaces the staged item-transcript rows for a feed.
pub fn replace_source_item_transcripts_for_feed(
    conn: &Connection,
    feed_guid: &str,
    transcripts: &[SourceItemTranscript],
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM source_item_transcripts WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for transcript in dedupe_source_item_transcripts(transcripts) {
        conn.execute(
            "INSERT INTO source_item_transcripts \
             (feed_guid, entity_type, entity_id, position, url, mime_type, language, rel, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                &transcript.feed_guid,
                &transcript.entity_type,
                &transcript.entity_id,
                transcript.position,
                &transcript.url,
                &transcript.mime_type,
                &transcript.language,
                &transcript.rel,
                &transcript.source,
                &transcript.extraction_path,
                transcript.observed_at,
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
/// Deletes in order: `value_time_splits`, `payment_routes`,
/// `entity_quality`, then the `tracks` row itself.
///
/// Idempotent: calling with a non-existent `track_guid` is a no-op.
///
/// # Errors
///
/// Returns [`DbError`] if any SQL delete fails or the raw `track_guid` is
/// ambiguous across feeds.
pub fn delete_track(conn: &mut Connection, track_guid: &str) -> Result<(), DbError> {
    let Some(track) = get_track_by_guid(conn, track_guid)? else {
        return Ok(());
    };
    delete_track_for_feed(conn, &track.feed_guid, &track.track_guid)
}

/// Idempotent: calling with a non-existent `(feed_guid, track_guid)` pair is a no-op.
pub fn delete_track_for_feed(
    conn: &mut Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<(), DbError> {
    let sp = conn.savepoint()?;
    delete_track_sql(&sp, feed_guid, track_guid)?;
    sp.commit()?;
    Ok(())
}

/// Inner implementation of track cascade-delete: executes all SQL deletes on
/// the provided connection without managing its own transaction.  Callers
/// must ensure they are already inside a transaction or savepoint.
pub(crate) fn delete_track_sql(
    conn: &Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM value_time_splits WHERE source_track_guid = ?2 AND (source_feed_guid = ?1 OR source_feed_guid IS NULL)",
        params![feed_guid, track_guid],
    )?;
    conn.execute(
        "DELETE FROM payment_routes WHERE feed_guid = ?1 AND track_guid = ?2",
        params![feed_guid, track_guid],
    )?;
    conn.execute(
        "DELETE FROM entity_quality WHERE entity_type = 'track' AND entity_id IN (?1, ?2)",
        params![canonical_track_entity_id(feed_guid, track_guid), track_guid],
    )?;
    conn.execute(
        "DELETE FROM track_remote_items_raw WHERE track_guid = ?2 AND (feed_guid = ?1 OR feed_guid IS NULL)",
        params![feed_guid, track_guid],
    )?;
    conn.execute(
        "DELETE FROM tracks WHERE feed_guid = ?1 AND track_guid = ?2",
        params![feed_guid, track_guid],
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
    let mut stmt =
        conn.prepare("SELECT track_guid FROM tracks WHERE feed_guid = ?1 ORDER BY track_guid ASC")?;
    let track_guids = stmt
        .query_map(params![feed_guid], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;

    conn.execute(
        "DELETE FROM value_time_splits WHERE source_feed_guid = ?1",
        params![feed_guid],
    )?;
    for track_guid in &track_guids {
        conn.execute(
            "DELETE FROM value_time_splits WHERE source_track_guid = ?2 AND source_feed_guid IS NULL",
            params![feed_guid, track_guid],
        )?;
    }

    conn.execute(
        "DELETE FROM payment_routes WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM track_remote_items_raw WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    for track_guid in &track_guids {
        conn.execute(
            "DELETE FROM track_remote_items_raw WHERE track_guid = ?2 AND feed_guid IS NULL",
            params![feed_guid, track_guid],
        )?;
    }

    conn.execute(
        "DELETE FROM feed_payment_routes WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    for track_guid in &track_guids {
        conn.execute(
            "DELETE FROM entity_quality WHERE entity_type = 'track' AND entity_id IN (?1, ?2)",
            params![canonical_track_entity_id(feed_guid, track_guid), track_guid],
        )?;
    }
    conn.execute(
        "DELETE FROM entity_quality WHERE entity_type = 'feed' AND entity_id = ?1",
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

    conn.execute(
        "DELETE FROM feed_remote_items_raw WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM live_events WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM live_events_legacy WHERE feed_guid = ?1",
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
        "DELETE FROM source_item_transcripts WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM source_platform_claims WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    conn.execute(
        "DELETE FROM tracks WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    conn.execute("DELETE FROM feeds WHERE feed_guid = ?1", params![feed_guid])?;

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
    delete_feed_sql(&tx, feed_guid)?;

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
    feed_guid: &str,
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
        "DELETE FROM value_time_splits WHERE source_track_guid = ?2 AND (source_feed_guid = ?1 OR source_feed_guid IS NULL)",
        params![feed_guid, track_guid],
    )?;
    tx.execute(
        "DELETE FROM payment_routes WHERE feed_guid = ?1 AND track_guid = ?2",
        params![feed_guid, track_guid],
    )?;
    tx.execute(
        "DELETE FROM entity_quality WHERE entity_type = 'track' AND entity_id IN (?1, ?2)",
        params![canonical_track_entity_id(feed_guid, track_guid), track_guid],
    )?;
    tx.execute(
        "DELETE FROM track_remote_items_raw WHERE track_guid = ?2 AND (feed_guid = ?1 OR feed_guid IS NULL)",
        params![feed_guid, track_guid],
    )?;
    tx.execute(
        "DELETE FROM tracks WHERE feed_guid = ?1 AND track_guid = ?2",
        params![feed_guid, track_guid],
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

/// Stats returned by [`cleanup_orphaned_artists`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OrphanCleanupStats {
    pub artists_deleted: usize,
    pub credits_deleted: usize,
}

/// Deletes artists that have no live references in feeds or tracks, and cleans
/// up their associated rows.
///
/// An artist is considered orphaned if none of its `artist_credit_name` rows
/// has an `artist_credit_id` that appears in `feeds.artist_credit_id` or
/// `tracks.artist_credit_id`.
///
/// # Errors
///
/// Returns [`DbError`] if any query or deletion fails.
pub fn cleanup_orphaned_artists(conn: &mut Connection) -> Result<OrphanCleanupStats, DbError> {
    let tx = conn.transaction()?;

    // Collect artist_ids with no live credit reference.
    let mut stmt = tx.prepare(
        "SELECT a.artist_id \
         FROM artists a \
         WHERE NOT EXISTS ( \
             SELECT 1 \
             FROM artist_credit_name acn \
             WHERE acn.artist_id = a.artist_id \
               AND ( \
                   EXISTS(SELECT 1 FROM feeds     f WHERE f.artist_credit_id = acn.artist_credit_id) \
                OR EXISTS(SELECT 1 FROM tracks    t WHERE t.artist_credit_id = acn.artist_credit_id) \
               ) \
         ) \
         ORDER BY a.artist_id",
    )?;
    let orphan_ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    drop(stmt);

    let mut stats = OrphanCleanupStats::default();
    for artist_id in &orphan_ids {
        // Collect the artist_credit ids exclusively owned by this orphan before
        // deleting artist_credit_name rows.
        let mut credit_stmt = tx.prepare(
            "SELECT DISTINCT acn.artist_credit_id \
             FROM artist_credit_name acn \
             WHERE acn.artist_id = ?1 \
               AND NOT EXISTS ( \
                   SELECT 1 FROM artist_credit_name other \
                   WHERE other.artist_credit_id = acn.artist_credit_id \
                     AND other.artist_id <> ?1 \
               )",
        )?;
        let exclusive_credits: Vec<i64> = credit_stmt
            .query_map(params![artist_id], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        drop(credit_stmt);

        tx.execute(
            "DELETE FROM artist_aliases WHERE artist_id = ?1",
            params![artist_id],
        )?;
        tx.execute(
            "DELETE FROM external_ids WHERE entity_type = 'artist' AND entity_id = ?1",
            params![artist_id],
        )?;
        tx.execute(
            "DELETE FROM artist_credit_name WHERE artist_id = ?1",
            params![artist_id],
        )?;
        tx.execute(
            "DELETE FROM artists WHERE artist_id = ?1",
            params![artist_id],
        )?;
        stats.artists_deleted += 1;

        for credit_id in exclusive_credits {
            tx.execute(
                "DELETE FROM artist_credit WHERE id = ?1",
                params![credit_id],
            )?;
            stats.credits_deleted += 1;
        }
    }

    tx.commit()?;
    Ok(stats)
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
         publisher, language, explicit, itunes_type, release_artist, release_artist_sort, release_date, \
         release_kind, episode_count, newest_item_at, oldest_item_at, created_at, updated_at, raw_medium \
         FROM feeds WHERE feed_guid = ?1",
        params![feed_guid],
        |row| {
            let explicit_i: i64 = row.get(9)?;
            Ok(Feed {
                feed_guid:        row.get(0)?,
                feed_url:         row.get(1)?,
                title:            row.get(2)?,
                title_lower:      row.get(3)?,
                artist_credit_id: row.get(4)?,
                description:      row.get(5)?,
                image_url:        row.get(6)?,
                publisher:        row.get(7)?,
                language:         row.get(8)?,
                explicit:         explicit_i != 0,
                itunes_type:      row.get(10)?,
                release_artist:   row.get(11)?,
                release_artist_sort: row.get(12)?,
                release_date:     row.get(13)?,
                release_kind:     row.get(14)?,
                episode_count:    row.get(15)?,
                newest_item_at:   row.get(16)?,
                oldest_item_at:   row.get(17)?,
                created_at:       row.get(18)?,
                updated_at:       row.get(19)?,
                raw_medium:       row.get(20)?,
            })
        },
    ).optional()?;

    Ok(result)
}

// ── list_all_feed_guids ─────────────────────────────────────────────────────

/// Returns the `feed_guid` of every feed row in the database, in insertion order.
///
/// Used by the one-shot search/quality rebuild tool to iterate all feeds.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn list_all_feed_guids(conn: &Connection) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare("SELECT feed_guid FROM feeds ORDER BY rowid")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut guids = Vec::new();
    for row in rows {
        guids.push(row?);
    }
    Ok(guids)
}

// ── get_track_by_guid ───────────────────────────────────────────────────────

/// Looks up the track row by `track_guid`, returning `None` if absent.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails or the raw `track_guid` is
/// ambiguous across feeds.
pub fn get_track_by_guid(conn: &Connection, track_guid: &str) -> Result<Option<Track>, DbError> {
    let tracks = get_tracks_by_guid(conn, track_guid)?;
    match tracks.as_slice() {
        [] => Ok(None),
        [track] => Ok(Some(track.clone())),
        _ => Err(DbError::Other(format!(
            "track_guid {track_guid} is ambiguous; resolve with feed scope"
        ))),
    }
}

/// Returns all track rows matching the given source `track_guid`.
///
/// Today this usually returns at most one row, but callers that expose the
/// flat `/v1/tracks/{guid}` compatibility route use this helper so they can
/// detect and report future cross-feed collisions safely.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_tracks_by_guid(conn: &Connection, track_guid: &str) -> Result<Vec<Track>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, \
         duration_secs, image_url, publisher, language, enclosure_url, enclosure_type, enclosure_bytes, track_number, \
         season, explicit, description, track_artist, track_artist_sort, created_at, updated_at \
         FROM tracks WHERE track_guid = ?1 ORDER BY feed_guid ASC",
    )?;

    let rows = stmt.query_map(params![track_guid], |row| {
        let explicit_i: i64 = row.get(15)?;
        Ok(Track {
            track_guid: row.get(0)?,
            feed_guid: row.get(1)?,
            artist_credit_id: row.get(2)?,
            title: row.get(3)?,
            title_lower: row.get(4)?,
            pub_date: row.get(5)?,
            duration_secs: row.get(6)?,
            image_url: row.get(7)?,
            publisher: row.get(8)?,
            language: row.get(9)?,
            enclosure_url: row.get(10)?,
            enclosure_type: row.get(11)?,
            enclosure_bytes: row.get(12)?,
            track_number: row.get(13)?,
            season: row.get(14)?,
            explicit: explicit_i != 0,
            description: row.get(16)?,
            track_artist: row.get(17)?,
            track_artist_sort: row.get(18)?,
            created_at: row.get(19)?,
            updated_at: row.get(20)?,
        })
    })?;

    rows.collect::<Result<_, _>>().map_err(DbError::from)
}

/// Looks up a track by the canonical `(feed_guid, track_guid)` pair.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_track_for_feed(
    conn: &Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<Option<Track>, DbError> {
    let result = conn
        .query_row(
            "SELECT track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, \
         duration_secs, image_url, publisher, language, enclosure_url, enclosure_type, enclosure_bytes, track_number, \
         season, explicit, description, track_artist, track_artist_sort, created_at, updated_at \
         FROM tracks WHERE feed_guid = ?1 AND track_guid = ?2",
            params![feed_guid, track_guid],
            |row| {
                let explicit_i: i64 = row.get(15)?;
                Ok(Track {
                    track_guid: row.get(0)?,
                    feed_guid: row.get(1)?,
                    artist_credit_id: row.get(2)?,
                    title: row.get(3)?,
                    title_lower: row.get(4)?,
                    pub_date: row.get(5)?,
                    duration_secs: row.get(6)?,
                    image_url: row.get(7)?,
                    publisher: row.get(8)?,
                    language: row.get(9)?,
                    enclosure_url: row.get(10)?,
                    enclosure_type: row.get(11)?,
                    enclosure_bytes: row.get(12)?,
                    track_number: row.get(13)?,
                    season: row.get(14)?,
                    explicit: explicit_i != 0,
                    description: row.get(16)?,
                    track_artist: row.get(17)?,
                    track_artist_sort: row.get(18)?,
                    created_at: row.get(19)?,
                    updated_at: row.get(20)?,
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
         duration_secs, image_url, publisher, language, enclosure_url, enclosure_type, enclosure_bytes, track_number, \
         season, explicit, description, track_artist, track_artist_sort, created_at, updated_at \
         FROM tracks WHERE feed_guid = ?1",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        let explicit_i: i64 = row.get(15)?;
        Ok(Track {
            track_guid: row.get(0)?,
            feed_guid: row.get(1)?,
            artist_credit_id: row.get(2)?,
            title: row.get(3)?,
            title_lower: row.get(4)?,
            pub_date: row.get(5)?,
            duration_secs: row.get(6)?,
            image_url: row.get(7)?,
            publisher: row.get(8)?,
            language: row.get(9)?,
            enclosure_url: row.get(10)?,
            enclosure_type: row.get(11)?,
            enclosure_bytes: row.get(12)?,
            track_number: row.get(13)?,
            season: row.get(14)?,
            explicit: explicit_i != 0,
            description: row.get(16)?,
            track_artist: row.get(17)?,
            track_artist_sort: row.get(18)?,
            created_at: row.get(19)?,
            updated_at: row.get(20)?,
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
         NULLIF(custom_key, ''), NULLIF(custom_value, ''), split, fee \
         FROM feed_payment_routes WHERE feed_guid = ?1",
    )?;
    let rows = stmt.query_map(params![feed_guid], |row| {
        let rt_str: String = row.get(3)?;
        let fee_i: i64 = row.get(8)?;
        Ok(FeedPaymentRoute {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            recipient_name: row.get(2)?,
            route_type: route_type_from_db(&rt_str, "feed_payment_routes"),
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
        || existing.publisher != new.publisher
        || existing.language != new.language
        || existing.explicit != new.explicit
        || existing.itunes_type != new.itunes_type
        || existing.release_artist != new.release_artist
        || existing.release_artist_sort != new.release_artist_sort
        || existing.release_date != new.release_date
        || existing.release_kind != new.release_kind
        || existing.raw_medium != new.raw_medium
        || existing.feed_url != new.feed_url
}

/// Compares two tracks by their content fields (ignoring timestamps).
fn track_fields_changed(existing: &Track, new: &Track) -> bool {
    existing.title != new.title
        || existing.pub_date != new.pub_date
        || existing.duration_secs != new.duration_secs
        || existing.image_url != new.image_url
        || existing.language != new.language
        || existing.enclosure_url != new.enclosure_url
        || existing.enclosure_type != new.enclosure_type
        || existing.enclosure_bytes != new.enclosure_bytes
        || existing.track_number != new.track_number
        || existing.season != new.season
        || existing.explicit != new.explicit
        || existing.description != new.description
        || existing.publisher != new.publisher
        || existing.track_artist != new.track_artist
        || existing.track_artist_sort != new.track_artist_sort
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

fn track_remote_items_changed(existing: &[TrackRemoteItemRaw], new: &[TrackRemoteItemRaw]) -> bool {
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

fn source_item_transcripts_changed(
    existing: &[SourceItemTranscript],
    new: &[SourceItemTranscript],
) -> bool {
    existing.len() != new.len()
        || existing.iter().zip(new.iter()).any(|(a, b)| {
            a.feed_guid != b.feed_guid
                || a.entity_type != b.entity_type
                || a.entity_id != b.entity_id
                || a.position != b.position
                || a.url != b.url
                || a.mime_type != b.mime_type
                || a.language != b.language
                || a.rel != b.rel
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
    source_item_transcripts: &[SourceItemTranscript],
    source_platform_claims: &[SourcePlatformClaim],
    feed_routes: &[FeedPaymentRoute],
    live_events: &[LiveEvent],
    tracks: &[TrackIngestBundle],
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
                source_item_transcripts,
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
                source_item_transcripts,
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
    source_item_transcripts: &[SourceItemTranscript],
    source_platform_claims: &[SourcePlatformClaim],
    feed_routes: &[FeedPaymentRoute],
    live_events: &[LiveEvent],
    tracks: &[TrackIngestBundle],
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
    if !source_item_transcripts.is_empty() {
        event_rows.push(build_source_item_transcripts_event(
            feed,
            source_item_transcripts,
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

    for (i, (track, routes, vts, track_remote_items)) in tracks.iter().enumerate() {
        let credit = if i < track_credits.len() {
            &track_credits[i]
        } else {
            artist_credit
        };
        event_rows.push(build_track_upserted_event(
            track, routes, vts, credit, now, &warn_vec,
        )?);

        if !track_remote_items.is_empty() {
            event_rows.push(build_track_remote_items_event(
                track,
                track_remote_items,
                now,
                &warn_vec,
            )?);
        }
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
    source_item_transcripts: &[SourceItemTranscript],
    source_platform_claims: &[SourcePlatformClaim],
    feed_routes: &[FeedPaymentRoute],
    live_events: &[LiveEvent],
    tracks: &[TrackIngestBundle],
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

    // --- Staged item-transcript diff ---
    let existing_source_item_transcripts =
        get_source_item_transcripts_for_feed(conn, &feed.feed_guid)?;
    if source_item_transcripts_changed(&existing_source_item_transcripts, source_item_transcripts) {
        event_rows.push(build_source_item_transcripts_event(
            feed,
            source_item_transcripts,
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

    for (i, (track, routes, vts, track_remote_items)) in tracks.iter().enumerate() {
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

            if !track_remote_items.is_empty() {
                event_rows.push(build_track_remote_items_event(
                    track,
                    track_remote_items,
                    now,
                    &warn_vec,
                )?);
            }
        } else {
            // Track fields didn't change, but check if remote items changed
            let existing_remote_items =
                get_track_remote_items_for_feed_track(conn, &track.feed_guid, &track.track_guid)?;
            if !track_remote_items.is_empty()
                && track_remote_items_changed(&existing_remote_items, track_remote_items)
            {
                event_rows.push(build_track_remote_items_event(
                    track,
                    track_remote_items,
                    now,
                    &warn_vec,
                )?);
            }
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
    _artist: &Artist,
    _credit: &ArtistCredit,
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::FeedUpsertedPayload { feed: feed.clone() };
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

fn build_track_remote_items_event(
    track: &Track,
    remote_items: &[TrackRemoteItemRaw],
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::TrackRemoteItemsReplacedPayload {
        track_guid: track.track_guid.clone(),
        feed_guid: Some(track.feed_guid.clone()),
        remote_items: remote_items.to_vec(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::TrackRemoteItemsReplaced,
        payload_json,
        subject_guid: track.track_guid.clone(),
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

fn build_source_item_transcripts_event(
    feed: &Feed,
    transcripts: &[SourceItemTranscript],
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::SourceItemTranscriptsReplacedPayload {
        feed_guid: feed.feed_guid.clone(),
        transcripts: transcripts.to_vec(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    Ok(EventRow {
        event_id: uuid::Uuid::new_v4().to_string(),
        event_type: EventType::SourceItemTranscriptsReplaced,
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
    _credit: &ArtistCredit,
    now: i64,
    warnings: &[String],
) -> Result<EventRow, DbError> {
    let payload = crate::event::TrackUpsertedPayload {
        track: track.clone(),
        routes: routes.to_vec(),
        value_time_splits: vts.to_vec(),
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

// ── Tags ─────────────────────────────────────────────────────────────────────

// ── Relationships ────────────────────────────────────────────────────────────

// ── get_existing_feed ─────────────────────────────────────────────────────────

/// Looks up the feed row whose `feed_url` matches, returning `None` if absent.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL query fails.
pub fn get_existing_feed(conn: &Connection, feed_url: &str) -> Result<Option<Feed>, DbError> {
    let result = conn.query_row(
        "SELECT feed_guid, feed_url, title, title_lower, artist_credit_id, description, image_url, \
         publisher, language, explicit, itunes_type, release_artist, release_artist_sort, release_date, \
         release_kind, episode_count, newest_item_at, oldest_item_at, created_at, updated_at, raw_medium \
         FROM feeds WHERE feed_url = ?1",
        params![feed_url],
        |row| {
            let explicit_i: i64 = row.get(9)?;
            Ok(Feed {
                feed_guid:        row.get(0)?,
                feed_url:         row.get(1)?,
                title:            row.get(2)?,
                title_lower:      row.get(3)?,
                artist_credit_id: row.get(4)?,
                description:      row.get(5)?,
                image_url:        row.get(6)?,
                publisher:        row.get(7)?,
                language:         row.get(8)?,
                explicit:         explicit_i != 0,
                itunes_type:      row.get(10)?,
                release_artist:   row.get(11)?,
                release_artist_sort: row.get(12)?,
                release_date:     row.get(13)?,
                release_kind:     row.get(14)?,
                episode_count:    row.get(15)?,
                newest_item_at:   row.get(16)?,
                oldest_item_at:   row.get(17)?,
                created_at:       row.get(18)?,
                updated_at:       row.get(19)?,
                raw_medium:       row.get(20)?,
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
    source_item_transcripts: Vec<SourceItemTranscript>,
    source_platform_claims: Vec<SourcePlatformClaim>,
    feed_routes: Vec<FeedPaymentRoute>,
    live_events: Vec<LiveEvent>,
    tracks: Vec<TrackIngestBundle>,
    event_rows: Vec<EventRow>,
    signer: &NodeSigner,
) -> Result<Vec<(i64, String, String)>, DbError> {
    let source_contributor_claims = dedupe_source_contributor_claims(&source_contributor_claims);
    let source_entity_ids = dedupe_source_entity_ids(&source_entity_ids);
    let source_entity_links = dedupe_source_entity_links(&source_entity_links);
    let source_release_claims = dedupe_source_release_claims(&source_release_claims);
    let source_item_enclosures = dedupe_source_item_enclosures(&source_item_enclosures);
    let source_item_transcripts = dedupe_source_item_transcripts(&source_item_transcripts);
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
        upsert_artist_credit_sql(&tx, &artist_credit)?;
    }

    // 3. Upsert feed
    tx.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, description, image_url, \
         publisher, language, explicit, itunes_type, release_artist, release_artist_sort, release_date, \
         release_kind, episode_count, newest_item_at, oldest_item_at, created_at, updated_at, raw_medium) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21) \
         ON CONFLICT(feed_guid) DO UPDATE SET \
           feed_url         = excluded.feed_url, \
           title            = excluded.title, \
           title_lower      = excluded.title_lower, \
           artist_credit_id = excluded.artist_credit_id, \
           description      = excluded.description, \
           image_url        = excluded.image_url, \
           publisher        = excluded.publisher, \
           language         = excluded.language, \
           explicit         = excluded.explicit, \
           itunes_type      = excluded.itunes_type, \
           release_artist   = excluded.release_artist, \
           release_artist_sort = excluded.release_artist_sort, \
           release_date     = excluded.release_date, \
           release_kind     = excluded.release_kind, \
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
            feed.publisher,
            feed.language,
            i64::from(feed.explicit),
            feed.itunes_type,
            feed.release_artist,
            feed.release_artist_sort,
            feed.release_date,
            feed.release_kind,
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
                r.custom_key.as_deref().unwrap_or(""),
                r.custom_value.as_deref().unwrap_or(""),
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
    for live_event in dedupe_live_events(&live_events) {
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

    // 3j. Replace staged item transcripts for this feed
    tx.execute(
        "DELETE FROM source_item_transcripts WHERE feed_guid = ?1",
        params![feed.feed_guid],
    )?;
    for transcript in &source_item_transcripts {
        tx.execute(
            "INSERT INTO source_item_transcripts \
             (feed_guid, entity_type, entity_id, position, url, mime_type, language, rel, source, extraction_path, observed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                &transcript.feed_guid,
                &transcript.entity_type,
                &transcript.entity_id,
                transcript.position,
                &transcript.url,
                &transcript.mime_type,
                &transcript.language,
                &transcript.rel,
                &transcript.source,
                &transcript.extraction_path,
                transcript.observed_at,
            ],
        )?;
    }

    // 3k. Replace staged platform claims for this feed
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
    for (track, routes, splits, remote_items) in &tracks {
        tx.execute(
            "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, \
             duration_secs, image_url, publisher, language, enclosure_url, enclosure_type, enclosure_bytes, track_number, season, \
             explicit, description, track_artist, track_artist_sort, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21) \
             ON CONFLICT(feed_guid, track_guid) DO UPDATE SET \
               artist_credit_id = excluded.artist_credit_id, \
               title            = excluded.title, \
               title_lower      = excluded.title_lower, \
               pub_date         = excluded.pub_date, \
               duration_secs    = excluded.duration_secs, \
               image_url        = excluded.image_url, \
               publisher        = excluded.publisher, \
               language         = excluded.language, \
               enclosure_url    = excluded.enclosure_url, \
               enclosure_type   = excluded.enclosure_type, \
               enclosure_bytes  = excluded.enclosure_bytes, \
               track_number     = excluded.track_number, \
               season           = excluded.season, \
               explicit         = excluded.explicit, \
               description      = excluded.description, \
               track_artist     = excluded.track_artist, \
               track_artist_sort = excluded.track_artist_sort, \
               updated_at       = excluded.updated_at",
            params![
                track.track_guid,
                track.feed_guid,
                track.artist_credit_id,
                track.title,
                track.title_lower,
                track.pub_date,
                track.duration_secs,
                track.image_url,
                track.publisher,
                track.language,
                track.enclosure_url,
                track.enclosure_type,
                track.enclosure_bytes,
                track.track_number,
                track.season,
                i64::from(track.explicit),
                track.description,
                track.track_artist,
                track.track_artist_sort,
                track.created_at,
                track.updated_at,
            ],
        )?;

        // replace payment routes
        tx.execute(
            "DELETE FROM payment_routes WHERE feed_guid = ?1 AND track_guid = ?2",
            params![track.feed_guid, track.track_guid],
        )?;
        for r in routes {
            let route_type = serde_json::to_string(&r.route_type)?;
            let route_type = route_type.trim_matches('"');
            tx.execute(
                "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, address, \
                 custom_key, custom_value, split, fee) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    &track.track_guid,
                    &track.feed_guid,
                    r.recipient_name,
                    route_type,
                    r.address,
                    r.custom_key.as_deref().unwrap_or(""),
                    r.custom_value.as_deref().unwrap_or(""),
                    r.split,
                    i64::from(r.fee),
                ],
            )?;
        }

        // replace value time splits
        tx.execute(
            "DELETE FROM value_time_splits WHERE source_track_guid = ?2 AND (source_feed_guid = ?1 OR source_feed_guid IS NULL)",
            params![track.feed_guid, track.track_guid],
        )?;
        for s in splits {
            tx.execute(
                "INSERT INTO value_time_splits (source_feed_guid, source_track_guid, start_time_secs, duration_secs, \
                 remote_feed_guid, remote_item_guid, split, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    if s.source_feed_guid.is_empty() {
                        track.feed_guid.as_str()
                    } else {
                        s.source_feed_guid.as_str()
                    },
                    &track.track_guid,
                    s.start_time_secs,
                    s.duration_secs,
                    s.remote_feed_guid,
                    s.remote_item_guid,
                    s.split,
                    s.created_at,
                ],
            )?;
        }

        // replace track remote items
        tx.execute(
            "DELETE FROM track_remote_items_raw WHERE track_guid = ?2 AND (feed_guid = ?1 OR feed_guid IS NULL)",
            params![track.feed_guid, track.track_guid],
        )?;
        for item in remote_items {
            tx.execute(
                "INSERT INTO track_remote_items_raw \
                 (feed_guid, track_guid, position, medium, remote_feed_guid, remote_feed_url, source) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    if item.feed_guid.is_empty() {
                        track.feed_guid.as_str()
                    } else {
                        item.feed_guid.as_str()
                    },
                    &track.track_guid,
                    item.position,
                    &item.medium,
                    &item.remote_feed_guid,
                    &item.remote_feed_url,
                    &item.source,
                ],
            )?;
        }
    }

    // 4b. Remove stale tracks that are no longer in the new crawl.
    // Issue-STALE-TRACKS — 2026-03-14
    let new_guids: std::collections::HashSet<&str> = tracks
        .iter()
        .map(|(t, _, _, _)| t.track_guid.as_str())
        .collect();
    let mut removal_event_rows: Vec<EventRow> = Vec::new();
    for removed_guid in &existing_guids {
        if new_guids.contains(removed_guid.as_str()) {
            continue;
        }
        // Look up the track to get search-index fields before deleting.
        let track_opt = get_track_for_feed(&tx, &feed.feed_guid, removed_guid)?;
        if let Some(track) = track_opt {
            // Remove the track's search index entry (best-effort).
            let _ = crate::search::delete_from_search_index(
                &tx,
                "track",
                &canonical_track_entity_id(&track.feed_guid, &track.track_guid),
                "",
                &track.title,
                track.description.as_deref().unwrap_or(""),
                "",
            );
            // Cascade-delete the track and its child rows.
            delete_track_sql(&tx, &feed.feed_guid, removed_guid)?;
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

    // 7. Rebuild search index + quality scores for the feed and all its tracks.
    // The retired resolver no longer handles this; inline it here so every
    // successful ingest leaves a fully-populated read model.
    sync_source_read_models_for_feed(&tx, &feed.feed_guid)?;

    // 8. Commit
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

/// Returns a source feed by GUID, or `None` if it does not exist.
pub fn get_feed(conn: &Connection, feed_guid: &str) -> Result<Option<Feed>, DbError> {
    conn.query_row(
        "SELECT feed_guid, feed_url, title, title_lower, artist_credit_id, description, \
         image_url, publisher, language, explicit, itunes_type, release_artist, release_artist_sort, \
         release_date, release_kind, episode_count, newest_item_at, oldest_item_at, created_at, updated_at, raw_medium \
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
                publisher: row.get(7)?,
                language: row.get(8)?,
                explicit: row.get(9)?,
                itunes_type: row.get(10)?,
                release_artist: row.get(11)?,
                release_artist_sort: row.get(12)?,
                release_date: row.get(13)?,
                release_kind: row.get(14)?,
                episode_count: row.get(15)?,
                newest_item_at: row.get(16)?,
                oldest_item_at: row.get(17)?,
                created_at: row.get(18)?,
                updated_at: row.get(19)?,
                raw_medium: row.get(20)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Returns a source track by GUID, or `None` if it does not exist.
pub fn get_track(conn: &Connection, track_guid: &str) -> Result<Option<Track>, DbError> {
    get_track_by_guid(conn, track_guid)
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

pub fn get_effective_source_contributor_claims_for_track(
    conn: &Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<Vec<SourceContributorClaim>, DbError> {
    let track_claims =
        get_source_contributor_claims_for_feed_entity(conn, feed_guid, "track", track_guid)?;
    if track_claims.is_empty() {
        get_source_contributor_claims_for_feed_entity(conn, feed_guid, "feed", feed_guid)
    } else {
        Ok(track_claims)
    }
}

pub fn get_source_contributor_claims_for_feed_entity(
    conn: &Connection,
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceContributorClaim>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, name, role, role_norm, \
         group_name, href, img, source, extraction_path, observed_at \
         FROM source_contributor_claims \
         WHERE feed_guid = ?1 AND entity_type = ?2 AND entity_id = ?3 \
         ORDER BY position, name, id",
    )?;
    let rows = stmt.query_map(params![feed_guid, entity_type, entity_id], |row| {
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
    rows.collect::<Result<_, _>>().map_err(DbError::from)
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

pub fn get_source_entity_ids_for_feed_entity(
    conn: &Connection,
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceEntityIdClaim>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, scheme, value, source, \
         extraction_path, observed_at \
         FROM source_entity_ids \
         WHERE feed_guid = ?1 AND entity_type = ?2 AND entity_id = ?3 \
         ORDER BY position, scheme, value, id",
    )?;
    let rows = stmt.query_map(params![feed_guid, entity_type, entity_id], |row| {
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
    rows.collect::<Result<_, _>>().map_err(DbError::from)
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

pub fn get_source_entity_links_for_feed_entity(
    conn: &Connection,
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceEntityLink>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, link_type, url, source, \
         extraction_path, observed_at \
         FROM source_entity_links \
         WHERE feed_guid = ?1 AND entity_type = ?2 AND entity_id = ?3 \
         ORDER BY position, link_type, url, id",
    )?;
    let rows = stmt.query_map(params![feed_guid, entity_type, entity_id], |row| {
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
    rows.collect::<Result<_, _>>().map_err(DbError::from)
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

pub fn get_source_release_claims_for_feed_entity(
    conn: &Connection,
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceReleaseClaim>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, claim_type, claim_value, \
         source, extraction_path, observed_at \
         FROM source_release_claims \
         WHERE feed_guid = ?1 AND entity_type = ?2 AND entity_id = ?3 \
         ORDER BY claim_type, position, id",
    )?;
    let rows = stmt.query_map(params![feed_guid, entity_type, entity_id], |row| {
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
    rows.collect::<Result<_, _>>().map_err(DbError::from)
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

pub fn get_source_item_enclosures_for_feed_entity(
    conn: &Connection,
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceItemEnclosure>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, url, mime_type, bytes, rel, \
         title, is_primary, source, extraction_path, observed_at \
         FROM source_item_enclosures \
         WHERE feed_guid = ?1 AND entity_type = ?2 AND entity_id = ?3 \
         ORDER BY position, url, id",
    )?;
    let rows = stmt.query_map(params![feed_guid, entity_type, entity_id], |row| {
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
    rows.collect::<Result<_, _>>().map_err(DbError::from)
}

pub fn get_source_item_transcripts_for_entity(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceItemTranscript>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, url, mime_type, language, rel, \
         source, extraction_path, observed_at \
         FROM source_item_transcripts \
         WHERE entity_type = ?1 AND entity_id = ?2 \
         ORDER BY position, url, id",
    )?;
    let rows = stmt.query_map(params![entity_type, entity_id], |row| {
        Ok(SourceItemTranscript {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            url: row.get(5)?,
            mime_type: row.get(6)?,
            language: row.get(7)?,
            rel: row.get(8)?,
            source: row.get(9)?,
            extraction_path: row.get(10)?,
            observed_at: row.get(11)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn get_source_item_transcripts_for_feed_entity(
    conn: &Connection,
    feed_guid: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<SourceItemTranscript>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, feed_guid, entity_type, entity_id, position, url, mime_type, language, rel, \
         source, extraction_path, observed_at \
         FROM source_item_transcripts \
         WHERE feed_guid = ?1 AND entity_type = ?2 AND entity_id = ?3 \
         ORDER BY position, url, id",
    )?;
    let rows = stmt.query_map(params![feed_guid, entity_type, entity_id], |row| {
        Ok(SourceItemTranscript {
            id: row.get(0)?,
            feed_guid: row.get(1)?,
            entity_type: row.get(2)?,
            entity_id: row.get(3)?,
            position: row.get(4)?,
            url: row.get(5)?,
            mime_type: row.get(6)?,
            language: row.get(7)?,
            rel: row.get(8)?,
            source: row.get(9)?,
            extraction_path: row.get(10)?,
            observed_at: row.get(11)?,
        })
    })?;
    rows.collect::<Result<_, _>>().map_err(DbError::from)
}
