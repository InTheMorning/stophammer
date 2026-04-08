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
    PaymentRoute, Recording, Release, ReleaseRecording, RouteType, SourceContributorClaim,
    SourceEntityIdClaim, SourceEntityLink, SourceFeedReleaseMap, SourceItemEnclosure,
    SourceItemRecordingMap, SourcePlatformClaim, SourceReleaseClaim, Track, ValueTimeSplit,
};
use crate::signing::NodeSigner;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::Digest;
use std::fmt;
use std::sync::{Arc, Mutex}; // Issue-SEQ-INTEGRITY — 2026-03-14

pub type Db = Arc<Mutex<Connection>>;

/// Default `SQLite` database path for local CLI tools and daemon env fallbacks.
pub const DEFAULT_DB_PATH: &str = "./stophammer.db";

/// Valid values for `wallets.wallet_class`.
pub const WALLET_CLASS_VALUES: &[&str] = &[
    "unknown",
    "person_artist",
    "organization_platform",
    "bot_service",
];

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
    // Migration 16: wallet entity tables — fact layer (endpoints, aliases, route maps)
    // and derived layer (owners, artist links, reviews, overrides).
    include_str!("../migrations/0016_wallet_entities.sql"),
    // Migration 17: allow force_confidence wallet overrides for operator review tooling.
    include_str!("../migrations/0017_wallet_force_confidence_override.sql"),
    // Migration 18: audit applied wallet merge batches for operator undo.
    include_str!("../migrations/0018_wallet_merge_apply_batches.sql"),
    // Migration 19: add cleanup triggers for direct feed/track deletes on legacy tables.
    include_str!("../migrations/0019_feed_delete_cleanup_triggers.sql"),
    // Migration 20: dedupe legacy NULL-scoped artist credits and enforce normalized uniqueness.
    include_str!("../migrations/0020_artist_credit_null_scope_dedup.sql"),
    // Migration 21: normalize route custom fields to empty strings instead of NULL.
    include_str!("../migrations/0021_route_custom_value_normalization.sql"),
    // Migration 22: clear wallet route maps in direct delete triggers and legacy live events.
    include_str!("../migrations/0022_wallet_route_delete_triggers.sql"),
    // Migration 23: normalize wallet review rows onto source/evidence/payload fields.
    include_str!("../migrations/0023_wallet_identity_review_normalization.sql"),
    // Migration 24: align wallet review statuses with artist review semantics.
    include_str!("../migrations/0024_wallet_review_status_parity.sql"),
    // Migration 25: add source-first feed/track artist, publisher, artwork, and date fields.
    include_str!("../migrations/0025_source_first_feed_track_fields.sql"),
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

/// Inserts or updates a track row keyed on `track_guid`.
///
/// # Errors
///
/// Returns [`DbError`] if the SQL upsert fails.
pub fn upsert_track(conn: &Connection, track: &Track) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, \
         duration_secs, image_url, language, enclosure_url, enclosure_type, enclosure_bytes, track_number, season, \
         explicit, description, track_artist, track_artist_sort, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20) \
         ON CONFLICT(track_guid) DO UPDATE SET \
           feed_guid        = excluded.feed_guid, \
           artist_credit_id = excluded.artist_credit_id, \
           title            = excluded.title, \
           title_lower      = excluded.title_lower, \
           pub_date         = excluded.pub_date, \
           duration_secs    = excluded.duration_secs, \
           image_url        = excluded.image_url, \
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

fn normalized_artist_text_key(prefix: &str, text: &str) -> Option<String> {
    let normalized = text.trim().to_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(format!("{prefix}:{normalized}"))
    }
}

fn artist_credit_display_key(conn: &Connection, artist_credit_id: i64) -> Result<String, DbError> {
    Ok(format!(
        "artist_credit_display:{}",
        get_artist_credit_display_name(conn, artist_credit_id)?
    ))
}

fn source_first_release_artist_key(feed: &Feed) -> Option<String> {
    feed.release_artist
        .as_deref()
        .and_then(|text| normalized_artist_text_key("release_artist", text))
}

fn source_first_track_artist_key(track: &Track, feed: &Feed) -> Option<String> {
    track
        .track_artist
        .as_deref()
        .and_then(|text| normalized_artist_text_key("track_artist", text))
        .or_else(|| source_first_release_artist_key(feed))
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

    source_first_release_artist_key(feed).map_or_else(
        || artist_credit_display_key(conn, feed.artist_credit_id),
        Ok,
    )
}

