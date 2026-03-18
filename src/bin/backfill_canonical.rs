use std::path::PathBuf;

fn parse_args() -> Result<(PathBuf, Option<usize>), String> {
    let mut db_path = PathBuf::from("./stophammer.db");
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
                    .map_err(|_| format!("invalid --limit value: {value}"))?;
                limit = Some(parsed);
            }
            "--help" | "-h" => {
                println!(
                    "Usage: backfill_canonical [--db PATH] [--limit N]\n\
                     Rebuilds canonical release/recording rows and high-confidence promotions\n\
                     for feeds already stored in the stophammer SQLite database."
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok((db_path, limit))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (db_path, limit) = parse_args().map_err(std::io::Error::other)?;
    let mut conn = stophammer::db::open_db(&db_path);

    let feed_guids: Vec<String> = {
        let sql = if limit.is_some() {
            "SELECT feed_guid FROM feeds ORDER BY created_at, feed_guid LIMIT ?1"
        } else {
            "SELECT feed_guid FROM feeds ORDER BY created_at, feed_guid"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = if let Some(limit) = limit {
            stmt.query_map([limit as i64], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?
        } else {
            stmt.query_map([], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?
        };
        rows
    };

    println!(
        "backfill_canonical: rebuilding {} feeds from {}",
        feed_guids.len(),
        db_path.display()
    );

    for (idx, feed_guid) in feed_guids.iter().enumerate() {
        let tx = conn.transaction()?;
        stophammer::db::sync_canonical_state_for_feed(&tx, feed_guid)
            .map_err(|err| std::io::Error::other(format!("feed {feed_guid}: {err}")))?;
        stophammer::db::sync_canonical_promotions_for_feed(&tx, feed_guid)
            .map_err(|err| std::io::Error::other(format!("feed {feed_guid}: {err}")))?;
        tx.commit()
            .map_err(|err| std::io::Error::other(format!("feed {feed_guid}: {err}")))?;

        if (idx + 1) % 100 == 0 || idx + 1 == feed_guids.len() {
            println!(
                "backfill_canonical: processed {}/{} feeds",
                idx + 1,
                feed_guids.len()
            );
        }
    }

    println!("backfill_canonical: complete");
    Ok(())
}
