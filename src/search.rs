//! Full-text search over the contentless FTS5 `search_index` table.
//!
//! The FTS5 table uses `content=''`, which means it is a contentless index:
//! data is stored for MATCH queries but cannot be read back with SELECT.
//! We manage rowids ourselves using a deterministic hash of
//! `entity_type + entity_id`.

use std::borrow::Cow;
use std::hash::Hasher;

use rusqlite::{Connection, params};
use siphasher::sip::SipHasher24;

use crate::db::DbError;

/// Maximum byte length for text fields inserted into the FTS5 index.
/// Fields exceeding this limit are truncated to prevent disproportionately
/// large index entries from a single feed submission.
const MAX_FTS_FIELD_BYTES: usize = 10_000;

/// Truncates a string to at most `MAX_FTS_FIELD_BYTES` bytes on a valid
/// UTF-8 char boundary.
fn truncate_fts_field(s: &str) -> Cow<'_, str> {
    if s.len() <= MAX_FTS_FIELD_BYTES {
        Cow::Borrowed(s)
    } else {
        // Find the last char boundary at or before the limit.
        let mut end = MAX_FTS_FIELD_BYTES;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        Cow::Owned(s[..end].to_string())
    }
}

/// A single search result returned by [`search`].
// CRIT-03 Debug derive — 2026-03-13
#[derive(Debug)]
pub struct SearchResult {
    pub entity_type:   String,
    pub entity_id:     String,
    pub rank:          f64,
    pub quality_score: i64,
}

// SP-01 stable FTS5 hash — 2026-03-13
/// Computes a deterministic positive `i64` rowid from entity type and id.
///
/// FTS5 with `content=''` requires us to manage rowids ourselves so that
/// updates and deletes can target the correct row.
///
/// Uses `SipHash-2-4` with fixed zero keys and raw byte input (not the `Hash`
/// trait) so the result is stable across Rust toolchain versions.
#[must_use]
pub fn rowid_for(entity_type: &str, entity_id: &str) -> i64 {
    let mut hasher = SipHasher24::new_with_keys(0x0, 0x0);
    // Hash the raw bytes with a NUL separator — avoids Hash-trait
    // instability and prevents prefix collisions ("ab","c" vs "a","bc").
    let combined = format!("{entity_type}\0{entity_id}");
    hasher.write(combined.as_bytes());
    // Mask to 63 bits so the result is always a positive i64.
    (hasher.finish() & 0x7FFF_FFFF_FFFF_FFFF).cast_signed()
}

/// Inserts or replaces a row in the `search_index` FTS5 table and its
/// companion `search_entities` lookup table.
///
/// Because the FTS5 table is contentless we first delete any existing row for
/// this entity (by rowid) and then insert a fresh one. The companion table
/// `search_entities` is maintained in lockstep so that search results can be
/// resolved back to `(entity_type, entity_id)` via a JOIN on rowid.
///
/// # Errors
///
/// Returns [`DbError`] if the FTS5 insert or companion-table upsert fails.
// Issue-FTS5-CONTENT — 2026-03-14
pub fn populate_search_index(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    name: &str,
    title: &str,
    description: &str,
    tags: &str,
) -> Result<(), DbError> {
    let rowid = rowid_for(entity_type, entity_id);

    // Truncate fields to prevent FTS5 index bombs from oversized input.
    let name        = truncate_fts_field(name);
    let title       = truncate_fts_field(title);
    let description = truncate_fts_field(description);
    let tags        = truncate_fts_field(tags);

    // Issue-FTS5-CONTENT — 2026-03-14
    // Only delete the existing FTS5 entry if one actually exists.  Issuing a
    // contentless FTS5 delete for a non-existent row corrupts the internal
    // term-frequency statistics and causes `rank`/`bm25()` to return NULL.
    // We use the companion `search_entities` table as the source of truth.
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM search_entities WHERE rowid = ?1)",
        params![rowid],
        |row| row.get(0),
    )?;
    if exists {
        conn.execute(
            "INSERT INTO search_index(search_index, rowid, entity_type, entity_id, name, title, description, tags) \
             VALUES('delete', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![rowid, entity_type, entity_id, &*name, &*title, &*description, &*tags],
        )?;
    }

    conn.execute(
        "INSERT INTO search_index(rowid, entity_type, entity_id, name, title, description, tags) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![rowid, entity_type, entity_id, &*name, &*title, &*description, &*tags],
    )?;

    // Issue-FTS5-CONTENT — 2026-03-14
    // Keep the companion lookup table in sync with the FTS5 rowid.
    conn.execute(
        "INSERT OR REPLACE INTO search_entities (rowid, entity_type, entity_id) \
         VALUES (?1, ?2, ?3)",
        params![rowid, entity_type, entity_id],
    )?;

    Ok(())
}