fn track_artist_evidence_key(
    conn: &Connection,
    feed: &Feed,
    track: &Track,
) -> Result<String, DbError> {
    source_first_track_artist_key(track, feed)
        .map_or_else(|| feed_artist_evidence_key(conn, feed), Ok)
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

    let artist_display_key = track_artist_evidence_key(conn, feed, track)?;
    let mut corroborating_platforms = current_platforms;
    let mut found_match = false;
    let mut has_lower_neighbor = false;
    let min_duration = duration_secs.saturating_sub(1);
    let max_duration = duration_secs.saturating_add(1);

    let mut stmt = conn.prepare(
        "SELECT f.feed_guid, f.artist_credit_id, f.release_artist, t.track_artist, t.duration_secs \
         FROM feeds f \
         JOIN tracks t ON t.feed_guid = f.feed_guid \
         WHERE f.feed_guid <> ?1 \
           AND f.title_lower = ?2 \
           AND t.title_lower = ?3 \
           AND t.duration_secs BETWEEN ?4 AND ?5 \
           AND (SELECT COUNT(*) FROM tracks t2 WHERE t2.feed_guid = f.feed_guid) = 1",
    )?;
    let candidates: Vec<(String, i64, Option<String>, Option<String>, i64)> = stmt
        .query_map(
            params![
                feed.feed_guid,
                feed.title_lower,
                track.title_lower,
                min_duration,
                max_duration,
            ],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?
        .collect::<Result<_, _>>()?;

    for (
        candidate_feed_guid,
        candidate_artist_credit_id,
        candidate_release_artist,
        candidate_track_artist,
        candidate_duration_secs,
    ) in candidates
    {
        let candidate_artist_key = candidate_track_artist
            .as_deref()
            .and_then(|text| normalized_artist_text_key("track_artist", text))
            .or_else(|| {
                candidate_release_artist
                    .as_deref()
                    .and_then(|text| normalized_artist_text_key("release_artist", text))
            })
            .map_or_else(
                || artist_credit_display_key(conn, candidate_artist_credit_id),
                Ok,
            )?;
        if candidate_artist_key != artist_display_key {
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

    let artist_key = track_artist_evidence_key(conn, feed, track)?;
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
    let mut seen_recordings = std::collections::BTreeSet::new();
    let mut position = 0_i64;
    for track in &tracks {
        let recording_id: Option<String> = conn
            .query_row(
                "SELECT recording_id FROM source_item_recording_map WHERE track_guid = ?1",
                params![track.track_guid],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(recording_id) = recording_id
            && seen_recordings.insert(recording_id.clone())
        {
            position = position
                .checked_add(1)
                .ok_or_else(|| DbError::Other("release track position overflow".to_string()))?;
            conn.execute(
                "INSERT INTO release_recordings (release_id, recording_id, position, source_track_guid) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![release_id, recording_id, position, track.track_guid],
            )?;
        }
    }
    Ok(())
}

pub(crate) fn cleanup_orphaned_canonical_rows(conn: &Connection) -> Result<(), DbError> {
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

    Ok(())
}

fn dedupe_release_recordings(rows: Vec<ReleaseRecording>) -> Vec<ReleaseRecording> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::with_capacity(rows.len());

    for mut row in rows {
        if seen.insert((row.release_id.clone(), row.recording_id.clone())) {
            row.position = i64::try_from(deduped.len() + 1).unwrap_or(i64::MAX);
            deduped.push(row);
        }
    }

    deduped
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
    for row in &dedupe_release_recordings(payload.release_recordings.clone()) {
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

/// Updates canonical search index rows for the release/recording objects
/// currently mapped from one feed.
///
/// Search stays canonical-first at the API layer. Source `feed`/`track`
/// search rows are maintained separately via [`sync_source_read_models_for_feed`].
pub fn sync_canonical_search_index_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<(), DbError> {
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
        "DELETE FROM value_time_splits WHERE source_track_guid = ?1",
        params![track_guid],
    )?;
    conn.execute(
        "DELETE FROM wallet_track_route_map WHERE route_id IN ( \
             SELECT id FROM payment_routes WHERE track_guid = ?1 \
         )",
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

    // 1. value_time_splits for all tracks (subquery)
    conn.execute(
        "DELETE FROM value_time_splits WHERE source_track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;

    // 2. payment_routes for all tracks (subquery)
    conn.execute(
        "DELETE FROM wallet_track_route_map WHERE route_id IN ( \
         SELECT id FROM payment_routes WHERE track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1))",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM payment_routes WHERE track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;

    // 5. feed_payment_routes
    conn.execute(
        "DELETE FROM wallet_feed_route_map WHERE route_id IN ( \
             SELECT id FROM feed_payment_routes WHERE feed_guid = ?1 \
         )",
        params![feed_guid],
    )?;
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

    // 8. proof_tokens & proof_challenges (SG-07)
    conn.execute(
        "DELETE FROM proof_tokens WHERE subject_feed_guid = ?1",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM proof_challenges WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    // 9. Feed-scoped relationships
    // 10. Feed-scoped staged/source rows
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
        "DELETE FROM source_platform_claims WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    // 10b. Derived canonical release/recording mappings for this feed
    conn.execute(
        "DELETE FROM source_item_recording_map WHERE track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    conn.execute(
        "DELETE FROM source_feed_release_map WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    // 11. tracks
    conn.execute(
        "DELETE FROM tracks WHERE feed_guid = ?1",
        params![feed_guid],
    )?;

    // 12. feeds
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
        "DELETE FROM value_time_splits WHERE source_track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM wallet_track_route_map WHERE route_id IN ( \
         SELECT id FROM payment_routes WHERE track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1))",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM payment_routes WHERE track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM wallet_feed_route_map WHERE route_id IN ( \
             SELECT id FROM feed_payment_routes WHERE feed_guid = ?1 \
         )",
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
        "DELETE FROM feed_remote_items_raw WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM live_events WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM live_events_legacy WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM source_contributor_claims WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM source_entity_ids WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM source_entity_links WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM source_release_claims WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM source_item_enclosures WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM source_platform_claims WHERE feed_guid = ?1",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM source_item_recording_map WHERE track_guid IN \
         (SELECT track_guid FROM tracks WHERE feed_guid = ?1)",
        params![feed_guid],
    )?;
    tx.execute(
        "DELETE FROM source_feed_release_map WHERE feed_guid = ?1",
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingReviewConfidenceSummary {
    pub confidence: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReviewScoreComponent {
    pub source: String,
    pub points: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingReviewScoreSummary {
    pub score_band: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingReviewConflictSummary {
    pub reason: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingReviewAgeSummary {
    pub total: usize,
    pub created_last_24h: usize,
    pub older_than_7d: usize,
    pub oldest_created_at: Option<i64>,
}
fn normalize_artist_similarity_key(name: &str) -> Option<String> {
    let normalized = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect::<String>();
    (normalized.len() >= 4).then_some(normalized)
}

const WALLET_HIGH_CONFIDENCE_MIN_SCORE: u16 = 55;

fn wallet_review_confidence(
    source: &str,
    score: Option<u16>,
    conflict_reasons: &[String],
) -> &'static str {
    match source {
        "likely_wallet_owner_match" if !conflict_reasons.is_empty() => "blocked",
        "likely_wallet_owner_match" => {
            if score.unwrap_or_default() >= WALLET_HIGH_CONFIDENCE_MIN_SCORE {
                "high_confidence"
            } else {
                "review_required"
            }
        }
        _ => "review_required",
    }
}

fn wallet_review_explanation(source: &str, conflict_reasons: &[String]) -> &'static str {
    match source {
        "likely_wallet_owner_match"
            if conflict_reasons
                .iter()
                .any(|reason| reason == "conflicting_artist_link") =>
        {
            "Multiple wallets share the same normalized alias, but conflicting artist links block likely-owner escalation."
        }
        "cross_wallet_alias" => {
            "Multiple wallets share the same normalized alias across feed evidence, but ownership is still ambiguous."
        }
        "likely_wallet_owner_match" => {
            "Multiple wallets share the same normalized alias and also share stronger feed or artist-link evidence, so they likely belong to one owner but still require review."
        }
        _ => {
            "This wallet review source requires operator confirmation before identity state changes."
        }
    }
}

fn wallets_share_artist_link(conn: &Connection, wallet_ids: &[String]) -> Result<bool, DbError> {
    let mut artist_wallet_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut stmt = conn.prepare(
        "SELECT artist_id
         FROM wallet_artist_links
         WHERE wallet_id = ?1
         ORDER BY artist_id",
    )?;
    for wallet_id in wallet_ids {
        let artist_ids = stmt
            .query_map(params![wallet_id], |row| row.get::<_, String>(0))?
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        for artist_id in artist_ids {
            let count = artist_wallet_counts.entry(artist_id).or_default();
            *count += 1;
            if *count >= 2 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn wallets_have_conflicting_artist_links(
    conn: &Connection,
    wallet_ids: &[String],
) -> Result<bool, DbError> {
    if wallets_share_artist_link(conn, wallet_ids)? {
        return Ok(false);
    }

    let mut all_artist_ids = std::collections::BTreeSet::new();
    let mut stmt = conn.prepare(
        "SELECT artist_id
         FROM wallet_artist_links
         WHERE wallet_id = ?1
         ORDER BY artist_id",
    )?;
    for wallet_id in wallet_ids {
        let artist_ids = stmt
            .query_map(params![wallet_id], |row| row.get::<_, String>(0))?
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        all_artist_ids.extend(artist_ids);
        if all_artist_ids.len() > 1 {
            return Ok(true);
        }
    }

    Ok(false)
}

fn wallets_share_feed(conn: &Connection, wallet_ids: &[String]) -> Result<bool, DbError> {
    let mut feed_wallet_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT fg FROM (
             SELECT pr.feed_guid AS fg
             FROM wallet_endpoints we
             JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id
             JOIN payment_routes pr ON pr.id = wtrm.route_id
             WHERE we.wallet_id = ?1
             UNION
             SELECT fpr.feed_guid AS fg
             FROM wallet_endpoints we
             JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id
             JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id
             WHERE we.wallet_id = ?1
         )",
    )?;
    for wallet_id in wallet_ids {
        let feed_guids = stmt
            .query_map(params![wallet_id], |row| row.get::<_, String>(0))?
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        for feed_guid in feed_guids {
            let count = feed_wallet_counts.entry(feed_guid).or_default();
            *count += 1;
            if *count >= 2 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn shared_artist_ids_for_wallets(
    conn: &Connection,
    wallet_ids: &[String],
) -> Result<Vec<String>, DbError> {
    let mut artist_wallet_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut stmt = conn.prepare(
        "SELECT artist_id
         FROM wallet_artist_links
         WHERE wallet_id = ?1
         ORDER BY artist_id",
    )?;
    for wallet_id in wallet_ids {
        let artist_ids = stmt
            .query_map(params![wallet_id], |row| row.get::<_, String>(0))?
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        for artist_id in artist_ids {
            *artist_wallet_counts.entry(artist_id).or_default() += 1;
        }
    }
    Ok(artist_wallet_counts
        .into_iter()
        .filter_map(|(artist_id, count)| (count >= 2).then_some(artist_id))
        .collect())
}

fn wallet_review_supporting_sources(
    conn: &Connection,
    source: &str,
    wallet_ids: &[String],
) -> Result<Vec<String>, DbError> {
    match source {
        "likely_wallet_owner_match" => {
            let mut supporting_sources = vec!["cross_wallet_alias".to_string()];
            if wallets_share_feed(conn, wallet_ids)? {
                supporting_sources.push("shared_feed_overlap".to_string());
            }
            if wallets_share_artist_link(conn, wallet_ids)? {
                supporting_sources.push("shared_artist_link".to_string());
            }
            Ok(supporting_sources)
        }
        _ => Ok(vec![]),
    }
}

fn wallet_review_conflict_reasons(
    conn: &Connection,
    source: &str,
    wallet_ids: &[String],
) -> Result<Vec<String>, DbError> {
    let mut conflict_reasons = Vec::new();
    if source == "likely_wallet_owner_match"
        && wallets_have_conflicting_artist_links(conn, wallet_ids)?
    {
        conflict_reasons.push("conflicting_artist_link".to_string());
    }
    Ok(conflict_reasons)
}

fn review_score_from_breakdown(score_breakdown: &[ReviewScoreComponent]) -> Option<u16> {
    (!score_breakdown.is_empty()).then(|| {
        score_breakdown
            .iter()
            .fold(0u16, |acc, component| acc.saturating_add(component.points))
            .min(100)
    })
}

fn wallet_review_score_breakdown(
    source: &str,
    supporting_sources: &[String],
) -> Vec<ReviewScoreComponent> {
    if source != "likely_wallet_owner_match" {
        return Vec::new();
    }
    supporting_sources
        .iter()
        .filter_map(|support| {
            let points = match support.as_str() {
                "cross_wallet_alias" | "shared_feed_overlap" => 25,
                "shared_artist_link" => 30,
                _ => 0,
            };
            (points > 0).then_some(ReviewScoreComponent {
                source: support.clone(),
                points,
            })
        })
        .collect()
}

fn review_confidence_priority(confidence: &str) -> u8 {
    match confidence {
        "high_confidence" => 0,
        "review_required" => 1,
        "blocked" => 2,
        _ => 3,
    }
}

fn review_score_priority(score: Option<u16>) -> std::cmp::Reverse<u16> {
    std::cmp::Reverse(score.unwrap_or(0))
}

fn review_score_band(score: Option<u16>) -> &'static str {
    match score {
        Some(80..=100) => "80_100",
        Some(60..=79) => "60_79",
        Some(1..=59) => "1_59",
        _ => "unscored",
    }
}

fn review_score_band_priority(score_band: &str) -> u8 {
    match score_band {
        "80_100" => 0,
        "60_79" => 1,
        "1_59" => 2,
        "unscored" => 3,
        _ => 4,
    }
}

/// Stats returned by [`cleanup_orphaned_artists`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OrphanCleanupStats {
    pub artists_deleted: usize,
    pub credits_deleted: usize,
}

/// Deletes artists that have no live references in feeds, tracks, releases, or
/// recordings, and cleans up their associated rows.
///
/// An artist is considered orphaned if none of its `artist_credit_name` rows
/// has an `artist_credit_id` that appears in `feeds.artist_credit_id`,
/// `tracks.artist_credit_id`, `releases.artist_credit_id`, or
/// `recordings.artist_credit_id`.
///
/// Does NOT delete `artist_id_redirect` targets — redirects are merge history,
/// not orphan state.
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
                   EXISTS(SELECT 1 FROM feeds     f WHERE f.artist_credit_id    = acn.artist_credit_id) \
                OR EXISTS(SELECT 1 FROM tracks    t WHERE t.artist_credit_id    = acn.artist_credit_id) \
                OR EXISTS(SELECT 1 FROM releases  r WHERE r.artist_credit_id    = acn.artist_credit_id) \
                OR EXISTS(SELECT 1 FROM recordings rc WHERE rc.artist_credit_id = acn.artist_credit_id) \
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
            "DELETE FROM artist_artist_rel \
             WHERE artist_id_a = ?1 OR artist_id_b = ?1",
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
         duration_secs, image_url, language, enclosure_url, enclosure_type, enclosure_bytes, track_number, \
         season, explicit, description, track_artist, track_artist_sort, created_at, updated_at \
         FROM tracks WHERE track_guid = ?1",
            params![track_guid],
            |row| {
                let explicit_i: i64 = row.get(14)?;
                Ok(Track {
                    track_guid: row.get(0)?,
                    feed_guid: row.get(1)?,
                    artist_credit_id: row.get(2)?,
                    title: row.get(3)?,
                    title_lower: row.get(4)?,
                    pub_date: row.get(5)?,
                    duration_secs: row.get(6)?,
                    image_url: row.get(7)?,
                    language: row.get(8)?,
                    enclosure_url: row.get(9)?,
                    enclosure_type: row.get(10)?,
                    enclosure_bytes: row.get(11)?,
                    track_number: row.get(12)?,
                    season: row.get(13)?,
                    explicit: explicit_i != 0,
                    description: row.get(15)?,
                    track_artist: row.get(16)?,
                    track_artist_sort: row.get(17)?,
                    created_at: row.get(18)?,
                    updated_at: row.get(19)?,
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
         duration_secs, image_url, language, enclosure_url, enclosure_type, enclosure_bytes, track_number, \
         season, explicit, description, track_artist, track_artist_sort, created_at, updated_at \
         FROM tracks WHERE feed_guid = ?1",
    )?;

    let rows = stmt.query_map(params![feed_guid], |row| {
        let explicit_i: i64 = row.get(14)?;
        Ok(Track {
            track_guid: row.get(0)?,
            feed_guid: row.get(1)?,
            artist_credit_id: row.get(2)?,
            title: row.get(3)?,
            title_lower: row.get(4)?,
            pub_date: row.get(5)?,
            duration_secs: row.get(6)?,
            image_url: row.get(7)?,
            language: row.get(8)?,
            enclosure_url: row.get(9)?,
            enclosure_type: row.get(10)?,
            enclosure_bytes: row.get(11)?,
            track_number: row.get(12)?,
            season: row.get(13)?,
            explicit: explicit_i != 0,
            description: row.get(15)?,
            track_artist: row.get(16)?,
            track_artist_sort: row.get(17)?,
            created_at: row.get(18)?,
            updated_at: row.get(19)?,
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

/// Returns the id of an existing tag with the given (lowercased) name, or
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
    source_platform_claims: Vec<SourcePlatformClaim>,
    feed_routes: Vec<FeedPaymentRoute>,
    live_events: Vec<LiveEvent>,
    tracks: Vec<(Track, Vec<PaymentRoute>, Vec<ValueTimeSplit>)>,
    event_rows: Vec<EventRow>,
    signer: &NodeSigner,
) -> Result<Vec<(i64, String, String)>, DbError> {
    let source_contributor_claims = dedupe_source_contributor_claims(&source_contributor_claims);
    let source_entity_ids = dedupe_source_entity_ids(&source_entity_ids);
    let source_entity_links = dedupe_source_entity_links(&source_entity_links);
    let source_release_claims = dedupe_source_release_claims(&source_release_claims);
    let source_item_enclosures = dedupe_source_item_enclosures(&source_item_enclosures);
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
        "DELETE FROM wallet_feed_route_map WHERE route_id IN ( \
             SELECT id FROM feed_payment_routes WHERE feed_guid = ?1 \
         )",
        params![feed.feed_guid],
    )?;
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
             duration_secs, image_url, language, enclosure_url, enclosure_type, enclosure_bytes, track_number, season, \
             explicit, description, track_artist, track_artist_sort, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20) \
             ON CONFLICT(track_guid) DO UPDATE SET \
               feed_guid        = excluded.feed_guid, \
               artist_credit_id = excluded.artist_credit_id, \
               title            = excluded.title, \
               title_lower      = excluded.title_lower, \
               pub_date         = excluded.pub_date, \
               duration_secs    = excluded.duration_secs, \
               image_url        = excluded.image_url, \
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
            "DELETE FROM wallet_track_route_map WHERE route_id IN ( \
                 SELECT id FROM payment_routes WHERE track_guid = ?1 \
             )",
            params![track.track_guid],
        )?;
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
                    r.custom_key.as_deref().unwrap_or(""),
                    r.custom_value.as_deref().unwrap_or(""),
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
    conn.query_row(
        "SELECT track_guid, feed_guid, artist_credit_id, title, title_lower, pub_date, \
         duration_secs, image_url, language, enclosure_url, enclosure_type, enclosure_bytes, track_number, \
         season, explicit, description, track_artist, track_artist_sort, created_at, updated_at \
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
                image_url: row.get(7)?,
                language: row.get(8)?,
                enclosure_url: row.get(9)?,
                enclosure_type: row.get(10)?,
                enclosure_bytes: row.get(11)?,
                track_number: row.get(12)?,
                season: row.get(13)?,
                explicit: row.get::<_, i64>(14)? != 0,
                description: row.get(15)?,
                track_artist: row.get(16)?,
                track_artist_sort: row.get(17)?,
                created_at: row.get(18)?,
                updated_at: row.get(19)?,
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

pub fn get_effective_source_contributor_claims_for_track(
    conn: &Connection,
    feed_guid: &str,
    track_guid: &str,
) -> Result<Vec<SourceContributorClaim>, DbError> {
    let track_claims = get_source_contributor_claims_for_entity(conn, "track", track_guid)?;
    if track_claims.is_empty() {
        get_source_contributor_claims_for_entity(conn, "feed", feed_guid)
    } else {
        Ok(track_claims)
    }
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

// ============================================================
// Wallet entity helpers — fact layer
// ============================================================

/// Normalize a payment address for identity matching.
///
/// Source route rows store addresses verbatim. This function produces the
/// canonical form used as the identity key in `wallet_endpoints`.
#[must_use]
pub fn normalize_wallet_address(_route_type: &str, address: &str) -> String {
    address.trim().to_lowercase()
}

/// Look up or create a `wallet_endpoints` row for the given identity 4-tuple.
///
/// If the endpoint already exists, updates the alias `last_seen_at` (if a
/// non-empty `recipient_name` is provided). If it does not exist, creates the
/// endpoint and an initial alias row.
///
/// Returns the `wallet_endpoints.id`.
///
/// **Does not create wallets** — `wallet_id` stays NULL until Pass 2.
pub fn get_or_create_endpoint(
    conn: &Connection,
    route_type: &str,
    address: &str,
    custom_key: &str,
    custom_value: &str,
    recipient_name: Option<&str>,
    timestamp: i64,
) -> Result<i64, DbError> {
    let norm_addr = normalize_wallet_address(route_type, address);
    let ck = custom_key.trim();
    let cv = custom_value.trim();

    // Try to find existing endpoint
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM wallet_endpoints \
             WHERE route_type = ?1 AND normalized_address = ?2 \
               AND custom_key = ?3 AND custom_value = ?4",
            params![route_type, norm_addr, ck, cv],
            |row| row.get(0),
        )
        .optional()?;

    let endpoint_id = if let Some(id) = existing {
        id
    } else {
        conn.execute(
            "INSERT INTO wallet_endpoints (route_type, normalized_address, custom_key, custom_value, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![route_type, norm_addr, ck, cv, timestamp],
        )?;
        conn.last_insert_rowid()
    };

    // Upsert alias if a non-empty name was given
    if let Some(name) = recipient_name {
        let name = name.trim();
        if !name.is_empty() {
            let name_lower = name.to_lowercase();
            conn.execute(
                "INSERT INTO wallet_aliases (endpoint_id, alias, alias_lower, first_seen_at, last_seen_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(endpoint_id, alias_lower) DO UPDATE SET last_seen_at = MAX(last_seen_at, excluded.last_seen_at)",
                params![endpoint_id, name, name_lower, timestamp, timestamp],
            )?;
        }
    }

    Ok(endpoint_id)
}

/// Create a route map entry linking a track-level payment route to an endpoint.
pub fn map_track_route_to_endpoint(
    conn: &Connection,
    route_id: i64,
    endpoint_id: i64,
    timestamp: i64,
) -> Result<(), DbError> {
    conn.execute(
        "INSERT OR IGNORE INTO wallet_track_route_map (route_id, endpoint_id, created_at) \
         VALUES (?1, ?2, ?3)",
        params![route_id, endpoint_id, timestamp],
    )?;
    Ok(())
}

/// Create a route map entry linking a feed-level payment route to an endpoint.
pub fn map_feed_route_to_endpoint(
    conn: &Connection,
    route_id: i64,
    endpoint_id: i64,
    timestamp: i64,
) -> Result<(), DbError> {
    conn.execute(
        "INSERT OR IGNORE INTO wallet_feed_route_map (route_id, endpoint_id, created_at) \
         VALUES (?1, ?2, ?3)",
        params![route_id, endpoint_id, timestamp],
    )?;
    Ok(())
}

// ============================================================
// Wallet entity helpers — owner (derived) layer
// ============================================================

/// Create a provisional wallet for the given endpoint and assign it.
///
/// Generates a new `wallet_id`, sets `display_name` from the endpoint's
/// first alias (or a placeholder), and applies hard-signal classification
/// only. Returns the new `wallet_id`.
pub fn create_provisional_wallet(
    conn: &Connection,
    endpoint_id: i64,
    timestamp: i64,
) -> Result<String, DbError> {
    let wallet_id = uuid::Uuid::new_v4().to_string();

    // Pick display name from the endpoint's first alias (by first_seen_at)
    let display_name: String = conn
        .query_row(
            "SELECT alias FROM wallet_aliases WHERE endpoint_id = ?1 \
             ORDER BY first_seen_at ASC, alias_lower ASC, id ASC LIMIT 1",
            params![endpoint_id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_else(|| format!("endpoint-{endpoint_id}"));

    let display_name_lower = display_name.to_lowercase();

    conn.execute(
        "INSERT INTO wallets (wallet_id, display_name, display_name_lower, wallet_class, class_confidence, created_at, updated_at) \
         VALUES (?1, ?2, ?3, 'unknown', 'provisional', ?4, ?5)",
        params![wallet_id, display_name, display_name_lower, timestamp, timestamp],
    )?;

    conn.execute(
        "UPDATE wallet_endpoints SET wallet_id = ?1 WHERE id = ?2",
        params![wallet_id, endpoint_id],
    )?;

    Ok(wallet_id)
}

/// Apply hard-signal classification to a wallet. Only `fee=true` and operator
/// overrides produce non-provisional confidence. Everything else stays as-is.
pub fn classify_wallet_hard_signals(conn: &Connection, wallet_id: &str) -> Result<(), DbError> {
    let now = unix_now();

    // Check operator overrides first (highest priority)
    let override_class: Option<String> = conn
        .query_row(
            "SELECT value FROM wallet_identity_override \
             WHERE wallet_id = ?1 AND override_type = 'force_class' \
             ORDER BY created_at DESC LIMIT 1",
            params![wallet_id],
            |r| r.get(0),
        )
        .optional()?;
    let override_confidence: Option<String> = conn
        .query_row(
            "SELECT value FROM wallet_identity_override \
             WHERE wallet_id = ?1 AND override_type = 'force_confidence' \
             ORDER BY created_at DESC LIMIT 1",
            params![wallet_id],
            |r| r.get(0),
        )
        .optional()?;

    if override_class.is_some() || override_confidence.is_some() {
        let current_class: String = conn.query_row(
            "SELECT wallet_class FROM wallets WHERE wallet_id = ?1",
            params![wallet_id],
            |r| r.get(0),
        )?;
        let class = override_class.unwrap_or(current_class);
        let confidence = override_confidence.unwrap_or_else(|| "reviewed".to_string());
        conn.execute(
            "UPDATE wallets SET wallet_class = ?1, class_confidence = ?2, updated_at = ?3 \
             WHERE wallet_id = ?4",
            params![class, confidence, now, wallet_id],
        )?;
        return Ok(());
    }

    // Check fee=true on any linked route (via endpoint → route map → source route)
    let has_fee: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM wallet_endpoints we
            JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id
            JOIN payment_routes pr ON pr.id = wtrm.route_id
            WHERE we.wallet_id = ?1 AND pr.fee = 1
            UNION ALL
            SELECT 1 FROM wallet_endpoints we
            JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id
            JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id
            WHERE we.wallet_id = ?1 AND fpr.fee = 1
        )",
        params![wallet_id],
        |r| r.get(0),
    )?;

    if has_fee {
        conn.execute(
            "UPDATE wallets SET wallet_class = 'bot_service', class_confidence = 'high_confidence', updated_at = ?1 \
             WHERE wallet_id = ?2",
            params![now, wallet_id],
        )?;
    }

    Ok(())
}

/// Apply an operator-reviewed class override to a wallet.
pub fn set_wallet_force_class(
    conn: &Connection,
    wallet_id: &str,
    wallet_class: &str,
) -> Result<(), DbError> {
    let now = unix_now();

    conn.execute(
        "INSERT INTO wallet_identity_override (override_type, wallet_id, target_id, value, created_at) \
         VALUES ('force_class', ?1, NULL, ?2, ?3)",
        params![wallet_id, wallet_class, now],
    )?;

    conn.execute(
        "UPDATE wallets SET wallet_class = ?1, class_confidence = 'reviewed', updated_at = ?2 \
         WHERE wallet_id = ?3",
        params![wallet_class, now, wallet_id],
    )?;

    Ok(())
}

/// Apply an operator-reviewed confidence override to a wallet.
pub fn set_wallet_force_confidence(
    conn: &Connection,
    wallet_id: &str,
    class_confidence: &str,
) -> Result<(), DbError> {
    let now = unix_now();

    conn.execute(
        "INSERT INTO wallet_identity_override (override_type, wallet_id, target_id, value, created_at) \
         VALUES ('force_confidence', ?1, NULL, ?2, ?3)",
        params![wallet_id, class_confidence, now],
    )?;

    conn.execute(
        "UPDATE wallets SET class_confidence = ?1, updated_at = ?2 \
         WHERE wallet_id = ?3",
        params![class_confidence, now, wallet_id],
    )?;

    Ok(())
}

/// Clear operator classification overrides and re-derive the wallet classification.
pub fn revert_wallet_operator_classification(
    conn: &Connection,
    wallet_id: &str,
) -> Result<(), DbError> {
    let now = unix_now();

    conn.execute(
        "DELETE FROM wallet_identity_override \
         WHERE wallet_id = ?1 AND override_type IN ('force_class', 'force_confidence')",
        params![wallet_id],
    )?;

    conn.execute(
        "UPDATE wallets SET wallet_class = 'unknown', class_confidence = 'provisional', updated_at = ?1 \
         WHERE wallet_id = ?2",
        params![now, wallet_id],
    )?;

    classify_wallet_hard_signals(conn, wallet_id)?;
    let _ = classify_wallet_soft_signals(conn, wallet_id)?;
    let _ = classify_wallet_split_heuristics(conn, wallet_id)?;

    Ok(())
}

/// Known platform alias patterns for soft-signal classification.
/// Each entry is (`alias_lower` exact match, `wallet_class`).
const PLATFORM_ALIAS_PATTERNS: &[(&str, &str)] = &[
    ("fountain", "organization_platform"),
    ("wavlake", "organization_platform"),
    ("alby", "organization_platform"),
    ("breez", "organization_platform"),
    ("podcast addict", "organization_platform"),
    ("rss blue", "organization_platform"),
    ("rssblue", "organization_platform"),
    ("buzzsprout", "organization_platform"),
    ("podverse", "organization_platform"),
    ("podhome", "organization_platform"),
    ("justcast", "organization_platform"),
];

/// Known lnaddress domains for soft-signal classification.
const PLATFORM_LNADDRESS_DOMAINS: &[(&str, &str)] = &[
    ("getalby.com", "organization_platform"),
    ("fountain.fm", "organization_platform"),
    ("wavlake.com", "organization_platform"),
    ("breez.technology", "organization_platform"),
];

/// Apply soft-signal classification to a wallet. Produces only `provisional`
/// confidence from known platform/app alias patterns and lnaddress domains.
/// Never overrides `high_confidence`, `reviewed`, or `blocked`.
pub fn classify_wallet_soft_signals(conn: &Connection, wallet_id: &str) -> Result<bool, DbError> {
    // Early exit: only classify wallets that are still unknown/provisional
    let (wallet_class, class_confidence): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wallet_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| DbError::Other(format!("wallet not found: {wallet_id}")))?;

    if class_confidence != "provisional" || wallet_class != "unknown" {
        return Ok(false);
    }

    // Check alias patterns
    let mut stmt = conn.prepare(
        "SELECT wa.alias_lower FROM wallet_aliases wa \
         JOIN wallet_endpoints we ON we.id = wa.endpoint_id \
         WHERE we.wallet_id = ?1",
    )?;
    let aliases: Vec<String> = stmt
        .query_map(params![wallet_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    for alias in &aliases {
        for &(pattern, class) in PLATFORM_ALIAS_PATTERNS {
            if alias == pattern {
                let now = unix_now();
                conn.execute(
                    "UPDATE wallets SET wallet_class = ?1, class_confidence = 'provisional', updated_at = ?2 \
                     WHERE wallet_id = ?3 AND class_confidence = 'provisional' AND wallet_class = 'unknown'",
                    params![class, now, wallet_id],
                )?;
                return Ok(true);
            }
        }
    }

    // Check lnaddress domains
    let mut ep_stmt = conn.prepare(
        "SELECT we.normalized_address FROM wallet_endpoints we \
         WHERE we.wallet_id = ?1 AND we.route_type = 'lnaddress'",
    )?;
    let addresses: Vec<String> = ep_stmt
        .query_map(params![wallet_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    for addr in &addresses {
        if let Some(domain) = addr.rsplit_once('@').map(|(_, d)| d) {
            for &(pattern_domain, class) in PLATFORM_LNADDRESS_DOMAINS {
                if domain == pattern_domain {
                    let now = unix_now();
                    conn.execute(
                        "UPDATE wallets SET wallet_class = ?1, class_confidence = 'provisional', updated_at = ?2 \
                         WHERE wallet_id = ?3 AND class_confidence = 'provisional' AND wallet_class = 'unknown'",
                        params![class, now, wallet_id],
                    )?;
                    return Ok(true);
                }
            }
        }
    }

    Ok(false)
}

/// Split-shape heuristic thresholds.
const SPLIT_SMALL_THRESHOLD: i64 = 5; // ≤5% = app-fee level
const SPLIT_DOMINANT_THRESHOLD: i64 = 50; // ≥50% = primary recipient
const SPLIT_MIN_FEED_COUNT: usize = 3; // across ≥3 unrelated feeds

/// Apply split-shape heuristics to a wallet. Produces only `provisional`
/// confidence. Never creates endpoints, auto-merges, or creates artist links.
pub fn classify_wallet_split_heuristics(
    conn: &Connection,
    wallet_id: &str,
) -> Result<bool, DbError> {
    // Early exit: only classify wallets still unknown/provisional
    let (wallet_class, class_confidence): (String, String) = conn
        .query_row(
            "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wallet_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| DbError::Other(format!("wallet not found: {wallet_id}")))?;

    if class_confidence != "provisional" || wallet_class != "unknown" {
        return Ok(false);
    }

    // Gather split data across all feeds
    let mut stmt = conn.prepare(
        "SELECT pr.split, pr.fee, pr.feed_guid \
         FROM wallet_endpoints we \
         JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id \
         JOIN payment_routes pr ON pr.id = wtrm.route_id \
         WHERE we.wallet_id = ?1 \
         UNION ALL \
         SELECT fpr.split, fpr.fee, fpr.feed_guid \
         FROM wallet_endpoints we \
         JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id \
         JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
         WHERE we.wallet_id = ?1",
    )?;

    let mut small_nonfee_feeds = std::collections::HashSet::new();
    let mut has_nonfee = false;
    let mut all_nonfee_dominant = true;
    let mut nonfee_feed_guids = std::collections::HashSet::new();

    stmt.query_map(params![wallet_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?
    .filter_map(Result::ok)
    .for_each(|(split, fee, feed_guid)| {
        if fee == 0 {
            has_nonfee = true;
            nonfee_feed_guids.insert(feed_guid.clone());
            if split <= SPLIT_SMALL_THRESHOLD {
                small_nonfee_feeds.insert(feed_guid);
            }
            if split < SPLIT_DOMINANT_THRESHOLD {
                all_nonfee_dominant = false;
            }
        }
    });

    let now = unix_now();

    // Repeated small non-fee share across many unrelated feeds → organization_platform
    if small_nonfee_feeds.len() >= SPLIT_MIN_FEED_COUNT {
        conn.execute(
            "UPDATE wallets SET wallet_class = 'organization_platform', class_confidence = 'provisional', updated_at = ?1 \
             WHERE wallet_id = ?2 AND class_confidence = 'provisional' AND wallet_class = 'unknown'",
            params![now, wallet_id],
        )?;
        return Ok(true);
    }

    // Dominant non-fee share in few feeds → person_artist
    if has_nonfee && all_nonfee_dominant && nonfee_feed_guids.len() <= 2 {
        conn.execute(
            "UPDATE wallets SET wallet_class = 'person_artist', class_confidence = 'provisional', updated_at = ?1 \
             WHERE wallet_id = ?2 AND class_confidence = 'provisional' AND wallet_class = 'unknown'",
            params![now, wallet_id],
        )?;
        return Ok(true);
    }

    Ok(false)
}

/// Re-derive the display name for a wallet from its grouped endpoints' aliases.
///
/// Uses the first-seen non-empty alias across all endpoints assigned to this
/// wallet, ordered deterministically by `first_seen_at ASC, alias_lower ASC, id ASC`.
pub fn update_wallet_display_name(conn: &Connection, wallet_id: &str) -> Result<(), DbError> {
    let now = unix_now();

    let display_name: Option<String> = conn
        .query_row(
            "SELECT wa.alias FROM wallet_aliases wa \
             JOIN wallet_endpoints we ON we.id = wa.endpoint_id \
             WHERE we.wallet_id = ?1 \
             ORDER BY wa.first_seen_at ASC, wa.alias_lower ASC, wa.id ASC LIMIT 1",
            params![wallet_id],
            |r| r.get(0),
        )
        .optional()?;

    if let Some(name) = display_name {
        let name_lower = name.to_lowercase();
        conn.execute(
            "UPDATE wallets SET display_name = ?1, display_name_lower = ?2, updated_at = ?3 \
             WHERE wallet_id = ?4",
            params![name, name_lower, now, wallet_id],
        )?;
    }

    Ok(())
}

/// Create a wallet→artist link if strong same-feed evidence exists.
///
/// Only creates links when the wallet is at `high_confidence` or `unknown`
/// classification (skips `bot_service/high_confidence`). Returns true if a
/// link was created.
pub fn link_wallet_to_artist_if_confident(
    conn: &Connection,
    wallet_id: &str,
    feed_guid: &str,
) -> Result<bool, DbError> {
    let now = unix_now();

    // Check wallet classification — skip bot_service/high_confidence
    let (wallet_class, class_confidence): (String, String) = conn.query_row(
        "SELECT wallet_class, class_confidence FROM wallets WHERE wallet_id = ?1",
        params![wallet_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    if wallet_class == "bot_service" && class_confidence == "high_confidence" {
        return Ok(false);
    }

    // Find artist IDs from the feed's artist credit that match wallet aliases
    // (exact same-feed artist credit evidence)
    let mut stmt = conn.prepare(
        "SELECT DISTINCT acn.artist_id FROM wallet_endpoints we \
         JOIN wallet_aliases wa ON wa.endpoint_id = we.id \
         JOIN artist_credit_name acn ON LOWER(acn.name) = wa.alias_lower \
         JOIN feeds f ON f.artist_credit_id = acn.artist_credit_id \
         WHERE we.wallet_id = ?1 AND f.feed_guid = ?2",
    )?;

    let alias_matched_artist_ids: Vec<String> = stmt
        .query_map(params![wallet_id, feed_guid], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    let (artist_ids, evidence_entity_type) = if alias_matched_artist_ids.is_empty() {
        (
            dominant_feed_artist_ids_for_wallet(conn, wallet_id, feed_guid)?,
            "feed_dominant_route",
        )
    } else {
        (alias_matched_artist_ids, "feed_alias")
    };

    let mut created = false;
    for artist_id in artist_ids {
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO wallet_artist_links \
             (wallet_id, artist_id, evidence_entity_type, evidence_entity_id, confidence, created_at) \
             VALUES (?1, ?2, ?3, ?4, 'high_confidence', ?5)",
            params![wallet_id, artist_id, evidence_entity_type, feed_guid, now],
        )?;
        if inserted > 0 {
            created = true;
        }
    }

    Ok(created)
}

fn dominant_feed_artist_ids_for_wallet(
    conn: &Connection,
    wallet_id: &str,
    feed_guid: &str,
) -> Result<Vec<String>, DbError> {
    let is_wavlake: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM source_platform_claims
             WHERE feed_guid = ?1 AND platform_key = 'wavlake'
         )",
        params![feed_guid],
        |row| row.get(0),
    )?;
    if is_wavlake
        || !wallet_has_dominant_feed_route(conn, wallet_id, feed_guid)?
        || !wallet_dominates_routed_tracks(conn, wallet_id, feed_guid)?
    {
        return Ok(vec![]);
    }

    let wallet_name_keys = wallet_name_keys_for_feed(conn, wallet_id, feed_guid)?;
    if wallet_name_keys.is_empty() {
        return Ok(vec![]);
    }

    let mut stmt = conn.prepare(
        "SELECT acn.artist_id, acn.name
         FROM feeds f
         JOIN artist_credit_name acn ON acn.artist_credit_id = f.artist_credit_id
         WHERE f.feed_guid = ?1
         ORDER BY acn.position, acn.artist_id",
    )?;
    let mut artist_ids = std::collections::BTreeSet::new();
    for (artist_id, artist_name) in stmt
        .query_map(params![feed_guid], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
    {
        let Some(artist_key) = normalize_artist_similarity_key(&artist_name) else {
            continue;
        };
        if wallet_name_keys
            .iter()
            .any(|wallet_key| wallet_name_matches_artist_key(wallet_key, &artist_key))
        {
            artist_ids.insert(artist_id);
        }
    }

    Ok(artist_ids.into_iter().collect())
}

fn wallet_has_dominant_feed_route(
    conn: &Connection,
    wallet_id: &str,
    feed_guid: &str,
) -> Result<bool, DbError> {
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1
             FROM wallet_endpoints we
             JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id
             JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id
             WHERE we.wallet_id = ?1
               AND fpr.feed_guid = ?2
               AND fpr.fee = 0
               AND fpr.split >= ?3
         )",
        params![wallet_id, feed_guid, SPLIT_DOMINANT_THRESHOLD],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn wallet_dominates_routed_tracks(
    conn: &Connection,
    wallet_id: &str,
    feed_guid: &str,
) -> Result<bool, DbError> {
    let total_routed_tracks: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT track_guid)
         FROM payment_routes
         WHERE feed_guid = ?1 AND fee = 0",
        params![feed_guid],
        |row| row.get(0),
    )?;
    if total_routed_tracks == 0 {
        return Ok(true);
    }

    let dominated_tracks: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT pr.track_guid)
         FROM wallet_endpoints we
         JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id
         JOIN payment_routes pr ON pr.id = wtrm.route_id
         WHERE we.wallet_id = ?1
           AND pr.feed_guid = ?2
           AND pr.fee = 0
           AND pr.split >= ?3
           AND pr.split = (
               SELECT MAX(other.split)
               FROM payment_routes other
               WHERE other.track_guid = pr.track_guid
                 AND other.fee = 0
           )",
        params![wallet_id, feed_guid, SPLIT_DOMINANT_THRESHOLD],
        |row| row.get(0),
    )?;

    Ok(dominated_tracks == total_routed_tracks)
}

fn wallet_name_keys_for_feed(
    conn: &Connection,
    wallet_id: &str,
    feed_guid: &str,
) -> Result<std::collections::BTreeSet<String>, DbError> {
    let mut keys = std::collections::BTreeSet::new();

    let mut alias_stmt = conn.prepare(
        "SELECT wa.alias
         FROM wallet_aliases wa
         JOIN wallet_endpoints we ON we.id = wa.endpoint_id
         WHERE we.wallet_id = ?1
         ORDER BY wa.first_seen_at ASC, wa.alias_lower ASC",
    )?;
    for alias in alias_stmt
        .query_map(params![wallet_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?
    {
        if let Some(key) = normalize_artist_similarity_key(&alias) {
            keys.insert(key);
        }
    }

    let mut route_stmt = conn.prepare(
        "SELECT DISTINCT name FROM (
             SELECT pr.recipient_name AS name
             FROM wallet_endpoints we
             JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id
             JOIN payment_routes pr ON pr.id = wtrm.route_id
             WHERE we.wallet_id = ?1
               AND pr.feed_guid = ?2
               AND pr.fee = 0
               AND pr.split >= ?3
             UNION
             SELECT fpr.recipient_name AS name
             FROM wallet_endpoints we
             JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id
             JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id
             WHERE we.wallet_id = ?1
               AND fpr.feed_guid = ?2
               AND fpr.fee = 0
               AND fpr.split >= ?3
         )
         WHERE TRIM(name) <> ''",
    )?;
    for name in route_stmt
        .query_map(
            params![wallet_id, feed_guid, SPLIT_DOMINANT_THRESHOLD],
            |row| row.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?
    {
        if let Some(key) = normalize_artist_similarity_key(&name) {
            keys.insert(key);
        }
    }

    Ok(keys)
}

fn wallet_name_matches_artist_key(wallet_key: &str, artist_key: &str) -> bool {
    wallet_key == artist_key
        || wallet_key.starts_with(artist_key)
        || wallet_key.ends_with(artist_key)
        || artist_key.starts_with(wallet_key)
        || artist_key.ends_with(wallet_key)
}

/// Inner wallet merge logic. Caller must hold a transaction.
fn merge_wallets_inner(conn: &Connection, old_id: &str, new_id: &str) -> Result<(), DbError> {
    let now = unix_now();

    // Repoint endpoints
    conn.execute(
        "UPDATE wallet_endpoints SET wallet_id = ?1 WHERE wallet_id = ?2",
        params![new_id, old_id],
    )?;

    // Repoint artist links (ignore conflicts on UNIQUE(wallet_id, artist_id))
    conn.execute(
        "UPDATE OR IGNORE wallet_artist_links SET wallet_id = ?1 WHERE wallet_id = ?2",
        params![new_id, old_id],
    )?;
    // Delete any orphaned links that couldn't be repointed due to conflict
    conn.execute(
        "DELETE FROM wallet_artist_links WHERE wallet_id = ?1",
        params![old_id],
    )?;

    // Repoint review items
    conn.execute(
        "UPDATE wallet_identity_review SET wallet_id = ?1 WHERE wallet_id = ?2",
        params![new_id, old_id],
    )?;

    // Insert redirect
    conn.execute(
        "INSERT OR REPLACE INTO wallet_id_redirect (old_wallet_id, new_wallet_id, created_at) \
         VALUES (?1, ?2, ?3)",
        params![old_id, new_id, now],
    )?;

    // Repoint any existing redirects that pointed to old_id
    conn.execute(
        "UPDATE wallet_id_redirect SET new_wallet_id = ?1 WHERE new_wallet_id = ?2",
        params![new_id, old_id],
    )?;

    // Delete the old wallet row
    conn.execute("DELETE FROM wallets WHERE wallet_id = ?1", params![old_id])?;

    // Re-derive display name of surviving wallet
    update_wallet_display_name(conn, new_id)?;

    Ok(())
}

/// Merge two wallets: repoint all references from `old_id` to `new_id`.
///
/// Updates endpoint assignments, artist links, review items, and inserts a
/// redirect. Re-derives the display name of the surviving wallet.
///
/// All writes are performed atomically within a single transaction.
pub fn merge_wallets(conn: &Connection, old_id: &str, new_id: &str) -> Result<(), DbError> {
    let tx = conn.unchecked_transaction()?;
    merge_wallets_inner(&tx, old_id, new_id)?;
    tx.commit()?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WalletUndoWalletRow {
    wallet_id: String,
    display_name: String,
    display_name_lower: String,
    wallet_class: String,
    class_confidence: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WalletUndoArtistLinkRow {
    artist_id: String,
    confidence: String,
    evidence_entity_type: String,
    evidence_entity_id: String,
    created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WalletUndoReviewRow {
    id: i64,
    wallet_id: String,
    #[serde(alias = "review_type")]
    source: String,
    #[serde(default, alias = "details")]
    evidence_key: Option<String>,
    #[serde(default)]
    wallet_ids: Vec<String>,
    #[serde(default)]
    endpoint_summary: Vec<WalletEndpointPreview>,
    status: String,
    created_at: i64,
    #[serde(default, alias = "resolved_at")]
    updated_at: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WalletUndoRedirectRow {
    old_wallet_id: String,
    new_wallet_id: String,
    created_at: i64,
}

#[derive(Debug, Default)]
struct WalletMergeBatchRecorder {
    batch_id: Option<i64>,
    next_seq: i64,
}

fn ensure_wallet_merge_batch(
    conn: &Connection,
    recorder: &mut WalletMergeBatchRecorder,
) -> Result<i64, DbError> {
    if let Some(batch_id) = recorder.batch_id {
        return Ok(batch_id);
    }

    let now = unix_now();
    let batch_id: i64 = conn.query_row(
        "INSERT INTO wallet_merge_apply_batch (source, created_at, merges_applied) \
         VALUES ('refresh', ?1, 0) RETURNING id",
        params![now],
        |r| r.get(0),
    )?;
    recorder.batch_id = Some(batch_id);
    Ok(batch_id)
}

fn audited_merge_wallets(
    conn: &Connection,
    old_id: &str,
    new_id: &str,
    reason: &str,
    recorder: &mut WalletMergeBatchRecorder,
) -> Result<bool, DbError> {
    if old_id == new_id {
        return Ok(false);
    }

    let old_wallet = conn
        .query_row(
            "SELECT wallet_id, display_name, display_name_lower, wallet_class, class_confidence, created_at, updated_at \
             FROM wallets WHERE wallet_id = ?1",
            params![old_id],
            |r| {
                Ok(WalletUndoWalletRow {
                    wallet_id: r.get(0)?,
                    display_name: r.get(1)?,
                    display_name_lower: r.get(2)?,
                    wallet_class: r.get(3)?,
                    class_confidence: r.get(4)?,
                    created_at: r.get(5)?,
                    updated_at: r.get(6)?,
                })
            },
        )
        .optional()?;
    let Some(old_wallet) = old_wallet else {
        return Ok(false);
    };

    let target_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM wallets WHERE wallet_id = ?1)",
        params![new_id],
        |r| r.get(0),
    )?;
    if !target_exists {
        return Err(DbError::Other(format!(
            "wallet merge target does not exist: {new_id}"
        )));
    }

    let old_endpoint_ids: Vec<i64> = conn
        .prepare("SELECT id FROM wallet_endpoints WHERE wallet_id = ?1 ORDER BY id")?
        .query_map(params![old_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    let old_artist_links: Vec<WalletUndoArtistLinkRow> = conn
        .prepare(
            "SELECT artist_id, confidence, evidence_entity_type, evidence_entity_id, created_at \
             FROM wallet_artist_links WHERE wallet_id = ?1 ORDER BY artist_id",
        )?
        .query_map(params![old_id], |r| {
            Ok(WalletUndoArtistLinkRow {
                artist_id: r.get(0)?,
                confidence: r.get(1)?,
                evidence_entity_type: r.get(2)?,
                evidence_entity_id: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let new_artist_ids: Vec<String> = conn
        .prepare(
            "SELECT artist_id FROM wallet_artist_links WHERE wallet_id = ?1 ORDER BY artist_id",
        )?
        .query_map(params![new_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    let moved_reviews: Vec<WalletUndoReviewRow> = conn
        .prepare(
            "SELECT id, wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, \
                    status, created_at, updated_at \
             FROM wallet_identity_review WHERE wallet_id = ?1 ORDER BY id",
        )?
        .query_map(params![old_id], |r| {
            Ok(WalletUndoReviewRow {
                id: r.get(0)?,
                wallet_id: r.get(1)?,
                source: r.get(2)?,
                evidence_key: Some(r.get(3)?),
                wallet_ids: serde_json::from_str::<Vec<String>>(&r.get::<_, String>(4)?).map_err(
                    |err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(err),
                        )
                    },
                )?,
                endpoint_summary: serde_json::from_str::<Vec<WalletEndpointPreview>>(
                    &r.get::<_, String>(5)?,
                )
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
                status: r.get(6)?,
                created_at: r.get(7)?,
                updated_at: Some(r.get(8)?),
            })
        })?
        .collect::<Result<_, _>>()?;

    let redirect_rows: Vec<WalletUndoRedirectRow> = conn
        .prepare(
            "SELECT old_wallet_id, new_wallet_id, created_at \
             FROM wallet_id_redirect \
             WHERE old_wallet_id = ?1 OR new_wallet_id = ?1 \
             ORDER BY old_wallet_id, new_wallet_id",
        )?
        .query_map(params![old_id], |r| {
            Ok(WalletUndoRedirectRow {
                old_wallet_id: r.get(0)?,
                new_wallet_id: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    // All writes — audit entry, merge, and batch counter — are committed atomically.
    let tx = conn.unchecked_transaction()?;

    let batch_id = ensure_wallet_merge_batch(&tx, recorder)?;
    tx.execute(
        "INSERT INTO wallet_merge_apply_entry \
         (batch_id, seq, reason, old_wallet_id, new_wallet_id, old_wallet_json, \
          old_endpoint_ids_json, old_artist_links_json, new_artist_ids_json, \
          moved_reviews_json, redirect_rows_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            batch_id,
            recorder.next_seq,
            reason,
            old_id,
            new_id,
            serde_json::to_string(&old_wallet)?,
            serde_json::to_string(&old_endpoint_ids)?,
            serde_json::to_string(&old_artist_links)?,
            serde_json::to_string(&new_artist_ids)?,
            serde_json::to_string(&moved_reviews)?,
            serde_json::to_string(&redirect_rows)?,
        ],
    )?;
    recorder.next_seq += 1;

    merge_wallets_inner(&tx, old_id, new_id)?;
    tx.execute(
        "UPDATE wallet_merge_apply_batch SET merges_applied = merges_applied + 1 WHERE id = ?1",
        params![batch_id],
    )?;
    tx.commit()?;
    Ok(true)
}

/// Stats returned by `cleanup_orphaned_wallets`.
#[derive(Debug, Default)]
pub struct WalletCleanupStats {
    pub wallets_deleted: usize,
}

/// Delete wallets that have no remaining endpoint references.
pub fn cleanup_orphaned_wallets(conn: &Connection) -> Result<WalletCleanupStats, DbError> {
    let deleted = conn.execute(
        "DELETE FROM wallets WHERE wallet_id NOT IN \
         (SELECT DISTINCT wallet_id FROM wallet_endpoints WHERE wallet_id IS NOT NULL)",
        [],
    )?;
    Ok(WalletCleanupStats {
        wallets_deleted: deleted,
    })
}

/// Purge wallet entities created from Wavlake routes.
///
/// Wavlake payment routes point to Wavlake infrastructure, not artist wallets.
/// This function removes the route-map links, detaches endpoints, and lets
/// `cleanup_orphaned_wallets` handle the rest.
///
/// Returns the number of route-map entries removed.
pub fn purge_wavlake_wallet_route_maps(conn: &Connection) -> Result<usize, DbError> {
    // Delete track-level route maps where the underlying route belongs to a Wavlake feed.
    let track_deleted = conn.execute(
        "DELETE FROM wallet_track_route_map WHERE route_id IN ( \
             SELECT pr.id FROM payment_routes pr \
             WHERE EXISTS ( \
                 SELECT 1 FROM source_platform_claims spc \
                 WHERE spc.feed_guid = pr.feed_guid AND spc.platform_key = 'wavlake' \
             ) \
         )",
        [],
    )?;

    // Delete feed-level route maps where the underlying route belongs to a Wavlake feed.
    let feed_deleted = conn.execute(
        "DELETE FROM wallet_feed_route_map WHERE route_id IN ( \
             SELECT fpr.id FROM feed_payment_routes fpr \
             WHERE EXISTS ( \
                 SELECT 1 FROM source_platform_claims spc \
                 WHERE spc.feed_guid = fpr.feed_guid AND spc.platform_key = 'wavlake' \
             ) \
         )",
        [],
    )?;

    // Detach endpoints that no longer have any route-map references.
    conn.execute(
        "UPDATE wallet_endpoints SET wallet_id = NULL WHERE id NOT IN ( \
             SELECT endpoint_id FROM wallet_track_route_map \
             UNION \
             SELECT endpoint_id FROM wallet_feed_route_map \
         )",
        [],
    )?;

    // Delete now-orphaned endpoints (no route maps at all).
    conn.execute(
        "DELETE FROM wallet_aliases WHERE endpoint_id NOT IN ( \
             SELECT endpoint_id FROM wallet_track_route_map \
             UNION \
             SELECT endpoint_id FROM wallet_feed_route_map \
         )",
        [],
    )?;
    conn.execute(
        "DELETE FROM wallet_endpoints WHERE id NOT IN ( \
             SELECT endpoint_id FROM wallet_track_route_map \
             UNION \
             SELECT endpoint_id FROM wallet_feed_route_map \
         )",
        [],
    )?;

    // Clean up dependent rows for wallets that are now orphaned (no endpoints),
    // so that cleanup_orphaned_wallets can DELETE them without FK violations.
    conn.execute(
        "DELETE FROM wallet_identity_review WHERE wallet_id NOT IN \
         (SELECT DISTINCT wallet_id FROM wallet_endpoints WHERE wallet_id IS NOT NULL)",
        [],
    )?;
    conn.execute(
        "DELETE FROM wallet_artist_links WHERE wallet_id NOT IN \
         (SELECT DISTINCT wallet_id FROM wallet_endpoints WHERE wallet_id IS NOT NULL)",
        [],
    )?;

    Ok(track_deleted + feed_deleted)
}

/// Within one feed, merge endpoints that share the same `alias_lower` and
/// the same `fee` status under one wallet. This is a conservative grouping
/// heuristic: same name + same fee flag within the same feed is strong
/// evidence of the same entity. Run only in Pass 5 (--refresh).
///
/// Returns the number of merges performed.
fn group_same_feed_endpoints_with_recorder(
    conn: &Connection,
    feed_guid: &str,
    recorder: &mut WalletMergeBatchRecorder,
) -> Result<usize, DbError> {
    // Use the wallet's classification as a proxy for fee status (bot_service = fee endpoint).
    // This avoids expensive multi-JOIN back through route maps to source routes.
    // Endpoints sharing the same alias_lower within the same feed, with the same
    // fee-derived classification, are merged.
    let mut stmt = conn.prepare(
        "SELECT wa.alias_lower, \
                GROUP_CONCAT(DISTINCT we.id) AS endpoint_ids, \
                w.wallet_class \
         FROM wallet_endpoints we \
         JOIN wallet_aliases wa ON wa.endpoint_id = we.id \
         JOIN wallets w ON w.wallet_id = we.wallet_id \
         WHERE we.wallet_id IS NOT NULL \
           AND we.id IN ( \
               SELECT wtrm.endpoint_id FROM wallet_track_route_map wtrm \
               JOIN payment_routes pr ON pr.id = wtrm.route_id \
               WHERE pr.feed_guid = ?1 \
               UNION \
               SELECT wfrm.endpoint_id FROM wallet_feed_route_map wfrm \
               JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
               WHERE fpr.feed_guid = ?1 \
           ) \
         GROUP BY wa.alias_lower, w.wallet_class \
         HAVING COUNT(DISTINCT we.id) > 1",
    )?;

    let groups: Vec<(String, String)> = stmt
        .query_map(params![feed_guid], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<_, _>>()?;

    let mut merges = 0;
    for (_alias, ep_ids_str) in groups {
        let ep_ids: Vec<i64> = ep_ids_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        if ep_ids.len() < 2 {
            continue;
        }

        // Find the wallet_ids for these endpoints
        let mut unique_wallets: Vec<String> = ep_ids
            .iter()
            .filter_map(|ep_id| {
                conn.query_row(
                    "SELECT wallet_id FROM wallet_endpoints WHERE id = ?1 AND wallet_id IS NOT NULL",
                    params![ep_id],
                    |r| r.get(0),
                )
                .ok()
            })
            .collect();

        unique_wallets.sort();
        unique_wallets.dedup();

        if unique_wallets.len() < 2 {
            continue;
        }

        let target = unique_wallets[0].clone();
        for other in &unique_wallets[1..] {
            if audited_merge_wallets(conn, other, &target, "grouping", recorder)? {
                merges += 1;
            }
        }
    }

    Ok(merges)
}

pub fn group_same_feed_endpoints(conn: &Connection, feed_guid: &str) -> Result<usize, DbError> {
    let mut recorder = WalletMergeBatchRecorder::default();
    group_same_feed_endpoints_with_recorder(conn, feed_guid, &mut recorder)
}

fn resolve_wallet_redirect(conn: &Connection, wallet_id: &str) -> Result<String, DbError> {
    let mut current = wallet_id.to_string();
    let mut seen = std::collections::BTreeSet::new();

    loop {
        if !seen.insert(current.clone()) {
            return Err(DbError::Other(format!(
                "wallet redirect cycle detected starting at {wallet_id}"
            )));
        }

        let next = conn
            .query_row(
                "SELECT new_wallet_id FROM wallet_id_redirect WHERE old_wallet_id = ?1",
                params![current.as_str()],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        match next {
            Some(next_wallet_id) => current = next_wallet_id,
            None => return Ok(current),
        }
    }
}

fn apply_wallet_merge_overrides_with_recorder(
    conn: &Connection,
    recorder: &mut WalletMergeBatchRecorder,
) -> Result<usize, DbError> {
    let mut stmt = conn.prepare(
        "SELECT wallet_id, target_id \
         FROM wallet_identity_override \
         WHERE override_type = 'merge' AND target_id IS NOT NULL \
         ORDER BY created_at ASC, id ASC",
    )?;
    let overrides: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;

    let mut merges = 0usize;
    for (wallet_id, target_id) in overrides {
        let source_id = resolve_wallet_redirect(conn, &wallet_id)?;
        let canonical_target_id = resolve_wallet_redirect(conn, &target_id)?;
        if source_id == canonical_target_id {
            continue;
        }

        let source_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM wallets WHERE wallet_id = ?1)",
            params![source_id.as_str()],
            |r| r.get(0),
        )?;
        if !source_exists {
            continue;
        }

        let target_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM wallets WHERE wallet_id = ?1)",
            params![canonical_target_id.as_str()],
            |r| r.get(0),
        )?;
        if !target_exists {
            return Err(DbError::Other(format!(
                "wallet merge override target does not exist: {canonical_target_id}"
            )));
        }

        if audited_merge_wallets(conn, &source_id, &canonical_target_id, "override", recorder)? {
            merges += 1;
        }
    }

    Ok(merges)
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WalletUndoStats {
    pub batch_id: i64,
    pub merges_reverted: usize,
}

pub fn undo_last_wallet_merge_batch(conn: &Connection) -> Result<Option<WalletUndoStats>, DbError> {
    let batch = conn
        .query_row(
            "SELECT id FROM wallet_merge_apply_batch \
             WHERE undone_at IS NULL \
             ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    let Some(batch_id) = batch else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT old_wallet_id, new_wallet_id, old_wallet_json, old_endpoint_ids_json, \
                old_artist_links_json, new_artist_ids_json, moved_reviews_json, redirect_rows_json \
         FROM wallet_merge_apply_entry \
         WHERE batch_id = ?1 \
         ORDER BY seq DESC, id DESC",
    )?;
    let entries = stmt.query_map(params![batch_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;

    let mut merges_reverted = 0usize;
    for row in entries {
        let (
            old_wallet_id,
            new_wallet_id,
            old_wallet_json,
            old_endpoint_ids_json,
            old_artist_links_json,
            new_artist_ids_json,
            moved_reviews_json,
            redirect_rows_json,
        ) = row?;

        let old_wallet: WalletUndoWalletRow = serde_json::from_str(&old_wallet_json)?;
        let old_endpoint_ids: Vec<i64> = serde_json::from_str(&old_endpoint_ids_json)?;
        let old_artist_links: Vec<WalletUndoArtistLinkRow> =
            serde_json::from_str(&old_artist_links_json)?;
        let new_artist_ids: std::collections::BTreeSet<String> =
            serde_json::from_str::<Vec<String>>(&new_artist_ids_json)?
                .into_iter()
                .collect();
        let moved_reviews: Vec<WalletUndoReviewRow> = serde_json::from_str(&moved_reviews_json)?;
        let redirect_rows: Vec<WalletUndoRedirectRow> = serde_json::from_str(&redirect_rows_json)?;

        conn.execute(
            "DELETE FROM wallets WHERE wallet_id = ?1",
            params![old_wallet_id.as_str()],
        )?;
        conn.execute(
            "INSERT INTO wallets (wallet_id, display_name, display_name_lower, wallet_class, class_confidence, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                old_wallet.wallet_id,
                old_wallet.display_name,
                old_wallet.display_name_lower,
                old_wallet.wallet_class,
                old_wallet.class_confidence,
                old_wallet.created_at,
                old_wallet.updated_at,
            ],
        )?;

        for endpoint_id in &old_endpoint_ids {
            conn.execute(
                "UPDATE wallet_endpoints SET wallet_id = ?1 WHERE id = ?2",
                params![old_wallet_id.as_str(), endpoint_id],
            )?;
        }

        for review in &moved_reviews {
            conn.execute(
                "DELETE FROM wallet_identity_review WHERE id = ?1",
                params![review.id],
            )?;
            conn.execute(
                "INSERT INTO wallet_identity_review \
                 (id, wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, \
                  status, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    review.id,
                    review.wallet_id,
                    review.source,
                    review
                        .evidence_key
                        .as_deref()
                        .unwrap_or(review.wallet_id.as_str()),
                    serde_json::to_string(&review.wallet_ids)?,
                    serde_json::to_string(&review.endpoint_summary)?,
                    review.status,
                    review.created_at,
                    review.updated_at.unwrap_or(review.created_at),
                ],
            )?;
        }

        conn.execute(
            "DELETE FROM wallet_artist_links WHERE wallet_id = ?1",
            params![old_wallet_id.as_str()],
        )?;
        for link in &old_artist_links {
            if !new_artist_ids.contains(&link.artist_id) {
                conn.execute(
                    "DELETE FROM wallet_artist_links WHERE wallet_id = ?1 AND artist_id = ?2",
                    params![new_wallet_id.as_str(), link.artist_id.as_str()],
                )?;
            }
            conn.execute(
                "INSERT INTO wallet_artist_links \
                 (wallet_id, artist_id, evidence_entity_type, evidence_entity_id, confidence, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    old_wallet_id.as_str(),
                    link.artist_id.as_str(),
                    link.evidence_entity_type.as_str(),
                    link.evidence_entity_id.as_str(),
                    link.confidence.as_str(),
                    link.created_at,
                ],
            )?;
        }

        let affected_redirect_old_ids = redirect_rows
            .iter()
            .map(|row| row.old_wallet_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        conn.execute(
            "DELETE FROM wallet_id_redirect WHERE old_wallet_id = ?1",
            params![old_wallet_id.as_str()],
        )?;
        for redirect_old_id in affected_redirect_old_ids {
            conn.execute(
                "DELETE FROM wallet_id_redirect WHERE old_wallet_id = ?1",
                params![redirect_old_id],
            )?;
        }
        conn.execute(
            "DELETE FROM wallet_id_redirect WHERE new_wallet_id = ?1",
            params![old_wallet_id.as_str()],
        )?;
        for redirect in &redirect_rows {
            conn.execute(
                "INSERT OR REPLACE INTO wallet_id_redirect (old_wallet_id, new_wallet_id, created_at) \
                 VALUES (?1, ?2, ?3)",
                params![
                    redirect.old_wallet_id.as_str(),
                    redirect.new_wallet_id.as_str(),
                    redirect.created_at,
                ],
            )?;
        }

        update_wallet_display_name(conn, old_wallet_id.as_str())?;
        update_wallet_display_name(conn, new_wallet_id.as_str())?;
        merges_reverted += 1;
    }

    conn.execute(
        "UPDATE wallet_merge_apply_batch SET undone_at = ?1 WHERE id = ?2",
        params![unix_now(), batch_id],
    )?;
    Ok(Some(WalletUndoStats {
        batch_id,
        merges_reverted,
    }))
}

/// Create review items for ambiguous wallet identity patterns.
///
/// Generates review items for:
/// - Same `alias_lower` across multiple wallets with different endpoints
/// - Endpoints with conflicting fee/non-fee signals
fn insert_cross_wallet_alias_review(
    conn: &Connection,
    wallet_id: &str,
    alias: &str,
    related_wallet_ids: &[String],
    now: i64,
) -> Result<bool, DbError> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM wallet_identity_review \
         WHERE wallet_id = ?1 AND source = 'cross_wallet_alias' AND evidence_key = ?2)",
        params![wallet_id, alias],
        |r| r.get(0),
    )?;
    if exists {
        return Ok(false);
    }

    conn.execute(
        "INSERT INTO wallet_identity_review \
         (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
         VALUES (?1, 'cross_wallet_alias', ?2, ?3, ?4, 'pending', ?5, ?5)",
        params![
            wallet_id,
            alias,
            serde_json::to_string(related_wallet_ids)?,
            serde_json::to_string(&get_wallet_endpoint_preview(conn, wallet_id, 3)?)?,
            now,
        ],
    )?;
    Ok(true)
}

fn insert_likely_wallet_owner_match_review(
    conn: &Connection,
    wallet_id: &str,
    evidence_key: &str,
    related_wallet_ids: &[String],
    now: i64,
) -> Result<bool, DbError> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM wallet_identity_review \
         WHERE wallet_id = ?1 AND source = 'likely_wallet_owner_match' AND evidence_key = ?2)",
        params![wallet_id, evidence_key],
        |r| r.get(0),
    )?;
    if exists {
        return Ok(false);
    }

    conn.execute(
        "INSERT INTO wallet_identity_review \
         (wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, status, created_at, updated_at) \
         VALUES (?1, 'likely_wallet_owner_match', ?2, ?3, ?4, 'pending', ?5, ?5)",
        params![
            wallet_id,
            evidence_key,
            serde_json::to_string(related_wallet_ids)?,
            serde_json::to_string(&get_wallet_endpoint_preview(conn, wallet_id, 3)?)?,
            now,
        ],
    )?;
    Ok(true)
}

pub fn generate_wallet_review_items(conn: &Connection) -> Result<usize, DbError> {
    let now = unix_now();
    let mut created = 0;

    // Same alias across multiple wallets
    let mut stmt = conn.prepare(
        "SELECT wa.alias_lower, GROUP_CONCAT(DISTINCT we.wallet_id) AS wallet_ids \
         FROM wallet_aliases wa \
         JOIN wallet_endpoints we ON we.id = wa.endpoint_id \
         WHERE we.wallet_id IS NOT NULL \
         GROUP BY wa.alias_lower \
         HAVING COUNT(DISTINCT we.wallet_id) > 1",
    )?;

    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;

    for (alias, wallet_ids_str) in rows {
        let wallet_ids = wallet_ids_str
            .split(',')
            .map(str::trim)
            .filter(|wallet_id| !wallet_id.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        for wallet_id in &wallet_ids {
            if insert_cross_wallet_alias_review(conn, wallet_id, &alias, &wallet_ids, now)? {
                created += 1;
            }
        }
        for artist_id in shared_artist_ids_for_wallets(conn, &wallet_ids)? {
            let evidence_key = format!("{alias}:artist:{artist_id}");
            for wallet_id in &wallet_ids {
                if insert_likely_wallet_owner_match_review(
                    conn,
                    wallet_id,
                    &evidence_key,
                    &wallet_ids,
                    now,
                )? {
                    created += 1;
                }
            }
        }
    }

    let mut stmt = conn.prepare(
        "SELECT wa.alias_lower, prf.feed_guid, GROUP_CONCAT(DISTINCT we.wallet_id) AS wallet_ids \
         FROM wallet_aliases wa \
         JOIN wallet_endpoints we ON we.id = wa.endpoint_id \
         JOIN ( \
             SELECT wtrm.endpoint_id, pr.feed_guid \
             FROM wallet_track_route_map wtrm \
             JOIN payment_routes pr ON pr.id = wtrm.route_id \
             UNION ALL \
             SELECT wfrm.endpoint_id, fpr.feed_guid \
             FROM wallet_feed_route_map wfrm \
             JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
         ) prf ON prf.endpoint_id = we.id \
         WHERE we.wallet_id IS NOT NULL \
         GROUP BY wa.alias_lower, prf.feed_guid \
         HAVING COUNT(DISTINCT we.wallet_id) > 1",
    )?;

    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;

    for (alias, feed_guid, wallet_ids_str) in rows {
        let wallet_ids = wallet_ids_str
            .split(',')
            .map(str::trim)
            .filter(|wallet_id| !wallet_id.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        for wallet_id in &wallet_ids {
            if insert_likely_wallet_owner_match_review(
                conn,
                wallet_id,
                &format!("{alias}:{feed_guid}"),
                &wallet_ids,
                now,
            )? {
                created += 1;
            }
        }
    }

    Ok(created)
}

// ============================================================
// Wallet backfill — multi-pass orchestration
// ============================================================

/// Stats returned by the wallet backfill passes.
#[derive(Debug, Default)]
pub struct WalletBackfillStats {
    pub endpoints_created: usize,
    pub endpoints_existing: usize,
    pub aliases_created: usize,
    pub track_maps_created: usize,
    pub feed_maps_created: usize,
    pub wallets_created: usize,
    pub hard_classified: usize,
    pub artist_links_created: usize,
    pub soft_classified: usize,
    pub split_classified: usize,
    pub review_items_created: usize,
    pub merges_from_grouping: usize,
}

fn link_wallets_to_artists_for_feed(conn: &Connection, feed_guid: &str) -> Result<usize, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT w.wallet_id \
         FROM wallets w \
         JOIN wallet_endpoints we ON we.wallet_id = w.wallet_id \
         LEFT JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id \
         LEFT JOIN payment_routes pr ON pr.id = wtrm.route_id \
         LEFT JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id \
         LEFT JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
         WHERE pr.feed_guid = ?1 OR fpr.feed_guid = ?1",
    )?;
    let wallet_ids: Vec<String> = stmt
        .query_map(params![feed_guid], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    let mut created = 0;
    for wallet_id in wallet_ids {
        if link_wallet_to_artist_if_confident(conn, &wallet_id, feed_guid)? {
            created += 1;
        }
    }

    Ok(created)
}

fn generate_wallet_review_items_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<usize, DbError> {
    let now = unix_now();
    let mut created = 0;

    // Find aliases used by this feed's endpoints, then look globally for any
    // other wallets sharing those aliases. This catches cross-feed collisions
    // where only one side of the pair touched the dirty feed.
    let mut stmt = conn.prepare(
        "SELECT g.alias_lower, GROUP_CONCAT(DISTINCT we2.wallet_id) AS wallet_ids \
         FROM ( \
             SELECT DISTINCT wa.alias_lower \
             FROM wallet_aliases wa \
             JOIN wallet_endpoints we ON we.id = wa.endpoint_id \
             JOIN ( \
                 SELECT wtrm.endpoint_id \
                 FROM wallet_track_route_map wtrm \
                 JOIN payment_routes pr ON pr.id = wtrm.route_id \
                 WHERE pr.feed_guid = ?1 \
                 UNION ALL \
                 SELECT wfrm.endpoint_id \
                 FROM wallet_feed_route_map wfrm \
                 JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
                 WHERE fpr.feed_guid = ?1 \
             ) prf ON prf.endpoint_id = we.id \
             WHERE we.wallet_id IS NOT NULL \
         ) g \
         JOIN wallet_aliases wa2 ON wa2.alias_lower = g.alias_lower \
         JOIN wallet_endpoints we2 ON we2.id = wa2.endpoint_id \
         WHERE we2.wallet_id IS NOT NULL \
         GROUP BY g.alias_lower \
         HAVING COUNT(DISTINCT we2.wallet_id) > 1",
    )?;

    let rows: Vec<(String, String)> = stmt
        .query_map(params![feed_guid], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;

    for (alias, wallet_ids_str) in rows {
        let wallet_ids = wallet_ids_str
            .split(',')
            .map(str::trim)
            .filter(|wid| !wid.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        for wid in &wallet_ids {
            if insert_cross_wallet_alias_review(conn, wid, &alias, &wallet_ids, now)? {
                created += 1;
            }
        }
    }

    Ok(created)
}

fn refresh_wallet_headaches_for_feed(
    conn: &Connection,
    feed_guid: &str,
    stats: &mut WalletBackfillStats,
) -> Result<(), DbError> {
    stats.merges_from_grouping = group_same_feed_endpoints(conn, feed_guid)?;

    let wallet_ids = get_wallet_ids_for_feed(conn, feed_guid)?;
    for wallet_id in &wallet_ids {
        update_wallet_display_name(conn, wallet_id)?;
        if classify_wallet_soft_signals(conn, wallet_id)? {
            stats.soft_classified += 1;
        }
        if classify_wallet_split_heuristics(conn, wallet_id)? {
            stats.split_classified += 1;
        }
    }

    stats.artist_links_created = link_wallets_to_artists_for_feed(conn, feed_guid)?;
    stats.review_items_created = generate_wallet_review_items_for_feed(conn, feed_guid)?;

    Ok(())
}

/// Per-feed wallet resolver: runs incremental wallet passes for one feed.
///
/// Called by the incremental resolver when `DIRTY_WALLET_IDENTITY` is set.
/// Only normalizes endpoint facts and creates provisional wallets with
/// hard-signal classification, then applies same-feed grouping, feed-scoped
/// wallet review generation, and same-feed wallet→artist linking. It does not
/// run the corpus-wide refresh heuristics from `backfill_wallet_pass5`.
pub fn resolve_wallet_identity_for_feed(
    conn: &Connection,
    feed_guid: &str,
) -> Result<WalletBackfillStats, DbError> {
    let mut stats = WalletBackfillStats::default();

    // Wavlake routes are platform infrastructure, not artist wallets — skip entirely.
    let is_wavlake: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM source_platform_claims \
         WHERE feed_guid = ?1 AND platform_key = 'wavlake')",
        params![feed_guid],
        |r| r.get(0),
    )?;
    if is_wavlake {
        return Ok(stats);
    }

    let now = unix_now();

    // Pass 1: track-level routes for this feed
    {
        let mut stmt = conn.prepare(
            "SELECT id, route_type, address, COALESCE(custom_key, ''), COALESCE(custom_value, ''), \
             recipient_name FROM payment_routes WHERE feed_guid = ?1",
        )?;
        let rows: Vec<(i64, String, String, String, String, Option<String>)> = stmt
            .query_map(params![feed_guid], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<Result<_, _>>()?;

        for (route_id, route_type, address, ck, cv, name) in rows {
            let before: i64 =
                conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0))?;
            let ep = get_or_create_endpoint(
                conn,
                &route_type,
                &address,
                &ck,
                &cv,
                name.as_deref(),
                now,
            )?;
            let after: i64 =
                conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0))?;
            if after > before {
                stats.endpoints_created += 1;
            }
            let mapped = conn.execute(
                "INSERT OR IGNORE INTO wallet_track_route_map (route_id, endpoint_id, created_at) \
                 VALUES (?1, ?2, ?3)",
                params![route_id, ep, now],
            )?;
            if mapped > 0 {
                stats.track_maps_created += 1;
            }
        }
    }

    // Pass 1: feed-level routes for this feed
    {
        let mut stmt = conn.prepare(
            "SELECT id, route_type, address, COALESCE(custom_key, ''), COALESCE(custom_value, ''), \
             recipient_name FROM feed_payment_routes WHERE feed_guid = ?1",
        )?;
        let rows: Vec<(i64, String, String, String, String, Option<String>)> = stmt
            .query_map(params![feed_guid], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<Result<_, _>>()?;

        for (route_id, route_type, address, ck, cv, name) in rows {
            let before: i64 =
                conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0))?;
            let ep = get_or_create_endpoint(
                conn,
                &route_type,
                &address,
                &ck,
                &cv,
                name.as_deref(),
                now,
            )?;
            let after: i64 =
                conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0))?;
            if after > before {
                stats.endpoints_created += 1;
            }
            let mapped = conn.execute(
                "INSERT OR IGNORE INTO wallet_feed_route_map (route_id, endpoint_id, created_at) \
                 VALUES (?1, ?2, ?3)",
                params![route_id, ep, now],
            )?;
            if mapped > 0 {
                stats.feed_maps_created += 1;
            }
        }
    }

    // Pass 2: create provisional wallets for any unassigned endpoints we just touched
    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT we.id FROM wallet_endpoints we \
             LEFT JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id \
             LEFT JOIN payment_routes pr ON pr.id = wtrm.route_id \
             LEFT JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id \
             LEFT JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
             WHERE we.wallet_id IS NULL \
               AND (pr.feed_guid = ?1 OR fpr.feed_guid = ?1)",
        )?;
        let ep_ids: Vec<i64> = stmt
            .query_map(params![feed_guid], |r| r.get(0))?
            .collect::<Result<_, _>>()?;

        for ep_id in ep_ids {
            let wid = create_provisional_wallet(conn, ep_id, now)?;
            stats.wallets_created += 1;
            classify_wallet_hard_signals(conn, &wid)?;
        }
    }

    refresh_wallet_headaches_for_feed(conn, feed_guid, &mut stats)?;

    Ok(stats)
}

/// Pass 1: scan all source routes, normalize endpoint facts.
///
/// For each `payment_routes` and `feed_payment_routes` row, calls
/// `get_or_create_endpoint` and creates a route map entry. No wallets
/// are created.
pub fn backfill_wallet_pass1(conn: &Connection) -> Result<WalletBackfillStats, DbError> {
    let mut stats = WalletBackfillStats::default();

    // Track-level routes
    {
        let mut stmt = conn.prepare(
            "SELECT id, route_type, address, COALESCE(custom_key, ''), COALESCE(custom_value, ''), \
             recipient_name FROM payment_routes pr \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM source_platform_claims spc \
                 WHERE spc.feed_guid = pr.feed_guid AND spc.platform_key = 'wavlake' \
             )",
        )?;
        let rows: Vec<(i64, String, String, String, String, Option<String>)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<Result<_, _>>()?;

        for (route_id, route_type, address, ck, cv, name) in rows {
            let before: i64 =
                conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0))?;
            let ep = get_or_create_endpoint(
                conn,
                &route_type,
                &address,
                &ck,
                &cv,
                name.as_deref(),
                unix_now(),
            )?;
            let after: i64 =
                conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0))?;
            if after > before {
                stats.endpoints_created += 1;
            } else {
                stats.endpoints_existing += 1;
            }

            let mapped = conn.execute(
                "INSERT OR IGNORE INTO wallet_track_route_map (route_id, endpoint_id, created_at) \
                 VALUES (?1, ?2, ?3)",
                params![route_id, ep, unix_now()],
            )?;
            if mapped > 0 {
                stats.track_maps_created += 1;
            }
        }
    }

    // Feed-level routes
    {
        let mut stmt = conn.prepare(
            "SELECT id, route_type, address, COALESCE(custom_key, ''), COALESCE(custom_value, ''), \
             recipient_name FROM feed_payment_routes fpr \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM source_platform_claims spc \
                 WHERE spc.feed_guid = fpr.feed_guid AND spc.platform_key = 'wavlake' \
             )",
        )?;
        let rows: Vec<(i64, String, String, String, String, Option<String>)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<Result<_, _>>()?;

        for (route_id, route_type, address, ck, cv, name) in rows {
            let before: i64 =
                conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0))?;
            let ep = get_or_create_endpoint(
                conn,
                &route_type,
                &address,
                &ck,
                &cv,
                name.as_deref(),
                unix_now(),
            )?;
            let after: i64 =
                conn.query_row("SELECT COUNT(*) FROM wallet_endpoints", [], |r| r.get(0))?;
            if after > before {
                stats.endpoints_created += 1;
            } else {
                stats.endpoints_existing += 1;
            }

            let mapped = conn.execute(
                "INSERT OR IGNORE INTO wallet_feed_route_map (route_id, endpoint_id, created_at) \
                 VALUES (?1, ?2, ?3)",
                params![route_id, ep, unix_now()],
            )?;
            if mapped > 0 {
                stats.feed_maps_created += 1;
            }
        }
    }

    stats.aliases_created =
        conn.query_row("SELECT COUNT(*) FROM wallet_aliases", [], |r| r.get(0))?;

    Ok(stats)
}

/// Pass 2: create a provisional wallet for each unassigned endpoint
/// and apply hard-signal classification.
pub fn backfill_wallet_pass2(conn: &Connection) -> Result<WalletBackfillStats, DbError> {
    let mut stats = WalletBackfillStats::default();
    let now = unix_now();

    let mut stmt = conn.prepare("SELECT id FROM wallet_endpoints WHERE wallet_id IS NULL")?;
    let ep_ids: Vec<i64> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    for ep_id in ep_ids {
        let wid = create_provisional_wallet(conn, ep_id, now)?;
        stats.wallets_created += 1;
        classify_wallet_hard_signals(conn, &wid)?;

        let confidence: String = conn.query_row(
            "SELECT class_confidence FROM wallets WHERE wallet_id = ?1",
            params![wid],
            |r| r.get(0),
        )?;
        if confidence != "provisional" {
            stats.hard_classified += 1;
        }
    }

    Ok(stats)
}

/// Pass 3: create wallet→artist links based on same-feed credit evidence.
pub fn backfill_wallet_pass3(conn: &Connection) -> Result<WalletBackfillStats, DbError> {
    let mut stats = WalletBackfillStats::default();

    // For each wallet, find all feeds it appears in (via endpoint → route map → route → feed)
    let mut stmt = conn.prepare(
        "SELECT DISTINCT w.wallet_id, pr.feed_guid \
         FROM wallets w \
         JOIN wallet_endpoints we ON we.wallet_id = w.wallet_id \
         LEFT JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id \
         LEFT JOIN payment_routes pr ON pr.id = wtrm.route_id \
         WHERE pr.feed_guid IS NOT NULL \
         UNION \
         SELECT DISTINCT w.wallet_id, fpr.feed_guid \
         FROM wallets w \
         JOIN wallet_endpoints we ON we.wallet_id = w.wallet_id \
         LEFT JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id \
         LEFT JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
         WHERE fpr.feed_guid IS NOT NULL",
    )?;
    let feed_guids: std::collections::BTreeSet<String> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(_wallet_id, feed_guid)| feed_guid)
        .collect();

    for feed_guid in feed_guids {
        stats.artist_links_created += link_wallets_to_artists_for_feed(conn, &feed_guid)?;
    }

    Ok(stats)
}

/// Stats returned by Pass 5 (--refresh).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WalletRefreshStats {
    pub apply_batch_id: Option<i64>,
    pub feeds_processed: usize,
    pub merges_from_grouping: usize,
    pub merges_from_overrides: usize,
    pub soft_classified: usize,
    pub split_classified: usize,
    pub review_items_created: usize,
    pub orphans_deleted: usize,
}

/// Pass 5: global refresh / owner grouping.
///
/// Run via `backfill_wallets --refresh` after major corpus changes.
///
/// 1. `group_same_feed_endpoints` for each feed
/// 2. Re-derive display names across grouped endpoints
/// 3. Generate review items for ambiguous patterns
/// 4. Orphan cleanup
pub fn backfill_wallet_pass5(conn: &Connection) -> Result<WalletRefreshStats, DbError> {
    let mut stats = WalletRefreshStats::default();
    let mut recorder = WalletMergeBatchRecorder::default();

    stats.merges_from_overrides = apply_wallet_merge_overrides_with_recorder(conn, &mut recorder)?;

    // Get feed GUIDs that have multiple distinct endpoints sharing an alias.
    // Only these feeds can possibly produce merges — skip the rest.
    let mut stmt = conn.prepare(
        "SELECT DISTINCT feed_guid FROM ( \
             SELECT pr.feed_guid, wa.alias_lower, COUNT(DISTINCT we.id) AS ep_count \
             FROM wallet_endpoints we \
             JOIN wallet_aliases wa ON wa.endpoint_id = we.id \
             JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id \
             JOIN payment_routes pr ON pr.id = wtrm.route_id \
             WHERE we.wallet_id IS NOT NULL \
             GROUP BY pr.feed_guid, wa.alias_lower \
             HAVING ep_count > 1 \
             UNION \
             SELECT fpr.feed_guid, wa.alias_lower, COUNT(DISTINCT we.id) AS ep_count \
             FROM wallet_endpoints we \
             JOIN wallet_aliases wa ON wa.endpoint_id = we.id \
             JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id \
             JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
             WHERE we.wallet_id IS NOT NULL \
             GROUP BY fpr.feed_guid, wa.alias_lower \
             HAVING ep_count > 1 \
         )",
    )?;
    let candidate_feeds: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    for feed_guid in &candidate_feeds {
        let merges = group_same_feed_endpoints_with_recorder(conn, feed_guid, &mut recorder)?;
        stats.merges_from_grouping += merges;
        stats.feeds_processed += 1;
    }
    stats.apply_batch_id = recorder.batch_id;

    // Re-derive display names for all wallets that were involved in merges
    if stats.merges_from_grouping > 0 || stats.merges_from_overrides > 0 {
        let mut wstmt = conn.prepare("SELECT wallet_id FROM wallets")?;
        let wallet_ids: Vec<String> = wstmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for wid in wallet_ids {
            update_wallet_display_name(conn, &wid)?;
        }
    }

    // Soft-signal classification (known platform aliases + lnaddress domains)
    {
        let mut wstmt = conn.prepare(
            "SELECT wallet_id FROM wallets \
             WHERE class_confidence = 'provisional' AND wallet_class = 'unknown'",
        )?;
        let provisional_ids: Vec<String> = wstmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for wid in &provisional_ids {
            if classify_wallet_soft_signals(conn, wid)? {
                stats.soft_classified += 1;
            }
        }
    }

    // Split-shape heuristics (only for wallets still unknown/provisional after soft signals)
    {
        let mut wstmt = conn.prepare(
            "SELECT wallet_id FROM wallets \
             WHERE class_confidence = 'provisional' AND wallet_class = 'unknown'",
        )?;
        let still_unknown: Vec<String> = wstmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for wid in &still_unknown {
            if classify_wallet_split_heuristics(conn, wid)? {
                stats.split_classified += 1;
            }
        }
    }

    // Generate review items
    stats.review_items_created = generate_wallet_review_items(conn)?;

    // Orphan cleanup
    let cleanup = cleanup_orphaned_wallets(conn)?;
    stats.orphans_deleted = cleanup.wallets_deleted;

    Ok(stats)
}

// ---------------------------------------------------------------------------
// Wallet identity review helpers
// ---------------------------------------------------------------------------

/// Summary of a pending wallet identity review item.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletReviewSummary {
    pub id: i64,
    pub wallet_id: String,
    pub display_name: String,
    pub wallet_class: String,
    pub class_confidence: String,
    pub source: String,
    pub confidence: String,
    pub explanation: String,
    pub supporting_sources: Vec<String>,
    pub conflict_reasons: Vec<String>,
    pub score: Option<u16>,
    pub score_breakdown: Vec<ReviewScoreComponent>,
    pub evidence_key: String,
    pub wallet_ids: Vec<String>,
    pub endpoint_summary: Vec<WalletEndpointPreview>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WalletPendingReviewSummary {
    pub source: String,
    pub count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletReviewItem {
    pub id: i64,
    pub wallet_id: String,
    pub source: String,
    pub confidence: String,
    pub explanation: String,
    pub supporting_sources: Vec<String>,
    pub conflict_reasons: Vec<String>,
    pub score: Option<u16>,
    pub score_breakdown: Vec<ReviewScoreComponent>,
    pub evidence_key: String,
    pub wallet_ids: Vec<String>,
    pub endpoint_summary: Vec<WalletEndpointPreview>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletIdentityReviewActionOutcome {
    pub review: WalletReviewItem,
}

/// Full detail of a wallet for review display.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletDetail {
    pub wallet_id: String,
    pub display_name: String,
    pub wallet_class: String,
    pub class_confidence: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub endpoints: Vec<WalletEndpointDetail>,
    pub aliases: Vec<WalletAliasDetail>,
    pub artist_links: Vec<WalletArtistLinkDetail>,
    pub feed_guids: Vec<String>,
    pub overrides: Vec<WalletOverrideDetail>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletEndpointDetail {
    pub id: i64,
    pub route_type: String,
    pub normalized_address: String,
    pub custom_key: String,
    pub custom_value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletAliasDetail {
    pub alias: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletArtistLinkDetail {
    pub artist_id: String,
    pub confidence: String,
    pub evidence_entity_type: String,
    pub evidence_entity_id: String,
    pub evidence_explanation: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletOverrideDetail {
    pub id: i64,
    pub override_type: String,
    pub target_id: Option<String>,
    pub value: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletRouteEvidence {
    pub route_scope: String,
    pub route_id: i64,
    pub track_guid: Option<String>,
    pub track_title: Option<String>,
    pub feed_guid: String,
    pub feed_title: String,
    pub feed_url: String,
    pub recipient_name: Option<String>,
    pub route_type: String,
    pub address: String,
    pub custom_key: String,
    pub custom_value: String,
    pub split: i64,
    pub fee: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletClaimFeed {
    pub feed_guid: String,
    pub title: String,
    pub feed_url: String,
    pub routes: Vec<WalletRouteEvidence>,
    pub contributor_claims: Vec<SourceContributorClaim>,
    pub entity_id_claims: Vec<SourceEntityIdClaim>,
    pub link_claims: Vec<SourceEntityLink>,
    pub release_claims: Vec<SourceReleaseClaim>,
    pub platform_claims: Vec<SourcePlatformClaim>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletEndpointPreview {
    pub route_type: String,
    pub normalized_address: String,
    pub custom_key: String,
    pub custom_value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletAliasPeer {
    pub wallet_id: String,
    pub display_name: String,
    pub wallet_class: String,
    pub class_confidence: String,
    pub endpoint_count: i64,
    pub feed_count: i64,
    pub alias_preview: Vec<String>,
    pub endpoint_preview: Vec<WalletEndpointPreview>,
    pub feed_title_preview: Vec<String>,
}

#[must_use]
pub fn wallet_artist_link_explanation(evidence_entity_type: &str) -> &'static str {
    match evidence_entity_type {
        "feed_alias" => "wallet alias exactly matched the feed artist credit on the same feed",
        "feed_dominant_route" => {
            "wallet dominated the non-Wavlake feed route and every routed track while its naming still matched the feed artist"
        }
        _ => "wallet linked to artist through feed-scoped evidence",
    }
}

/// Returns wallet ids that currently touch the given feed through either
/// feed-level or track-level payment routes.
pub fn get_wallet_ids_for_feed(conn: &Connection, feed_guid: &str) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT we.wallet_id
         FROM wallet_endpoints we
         LEFT JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id
         LEFT JOIN payment_routes pr ON pr.id = wtrm.route_id
         LEFT JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id
         LEFT JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id
         WHERE we.wallet_id IS NOT NULL
           AND (pr.feed_guid = ?1 OR fpr.feed_guid = ?1)
         ORDER BY we.wallet_id",
    )?;
    stmt.query_map(params![feed_guid], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Returns wallet ids currently linked to the given artist through
/// `wallet_artist_links`.
pub fn get_wallet_ids_for_artist(
    conn: &Connection,
    artist_id: &str,
) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT wallet_id
          FROM wallet_artist_links
          WHERE artist_id = ?1
          ORDER BY wallet_id",
    )?;
    stmt.query_map(params![artist_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn get_releases_for_artist(
    conn: &Connection,
    artist_id: &str,
) -> Result<Vec<Release>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT r.release_id, r.title, r.title_lower, r.artist_credit_id, r.description, r.image_url, r.release_date, r.created_at, r.updated_at \
         FROM releases r \
         JOIN artist_credit ac ON r.artist_credit_id = ac.id \
         JOIN artist_credit_name acn ON ac.id = acn.artist_credit_id \
         WHERE acn.artist_id = ?1 \
         ORDER BY r.release_date DESC, r.title",
    )?;
    stmt.query_map(params![artist_id], |row| {
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
    })?
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}
fn get_wallet_endpoint_preview(
    conn: &Connection,
    wallet_id: &str,
    limit: usize,
) -> Result<Vec<WalletEndpointPreview>, DbError> {
    let limit = i64::try_from(limit).map_err(|err| {
        DbError::Other(format!("wallet endpoint preview limit exceeds i64: {err}"))
    })?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT route_type, normalized_address, \
                COALESCE(custom_key, ''), COALESCE(custom_value, '') \
         FROM wallet_endpoints \
         WHERE wallet_id = ?1 \
         ORDER BY route_type, normalized_address, custom_key, custom_value \
         LIMIT ?2",
    )?;
    stmt.query_map(params![wallet_id, limit], |row| {
        Ok(WalletEndpointPreview {
            route_type: row.get(0)?,
            normalized_address: row.get(1)?,
            custom_key: row.get(2)?,
            custom_value: row.get(3)?,
        })
    })?
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

/// List pending wallet identity reviews.
pub fn list_pending_wallet_reviews(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<WalletReviewSummary>, DbError> {
    list_pending_wallet_reviews_with_max_created_at(conn, None, limit)
}

fn list_pending_wallet_reviews_with_max_created_at(
    conn: &Connection,
    max_created_at: Option<i64>,
    limit: usize,
) -> Result<Vec<WalletReviewSummary>, DbError> {
    let limit = i64::try_from(limit)
        .map_err(|err| DbError::Other(format!("wallet review limit exceeds i64: {err}")))?;
    let mut stmt = conn.prepare(
        "SELECT r.id, r.wallet_id, w.display_name, w.wallet_class, w.class_confidence, \
                r.source, r.evidence_key, r.wallet_ids_json, r.endpoint_summary_json, \
                r.created_at \
         FROM wallet_identity_review r \
         JOIN wallets w ON w.wallet_id = r.wallet_id \
         WHERE r.status = 'pending' \
           AND (?1 IS NULL OR r.created_at <= ?1) \
         ORDER BY r.created_at DESC \
         LIMIT ?2",
    )?;
    let mut rows = stmt.query(params![max_created_at, limit])?;
    let mut summaries = Vec::new();
    while let Some(row) = rows.next()? {
        let source: String = row.get(5)?;
        let wallet_ids_json: String = row.get(7)?;
        let endpoint_summary_json: String = row.get(8)?;
        let wallet_ids: Vec<String> = serde_json::from_str(&wallet_ids_json)?;
        let supporting_sources = wallet_review_supporting_sources(conn, &source, &wallet_ids)?;
        let conflict_reasons = wallet_review_conflict_reasons(conn, &source, &wallet_ids)?;
        let score_breakdown = wallet_review_score_breakdown(&source, &supporting_sources);
        let score = review_score_from_breakdown(&score_breakdown);
        summaries.push(WalletReviewSummary {
            id: row.get(0)?,
            wallet_id: row.get(1)?,
            display_name: row.get(2)?,
            wallet_class: row.get(3)?,
            class_confidence: row.get(4)?,
            source: source.clone(),
            confidence: wallet_review_confidence(&source, score, &conflict_reasons).to_string(),
            explanation: wallet_review_explanation(&source, &conflict_reasons).to_string(),
            conflict_reasons,
            score,
            score_breakdown,
            supporting_sources,
            evidence_key: row.get(6)?,
            wallet_ids,
            endpoint_summary: serde_json::from_str(&endpoint_summary_json)?,
            created_at: row.get(9)?,
        });
    }
    summaries.sort_by(|left, right| {
        review_confidence_priority(&left.confidence)
            .cmp(&review_confidence_priority(&right.confidence))
            .then_with(|| {
                review_score_priority(left.score).cmp(&review_score_priority(right.score))
            })
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(summaries)
}

/// Returns one pending-style wallet review summary row by `id`.
///
/// # Errors
///
/// Returns [`DbError`] if the review row cannot be loaded.
pub fn get_wallet_review_summary(
    conn: &Connection,
    review_id: i64,
) -> Result<Option<WalletReviewSummary>, DbError> {
    conn.query_row(
        "SELECT r.id, r.wallet_id, w.display_name, w.wallet_class, w.class_confidence, \
                r.source, r.evidence_key, r.wallet_ids_json, r.endpoint_summary_json, \
                r.created_at \
         FROM wallet_identity_review r \
         JOIN wallets w ON w.wallet_id = r.wallet_id \
         WHERE r.id = ?1",
        params![review_id],
        |row| {
            let source: String = row.get(5)?;
            let wallet_ids_json: String = row.get(7)?;
            let endpoint_summary_json: String = row.get(8)?;
            let wallet_ids: Vec<String> =
                serde_json::from_str(&wallet_ids_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
            let supporting_sources = wallet_review_supporting_sources(conn, &source, &wallet_ids)
                .map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::other(err.to_string())),
                )
            })?;
            let conflict_reasons = wallet_review_conflict_reasons(conn, &source, &wallet_ids)
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::other(err.to_string())),
                    )
                })?;
            let score_breakdown = wallet_review_score_breakdown(&source, &supporting_sources);
            let score = review_score_from_breakdown(&score_breakdown);
            Ok(WalletReviewSummary {
                id: row.get(0)?,
                wallet_id: row.get(1)?,
                display_name: row.get(2)?,
                wallet_class: row.get(3)?,
                class_confidence: row.get(4)?,
                source: source.clone(),
                confidence: wallet_review_confidence(&source, score, &conflict_reasons).to_string(),
                explanation: wallet_review_explanation(&source, &conflict_reasons).to_string(),
                conflict_reasons,
                score,
                score_breakdown,
                supporting_sources,
                evidence_key: row.get(6)?,
                wallet_ids,
                endpoint_summary: serde_json::from_str(&endpoint_summary_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        8,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
                created_at: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Lists pending wallet-identity reviews older than `min_age_secs`.
///
/// # Errors
///
/// Returns [`DbError`] if the pending review rows cannot be loaded.
pub fn list_stale_pending_wallet_reviews(
    conn: &Connection,
    min_age_secs: i64,
    limit: usize,
) -> Result<Vec<WalletReviewSummary>, DbError> {
    list_pending_wallet_reviews_with_max_created_at(conn, Some(unix_now() - min_age_secs), limit)
}

/// Lists pending wallet-identity reviews newer than `max_age_secs`.
///
/// # Errors
///
/// Returns [`DbError`] if the pending review rows cannot be loaded.
pub fn list_recent_pending_wallet_reviews(
    conn: &Connection,
    max_age_secs: i64,
    limit: usize,
) -> Result<Vec<WalletReviewSummary>, DbError> {
    let cutoff = unix_now() - max_age_secs;
    let limit = i64::try_from(limit)
        .map_err(|err| DbError::Other(format!("wallet review limit exceeds i64: {err}")))?;
    let mut stmt = conn.prepare(
        "SELECT r.id, r.wallet_id, w.display_name, w.wallet_class, w.class_confidence, \
                r.source, r.evidence_key, r.wallet_ids_json, r.endpoint_summary_json, \
                r.created_at \
         FROM wallet_identity_review r \
         JOIN wallets w ON w.wallet_id = r.wallet_id \
         WHERE r.status = 'pending' \
           AND r.created_at >= ?1 \
         ORDER BY r.created_at DESC, r.id DESC \
         LIMIT ?2",
    )?;
    let mut rows = stmt.query(params![cutoff, limit])?;
    let mut summaries = Vec::new();
    while let Some(row) = rows.next()? {
        let source: String = row.get(5)?;
        let wallet_ids_json: String = row.get(7)?;
        let endpoint_summary_json: String = row.get(8)?;
        let wallet_ids: Vec<String> = serde_json::from_str(&wallet_ids_json)?;
        let supporting_sources = wallet_review_supporting_sources(conn, &source, &wallet_ids)?;
        let conflict_reasons = wallet_review_conflict_reasons(conn, &source, &wallet_ids)?;
        let score_breakdown = wallet_review_score_breakdown(&source, &supporting_sources);
        let score = review_score_from_breakdown(&score_breakdown);
        summaries.push(WalletReviewSummary {
            id: row.get(0)?,
            wallet_id: row.get(1)?,
            display_name: row.get(2)?,
            wallet_class: row.get(3)?,
            class_confidence: row.get(4)?,
            source: source.clone(),
            confidence: wallet_review_confidence(&source, score, &conflict_reasons).to_string(),
            explanation: wallet_review_explanation(&source, &conflict_reasons).to_string(),
            conflict_reasons,
            score,
            score_breakdown,
            supporting_sources,
            evidence_key: row.get(6)?,
            wallet_ids,
            endpoint_summary: serde_json::from_str(&endpoint_summary_json)?,
            created_at: row.get(9)?,
        });
    }
    Ok(summaries)
}

/// Returns pending wallet-identity review counts grouped by `source`.
///
/// # Errors
///
/// Returns [`DbError`] if the grouped rows cannot be loaded.
pub fn summarize_pending_wallet_reviews(
    conn: &Connection,
) -> Result<Vec<WalletPendingReviewSummary>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT source, COUNT(*)
         FROM wallet_identity_review
         WHERE status = 'pending'
         GROUP BY source
         ORDER BY COUNT(*) DESC, source ASC",
    )?;
    stmt.query_map([], |row| {
        let count_i64: i64 = row.get(1)?;
        Ok(WalletPendingReviewSummary {
            source: row.get(0)?,
            count: usize::try_from(count_i64).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Integer,
                    Box::new(err),
                )
            })?,
        })
    })?
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

/// Returns pending wallet-identity review counts grouped by derived
/// `confidence`.
///
/// # Errors
///
/// Returns [`DbError`] if the grouped rows cannot be loaded.
pub fn summarize_pending_wallet_review_confidence(
    conn: &Connection,
) -> Result<Vec<PendingReviewConfidenceSummary>, DbError> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let max_limit = usize::try_from(i64::MAX)
        .map_err(|err| DbError::Other(format!("wallet review limit exceeds usize: {err}")))?;
    for summary in list_pending_wallet_reviews(conn, max_limit)? {
        *counts.entry(summary.confidence).or_default() += 1;
    }
    let mut summary = counts
        .into_iter()
        .map(|(confidence, count)| PendingReviewConfidenceSummary { confidence, count })
        .collect::<Vec<_>>();
    summary.sort_by(|left, right| {
        review_confidence_priority(&left.confidence)
            .cmp(&review_confidence_priority(&right.confidence))
            .then_with(|| right.count.cmp(&left.count))
            .then_with(|| left.confidence.cmp(&right.confidence))
    });
    Ok(summary)
}

/// Returns pending wallet-identity review counts grouped by derived score band.
///
/// # Errors
///
/// Returns [`DbError`] if the pending review rows cannot be loaded.
pub fn summarize_pending_wallet_review_scores(
    conn: &Connection,
) -> Result<Vec<PendingReviewScoreSummary>, DbError> {
    let max_limit = usize::try_from(i64::MAX)
        .map_err(|err| DbError::Other(format!("wallet review limit exceeds usize: {err}")))?;
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for review in list_pending_wallet_reviews(conn, max_limit)? {
        *counts
            .entry(review_score_band(review.score).to_string())
            .or_default() += 1;
    }
    let mut summary = counts
        .into_iter()
        .map(|(score_band, count)| PendingReviewScoreSummary { score_band, count })
        .collect::<Vec<_>>();
    summary.sort_by(|left, right| {
        review_score_band_priority(&left.score_band)
            .cmp(&review_score_band_priority(&right.score_band))
            .then_with(|| right.count.cmp(&left.count))
            .then_with(|| left.score_band.cmp(&right.score_band))
    });
    Ok(summary)
}

/// Returns pending wallet review counts grouped by derived conflict reason.
///
/// # Errors
///
/// Returns [`DbError`] if the pending review rows cannot be loaded.
pub fn summarize_pending_wallet_review_conflicts(
    conn: &Connection,
) -> Result<Vec<PendingReviewConflictSummary>, DbError> {
    let max_limit = usize::try_from(i64::MAX)
        .map_err(|err| DbError::Other(format!("pending review limit exceeds usize: {err}")))?;
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for review in list_pending_wallet_reviews(conn, max_limit)? {
        for reason in review.conflict_reasons {
            *counts.entry(reason).or_default() += 1;
        }
    }
    let mut summary = counts
        .into_iter()
        .map(|(reason, count)| PendingReviewConflictSummary { reason, count })
        .collect::<Vec<_>>();
    summary.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.reason.cmp(&right.reason))
    });
    Ok(summary)
}

/// Returns age buckets for pending wallet-identity reviews.
///
/// # Errors
///
/// Returns [`DbError`] if the aggregate query fails.
pub fn summarize_pending_wallet_review_age(
    conn: &Connection,
) -> Result<PendingReviewAgeSummary, DbError> {
    let now = unix_now();
    conn.query_row(
        "SELECT
             COUNT(*),
             SUM(CASE WHEN created_at >= ?1 THEN 1 ELSE 0 END),
             SUM(CASE WHEN created_at < ?2 THEN 1 ELSE 0 END),
             MIN(created_at)
         FROM wallet_identity_review
         WHERE status = 'pending'",
        params![now - 24 * 60 * 60, now - 7 * 24 * 60 * 60],
        |row| {
            let total: i64 = row.get(0)?;
            let created_last_24h: i64 = row.get(1)?;
            let older_than_7d: i64 = row.get(2)?;
            Ok(PendingReviewAgeSummary {
                total: usize::try_from(total).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Integer,
                        Box::new(err),
                    )
                })?,
                created_last_24h: usize::try_from(created_last_24h).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Integer,
                        Box::new(err),
                    )
                })?,
                older_than_7d: usize::try_from(older_than_7d).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Integer,
                        Box::new(err),
                    )
                })?,
                oldest_created_at: row.get(3)?,
            })
        },
    )
    .map_err(Into::into)
}

