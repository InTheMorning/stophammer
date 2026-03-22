use std::path::PathBuf;

fn parse_args() -> Result<PathBuf, String> {
    let mut db_path = PathBuf::from("./stophammer.db");

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--db requires a path".to_string())?;
                db_path = PathBuf::from(value);
            }
            "--help" | "-h" => {
                println!(
                    "Usage: backfill_artist_identity [--db PATH]\n\
                     Deterministically merges split canonical artists when strong source evidence\n\
                     agrees across staged IDs, publisher links, websites, or release clusters."
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(db_path)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = parse_args().map_err(std::io::Error::other)?;
    let mut conn = stophammer::db::open_db(&db_path);
    let stats = stophammer::db::backfill_artist_identity(&mut conn)?;
    println!(
        "backfill_artist_identity: processed {} merge groups, applied {} merges in {}",
        stats.groups_processed,
        stats.merges_applied,
        db_path.display()
    );
    let orphan_stats = stophammer::db::cleanup_orphaned_artists(&mut conn)?;
    println!(
        "cleanup_orphaned_artists: deleted {} artists, {} credits in {}",
        orphan_stats.artists_deleted,
        orphan_stats.credits_deleted,
        db_path.display()
    );
    Ok(())
}
