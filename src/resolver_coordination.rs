//! Resolver coordination helpers shared by maintenance tools.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::db;

const BACKFILL_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);

/// Keeps `resolver_state.backfill_active` live for the lifetime of a backfill
/// process and clears it on drop.
#[derive(Debug)]
pub struct ResolverBackfillGuard {
    db_path: PathBuf,
    stop_tx: Option<mpsc::Sender<()>>,
    heartbeat_thread: Option<JoinHandle<()>>,
}

impl ResolverBackfillGuard {
    /// Enters coordinated backfill mode for `db_path`.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or another import/
    /// backfill is already actively holding the resolver pause state.
    pub fn enter(db_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let conn = db::open_db(db_path);
        let import_state = db::resolver_import_state(&conn)?;
        if import_state.active {
            return Err(std::io::Error::other(
                "resolver import coordination is already active; stop importer before running backfill",
            )
            .into());
        }
        let backfill_state = db::resolver_backfill_state(&conn)?;
        if backfill_state.active {
            return Err(std::io::Error::other(
                "resolver backfill coordination is already active; stop the other backfill before running another",
            )
            .into());
        }

        db::set_resolver_backfill_active(&conn, true)?;

        let db_path = db_path.to_path_buf();
        let (stop_tx, stop_rx) = mpsc::channel();
        let heartbeat_path = db_path.clone();
        let heartbeat_thread = thread::spawn(move || {
            loop {
                match stop_rx.recv_timeout(BACKFILL_HEARTBEAT_INTERVAL) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let conn = db::open_db(&heartbeat_path);
                        if let Err(err) = db::touch_resolver_backfill_active(&conn) {
                            tracing::warn!(
                                error = %err,
                                db_path = %heartbeat_path.display(),
                                "backfill: failed to refresh resolver heartbeat"
                            );
                        }
                    }
                }
            }
        });

        Ok(Self {
            db_path,
            stop_tx: Some(stop_tx),
            heartbeat_thread: Some(heartbeat_thread),
        })
    }
}

impl Drop for ResolverBackfillGuard {
    fn drop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(handle) = self.heartbeat_thread.take() {
            let _ = handle.join();
        }
        let conn = db::open_db(&self.db_path);
        if let Err(err) = db::set_resolver_backfill_active(&conn, false) {
            tracing::warn!(
                error = %err,
                db_path = %self.db_path.display(),
                "backfill: failed to clear resolver coordination state"
            );
        }
    }
}
