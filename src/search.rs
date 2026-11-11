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
    parse_phrases(input).0
}

/// The phrases of [`fts5_phrases`], and whether the last phrase is a word with
/// no double quotes around it (ADR 0086 §1).
fn parse_phrases(input: &str) -> (Vec<String>, bool) {
    let cleaned: String = input.chars().filter(|c| !c.is_control()).collect();
    let mut phrases = Vec::new();
    let mut last_is_word = false;
    let mut rest = cleaned.as_str();
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else {
            break;
        };
        last_is_word |= push_words(&mut phrases, &rest[..open]);
        let words: Vec<&str> = after[..close].split_whitespace().collect();
        if !words.is_empty() {
            phrases.push(format!("\"{}\"", words.join(" ")));
            last_is_word = false;
        }
        rest = &after[close + 1..];
    }
    last_is_word |= push_words(&mut phrases, rest);
    (phrases, last_is_word)
}

/// Adds each word of `text` to `phrases` as one quoted phrase. Returns whether
/// `text` has a word.
fn push_words(phrases: &mut Vec<String>, text: &str) -> bool {
    let before = phrases.len();
    phrases.extend(
        text.split_whitespace()
            .map(|word| format!("\"{}\"", word.replace('"', "\"\""))),
    );
    phrases.len() > before
}

/// Makes an FTS5 query of quoted phrases from the words and phrases of `input`.
///
/// ADR 0080 §1 and ADR 0078 §5: the query is the phrases of [`fts5_phrases`],
/// joined with one space. The result is empty when `input` has no word.
#[must_use]
pub fn fts5_query_from_words(input: &str) -> String {
    fts5_phrases(input).join(" ")
}

/// Makes the prefix form of [`fts5_query_from_words`] (ADR 0086 §1): FTS5 reads
/// the last word as the start of a word. The result is `None` when the last
/// phrase of `input` is text in double quotes, or when `input` has no word.
#[must_use]
pub fn fts5_prefix_query(input: &str) -> Option<String> {
    let (phrases, last_is_word) = parse_phrases(input);
    last_is_word.then(|| format!("{}*", phrases.join(" ")))
}

/// The hits of a search with a word in `query`: an SQL select with the columns
/// `rowid`, `fts_rank` and `whole`. It pushes the `MATCH` values to `values`,
/// which must be empty.
///
/// ADR 0086 §2: the hits with the whole word have the BM25 score of the exact
/// query, and `whole` is 1. Each other hit of the prefix query follows with its
/// own score.
fn hit_source(query: &str, values: &mut Vec<Value>) -> String {
    values.push(Value::Text(fts5_query_from_words(query)));
    let exact = "SELECT rowid, bm25(search_index) AS fts_rank, 1 AS whole \
                 FROM search_index WHERE search_index MATCH ?1";
    if let Some(prefix) = fts5_prefix_query(query) {
        values.push(Value::Text(prefix));
        format!(
            "{exact} UNION ALL \
             SELECT rowid, bm25(search_index), 0 \
             FROM search_index WHERE search_index MATCH ?2 \
             AND rowid NOT IN (SELECT rowid FROM search_index WHERE search_index MATCH ?1)"
        )
    } else {
        exact.to_owned()
    }
}

/// ADR 0087 §3: the type of the current row of `search_index`. The engine calculates
/// `bm25` only for a row that passes `WHERE`, so a row of another type costs one
/// lookup and no score.
const ROW_TYPE: &str =
    "(SELECT s.entity_type FROM search_entities s WHERE s.rowid = search_index.rowid)";

