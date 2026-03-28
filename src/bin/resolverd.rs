use std::sync::Arc;

use stophammer::{db_pool, resolver, signing};

fn parse_truthy_opt_out(value: Option<&str>) -> bool {
    !matches!(value, Some("0" | "false" | "no" | "off"))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "stophammer=info".parse().expect("valid filter")),
        )
        .init();

    if matches!(
        std::env::var("NODE_MODE").ok().as_deref(),
        Some("community")
    ) {
        tracing::error!(
            "resolverd is primary-only; community nodes follow primary-authored resolved events"
        );
        std::process::exit(2);
    }

    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| "stophammer.db".into());
    let interval_secs: u64 = std::env::var("RESOLVER_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    if interval_secs == 0 {
        tracing::error!("RESOLVER_INTERVAL_SECS must be >= 1; got 0 (would cause a busy loop)");
        std::process::exit(1);
    }
    let batch_size: i64 = std::env::var("RESOLVER_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(25);
    if batch_size < 1 {
        tracing::error!(
            batch_size,
            "RESOLVER_BATCH_SIZE must be >= 1; got {batch_size}"
        );
        std::process::exit(1);
    }
    let worker_id = std::env::var("RESOLVER_WORKER_ID")
        .unwrap_or_else(|_| format!("resolverd-{}", std::process::id()));
    let emit_resolved_state_events = parse_truthy_opt_out(
        std::env::var("RESOLVER_EMIT_RESOLVED_STATE_EVENTS")
            .ok()
            .as_deref(),
    );

    let pool = db_pool::DbPool::open(std::path::Path::new(&db_path))
        .expect("failed to open database pool");
    let signer = emit_resolved_state_events.then(|| {
        let key_path = std::env::var("KEY_PATH").unwrap_or_else(|_| "signing.key".into());
        Arc::new(
            signing::NodeSigner::load_or_create(&key_path)
                .expect("failed to load signer for resolved-state event emission"),
        )
    });

    tracing::info!(
        db_path,
        interval_secs,
        batch_size,
        worker_id,
        emit_resolved_state_events,
        "resolver: starting background worker"
    );

    resolver::worker::run_forever(pool, interval_secs, batch_size, worker_id, signer).await;
}
