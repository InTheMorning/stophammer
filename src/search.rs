//! Full-text search over the FTS5 `search_index` table.
//!
//! The FTS5 table holds only `name`, `title` and `description` (ADR 0078 §1).
//! We manage rowids ourselves using a deterministic hash of
//! `entity_type + entity_id`.

use std::borrow::Cow;
use std::hash::Hasher;

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};
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
    pub entity_type: String,
    pub entity_id: String,
    pub rank: f64,
    pub quality_score: i64,
    /// Quality-adjusted rank used as the primary sort key for keyset pagination.
    // Issue-SEARCH-KEYSET — 2026-03-14
    pub effective_rank: f64,
    /// FTS5 rowid, used as tiebreaker in keyset pagination cursors.
    // Issue-SEARCH-KEYSET — 2026-03-14
    pub rowid: i64,
}

// SP-01 stable FTS5 hash — 2026-03-13
/// Computes a deterministic positive `i64` rowid from entity type and id.
///
/// We manage the FTS5 rowids ourselves so that updates and deletes can
/// target the correct row.
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
/// ADR 0078 §1 and §2: the index holds only `name`, `title` and `description`.
/// An update runs a `DELETE` by rowid and then inserts the new text at the same
/// rowid. No write sends old text to FTS5. The companion table
/// `search_entities` changes in lockstep, so that a search result resolves to
/// `(entity_type, entity_id)` with a JOIN on rowid.
///
/// If a hash collision is detected (the computed rowid already belongs to a
/// *different* entity), the colliding entity is **skipped** from FTS5 and
/// a `tracing::warn!` is emitted. The entity remains queryable via the
/// normal REST API; it simply will not appear in full-text search results.
///
/// # Errors
///
/// Returns [`DbError`] if the FTS5 delete or insert, or the companion-table
/// upsert, fails.
// Issue-FTS5-CONTENT — 2026-03-14
// Issue-HASH-COLLISION — 2026-03-14
pub fn populate_search_index(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    name: &str,
    title: &str,
    description: &str,
) -> Result<(), DbError> {
    let rowid = rowid_for(entity_type, entity_id);

    // Truncate fields to prevent FTS5 index bombs from oversized input.
    let name = truncate_fts_field(name);
    let title = truncate_fts_field(title);
    let description = truncate_fts_field(description);

    // Issue-HASH-COLLISION — 2026-03-14
    // Before touching FTS5 or the companion table, check whether the rowid is
    // already occupied by a *different* entity. If so, we have a hash collision
    // and must skip this entity to avoid corrupting the existing entry.
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT entity_type, entity_id FROM search_entities WHERE rowid = ?1",
            params![rowid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    if let Some((ref existing_type, ref existing_id)) = existing
        && (existing_type != entity_type || existing_id != entity_id)
    {
        // Issue-HASH-COLLISION — 2026-03-14
        // Collision: rowid is owned by a different entity. Log and skip.
        tracing::warn!(
            rowid,
            existing_entity_type = %existing_type,
            existing_entity_id   = %existing_id,
            colliding_entity_type = %entity_type,
            colliding_entity_id   = %entity_id,
            "FTS5 rowid hash collision detected; skipping colliding entity from search index",
        );
        return Ok(());
    }

    // ADR 0078 §2: delete the old row by rowid. FTS5 keeps a copy of the
    // text, so the delete needs no old text. The `search_entities` row is the guard: a delete of a row that the index
    // does not hold is skipped.
    if existing.is_some() {
        conn.execute("DELETE FROM search_index WHERE rowid = ?1", params![rowid])?;
    }

    conn.execute(
        "INSERT INTO search_index(rowid, name, title, description) \
         VALUES(?1, ?2, ?3, ?4)",
        params![rowid, &*name, &*title, &*description],
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
/// ADR 0078 §2: the delete is a `DELETE` by rowid and needs no old text.
///
/// # Errors
///
/// Returns [`DbError`] if the FTS5 delete or the companion delete fails.
// Issue-FTS5-CONTENT — 2026-03-14
pub fn delete_from_search_index(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<(), DbError> {
    let rowid = rowid_for(entity_type, entity_id);

    // ADR 0078 §2: issue the FTS5 delete only if the `search_entities` row
    // shows that the index holds the row.
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM search_entities WHERE rowid = ?1)",
        params![rowid],
        |row| row.get(0),
    )?;
    if exists {
        conn.execute("DELETE FROM search_index WHERE rowid = ?1", params![rowid])?;
    }

    conn.execute(
        "DELETE FROM search_entities WHERE rowid = ?1",
        params![rowid],
    )?;

    Ok(())
}

/// Divides `input` into quoted FTS5 phrases, in the sequence of the input.
///
/// ADR 0080 §1 and ADR 0078 §5: the node removes each control character. Text
/// between two double quotes is one phrase. Each other word is one phrase. A
/// double quote with no second double quote after it is part of its word, and
/// the node doubles it. No character of `input` is then an FTS5 operator, a
/// column filter or a prefix.
#[must_use]
pub fn fts5_phrases(input: &str) -> Vec<String> {
    let cleaned: String = input.chars().filter(|c| !c.is_control()).collect();
    let mut phrases = Vec::new();
    let mut rest = cleaned.as_str();
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else {
            break;
        };
        push_words(&mut phrases, &rest[..open]);
        let words: Vec<&str> = after[..close].split_whitespace().collect();
        if !words.is_empty() {
            phrases.push(format!("\"{}\"", words.join(" ")));
        }
        rest = &after[close + 1..];
    }
    push_words(&mut phrases, rest);
    phrases
}

/// Adds each word of `text` to `phrases` as one quoted phrase.
fn push_words(phrases: &mut Vec<String>, text: &str) {
    phrases.extend(
        text.split_whitespace()
            .map(|word| format!("\"{}\"", word.replace('"', "\"\""))),
    );
}

/// Makes an FTS5 query of quoted phrases from the words and phrases of `input`.
///
/// ADR 0080 §1 and ADR 0078 §5: the query is the phrases of [`fts5_phrases`],
/// joined with one space. The result is empty when `input` has no word.
#[must_use]
pub fn fts5_query_from_words(input: &str) -> String {
    fts5_phrases(input).join(" ")
}

/// A field that a text filter reads (ADR 0080 §2 and §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextField {
    /// `artist`: the column `name`, and on a publisher the column `title`.
    Artist,
    /// `title`: the column `title`.
    Title,
    /// `release`: the title of the feed. Unsupported on a publisher.
    Release,
    /// `not_q`: the columns `name`, `title` and `description`.
    Any,
}