/// The source of a rank search with a word in `query`: an SQL subquery with the
/// columns `rowid`, `fts_rank` and `group_offset`. It pushes the `MATCH` values
/// to `values`, which must be empty.
///
/// ADR 0087 §1: the groups are all words in one field, all words in the title
/// and the artist name together, and each other hit. In each group a hit with
/// the whole word comes before a hit by prefix.
///
/// ADR 0087 §2: `group_offset` goes only into the effective rank, because the
/// cursor holds one number and a `rowid`. Each group holds
/// 1,000,000 values, which is much more than the lowest effective rank: in the
/// backup of 2026-11-07 a word scores -18.7 or more, and the quality weight at
/// most doubles a score.
fn match_source(query: &str, scope: RankScope<'_>, values: &mut Vec<Value>) -> String {
    let exact = fts5_query_from_words(query);
    values.push(Value::Text(exact.clone()));
    let prefix = fts5_prefix_query(query);
    if let Some(prefix) = &prefix {
        values.push(Value::Text(prefix.clone()));
    }
    let in_scope = match scope {
        RankScope::All => String::new(),
        RankScope::One(entity_type) => {
            values.push(Value::Text(entity_type.to_owned()));
            format!("AND {ROW_TYPE} = ?{} ", values.len())
        }
        RankScope::FeedsAndTracks => format!("AND {ROW_TYPE} IN ('feed', 'track') "),
        RankScope::FeedMedium(scope) => format!("AND {ROW_TYPE} IN {} ", scope.entity_types()),
        RankScope::FeedsAndTracksMedium(scope) => format!(
            "AND ({ROW_TYPE} IN {} OR {ROW_TYPE} = 'track') ",
            scope.entity_types()
        ),
    };
    let whole = format!(
        "SELECT rowid, bm25(search_index) AS fts_rank, 1 AS whole \
         FROM search_index WHERE search_index MATCH ?1 {in_scope}"
    );
    if exact.is_empty() {
        return format!("SELECT rowid, fts_rank, 0.0 AS group_offset FROM ({whole})");
    }
    // The prefix branch reads the whole-word hits once more only to skip them.
    let hits = if prefix.is_some() {
        format!(
            "SELECT rowid, fts_rank, whole FROM x UNION ALL \
             SELECT rowid, bm25(search_index), 0 \
             FROM search_index WHERE search_index MATCH ?2 {in_scope}\
             AND rowid NOT IN (SELECT rowid FROM x)"
        )
    } else {
        "SELECT rowid, fts_rank, whole FROM x".to_owned()
    };
    // One phrase is in one field, so the second group has no hit, and the first
    // group needs only one column filter.
    let one_phrase = fts5_phrases(query).len() == 1;
    let both_fields = |words: &str| format!("{{name title}} : ({words})");
    let one_field = |words: &str| {
        if one_phrase {
            both_fields(words)
        } else {
            format!("title : ({words}) OR name : ({words})")
        }
    };

    let mut arms = Vec::new();
    let mut push_arm = |offset: &str, value: String| {
        values.push(Value::Text(value));
        arms.push(format!(
            "WHEN h.rowid IN (SELECT rowid FROM search_index WHERE search_index MATCH ?{}) \
             THEN {offset} ",
            values.len()
        ));
    };
    push_arm("0.0", one_field(&exact));
    if let Some(prefix) = &prefix {
        push_arm("1000000.0", one_field(prefix));
    }
    if !one_phrase {
        push_arm("2000000.0", both_fields(&exact));
        if let Some(prefix) = &prefix {
            push_arm("3000000.0", both_fields(prefix));
        }
    }
    let arms = arms.concat();
    format!(
        "WITH x AS MATERIALIZED ({whole}), h AS MATERIALIZED ({hits}), \
         g AS MATERIALIZED (SELECT h.rowid AS rowid, h.fts_rank AS fts_rank, \
             CASE {arms}WHEN h.whole = 1 THEN 4000000.0 ELSE 5000000.0 END AS group_offset \
             FROM h) \
         SELECT rowid, fts_rank, group_offset FROM g"
    )
}

/// A field that a text filter reads (ADR 0080 §2 and §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextField {
    /// `artist`: the column `name`, and on a publisher the column `title`.
    Artist,
    /// `title`: the column `title`. On a track it also reads the title of the
    /// feed (ADR 0085 §4).
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
    pub values: ValueFilters,
}

/// The value filters of a search (ADR 0080 §2): the kind, the track count, the
/// dates, the duration and the host.
///
/// Each field that is set is a condition in the outer `WHERE` of the search
/// SQL. Each limit is included.
#[derive(Clone, Debug, Default)]
pub struct ValueFilters {
    /// `kind`: the release kinds of the feed. The kinds are an OR.
    pub kinds: Option<Vec<String>>,
    /// `tracks`: the exact `episode_count` of the feed.
    pub tracks: Option<i64>,
    pub min_tracks: Option<i64>,
    pub max_tracks: Option<i64>,
    /// `after`: the first second that matches, as Unix seconds in UTC.
    pub after: Option<i64>,
    /// `before`: the first second after the last day that matches.
    pub before_end: Option<i64>,
    pub min_duration: Option<i64>,
    pub max_duration: Option<i64>,
    /// `host`: the host of the feed, or a parent domain of it (ADR 0080 §5).
    pub host: Option<String>,
    /// `not_host`: each host to remove. A feed with the empty host stays.
    pub not_hosts: Vec<String>,
}

impl SearchFilters {
    /// Whether the request has a positive filter: a text filter, a value filter
    /// or `host`. An exclusion is not one (ADR 0080 §4 and §6).
    #[must_use]
    pub fn has_positive(&self) -> bool {
        !self.include.is_empty() || self.values.has_positive()
    }

