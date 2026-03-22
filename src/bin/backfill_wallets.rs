use std::path::PathBuf;

struct Args {
    db_path: PathBuf,
}

fn parse_args() -> Result<Args, String> {
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
                    "Usage: backfill_wallets [--db PATH]\n\n\
                     Multi-pass wallet identity backfill:\n\
                     Pass 1: Normalize endpoint facts from all payment routes\n\
                     Pass 2: Create provisional wallets + hard-signal classification\n\
                     Pass 3: Same-feed artist evidence linking\n\
                     Pass 4: Orphan cleanup"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args { db_path })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(std::io::Error::other)?;
    let conn = stophammer::db::open_db(&args.db_path);

    println!("Pass 1: normalizing endpoint facts...");
    let s1 = stophammer::db::backfill_wallet_pass1(&conn)?;
    println!(
        "  endpoints: {} created, {} existing",
        s1.endpoints_created, s1.endpoints_existing
    );
    println!(
        "  aliases: {}, track_maps: {}, feed_maps: {}",
        s1.aliases_created, s1.track_maps_created, s1.feed_maps_created
    );

    println!("Pass 2: creating provisional wallets + hard classification...");
    let s2 = stophammer::db::backfill_wallet_pass2(&conn)?;
    println!(
        "  wallets: {} created, {} hard-classified",
        s2.wallets_created, s2.hard_classified
    );

    println!("Pass 3: same-feed artist evidence linking...");
    let s3 = stophammer::db::backfill_wallet_pass3(&conn)?;
    println!("  artist_links: {} created", s3.artist_links_created);

    println!("Pass 4: orphan cleanup...");
    let s4 = stophammer::db::cleanup_orphaned_wallets(&conn)?;
    println!("  orphaned wallets deleted: {}", s4.wallets_deleted);

    println!("Done. Database: {}", args.db_path.display());
    Ok(())
}
