use std::path::PathBuf;

use stophammer::db::DEFAULT_DB_PATH;

fn parse_args() -> Result<(PathBuf, Option<usize>), String> {
    let mut db_path = PathBuf::from(DEFAULT_DB_PATH);
    let mut limit = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--db requires a path".to_string())?;
                db_path = PathBuf::from(value);
            }
            "--limit" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--limit requires a number".to_string())?;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_err| format!("invalid --limit value: {value}"))?;
                limit = Some(parsed);
            }
            "--help" | "-h" => {
                println!(
                    "Usage: backfill_canonical [--db PATH] [--limit N]\n\
                     Rebuilds canonical release/recording rows and high-confidence promotions\n\
                     for feeds already stored in the stophammer SQLite database.\n\
                     Coordinates with stophammer-resolverd via resolver_state.backfill_active while it runs."
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok((db_path, limit))
}

/// Number of feed GUIDs fetched per database page. Bounds peak memory use to
/// roughly `PAGE_SIZE` × (average GUID length) rather than the full table.
const PAGE_SIZE: usize = 500;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (db_path, limit) = parse_args().map_err(std::io::Error::other)?;
    let _backfill_guard =
        stophammer::resolver_coordination::ResolverBackfillGuard::enter(&db_path)?;
    let mut conn = stophammer::db::open_db(&db_path);

    // Count how many feeds we will process (for progress output only).
    let total: usize = {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM feeds", [], |r| r.get(0))?;
        let count = usize::try_from(count).unwrap_or(usize::MAX);
        limit.map_or(count, |l| l.min(count))
    };

    println!(
        "backfill_canonical: rebuilding {} feeds from {}",
        total,
        db_path.display()
    );

    // Cursor-based pagination: advance `last_guid` after each page so only
    // PAGE_SIZE GUIDs are held in memory at a time.
    let mut processed = 0usize;
    let mut last_guid = String::new();

    if let Err(e) = conn.execute(
        "INSERT INTO search_index(search_index, rank) VALUES('automerge', 0)",
        [],
    ) {
        eprintln!("backfill_canonical: WARNING: failed to disable FTS5 automerge: {e}");
    }

    let backfill_result = (|| -> Result<(), Box<dyn std::error::Error>> {
        loop {
            let fetch = match limit {
                Some(l) => PAGE_SIZE.min(l.saturating_sub(processed)),
                None => PAGE_SIZE,
            };
            if fetch == 0 {
                break;
            }

            let page: Vec<String> = {
                let mut stmt = conn.prepare(
                    "SELECT feed_guid FROM feeds \
                     WHERE feed_guid > ?1 \
                     ORDER BY feed_guid \
                     LIMIT ?2",
                )?;
                let fetch_i64 = i64::try_from(fetch).map_err(|_err| {
                    rusqlite::Error::ToSqlConversionFailure(
                        "page size exceeded supported SQLite integer range"
                            .to_string()
                            .into(),
                    )
                })?;
                stmt.query_map(rusqlite::params![last_guid, fetch_i64], |row| row.get(0))?
                    .collect::<Result<Vec<String>, _>>()?
            };

            if page.is_empty() {
                break;
            }

            for feed_guid in &page {
                let tx = conn.transaction()?;
                stophammer::db::sync_canonical_state_for_feed(&tx, feed_guid)
                    .map_err(|err| std::io::Error::other(format!("feed {feed_guid}: {err}")))?;
                stophammer::db::sync_canonical_promotions_for_feed(&tx, feed_guid)
                    .map_err(|err| std::io::Error::other(format!("feed {feed_guid}: {err}")))?;
                stophammer::db::sync_canonical_search_index_for_feed(&tx, feed_guid)
                    .map_err(|err| std::io::Error::other(format!("feed {feed_guid}: {err}")))?;
                tx.commit()
                    .map_err(|err| std::io::Error::other(format!("feed {feed_guid}: {err}")))?;

                processed += 1;
                if processed.is_multiple_of(100) || processed == total {
                    println!("backfill_canonical: processed {processed}/{total} feeds");
                }
            }

            // Safe: loop only continues when page is non-empty.
            last_guid.clone_from(page.last().unwrap());

            if page.len() < fetch {
                // Last page — no more feeds in the table.
                break;
            }
        }
        Ok(())
    })();

    if let Err(e) = conn.execute(
        "INSERT INTO search_index(search_index, rank) VALUES('automerge', 8)",
        [],
    ) {
        eprintln!("backfill_canonical: WARNING: failed to re-enable FTS5 automerge: {e}");
    }
    if let Err(e) = conn.execute(
        "INSERT INTO search_index(search_index, rank) VALUES('merge', 500)",
        [],
    ) {
        eprintln!("backfill_canonical: WARNING: failed to run FTS5 merge pass: {e}");
    }

    backfill_result?;

    println!("backfill_canonical: complete");
    Ok(())
}
