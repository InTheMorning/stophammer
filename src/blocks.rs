// Rust guideline compliant (M-MODULE-DOCS) — 2026-09-25

//! Durable feed blocks: environment seed (ADR 0053 section 2).
//!
//! At primary startup, `seed_blocks` reads `BLOCKED_FEED_GUIDS` and
//! `BLOCKED_FEED_URLS` through [`blocks_from_env`] and writes a `feed_blocks`
//! row and a signed `FeedBlocked` event for each value with no row yet. A
//! value removed from the environment removes no block; this module never
//! deletes a row. `src/verifiers/feed_blocklist.rs` is gone: [`build_chain`]
//! skips that chain name with a warning, and this module is the only reader
//! of `BLOCKED_FEED_GUIDS` and `BLOCKED_FEED_URLS` left in the crate.
//!
//! [`build_chain`]: crate::verify::build_chain

use rusqlite::Connection;

use crate::db::{self, DbError, FeedBlock, FeedBlockKind};
use crate::event::{EventType, FeedBlockedPayload};
use crate::signing::NodeSigner;

/// The reason recorded on a block row this module writes.
const SEEDED_REASON: &str = "seeded from environment";

/// Reads `BLOCKED_FEED_GUIDS` and `BLOCKED_FEED_URLS` from the environment.
///
/// Each variable is a comma-separated list. A missing or empty variable
/// gives no entries for its kind. Each value is trimmed; an empty value
/// after the trim is dropped. The value keeps its original case here;
/// [`seed_blocks`] normalizes it with [`FeedBlockKind::normalize`] before it
/// writes a row.
#[must_use]
pub fn blocks_from_env() -> Vec<(FeedBlockKind, String)> {
    let mut entries: Vec<(FeedBlockKind, String)> = read_csv_env("BLOCKED_FEED_GUIDS")
        .into_iter()
        .map(|value| (FeedBlockKind::Guid, value))
        .collect();
    entries.extend(
        read_csv_env("BLOCKED_FEED_URLS")
            .into_iter()
            .map(|value| (FeedBlockKind::Url, value)),
    );
    entries
}

/// Splits `var` on commas, trims each part, and drops an empty part.
fn read_csv_env(var: &str) -> Vec<String> {
    std::env::var(var)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

/// Inserts a `feed_blocks` row and signs one `FeedBlocked` event for each
/// entry of `entries` whose kind and normalized value has no row yet.
///
/// Runs in one transaction. A new row gets a new UUID v4 `block_id` and the
/// reason `"seeded from environment"`. An entry whose pair already has a
/// row, under any reason, writes nothing and signs no event. This function
/// never deletes a row.
///
/// Returns the number of new rows.
///
/// ADR 0053 section 2.
///
/// # Errors
///
/// Returns [`DbError`] if a write, the JSON encoding of an event payload, or
/// the transaction commit fails.
pub fn seed_blocks(
    conn: &mut Connection,
    entries: &[(FeedBlockKind, String)],
    signer: &NodeSigner,
    now: i64,
) -> Result<usize, DbError> {
    let tx = conn.transaction()?;
    let mut written = 0usize;

    for (kind, value) in entries {
        let block = FeedBlock {
            block_id: uuid::Uuid::new_v4().to_string(),
            kind: *kind,
            value: kind.normalize(value),
            reason: SEEDED_REASON.to_string(),
            blocked_at: now,
        };
        if !db::insert_feed_block(&tx, &block)? {
            continue;
        }

        let payload = FeedBlockedPayload {
            block_id: block.block_id.clone(),
            kind: block.kind,
            value: block.value.clone(),
            reason: block.reason.clone(),
            blocked_at: block.blocked_at,
        };
        let payload_json = serde_json::to_string(&payload)?;
        let event_id = uuid::Uuid::new_v4().to_string();
        db::insert_event(
            &tx,
            &event_id,
            &EventType::FeedBlocked,
            &payload_json,
            &block.block_id,
            signer,
            now,
            &[],
        )?;
        written += 1;
    }

    tx.commit()?;
    Ok(written)
}