/// List all wallet identity review rows for one wallet.
pub fn list_wallet_reviews_for_wallet(
    conn: &Connection,
    wallet_id: &str,
) -> Result<Vec<WalletReviewItem>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, wallet_id, source, evidence_key, wallet_ids_json, endpoint_summary_json, \
                status, created_at, updated_at
         FROM wallet_identity_review
         WHERE wallet_id = ?1
         ORDER BY created_at DESC, id DESC",
    )?;
    let mut rows = stmt.query(params![wallet_id])?;
    let mut reviews = Vec::new();
    while let Some(row) = rows.next()? {
        let source: String = row.get(2)?;
        let wallet_ids_json: String = row.get(4)?;
        let endpoint_summary_json: String = row.get(5)?;
        let wallet_ids: Vec<String> = serde_json::from_str(&wallet_ids_json)?;
        let supporting_sources = wallet_review_supporting_sources(conn, &source, &wallet_ids)?;
        let conflict_reasons = wallet_review_conflict_reasons(conn, &source, &wallet_ids)?;
        let score_breakdown = wallet_review_score_breakdown(&source, &supporting_sources);
        let score = review_score_from_breakdown(&score_breakdown);
        reviews.push(WalletReviewItem {
            id: row.get(0)?,
            wallet_id: row.get(1)?,
            source: source.clone(),
            confidence: wallet_review_confidence(&source, score, &conflict_reasons).to_string(),
            explanation: wallet_review_explanation(&source, &conflict_reasons).to_string(),
            conflict_reasons,
            score,
            score_breakdown,
            supporting_sources,
            evidence_key: row.get(3)?,
            wallet_ids,
            endpoint_summary: serde_json::from_str(&endpoint_summary_json)?,
            status: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        });
    }
    Ok(reviews)
}