    /// The number of exclusions: each `not_` text filter and each `not_host`
    /// (ADR 0080 §3).
    #[must_use]
    pub fn exclusion_count(&self) -> usize {
        self.exclude.len() + self.values.not_hosts.len()
    }
}

impl ValueFilters {
    /// Whether a filter other than `not_host` is set (ADR 0080 §4).
    fn has_positive(&self) -> bool {
        self.kinds.is_some()
            || self.tracks.is_some()
            || self.min_tracks.is_some()
            || self.max_tracks.is_some()
            || self.after.is_some()
            || self.before_end.is_some()
            || self.min_duration.is_some()
            || self.max_duration.is_some()
            || self.host.is_some()
    }

    /// The `LEFT JOIN` clauses of the feed and the track of the hit.
    ///
    /// They are present only for a filter that reads them, unless `always` is
    /// set. `CASE` keeps `json_extract` away from the ID of a feed.
    fn joins(&self, always: bool) -> String {
        let mut joins = String::new();
        if always || self.needs_feed() {
            joins.push_str(
                "LEFT JOIN feeds f ON f.feed_guid = \
                   CASE e.entity_type WHEN 'track' THEN json_extract(e.entity_id, '$[0]') \
                   ELSE e.entity_id END ",
            );
        }
        if always || self.needs_track() {
            joins.push_str(
                "LEFT JOIN tracks t ON e.entity_type = 'track' AND t.feed_guid = f.feed_guid \
                   AND t.track_guid = CASE e.entity_type WHEN 'track' \
                     THEN json_extract(e.entity_id, '$[1]') END ",
            );
        }
        joins
    }

    /// Whether a condition reads the track row of the hit.
    fn needs_track(&self) -> bool {
        self.after.is_some()
            || self.before_end.is_some()
            || self.min_duration.is_some()
            || self.max_duration.is_some()
    }

    /// Whether a condition reads the feed row of the hit.
    fn needs_feed(&self) -> bool {
        self.needs_track()
            || self.kinds.is_some()
            || self.tracks.is_some()
            || self.min_tracks.is_some()
            || self.max_tracks.is_some()
            || self.host.is_some()
            || !self.not_hosts.is_empty()
    }

    /// Pushes one condition and its bound values for each filter that is set.
    /// A parameter number follows the length of `values`.
    fn push_conditions(&self, values: &mut Vec<Value>, where_parts: &mut Vec<String>) {
        if let Some(kinds) = &self.kinds {
            let mut parts = Vec::new();
            for kind in kinds {
                values.push(Value::Text(kind.clone()));
                parts.push(format!("f.release_kind = ?{}", values.len()));
            }
            // ADR 0080 §2: `unknown` also matches a feed with no kind.
            if kinds.iter().any(|k| k == "unknown") {
                parts.push("f.release_kind IS NULL".to_owned());
            }
            where_parts.push(format!("({})", parts.join(" OR ")));
        }
        let mut push_limit = |column: &str, op: &str, limit: Option<i64>| {
            if let Some(limit) = limit {
                values.push(Value::Integer(limit));
                where_parts.push(format!("{column} {op} ?{}", values.len()));
            }
        };
        let date_column =
            "CASE e.entity_type WHEN 'track' THEN t.pub_date ELSE f.newest_item_at END";
        push_limit("f.episode_count", "=", self.tracks);
        push_limit("f.episode_count", ">=", self.min_tracks);
        push_limit("f.episode_count", "<=", self.max_tracks);
        push_limit(date_column, ">=", self.after);
        push_limit(date_column, "<", self.before_end);
        push_limit("t.duration_secs", ">=", self.min_duration);
        push_limit("t.duration_secs", "<=", self.max_duration);
        // ADR 0080 §5: the host or a subdomain of it. The empty host never
        // matches `host`. `COALESCE` keeps a row with a null or empty host
        // under `not_host`.
        let host_match = |values: &mut Vec<Value>, host: &str| {
            values.push(Value::Text(host.to_owned()));
            let n = values.len();
            format!("(f.feed_host = ?{n} OR substr(f.feed_host, -length(?{n}) - 1) = '.' || ?{n})")
        };
        if let Some(host) = &self.host {
            let condition = host_match(values, host);
            where_parts.push(condition);
        }
        for host in &self.not_hosts {
            let condition = host_match(values, host);
            where_parts.push(format!("COALESCE({condition}, 0) = 0"));
        }
    }
}

/// ADR 0080 §2: the row of the feed of a track, as an SQL expression.
const FEED_ROW: &str = "(SELECT fe.rowid FROM search_entities fe \
     WHERE fe.entity_type = 'feed' \
     AND fe.entity_id = json_extract(e.entity_id, '$[0]'))";

