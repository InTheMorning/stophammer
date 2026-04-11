//! One-shot search-index and quality-score rebuild for existing databases.
//!
//! Run this once after upgrading from a version where the now-retired resolver
//! was responsible for populating search/quality data.  Every feed and all its
//! tracks are processed; the operation is safe to re-run (upsert semantics).
//!
//! Usage:
//!   `rebuild_search <path/to/stophammer.db>`

use std::path::PathBuf;
use std::process;

fn main() {
    let Some(p) = std::env::args().nth(1) else {
        eprintln!("usage: rebuild_search <path/to/stophammer.db>");
        process::exit(1);
    };
    let db_path = PathBuf::from(p);

    let conn = rusqlite::Connection::open(&db_path).unwrap_or_else(|e| {
        eprintln!("error: could not open {}: {e}", db_path.display());
        process::exit(1);
    });

    let guids = stophammer::db::list_all_feed_guids(&conn).unwrap_or_else(|e| {
        eprintln!("error: failed to list feeds: {e}");
        process::exit(1);
    });

    let total = guids.len();
    println!("rebuilding search index and quality scores for {total} feeds…");

    let mut ok = 0usize;
    let mut failed = 0usize;

    for guid in &guids {
        if let Err(e) = stophammer::db::sync_source_read_models_for_feed(&conn, guid) {
            eprintln!("  error: feed {guid}: {e}");
            failed += 1;
        } else {
            ok += 1;
        }
    }

    println!("done: {ok} succeeded, {failed} failed");
    if failed > 0 {
        process::exit(1);
    }
}