/// Get full wallet detail for review display.
pub fn get_wallet_detail(
    conn: &Connection,
    wallet_id: &str,
) -> Result<Option<WalletDetail>, DbError> {
    let row = conn
        .query_row(
            "SELECT wallet_id, display_name, wallet_class, class_confidence, created_at, updated_at \
             FROM wallets WHERE wallet_id = ?1",
            params![wallet_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()?;

    let Some((wid, display_name, wallet_class, class_confidence, created_at, updated_at)) = row
    else {
        return Ok(None);
    };

    let mut ep_stmt = conn.prepare(
        "SELECT id, route_type, normalized_address, custom_key, custom_value \
         FROM wallet_endpoints WHERE wallet_id = ?1 ORDER BY id",
    )?;
    let endpoints: Vec<WalletEndpointDetail> = ep_stmt
        .query_map(params![wid], |r| {
            Ok(WalletEndpointDetail {
                id: r.get(0)?,
                route_type: r.get(1)?,
                normalized_address: r.get(2)?,
                custom_key: r.get(3)?,
                custom_value: r.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let mut alias_stmt = conn.prepare(
        "SELECT wa.alias, wa.first_seen_at, wa.last_seen_at \
         FROM wallet_aliases wa \
         JOIN wallet_endpoints we ON we.id = wa.endpoint_id \
         WHERE we.wallet_id = ?1 \
         ORDER BY wa.first_seen_at ASC, wa.alias_lower ASC",
    )?;
    let aliases: Vec<WalletAliasDetail> = alias_stmt
        .query_map(params![wid], |r| {
            Ok(WalletAliasDetail {
                alias: r.get(0)?,
                first_seen_at: r.get(1)?,
                last_seen_at: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let mut link_stmt = conn.prepare(
        "SELECT artist_id, confidence, evidence_entity_type, evidence_entity_id \
         FROM wallet_artist_links WHERE wallet_id = ?1 ORDER BY artist_id",
    )?;
    let artist_links: Vec<WalletArtistLinkDetail> = link_stmt
        .query_map(params![wid], |r| {
            let evidence_entity_type: String = r.get(2)?;
            Ok(WalletArtistLinkDetail {
                artist_id: r.get(0)?,
                confidence: r.get(1)?,
                evidence_entity_type: evidence_entity_type.clone(),
                evidence_entity_id: r.get(3)?,
                evidence_explanation: wallet_artist_link_explanation(&evidence_entity_type)
                    .to_string(),
            })
        })?
        .collect::<Result<_, _>>()?;

    let mut feed_stmt = conn.prepare(
        "SELECT DISTINCT fg FROM ( \
             SELECT pr.feed_guid AS fg FROM wallet_endpoints we \
             JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id \
             JOIN payment_routes pr ON pr.id = wtrm.route_id \
             WHERE we.wallet_id = ?1 \
             UNION \
             SELECT fpr.feed_guid AS fg FROM wallet_endpoints we \
             JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id \
             JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
             WHERE we.wallet_id = ?1 \
         ) ORDER BY fg",
    )?;
    let feed_guids: Vec<String> = feed_stmt
        .query_map(params![wid], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    let mut override_stmt = conn.prepare(
        "SELECT id, override_type, target_id, value, created_at \
         FROM wallet_identity_override WHERE wallet_id = ?1 ORDER BY created_at DESC",
    )?;
    let overrides: Vec<WalletOverrideDetail> = override_stmt
        .query_map(params![wid], |r| {
            Ok(WalletOverrideDetail {
                id: r.get(0)?,
                override_type: r.get(1)?,
                target_id: r.get(2)?,
                value: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    Ok(Some(WalletDetail {
        wallet_id: wid,
        display_name,
        wallet_class,
        class_confidence,
        created_at,
        updated_at,
        endpoints,
        aliases,
        artist_links,
        feed_guids,
        overrides,
    }))
}

/// Returns all route rows and staged source claims for feeds touched by one wallet.
pub fn get_wallet_claim_feeds(
    conn: &Connection,
    wallet_id: &str,
) -> Result<Vec<WalletClaimFeed>, DbError> {
    let mut route_stmt = conn.prepare(
        "SELECT route_scope, route_id, track_guid, track_title, feed_guid, feed_title, feed_url, recipient_name, \
                route_type, address, custom_key, custom_value, split, fee \
         FROM ( \
             SELECT 'track' AS route_scope, pr.id AS route_id, pr.track_guid AS track_guid, \
                    t.title AS track_title, \
                    pr.feed_guid AS feed_guid, f.title AS feed_title, f.feed_url AS feed_url, \
                    pr.recipient_name AS recipient_name, pr.route_type AS route_type, \
                    pr.address AS address, COALESCE(pr.custom_key, '') AS custom_key, \
                    COALESCE(pr.custom_value, '') AS custom_value, pr.split AS split, \
                    pr.fee AS fee \
             FROM wallet_endpoints we \
             JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id \
             JOIN payment_routes pr ON pr.id = wtrm.route_id \
             LEFT JOIN tracks t ON t.track_guid = pr.track_guid \
             JOIN feeds f ON f.feed_guid = pr.feed_guid \
             WHERE we.wallet_id = ?1 \
             UNION ALL \
             SELECT 'feed' AS route_scope, fpr.id AS route_id, NULL AS track_guid, NULL AS track_title, \
                    fpr.feed_guid AS feed_guid, f.title AS feed_title, f.feed_url AS feed_url, \
                    fpr.recipient_name AS recipient_name, fpr.route_type AS route_type, \
                    fpr.address AS address, COALESCE(fpr.custom_key, '') AS custom_key, \
                    COALESCE(fpr.custom_value, '') AS custom_value, fpr.split AS split, \
                    fpr.fee AS fee \
             FROM wallet_endpoints we \
             JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id \
             JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
             JOIN feeds f ON f.feed_guid = fpr.feed_guid \
             WHERE we.wallet_id = ?1 \
         ) \
         ORDER BY feed_title COLLATE NOCASE, route_scope, route_id",
    )?;
    let route_rows = route_stmt.query_map(params![wallet_id], |row| {
        Ok(WalletRouteEvidence {
            route_scope: row.get(0)?,
            route_id: row.get(1)?,
            track_guid: row.get(2)?,
            track_title: row.get(3)?,
            feed_guid: row.get(4)?,
            feed_title: row.get(5)?,
            feed_url: row.get(6)?,
            recipient_name: row.get(7)?,
            route_type: row.get(8)?,
            address: row.get(9)?,
            custom_key: row.get(10)?,
            custom_value: row.get(11)?,
            split: row.get(12)?,
            fee: row.get(13)?,
        })
    })?;

    let mut all_routes = Vec::new();
    for row in route_rows {
        all_routes.push(row?);
    }

    let mut feed_stmt = conn.prepare(
        "SELECT DISTINCT fg, title, feed_url FROM ( \
             SELECT pr.feed_guid AS fg, f.title AS title, f.feed_url AS feed_url \
             FROM wallet_endpoints we \
             JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id \
             JOIN payment_routes pr ON pr.id = wtrm.route_id \
             JOIN feeds f ON f.feed_guid = pr.feed_guid \
             WHERE we.wallet_id = ?1 \
             UNION \
             SELECT fpr.feed_guid AS fg, f.title AS title, f.feed_url AS feed_url \
             FROM wallet_endpoints we \
             JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id \
             JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
             JOIN feeds f ON f.feed_guid = fpr.feed_guid \
             WHERE we.wallet_id = ?1 \
         ) \
         ORDER BY title COLLATE NOCASE, fg",
    )?;
    let feed_rows = feed_stmt.query_map(params![wallet_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut claim_feeds = Vec::new();
    for row in feed_rows {
        let (feed_guid, title, feed_url) = row?;
        let routes = all_routes
            .iter()
            .filter(|route| route.feed_guid == feed_guid)
            .cloned()
            .collect();
        claim_feeds.push(WalletClaimFeed {
            feed_guid: feed_guid.clone(),
            title,
            feed_url,
            routes,
            contributor_claims: get_source_contributor_claims_for_feed(conn, &feed_guid)?,
            entity_id_claims: get_source_entity_ids_for_feed(conn, &feed_guid)?,
            link_claims: get_source_entity_links_for_feed(conn, &feed_guid)?,
            release_claims: get_source_release_claims_for_feed(conn, &feed_guid)?,
            platform_claims: get_source_platform_claims_for_feed(conn, &feed_guid)?,
        });
    }

    Ok(claim_feeds)
}

/// Returns other wallets that currently share one normalized alias.
pub fn get_wallet_alias_peers(
    conn: &Connection,
    alias_lower: &str,
) -> Result<Vec<WalletAliasPeer>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT w.wallet_id, w.display_name, w.wallet_class, w.class_confidence, \
                (SELECT COUNT(*) FROM wallet_endpoints we2 WHERE we2.wallet_id = w.wallet_id), \
                (SELECT COUNT(DISTINCT fg) FROM ( \
                    SELECT pr.feed_guid AS fg \
                    FROM wallet_endpoints we3 \
                    JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we3.id \
                    JOIN payment_routes pr ON pr.id = wtrm.route_id \
                    WHERE we3.wallet_id = w.wallet_id \
                    UNION \
                    SELECT fpr.feed_guid AS fg \
                    FROM wallet_endpoints we4 \
                    JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we4.id \
                    JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
                    WHERE we4.wallet_id = w.wallet_id \
                )) \
         FROM wallet_aliases wa \
         JOIN wallet_endpoints we ON we.id = wa.endpoint_id \
         JOIN wallets w ON w.wallet_id = we.wallet_id \
         WHERE wa.alias_lower = ?1 \
         ORDER BY w.display_name_lower, w.wallet_id",
    )?;
    let rows = stmt.query_map(params![alias_lower], |row| {
        Ok(WalletAliasPeer {
            wallet_id: row.get(0)?,
            display_name: row.get(1)?,
            wallet_class: row.get(2)?,
            class_confidence: row.get(3)?,
            endpoint_count: row.get(4)?,
            feed_count: row.get(5)?,
            alias_preview: Vec::new(),
            endpoint_preview: Vec::new(),
            feed_title_preview: Vec::new(),
        })
    })?;

    let mut peers = Vec::new();
    for row in rows {
        let mut peer = row?;
        let mut alias_stmt = conn.prepare(
            "SELECT DISTINCT wa.alias \
             FROM wallet_aliases wa \
             JOIN wallet_endpoints we ON we.id = wa.endpoint_id \
             WHERE we.wallet_id = ?1 \
             ORDER BY wa.first_seen_at ASC, wa.alias_lower ASC \
             LIMIT 3",
        )?;
        peer.alias_preview = alias_stmt
            .query_map(params![peer.wallet_id.as_str()], |alias_row| {
                alias_row.get(0)
            })?
            .collect::<Result<Vec<String>, _>>()?;

        let mut endpoint_stmt = conn.prepare(
            "SELECT DISTINCT we.route_type, we.normalized_address, \
                    COALESCE(we.custom_key, ''), COALESCE(we.custom_value, '') \
             FROM wallet_endpoints we \
             WHERE we.wallet_id = ?1 \
             ORDER BY we.route_type, we.normalized_address, we.custom_key, we.custom_value \
             LIMIT 3",
        )?;
        peer.endpoint_preview = endpoint_stmt
            .query_map(params![peer.wallet_id.as_str()], |endpoint_row| {
                Ok(WalletEndpointPreview {
                    route_type: endpoint_row.get(0)?,
                    normalized_address: endpoint_row.get(1)?,
                    custom_key: endpoint_row.get(2)?,
                    custom_value: endpoint_row.get(3)?,
                })
            })?
            .collect::<Result<Vec<WalletEndpointPreview>, _>>()?;

        let mut feed_title_stmt = conn.prepare(
            "SELECT DISTINCT title FROM ( \
                 SELECT f.title AS title \
                 FROM wallet_endpoints we \
                 JOIN wallet_track_route_map wtrm ON wtrm.endpoint_id = we.id \
                 JOIN payment_routes pr ON pr.id = wtrm.route_id \
                 JOIN feeds f ON f.feed_guid = pr.feed_guid \
                 WHERE we.wallet_id = ?1 \
                 UNION \
                 SELECT f.title AS title \
                 FROM wallet_endpoints we \
                 JOIN wallet_feed_route_map wfrm ON wfrm.endpoint_id = we.id \
                 JOIN feed_payment_routes fpr ON fpr.id = wfrm.route_id \
                 JOIN feeds f ON f.feed_guid = fpr.feed_guid \
                 WHERE we.wallet_id = ?1 \
             ) \
             ORDER BY title COLLATE NOCASE \
             LIMIT 3",
        )?;
        peer.feed_title_preview = feed_title_stmt
            .query_map(params![peer.wallet_id.as_str()], |feed_row| feed_row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        peers.push(peer);
    }
    Ok(peers)
}

/// Store a wallet identity override and resolve the associated review.
pub fn set_wallet_identity_override_for_review(
    conn: &Connection,
    review_id: i64,
    override_type: &str,
    target_id: Option<&str>,
    value: Option<&str>,
) -> Result<(), DbError> {
    let now = unix_now();
    let review_status = match override_type {
        "merge" => "merged",
        "do_not_merge" | "block_artist_link" => "blocked",
        "force_class" | "force_artist_link" => "resolved",
        other => {
            return Err(DbError::Other(format!(
                "unsupported wallet identity override type: {other}"
            )));
        }
    };

    // Look up the review to get the wallet_id
    let wallet_id: String = conn
        .query_row(
            "SELECT wallet_id FROM wallet_identity_review WHERE id = ?1",
            params![review_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| DbError::Other(format!("wallet identity review not found: {review_id}")))?;

    // Insert the override
    conn.execute(
        "INSERT INTO wallet_identity_override (override_type, wallet_id, target_id, value, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![override_type, wallet_id, target_id, value, now],
    )?;

    // Resolve the review
    conn.execute(
        "UPDATE wallet_identity_review SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![review_status, now, review_id],
    )?;

    Ok(())
}

/// Applies one durable action to a wallet-identity review item.
///
/// Supported actions:
///
/// - `merge` requires `target_id`
/// - `do_not_merge` requires neither `target_id` nor `value`
/// - `force_class` requires `value`
/// - `force_artist_link` requires `target_id`
/// - `block_artist_link` requires `target_id`
///
/// # Errors
///
/// Returns [`DbError`] if the review item does not exist or the action payload
/// is invalid for the requested action.
pub fn apply_wallet_identity_review_action(
    conn: &Connection,
    review_id: i64,
    action: &str,
    target_id: Option<&str>,
    value: Option<&str>,
) -> Result<WalletIdentityReviewActionOutcome, DbError> {
    let wallet_id: String = conn
        .query_row(
            "SELECT wallet_id FROM wallet_identity_review WHERE id = ?1",
            params![review_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| DbError::Other(format!("wallet identity review not found: {review_id}")))?;

    match action {
        "merge" => {
            if target_id.is_none() || value.is_some() {
                return Err(DbError::Other(
                    "wallet identity merge action requires target_id and does not accept value"
                        .into(),
                ));
            }
        }
        "do_not_merge" => {
            if target_id.is_some() || value.is_some() {
                return Err(DbError::Other(
                    "wallet identity do_not_merge action does not accept target_id or value".into(),
                ));
            }
        }
        "force_class" => {
            if target_id.is_some() || value.is_none() {
                return Err(DbError::Other(
                    "wallet identity force_class action requires value and does not accept target_id"
                        .into(),
                ));
            }
        }
        "force_artist_link" | "block_artist_link" => {
            if target_id.is_none() || value.is_some() {
                return Err(DbError::Other(format!(
                    "wallet identity {action} action requires target_id and does not accept value"
                )));
            }
        }
        other => {
            return Err(DbError::Other(format!(
                "unsupported wallet identity review action: {other}"
            )));
        }
    }

    set_wallet_identity_override_for_review(conn, review_id, action, target_id, value)?;

    let review = list_wallet_reviews_for_wallet(conn, &wallet_id)?
        .into_iter()
        .find(|review| review.id == review_id)
        .ok_or_else(|| DbError::Other(format!("wallet identity review not found: {review_id}")))?;

    Ok(WalletIdentityReviewActionOutcome { review })
}