impl TextFilter {
    /// The row of `search_index` that this filter reads, as an SQL
    /// expression, and the columns it reads in that row (ADR 0080 §2).
    ///
    /// `own_row` is the expression of the row of the hit itself: `m.rowid` in a
    /// search with `q`, and `e.rowid` in a search with no `q` (ADR 0080 §6). An
    /// empty `entity_type` is a search with no `type`, which has feed and
    /// track rows.
    ///
    /// Returns `None` when the entity type does not apply the field.
    fn target(&self, entity_type: &str, own_row: &str) -> Option<(String, &'static str)> {
        let own = own_row.to_owned();
        match (self.field, entity_type) {
            (TextField::Release, "publisher") => None,
            (TextField::Release, "track") => Some((FEED_ROW.to_owned(), "title")),
            (TextField::Release, "") => Some((
                format!("CASE e.entity_type WHEN 'track' THEN {FEED_ROW} ELSE {own_row} END"),
                "title",
            )),
            (TextField::Release | TextField::Title, _) | (TextField::Artist, "publisher") => {
                Some((own, "title"))
            }
            (TextField::Artist, _) => Some((own, "name")),
            (TextField::Any, _) => Some((own, "{name title description}")),
        }
    }

    /// Pushes the condition of each text filter and each exclusion of
    /// `filters` (ADR 0080 §2 and §3). Each value is a bound parameter.
    fn push_conditions(
        filters: &SearchFilters,
        entity_type: &str,
        own_row: &str,
        values: &mut Vec<Value>,
        where_parts: &mut Vec<String>,
    ) {
        for (filter, excluded) in filters
            .include
            .iter()
            .map(|f| (f, false))
            .chain(filters.exclude.iter().map(|f| (f, true)))
        {
            let membership = if filter.field == TextField::Title
                && matches!(entity_type, "track" | "")
                && !filter.phrases.is_empty()
            {
                filter.title_condition(entity_type, own_row, values)
            } else {
                let Some((row, columns)) = filter.target(entity_type, own_row) else {
                    continue;
                };
                values.push(Value::Text(filter.expression(columns)));
                format!(
                    "{row} IN (SELECT rowid FROM search_index WHERE search_index MATCH ?{})",
                    values.len()
                )
            };
            // The feed of a track can be missing from the index. `COALESCE`
            // keeps such a track under an exclusion.
            where_parts.push(if excluded {
                format!("COALESCE({membership}, 0) = 0")
            } else {
                membership
            });
        }
    }

    /// The condition of `title` on a track row (ADR 0085 §4).
    ///
    /// Each phrase has its own bound `MATCH` value, and it can match the title
    /// of the track or the title of its feed. The phrases are an AND, so each
    /// word can be in a different title. A phrase in double quotes stays in one
    /// title. `COALESCE` keeps a null result of a track with no feed in the
    /// index from making the row unknown. A feed row, in a search with no
    /// `type`, reads only its own title.
    fn title_condition(&self, entity_type: &str, own_row: &str, values: &mut Vec<Value>) -> String {
        let mut parts = Vec::new();
        for phrase in &self.phrases {
            values.push(Value::Text(format!("title : {phrase}")));
            let matching = format!(
                "(SELECT rowid FROM search_index WHERE search_index MATCH ?{})",
                values.len()
            );
            let own_in = format!("{own_row} IN {matching}");
            let track_in = format!("({own_in} OR COALESCE({FEED_ROW} IN {matching}, 0))");
            parts.push(if entity_type == "track" {
                track_in
            } else {
                format!("CASE e.entity_type WHEN 'track' THEN {track_in} ELSE {own_in} END")
            });
        }
        format!("({})", parts.join(" AND "))
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

/// The rows that a rank search reads.
#[derive(Clone, Copy, Debug)]
enum RankScope<'a> {
    /// Each row type.
    All,
    /// One row type.
    One(&'a str),
    /// The rows of a request with no `type`.
    FeedsAndTracks,
    /// A feed search whose feed rows are selected by `medium`.
    FeedMedium(FeedMediumScope),
    /// A search with no `type`, whose feed rows are selected by `medium`.
    FeedsAndTracksMedium(FeedMediumScope),
}

/// The feed row types that a `medium` parameter selects.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FeedMediumScope {
    pub(crate) music: bool,
    pub(crate) music_l: bool,
}

impl FeedMediumScope {
    fn entity_types(self) -> &'static str {
        match (self.music, self.music_l) {
            (true, true) => "('feed', 'playlist')",
            (true, false) => "('feed')",
            (false, true) => "('playlist')",
            (false, false) => "('')",
        }
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
/// The query is made by [`match_source`] before it goes to FTS5. If `query` has
/// no word, the function returns an empty result set without a database query.
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
    let scope = entity_type_filter.map_or(RankScope::All, RankScope::One);
    rank_search(
        conn,
        query,
        filters,
        scope,
        limit,
        cursor_rank,
        cursor_rowid,
    )
}

/// Searches feed rows selected by `medium`, with optional track rows.
///
/// # Errors
///
/// Returns [`DbError`] if the FTS5 MATCH query fails.
#[expect(
    clippy::too_many_arguments,
    reason = "Search inputs map directly to the existing query controls."
)]
pub(crate) fn search_filtered_with_medium(
    conn: &Connection,
    query: &str,
    filters: &SearchFilters,
    entity_type_filter: Option<&str>,
    medium: FeedMediumScope,
    limit: i64,
    cursor_rank: Option<f64>,
    cursor_rowid: Option<i64>,
) -> Result<Vec<SearchResult>, DbError> {
    let scope = match entity_type_filter {
        Some("feed") => RankScope::FeedMedium(medium),
        None => RankScope::FeedsAndTracksMedium(medium),
        Some(other) => RankScope::One(other),
    };
    rank_search(
        conn,
        query,
        filters,
        scope,
        limit,
        cursor_rank,
        cursor_rowid,
    )
}

