use stophammer::{db_pool, resolver};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "stophammer=info".parse().expect("valid filter")),
        )
        .init();

    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| "stophammer.db".into());
    let interval_secs: u64 = std::env::var("RESOLVER_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let batch_size: i64 = std::env::var("RESOLVER_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(25);
    let worker_id = std::env::var("RESOLVER_WORKER_ID")
        .unwrap_or_else(|_| format!("resolverd-{}", std::process::id()));

    let pool = db_pool::DbPool::open(std::path::Path::new(&db_path))
        .expect("failed to open database pool");

    tracing::info!(
        db_path,
        interval_secs,
        batch_size,
        worker_id,
        "resolver: starting phase 1 worker"
    );

    resolver::worker::run_forever(pool, interval_secs, batch_size, worker_id).await;
}
