use std::path::PathBuf;

use stophammer::db::DEFAULT_DB_PATH;

struct Args {
    db_path: PathBuf,
    refresh: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut db_path = PathBuf::from(DEFAULT_DB_PATH);
    let mut refresh = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--db requires a path".to_string())?;
                db_path = PathBuf::from(value);
            }
            "--refresh" => refresh = true,
            "--help" | "-h" => {
                println!(
                    "Usage: backfill_wallets [--db PATH] [--refresh]\n\n\
                     Default mode (Passes 1-4):\n\
                     Pass 1: Normalize endpoint facts from all payment routes\n\
                     Pass 2: Create provisional wallets + hard-signal classification\n\
                     Pass 3: Same-feed artist evidence linking\n\
                     Pass 4: Orphan cleanup\n\n\
                     --refresh mode (Pass 5):\n\
                     Group same-feed endpoints, re-derive display names,\n\
                     generate review items, orphan cleanup"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args { db_path, refresh })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(std::io::Error::other)?;
    let conn = stophammer::db::open_db(&args.db_path);

    println!("Purging Wavlake wallet entities...");
    let wavlake_purged = stophammer::db::purge_wavlake_wallet_route_maps(&conn)?;
    let wavlake_orphans = stophammer::db::cleanup_orphaned_wallets(&conn)?;
    println!(
        "  route maps removed: {}, orphaned wallets deleted: {}",
        wavlake_purged, wavlake_orphans.wallets_deleted
    );

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

    if args.refresh {
        println!("Pass 5: global refresh / owner grouping...");
        let s5 = stophammer::db::backfill_wallet_pass5(&conn)?;
        println!(
            "  feeds processed: {}, merges from overrides: {}, merges from grouping: {}",
            s5.feeds_processed, s5.merges_from_overrides, s5.merges_from_grouping
        );
        if let Some(batch_id) = s5.apply_batch_id {
            println!("  apply batch id: {batch_id}");
        }
        println!(
            "  soft-classified: {}, split-classified: {}",
            s5.soft_classified, s5.split_classified
        );
        println!(
            "  review items created: {}, orphans deleted: {}",
            s5.review_items_created, s5.orphans_deleted
        );
    }

    println!("Done. Database: {}", args.db_path.display());
    Ok(())
}