/// One text filter or one exclusion: a field and its FTS5 phrases.
#[derive(Clone, Debug)]
pub struct TextFilter {
    pub field: TextField,
    /// The phrases of [`fts5_phrases`]. Each one must be in the field.
    pub phrases: Vec<String>,
}

/// The text filters and the exclusions of a search (ADR 0080 §2 and §3).
///
/// Each text filter is a condition in the outer `WHERE` of the search SQL.
/// None of them is part of the `MATCH` of `q`, so the rank does not change.
#[derive(Clone, Debug, Default)]
pub struct SearchFilters {
    pub include: Vec<TextFilter>,
    pub exclude: Vec<TextFilter>,
}

impl TextFilter {
    /// The row of `search_index` that this filter reads, as an SQL
    /// expression, and the columns it reads in that row (ADR 0080 §2).
    ///
    /// Returns `None` when the entity type does not apply the field.
    fn target(&self, entity_type: &str) -> Option<(&'static str, &'static str)> {
        const OWN_ROW: &str = "m.rowid";
        // ADR 0080 §2: the row of the feed of a track.
        const FEED_ROW: &str = "(SELECT fe.rowid FROM search_entities fe \
             WHERE fe.entity_type = 'feed' \
             AND fe.entity_id = json_extract(e.entity_id, '$[0]'))";
        match (self.field, entity_type) {
            (TextField::Release, "publisher") => None,
            (TextField::Release, "track") => Some((FEED_ROW, "title")),
            (TextField::Release | TextField::Title, _) | (TextField::Artist, "publisher") => {
                Some((OWN_ROW, "title"))
            }
            (TextField::Artist, _) => Some((OWN_ROW, "name")),
            (TextField::Any, _) => Some((OWN_ROW, "{name title description}")),
        }
    }

