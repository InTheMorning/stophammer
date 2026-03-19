use stophammer::db;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut db_path = String::from("stophammer.db");
    let mut command: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                db_path = args
                    .next()
                    .ok_or_else(|| "--db requires a path".to_string())?;
            }
            "status" | "import-active" | "import-idle" => {
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
                     Usage: resolverctl [--db PATH] <status|import-active|import-idle>\n\
                     \n\
                     Commands:\n\
                       status         Print queue counts and import pause state\n\
                       import-active  Set resolver_state.import_active=true and refresh heartbeat\n\
                       import-idle    Set resolver_state.import_active=false"
                )
                .into());
            }
        }
    }

    let command = command.unwrap_or_else(|| "status".to_string());
    let conn = db::open_db(&db_path);

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
        _ => unreachable!("validated above"),
    }

    Ok(())
}

fn print_usage() {
    println!(
        "Usage: resolverctl [--db PATH] <status|import-active|import-idle>\n\
         \n\
         Commands:\n\
           status         Print queue counts and import pause state\n\
           import-active  Set resolver_state.import_active=true and refresh heartbeat\n\
           import-idle    Set resolver_state.import_active=false"
    );
}

fn print_status(conn: &rusqlite::Connection) -> Result<(), db::DbError> {
    let counts = db::get_resolver_queue_counts(conn)?;
    let import_state = db::resolver_import_state(conn)?;

    println!("import_active={}", import_state.active);
    println!("import_stale={}", import_state.stale);
    match import_state.heartbeat_at {
        Some(ts) => println!("import_heartbeat_at={ts}"),
        None => println!("import_heartbeat_at="),
    }
    println!("queue_total={}", counts.total);
    println!("queue_ready={}", counts.ready);
    println!("queue_locked={}", counts.locked);
    println!("queue_failed={}", counts.failed);

    Ok(())
}
