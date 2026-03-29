use std::path::PathBuf;

use stophammer::db;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut db_path = PathBuf::from(db::DEFAULT_DB_PATH);
    let mut command: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                db_path = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--db requires a path".to_string())?,
                );
            }
            "status" | "import-active" | "import-idle" | "re-resolve" => {
                if command.replace(arg).is_some() {
                    return Err("only one resolverctl command may be specified".into());
                }
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            other => {
                return Err(format!(
                    "unknown argument: {other}\n\n\
                     Usage: resolverctl [--db PATH] <command>\n\
                     \n\
                     Commands:\n\
                       status         Print queue counts plus import/backfill pause state\n\
                       import-active  Set resolver_state.import_active=true and refresh heartbeat\n\
                       import-idle    Set resolver_state.import_active=false\n\
                       re-resolve     Wipe all resolved state and re-queue every feed"
                )
                .into());
            }
        }
    }

    let command = command.unwrap_or_else(|| "status".to_string());
    let mut conn = db::open_db(&db_path);

    match command.as_str() {
        "status" => print_status(&conn)?,
        "import-active" => {
            db::set_resolver_import_active(&conn, true)?;
            println!("resolver_state.import_active=true");
        }
        "import-idle" => {
            db::set_resolver_import_active(&conn, false)?;
            println!("resolver_state.import_active=false");
        }
        "re-resolve" => {
            let queued = db::reset_resolved_state(&mut conn)?;
            println!("wiped all resolved state, queued {queued} feeds for re-resolution");
        }
        _ => unreachable!("validated above"),
    }

    Ok(())
}

fn print_usage() {
    println!(
        "Usage: resolverctl [--db PATH] <command>\n\
         \n\
         Commands:\n\
           status         Print queue counts plus import/backfill pause state\n\
           import-active  Set resolver_state.import_active=true and refresh heartbeat\n\
           import-idle    Set resolver_state.import_active=false\n\
           re-resolve     Wipe all resolved state and re-queue every feed"
    );
}

fn print_status(conn: &rusqlite::Connection) -> Result<(), db::DbError> {
    let counts = db::get_resolver_queue_counts(conn)?;
    let import_state = db::resolver_import_state(conn)?;
    let backfill_state = db::resolver_backfill_state(conn)?;

    println!("import_active={}", import_state.active);
    println!("import_stale={}", import_state.stale);
    match import_state.heartbeat_at {
        Some(ts) => println!("import_heartbeat_at={ts}"),
        None => println!("import_heartbeat_at="),
    }
    println!("backfill_active={}", backfill_state.active);
    println!("backfill_stale={}", backfill_state.stale);
    match backfill_state.heartbeat_at {
        Some(ts) => println!("backfill_heartbeat_at={ts}"),
        None => println!("backfill_heartbeat_at="),
    }
    println!("queue_total={}", counts.total);
    println!("queue_ready={}", counts.ready);
    println!("queue_locked={}", counts.locked);
    println!("queue_failed={}", counts.failed);

    let artist_reviews = db::count_pending_artist_identity_reviews(&conn)?;
    let wallet_reviews = db::count_pending_wallet_reviews(&conn)?;
    println!("review_artist_identity_pending={artist_reviews}");
    println!("review_wallet_pending={wallet_reviews}");

    Ok(())
}
