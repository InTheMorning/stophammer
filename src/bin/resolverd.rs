use std::fs::File;
use std::process::ExitCode;
use std::sync::Arc;

use stophammer::{db, db_pool, resolver, signing};

fn parse_truthy_opt_out(value: Option<&str>) -> bool {
    !matches!(value, Some("0" | "false" | "no" | "off"))
}

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--help" || a == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }

    init_tracing();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("resolverd: {err}");
            ExitCode::FAILURE
        }
    }
}

type StartupError = Box<dyn std::error::Error + Send + Sync>;

fn startup_error(msg: impl Into<String>) -> StartupError {
    Box::new(std::io::Error::other(msg.into()))
}

const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

fn acquire_lock(path: &str) -> Result<File, StartupError> {
    use std::os::unix::io::AsRawFd;

    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|e| startup_error(format!("failed to open lock file {path}: {e}")))?;

    // SAFETY: file descriptor is valid and open.
    let ret = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };

    if ret == -1 {
        let err = std::io::Error::last_os_error();
        return Err(if err.kind() == std::io::ErrorKind::WouldBlock {
            startup_error("another resolverd instance is already running against this database")
        } else {
            startup_error(format!("failed to acquire lock on {path}: {err}"))
        });
    }

    // Write our PID for diagnostics.
    use std::io::Write;
    let _ = (&file).write_all(format!("{}\n", std::process::id()).as_bytes());

    Ok(file)
}

fn print_help() {
    println!(
        "\
resolverd — background resolver worker for stophammer

Periodically resolves pending canonical-URL claims by fetching feeds,
comparing content hashes, and emitting signed resolved-state events.

USAGE:
    resolverd [--help | -h]

ENVIRONMENT VARIABLES:
    DB_PATH                              Path to stophammer SQLite database
                                         [default: ./stophammer.db]
    RESOLVER_INTERVAL_SECS               Seconds between resolve batches
                                         [default: 30]
    RESOLVER_BATCH_SIZE                   Claims per batch
                                         [default: 25]
    RESOLVER_WORKER_ID                    Worker identifier for claim locking
                                         [default: resolverd-<pid>]
    RESOLVER_EMIT_RESOLVED_STATE_EVENTS   Emit signed resolved-state events
                                         [default: true] (set 0/false/no/off to disable)
    KEY_PATH                              Path to Ed25519 signing key
                                         [default: signing.key]
    RUST_LOG                              Tracing filter directive
                                         [default: stophammer=info]
    NODE_MODE                             Must not be \"community\" (resolverd is primary-only)"
    );
}

fn init_tracing() {
    let (env_filter, invalid_filter_error) =
        match tracing_subscriber::EnvFilter::try_from_default_env() {
            Ok(filter) => (filter, None),
            Err(err) => (
                tracing_subscriber::EnvFilter::new("stophammer=info"),
                Some(err),
            ),
        };
    tracing_subscriber::fmt().with_env_filter(env_filter).init();
    if let Some(err) = invalid_filter_error {
        tracing::warn!(error = %err, "invalid RUST_LOG, defaulting to stophammer=info");
    }
}

async fn run() -> Result<(), StartupError> {
    if matches!(
        std::env::var("NODE_MODE").ok().as_deref(),
        Some("community")
    ) {
        return Err(startup_error(
            "resolverd is primary-only; community nodes follow primary-authored resolved events",
        ));
    }

    let db_path =
        std::env::var("DB_PATH").unwrap_or_else(|_| stophammer::db::DEFAULT_DB_PATH.into());
    let interval_secs: u64 = std::env::var("RESOLVER_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    if interval_secs == 0 {
        return Err(startup_error(
            "RESOLVER_INTERVAL_SECS must be >= 1; got 0 (would cause a busy loop)",
        ));
    }
    let batch_size: i64 = std::env::var("RESOLVER_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(25);
    if batch_size < 1 {
        return Err(startup_error(format!(
            "RESOLVER_BATCH_SIZE must be >= 1; got {batch_size}"
        )));
    }
    let worker_id = std::env::var("RESOLVER_WORKER_ID")
        .unwrap_or_else(|_| format!("resolverd-{}", std::process::id()));
    let emit_resolved_state_events = parse_truthy_opt_out(
        std::env::var("RESOLVER_EMIT_RESOLVED_STATE_EVENTS")
            .ok()
            .as_deref(),
    );

    // Advisory file lock prevents multiple resolver instances from running
    // against the same database. The lock is held for the lifetime of the
    // process and released automatically by the OS on exit or crash.
    let lock_path = format!("{db_path}.resolverd.lock");
    let _lock_file = acquire_lock(&lock_path)?;

    let pool = db_pool::DbPool::open(std::path::Path::new(&db_path))
        .map_err(|err| startup_error(format!("failed to open database pool: {err}")))?;
    let signer = if emit_resolved_state_events {
        let key_path = std::env::var("KEY_PATH").unwrap_or_else(|_| "signing.key".into());
        Some(Arc::new(
            signing::NodeSigner::load_or_create(&key_path).map_err(|err| {
                startup_error(format!(
                    "failed to load signer for resolved-state event emission: {err}"
                ))
            })?,
        ))
    } else {
        None
    };

    println!(
        "resolverd: db={db_path} interval={interval_secs}s batch={batch_size} worker={worker_id} events={emit_resolved_state_events}"
    );

    // Show if there are review items awaiting operator action.
    {
        let reader = pool
            .reader()
            .map_err(|_| startup_error("failed to get reader lock for review count"))?;
        let artist_reviews = db::count_pending_artist_identity_reviews(&reader)
            .map_err(|e| startup_error(format!("failed to count pending artist reviews: {e}")))?;
        let wallet_reviews = db::count_pending_wallet_reviews(&reader)
            .map_err(|e| startup_error(format!("failed to count pending wallet reviews: {e}")))?;
        if artist_reviews > 0 || wallet_reviews > 0 {
            println!("resolverd: {artist_reviews} artist identity reviews pending, {wallet_reviews} wallet reviews pending");
        }
    }

    resolver::worker::run_forever(pool, interval_secs, batch_size, worker_id, signer).await;
    Ok(())
}