    /// The FTS5 expression of this filter: the column before each phrase.
    fn expression(&self, columns: &str) -> String {
        self.phrases
            .iter()
            .map(|phrase| format!("{columns} : {phrase}"))
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

/// Searches the FTS5 index with no text filter. See [`search_filtered`].
///
/// # Errors
///
/// Returns [`DbError`] if the FTS5 MATCH query fails.
pub fn search(
    conn: &Connection,
    query: &str,
    entity_type_filter: Option<&str>,
    limit: i64,
    cursor_rank: Option<f64>,
    cursor_rowid: Option<i64>,
) -> Result<Vec<SearchResult>, DbError> {
    search_filtered(
        conn,
        query,
        &SearchFilters::default(),
        entity_type_filter,
        limit,
        cursor_rank,
        cursor_rowid,
    )
}

/// Searches the FTS5 index using a `MATCH` query, ordered by BM25 rank
/// weighted by an optional quality score from `entity_quality`.
///
/// `entity_type_filter` — if `Some`, restricts results to that entity type.
/// `limit` controls the maximum number of results returned.
///
/// Pagination uses keyset cursors: if `cursor_rank` and `cursor_rowid` are
/// provided, only results after the given `(effective_rank, rowid)` position
/// are returned. The effective rank is the quality-adjusted BM25 score used
/// as the primary ORDER BY key; `rowid` is the tiebreaker.
///
/// The query is made by [`fts5_query_from_words`] before it goes to FTS5. If
/// `query` has no word, the function returns an empty result set without a
/// database query.
///
/// ADR 0080 §2 and §3: `filters` adds one condition to the outer `WHERE` for
/// each text filter and each exclusion. A filter selects rows and does not
/// change the rank. A filter that the entity type does not apply is ignored,
/// because the handler already refused it.
///
/// # Errors
///
/// Returns [`DbError`] if the FTS5 MATCH query fails.
// Issue-SEARCH-KEYSET — 2026-03-14
pub fn search_filtered(
    conn: &Connection,
    query: &str,
    filters: &SearchFilters,
    entity_type_filter: Option<&str>,
    limit: i64,
    cursor_rank: Option<f64>,
    cursor_rowid: Option<i64>,
) -> Result<Vec<SearchResult>, DbError> {
    let safe_query = fts5_query_from_words(query);
    tracing::info!(original_query = ?query, safe_query = ?safe_query, entity_type_filter = ?entity_type_filter, "search called");
    if safe_query.trim().is_empty() {
        tracing::warn!("search query has no word");
        return Ok(Vec::new());
    }

    // Issue-SEARCH-KEYSET — 2026-03-14
    // The FTS5 table holds no entity identity (ADR 0078 §1).  We use a
    // subquery to obtain rowid + bm25() rank from FTS5
    // (the bm25() function is only valid in queries where FTS5 is the primary
    // table with a MATCH clause), then JOIN the companion `search_entities`
    // table in the outer query to resolve (entity_type, entity_id).
    //
    // Keyset pagination: when a cursor is present, the outer WHERE filters
    // to rows strictly after the cursor position in sort order. The sort key
    // is (effective_rank ASC, rowid ASC) where effective_rank is the quality-
    // adjusted BM25 score (lower = better match). BM25 scores are negative,
    // so "next page" means effective_rank > cursor_rank (less negative =
    // worse match), with rowid as the tiebreaker for identical ranks.
    //
    // The effective_rank expression must be repeated in the WHERE clause
    // because SQL does not allow referencing SELECT aliases in WHERE.
    // ?1 = MATCH query.  Each later value is pushed to `values` in the same
    // sequence as its `?N` in the SQL text, so the two cannot drift apart.
    // ADR 0078 §4: a publisher feed has one quality row, of the type `feed`.
    // The join below reads that row for a `publisher` hit.  The select list,
    // the order and the keyset filter all use the same `q.score`.
    let eff_rank_expr = "(m.fts_rank * (1.0 + CAST(COALESCE(q.score, 0) AS REAL) / 100.0))";

    let mut values: Vec<Value> = vec![Value::Text(safe_query)];
    let mut where_parts = Vec::new();

    if let (Some(rank), Some(rowid)) = (cursor_rank, cursor_rowid) {
        values.push(Value::Real(rank));
        let p1 = values.len();
        values.push(Value::Integer(rowid));
        let p2 = values.len();
        where_parts.push(format!(
            "({eff_rank_expr} > ?{p1} OR ({eff_rank_expr} = ?{p1} AND m.rowid > ?{p2}))"
        ));
    }

    if let Some(entity_type) = entity_type_filter {
        values.push(Value::Text(entity_type.to_owned()));
        where_parts.push(format!("e.entity_type = ?{}", values.len()));
    }

    // ADR 0080 §2 and §3: a text filter is a condition on the index row, never
    // part of the `MATCH` above.  Each value is a bound parameter.
    let entity_type = entity_type_filter.unwrap_or("");
    for (filter, excluded) in filters
        .include
        .iter()
        .map(|f| (f, false))
        .chain(filters.exclude.iter().map(|f| (f, true)))
    {
        let Some((row, columns)) = filter.target(entity_type) else {
            continue;
        };
        values.push(Value::Text(filter.expression(columns)));
        let membership = format!(
            "{row} IN (SELECT rowid FROM search_index WHERE search_index MATCH ?{})",
            values.len()
        );
        // The feed of a track can be missing from the index. `COALESCE` keeps
        // such a track under an exclusion.
        where_parts.push(if excluded {
            format!("COALESCE({membership}, 0) = 0")
        } else {
            membership
        });
    }

    values.push(Value::Integer(limit));
    let limit_param_idx = values.len();

    let where_clause = if where_parts.is_empty() {
        String::new()
    } else {
        format!("WHERE {} ", where_parts.join(" AND "))
    };

    let sql = format!(
        "SELECT e.entity_type, e.entity_id, m.fts_rank, \
                COALESCE(q.score, 0) AS quality_score, \
                {eff_rank_expr} AS eff_rank, \
                m.rowid \
         FROM (SELECT rowid, bm25(search_index) AS fts_rank \
               FROM search_index WHERE search_index MATCH ?1) m \
         JOIN search_entities e ON e.rowid = m.rowid \
         LEFT JOIN entity_quality q \
           ON q.entity_type = CASE e.entity_type WHEN 'publisher' THEN 'feed' ELSE e.entity_type END \
          AND q.entity_id = e.entity_id \
         {where_clause}\
         ORDER BY eff_rank ASC, m.rowid ASC \
         LIMIT ?{limit_param_idx}"
    );

    let mut stmt = conn.prepare(&sql)?;

    let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<SearchResult> {
        Ok(SearchResult {
            entity_type: row.get(0)?,
            entity_id: row.get(1)?,
            rank: row.get(2)?,
            quality_score: row.get(3)?,
            effective_rank: row.get(4)?,
            rowid: row.get(5)?,
        })
    };

    let rows: Vec<SearchResult> = stmt
        .query_map(rusqlite::params_from_iter(values), map_row)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}