/// Searches the feed rows and the track rows with one statement, for a request
/// with no `type`. See [`search_filtered`].
///
/// A filter reads each row as ADR 0084 §6 gives for a search with no `type`.
///
/// # Errors
///
/// Returns [`DbError`] if the FTS5 MATCH query fails.
pub fn search_feeds_and_tracks(
    conn: &Connection,
    query: &str,
    filters: &SearchFilters,
    limit: i64,
    cursor_rank: Option<f64>,
    cursor_rowid: Option<i64>,
) -> Result<Vec<SearchResult>, DbError> {
    rank_search(
        conn,
        query,
        filters,
        RankScope::FeedsAndTracks,
        limit,
        cursor_rank,
        cursor_rowid,
    )
}

fn rank_search(
    conn: &Connection,
    query: &str,
    filters: &SearchFilters,
    scope: RankScope<'_>,
    limit: i64,
    cursor_rank: Option<f64>,
    cursor_rowid: Option<i64>,
) -> Result<Vec<SearchResult>, DbError> {
    let safe_query = fts5_query_from_words(query);
    tracing::info!(original_query = ?query, safe_query = ?safe_query, ?scope, "search called");
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
    // ADR 0087 §3: `CROSS JOIN` makes the hits the outer loop. The planner does
    // not know how many rows a materialized source holds, and it can read each
    // row of the type from `search_entities`.
    // `match_source` pushes the MATCH values first.  Each later value is pushed
    // to `values` in the same sequence as its `?N` in the SQL text, so the two
    // cannot drift apart.
    // ADR 0078 §4: a publisher feed has one quality row, of the type `feed`.
    // The join below reads that row for a `publisher` hit.  The select list,
    // the order and the keyset filter all use the same `q.score`.
    // ADR 0087 §2: the quality weight applies to the score and not to the
    // offset of the group.
    let eff_rank_expr =
        "(m.fts_rank * (1.0 + CAST(COALESCE(q.score, 0) AS REAL) / 100.0) + m.group_offset)";

    let mut values: Vec<Value> = Vec::new();
    let source = match_source(query, scope, &mut values);
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

    let filter_type = match scope {
        RankScope::All => "",
        RankScope::One(entity_type) => {
            values.push(Value::Text(entity_type.to_owned()));
            where_parts.push(format!("e.entity_type = ?{}", values.len()));
            entity_type
        }
        RankScope::FeedsAndTracks => {
            where_parts.push("e.entity_type IN ('feed', 'track')".to_owned());
            ""
        }
        RankScope::FeedMedium(feed_scope) => {
            where_parts.push(format!("e.entity_type IN {}", feed_scope.entity_types()));
            "feed"
        }
        RankScope::FeedsAndTracksMedium(feed_scope) => {
            where_parts.push(format!(
                "(e.entity_type IN {} OR e.entity_type = 'track')",
                feed_scope.entity_types()
            ));
            ""
        }
    };

    // ADR 0080 §2 and §3: a text filter is a condition on the index row, never
    // part of the `MATCH` above.  Each value is a bound parameter.
    TextFilter::push_conditions(
        filters,
        filter_type,
        "m.rowid",
        &mut values,
        &mut where_parts,
    );

    // ADR 0080 §2: a value filter is a condition on the feed or the track of
    // the hit.  Each value is a bound parameter.
    let vf = &filters.values;
    vf.push_conditions(&mut values, &mut where_parts);

    values.push(Value::Integer(limit));
    let limit_param_idx = values.len();

    let where_clause = if where_parts.is_empty() {
        String::new()
    } else {
        format!("WHERE {} ", where_parts.join(" AND "))
    };

    // ADR 0080 §2: join the feed and the track only for a filter that reads
    // them.
    let value_joins = vf.joins(false);

    let sql = format!(
        "SELECT e.entity_type, e.entity_id, m.fts_rank, \
                COALESCE(q.score, 0) AS quality_score, \
                {eff_rank_expr} AS eff_rank, \
                m.rowid \
         FROM ({source}) m \
         CROSS JOIN search_entities e ON e.rowid = m.rowid \
         LEFT JOIN entity_quality q \
           ON q.entity_type = CASE e.entity_type WHEN 'publisher' THEN 'feed' WHEN 'playlist' THEN 'feed' ELSE e.entity_type END \
          AND q.entity_id = e.entity_id \
         {value_joins}\
         {where_clause}\
         ORDER BY eff_rank ASC, m.rowid ASC \
         LIMIT ?{limit_param_idx}"
    );

    let mut stmt = conn.prepare_cached(&sql)?;

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

/// A row of a search with no `q`, with its date (ADR 0080 §6).
#[derive(Debug)]
pub struct DatedSearchResult {
    /// The row. `rank` and `effective_rank` are 0.
    pub result: SearchResult,
    /// `newest_item_at` of a feed or a publisher, `pub_date` of a track.
    pub date: Option<i64>,
}

/// Searches the index rows with filters and no `q`, newest first (ADR 0080 §6).
///
/// The order is `date DESC NULLS LAST, rowid ASC`. `entity_type_filter` of
/// `None` reads feed and track rows in one query.
///
/// `cursor` is the position of the last row of the previous page: its date
/// and its rowid. The page starts after that position.
///
/// # Errors
///
/// Returns [`DbError`] if the query fails.
pub fn search_by_date(
    conn: &Connection,
    filters: &SearchFilters,
    entity_type_filter: Option<&str>,
    limit: i64,
    cursor: Option<(Option<i64>, i64)>,
) -> Result<Vec<DatedSearchResult>, DbError> {
    search_by_date_with_medium(conn, filters, entity_type_filter, None, limit, cursor)
}

/// ADR 0094 §1: the condition on the row type of a search with no `q` or with
/// a `sort`. `medium` selects the feed row types of `type=feed` and of a
/// request with no `type`.
fn row_type_condition(
    entity_type_filter: Option<&str>,
    medium: Option<FeedMediumScope>,
    values: &mut Vec<Value>,
) -> String {
    match (entity_type_filter, medium) {
        (Some("feed"), Some(feed_scope)) => {
            format!("e.entity_type IN {}", feed_scope.entity_types())
        }
        (None, Some(feed_scope)) => format!(
            "(e.entity_type IN {} OR e.entity_type = 'track')",
            feed_scope.entity_types()
        ),
        (Some(entity_type), _) => {
            values.push(Value::Text(entity_type.to_owned()));
            format!("e.entity_type = ?{}", values.len())
        }
        (None, None) => "e.entity_type IN ('feed', 'track')".to_owned(),
    }
}

/// Searches dated rows with an optional feed medium scope.
///
/// # Errors
///
/// Returns [`DbError`] if the query fails.
pub(crate) fn search_by_date_with_medium(
    conn: &Connection,
    filters: &SearchFilters,
    entity_type_filter: Option<&str>,
    medium: Option<FeedMediumScope>,
    limit: i64,
    cursor: Option<(Option<i64>, i64)>,
) -> Result<Vec<DatedSearchResult>, DbError> {
    tracing::info!(entity_type_filter = ?entity_type_filter, "search with no q called");
    let date_expr = "CASE e.entity_type WHEN 'track' THEN t.pub_date ELSE f.newest_item_at END";
    let mut values: Vec<Value> = Vec::new();
    let mut where_parts = Vec::new();

    where_parts.push(row_type_condition(entity_type_filter, medium, &mut values));

    // A row with no date comes after each dated row, so a cursor with a date
    // also admits each row with no date.
    match cursor {
        Some((Some(date), rowid)) => {
            values.push(Value::Integer(date));
            let p1 = values.len();
            values.push(Value::Integer(rowid));
            let p2 = values.len();
            where_parts.push(format!(
                "({date_expr} < ?{p1} OR {date_expr} IS NULL \
                  OR ({date_expr} = ?{p1} AND e.rowid > ?{p2}))"
            ));
        }
        Some((None, rowid)) => {
            values.push(Value::Integer(rowid));
            where_parts.push(format!(
                "({date_expr} IS NULL AND e.rowid > ?{})",
                values.len()
            ));
        }
        None => {}
    }

    TextFilter::push_conditions(
        filters,
        entity_type_filter.unwrap_or(""),
        "e.rowid",
        &mut values,
        &mut where_parts,
    );
    filters
        .values
        .push_conditions(&mut values, &mut where_parts);

    values.push(Value::Integer(limit));
    let limit_param_idx = values.len();

    // The date order reads the feed and the track of each hit.
    let value_joins = filters.values.joins(true);
    let sql = format!(
        "SELECT e.entity_type, e.entity_id, COALESCE(q.score, 0) AS quality_score, \
                {date_expr} AS row_date, e.rowid \
         FROM search_entities e \
         LEFT JOIN entity_quality q \
           ON q.entity_type = CASE e.entity_type WHEN 'publisher' THEN 'feed' WHEN 'playlist' THEN 'feed' ELSE e.entity_type END \
          AND q.entity_id = e.entity_id \
         {value_joins}\
         WHERE {} \
         ORDER BY row_date DESC NULLS LAST, e.rowid ASC \
         LIMIT ?{limit_param_idx}",
        where_parts.join(" AND ")
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(values), |row| {
            Ok(DatedSearchResult {
                result: SearchResult {
                    entity_type: row.get(0)?,
                    entity_id: row.get(1)?,
                    rank: 0.0,
                    quality_score: row.get(2)?,
                    effective_rank: 0.0,
                    rowid: row.get(4)?,
                },
                date: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// A sort that has its own query form (ADR 0084 §2): `name` or `date`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySort {
    /// `feeds.title_sort` of a feed and a publisher, `tracks.title_sort` of a
    /// track. The usual direction is A to Z.
    Name,
    /// `newest_item_at` of a feed and a publisher, `pub_date` of a track. The
    /// usual direction is newest first.
    Date,
}

/// The value of the sort key of one row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SortValue {
    /// A sort key of `KeySort::Name`.
    Text(String),
    /// A date of `KeySort::Date`, as Unix seconds.
    Date(i64),
}

/// The position of the last row of a page, in a search with a [`KeySort`].
///
/// `key` is `None` for a row with no key. The state "no key" is explicit, so
/// that no value of a key can stand for it (ADR 0084 §6 and §9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyCursor {
    pub key: Option<SortValue>,
    pub rowid: i64,
}

/// A row of a sorted search, with its sort key. `key` is `None` for a row
/// with no key, and for a sort that gives no key (`relevance`).
#[derive(Debug)]
pub struct KeyedSearchResult {
    pub result: SearchResult,
    pub key: Option<SortValue>,
}

/// Pushes the keyset condition of a [`KeyCursor`] and its bound values.
///
/// A row with no key comes after each row with a key, so a cursor with a key
/// also admits each row with no key. A comparison with a null key is never
/// true, so the rows with no key have their own terms. `ops` is the key
/// expression, the key operator and the `rowid` operator.
fn push_key_cursor(
    cursor: &KeyCursor,
    ops: (&str, &str, &str),
    values: &mut Vec<Value>,
    where_parts: &mut Vec<String>,
) {
    let (key_expr, key_op, row_op) = ops;
    if let Some(key) = &cursor.key {
        values.push(match key {
            SortValue::Text(text) => Value::Text(text.clone()),
            SortValue::Date(date) => Value::Integer(*date),
        });
        let p1 = values.len();
        values.push(Value::Integer(cursor.rowid));
        let p2 = values.len();
        where_parts.push(format!(
            "({key_expr} {key_op} ?{p1} OR {key_expr} IS NULL \
              OR ({key_expr} = ?{p1} AND e.rowid {row_op} ?{p2}))"
        ));
    } else {
        values.push(Value::Integer(cursor.rowid));
        where_parts.push(format!(
            "({key_expr} IS NULL AND e.rowid {row_op} ?{})",
            values.len()
        ));
    }
}

/// Searches the index rows by name or by date (ADR 0084 §2 and §3).
///
/// This is one query form for both sorts and both directions. `query` can
/// have no word. The rows are then the rows of the filters, as in
/// [`search_by_date`], with `rank` 0. With a word, `rank` is the BM25 score,
/// as in [`search_filtered`]. A filter selects rows and never changes `rank`.
///
/// The order is the key, with a row that has no key last in both directions,
/// and then `rowid`. `reverse` reverses the key and the tie key `rowid`
/// (ADR 0084 §6). `entity_type_filter` of `None` reads feed and track rows in
/// one query, so each row has one `rowid` and no row repeats between pages.
///
/// `cursor` is the position of the last row of the previous page.
///
/// # Errors
///
/// Returns [`DbError`] if the query fails.
pub fn search_sorted(
    conn: &Connection,
    query: &str,
    filters: &SearchFilters,
    entity_type_filter: Option<&str>,
    order: (KeySort, bool),
    limit: i64,
    cursor: Option<&KeyCursor>,
) -> Result<Vec<KeyedSearchResult>, DbError> {
    search_sorted_with_medium(
        conn,
        query,
        filters,
        entity_type_filter,
        None,
        order,
        limit,
        cursor,
    )
}

/// Searches rows by name or date with an optional feed medium scope.
///
/// # Errors
///
/// Returns [`DbError`] if the query fails.
#[expect(
    clippy::too_many_arguments,
    reason = "The query and its independently optional controls are separate values."
)]
pub(crate) fn search_sorted_with_medium(
    conn: &Connection,
    query: &str,
    filters: &SearchFilters,
    entity_type_filter: Option<&str>,
    medium: Option<FeedMediumScope>,
    order: (KeySort, bool),
    limit: i64,
    cursor: Option<&KeyCursor>,
) -> Result<Vec<KeyedSearchResult>, DbError> {
    let (sort, reverse) = order;
    tracing::info!(entity_type_filter = ?entity_type_filter, ?sort, reverse, "sorted search called");
    let has_word = !fts5_query_from_words(query).trim().is_empty();

    let key_expr = match sort {
        KeySort::Name => "CASE e.entity_type WHEN 'track' THEN t.title_sort ELSE f.title_sort END",
        KeySort::Date => {
            "CASE e.entity_type WHEN 'track' THEN t.pub_date ELSE f.newest_item_at END"
        }
    };
    // The key of a name goes up in the usual direction, and the key of a date
    // goes down. `rowid` goes up in the usual direction.
    let key_descends = (sort == KeySort::Date) != reverse;
    let (key_dir, key_op) = if key_descends {
        ("DESC", "<")
    } else {
        ("ASC", ">")
    };
    let (row_dir, row_op) = if reverse { ("DESC", "<") } else { ("ASC", ">") };

    let mut values: Vec<Value> = Vec::new();
    let (source, rank_expr, own_row) = if has_word {
        let source = hit_source(query, &mut values);
        (
            format!("({source}) m JOIN search_entities e ON e.rowid = m.rowid"),
            "m.fts_rank",
            "m.rowid",
        )
    } else {
        ("search_entities e".to_owned(), "0.0", "e.rowid")
    };

    let mut where_parts = Vec::new();
    where_parts.push(row_type_condition(entity_type_filter, medium, &mut values));

    if let Some(cursor) = cursor {
        push_key_cursor(
            cursor,
            (key_expr, key_op, row_op),
            &mut values,
            &mut where_parts,
        );
    }

    TextFilter::push_conditions(
        filters,
        entity_type_filter.unwrap_or(""),
        own_row,
        &mut values,
        &mut where_parts,
    );
    filters
        .values
        .push_conditions(&mut values, &mut where_parts);

    values.push(Value::Integer(limit));
    let limit_param_idx = values.len();

    // The key reads the feed and the track of each hit.
    let value_joins = filters.values.joins(true);
    let sql = format!(
        "SELECT e.entity_type, e.entity_id, {rank_expr} AS fts_rank, \
                COALESCE(q.score, 0) AS quality_score, \
                {key_expr} AS row_key, e.rowid \
         FROM {source} \
         LEFT JOIN entity_quality q \
           ON q.entity_type = CASE e.entity_type WHEN 'publisher' THEN 'feed' WHEN 'playlist' THEN 'feed' ELSE e.entity_type END \
          AND q.entity_id = e.entity_id \
         {value_joins}\
         WHERE {} \
         ORDER BY row_key {key_dir} NULLS LAST, e.rowid {row_dir} \
         LIMIT ?{limit_param_idx}",
        where_parts.join(" AND ")
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(values), |row| {
            let rank: f64 = row.get(2)?;
            let key = match row.get::<_, Value>(4)? {
                Value::Text(text) => Some(SortValue::Text(text)),
                Value::Integer(date) => Some(SortValue::Date(date)),
                _ => None,
            };
            Ok(KeyedSearchResult {
                result: SearchResult {
                    entity_type: row.get(0)?,
                    entity_id: row.get(1)?,
                    rank,
                    quality_score: row.get(3)?,
                    effective_rank: rank,
                    rowid: row.get(5)?,
                },
                key,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