/// Removes the search index entry for the given entity from both the FTS5
/// table and the companion `search_entities` lookup table.
///
/// # Errors
///
/// Returns [`DbError`] if the FTS5 delete command or companion delete fails.
// Issue-FTS5-CONTENT — 2026-03-14
pub fn delete_from_search_index(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    name: &str,
    title: &str,
    description: &str,
    tags: &str,
) -> Result<(), DbError> {
    let rowid = rowid_for(entity_type, entity_id);

    // Issue-FTS5-CONTENT — 2026-03-14
    // Only issue the FTS5 delete if the row actually exists.  Issuing a
    // contentless FTS5 delete for a non-existent row corrupts the internal
    // term-frequency statistics (same guard as in populate_search_index).
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM search_entities WHERE rowid = ?1)",
        params![rowid],
        |row| row.get(0),
    )?;
    if exists {
        conn.execute(
            "INSERT INTO search_index(search_index, rowid, entity_type, entity_id, name, title, description, tags) \
             VALUES('delete', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![rowid, entity_type, entity_id, name, title, description, tags],
        )?;
    }

    conn.execute(
        "DELETE FROM search_entities WHERE rowid = ?1",
        params![rowid],
    )?;

    Ok(())
}

// Issue-21 FTS5 sanitize — 2026-03-13
/// Strips FTS5 special operators and syntax characters from user input to
/// prevent malformed queries from causing parse errors.
///
/// Removes: `"`, `(`, `)`, `*`, and the keywords `AND`, `OR`, `NOT`, `NEAR`.
/// The result is safe to pass directly into an FTS5 `MATCH` clause as a
/// simple implicit-AND token list.
#[must_use]
pub fn sanitize_fts5_query(input: &str) -> String {
    // Strip syntax characters.
    let cleaned: String = input
        .chars()
        .map(|c| match c {
            '"' | '(' | ')' | '*' => ' ',
            _ => c,
        })
        .collect();

    // Strip FTS5 boolean/proximity keywords (whole-word, case-sensitive).
    cleaned
        .split_whitespace()
        .filter(|word| !matches!(*word, "AND" | "OR" | "NOT" | "NEAR"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Searches the FTS5 index using a `MATCH` query, ordered by BM25 rank
/// weighted by an optional quality score from `entity_quality`.
///
/// `entity_type_filter` — if `Some`, restricts results to that entity type.
/// `limit` and `offset` provide pagination.
///
/// The query is sanitized via [`sanitize_fts5_query`] before being passed to
/// FTS5. If the sanitized query is empty, an empty result set is returned
/// without hitting the database.
///
/// # Errors
///
/// Returns [`DbError`] if the FTS5 MATCH query fails.
// Issue-FTS5-CONTENT — 2026-03-14
pub fn search(
    conn: &Connection,
    query: &str,
    entity_type_filter: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<SearchResult>, DbError> {
    // Issue-21 FTS5 sanitize — 2026-03-13
    let safe_query = sanitize_fts5_query(query);
    if safe_query.trim().is_empty() {
        return Ok(Vec::new());
    }

    // Issue-FTS5-CONTENT — 2026-03-14
    // The FTS5 table is contentless (content=''), so column values cannot be
    // read back.  We use a subquery to obtain rowid + bm25() rank from FTS5
    // (the bm25() function is only valid in queries where FTS5 is the primary
    // table with a MATCH clause), then JOIN the companion `search_entities`
    // table in the outer query to resolve (entity_type, entity_id).
    let sql = if entity_type_filter.is_some() {
        "SELECT e.entity_type, e.entity_id, m.fts_rank, \
                COALESCE(q.score, 0) AS quality_score \
         FROM (SELECT rowid, bm25(search_index) AS fts_rank \
               FROM search_index WHERE search_index MATCH ?1) m \
         JOIN search_entities e ON e.rowid = m.rowid \
         LEFT JOIN entity_quality q \
           ON q.entity_type = e.entity_type AND q.entity_id = e.entity_id \
         WHERE e.entity_type = ?2 \
         ORDER BY (m.fts_rank * (1.0 + CAST(COALESCE(q.score, 0) AS REAL) / 100.0)) \
         LIMIT ?3 OFFSET ?4"
    } else {
        "SELECT e.entity_type, e.entity_id, m.fts_rank, \
                COALESCE(q.score, 0) AS quality_score \
         FROM (SELECT rowid, bm25(search_index) AS fts_rank \
               FROM search_index WHERE search_index MATCH ?1) m \
         JOIN search_entities e ON e.rowid = m.rowid \
         LEFT JOIN entity_quality q \
           ON q.entity_type = e.entity_type AND q.entity_id = e.entity_id \
         ORDER BY (m.fts_rank * (1.0 + CAST(COALESCE(q.score, 0) AS REAL) / 100.0)) \
         LIMIT ?3 OFFSET ?4"
    };

    let filter = entity_type_filter.unwrap_or("");
    let mut stmt = conn.prepare(sql)?;

    let rows = stmt.query_map(params![safe_query, filter, limit, offset], |row| {
        Ok(SearchResult {
            entity_type:   row.get(0)?,
            entity_id:     row.get(1)?,
            rank:          row.get(2)?,
            quality_score: row.get(3)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(results)
}
