// Issue-WAL-POOL — 2026-03-14

//! SQLite WAL connection pool.
//!
//! Provides a writer (single connection under `Mutex`) and a bounded reader
//! pool (`r2d2`) so WAL concurrency materialises at the application level.
//!
//! Clone is cheap — both fields are `Arc`-wrapped.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

use crate::db::DbError;

/// Abstraction over a borrowed connection that can come from either the
/// reader pool or the writer mutex (test-util fallback).
// Issue-WAL-POOL — 2026-03-14
// CRIT-03 Debug — 2026-03-13
pub enum ReadConn<'a> {
    /// A pooled reader connection from the r2d2 pool.
    Pooled(r2d2::PooledConnection<SqliteConnectionManager>),
    /// A reference through the writer mutex (used in test-util mode).
    Writer(MutexGuard<'a, Connection>),
}

impl std::fmt::Debug for ReadConn<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pooled(_) => f.write_str("ReadConn::Pooled(..)"),
            Self::Writer(_) => f.write_str("ReadConn::Writer(..)"),
        }
    }
}

impl std::ops::Deref for ReadConn<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        match self {
            Self::Pooled(c) => c,
            Self::Writer(g) => g,
        }
    }
}

/// A cloneable handle to the WAL-mode connection pool.
///
/// The writer is a single `Mutex<Connection>` (SQLite allows only one
/// concurrent writer). The reader pool is bounded at 8 connections with
/// `PRAGMA query_only = ON` enforced per connection.
#[derive(Clone)]
pub struct DbPool {
    /// Single write connection. SQLite allows only one concurrent writer.
    writer: Arc<Mutex<Connection>>,
    /// Bounded pool of read connections (WAL snapshot isolation).
    /// `None` in test-util writer-only mode.
    readers: Option<Pool<SqliteConnectionManager>>,
}

impl std::fmt::Debug for DbPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbPool")
            .field("has_reader_pool", &self.readers.is_some())
            .finish_non_exhaustive()
    }
}

impl DbPool {
    /// Opens `path` in WAL mode, runs migrations on the writer, and creates
    /// a bounded reader pool.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the database file cannot be opened, WAL mode
    /// cannot be set, or the reader pool fails to initialise.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        // Writer connection — opens and migrates via the existing helper.
        let writer = crate::db::open_db(path);

        // Reader pool — r2d2_sqlite opens separate connections.
        let manager = SqliteConnectionManager::file(path)
            .with_init(|conn| {
                conn.execute_batch(
                    "PRAGMA journal_mode=WAL;\
                     PRAGMA synchronous=NORMAL;\
                     PRAGMA foreign_keys=ON;\
                     PRAGMA query_only=ON;"
                )?;
                Ok(())
            });
        let readers = Pool::builder()
            .max_size(8)
            .connection_timeout(Duration::from_secs(5))
            .build(manager)
            .map_err(|e| DbError::Other(format!("reader pool error: {e}")))?;

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            readers: Some(readers),
        })
    }

    /// Creates a `DbPool` from an existing `Arc<Mutex<Connection>>`.
    ///
    /// Both reads and writes go through the single writer connection.
    /// This is intended **only for tests** that use in-memory databases
    /// (where separate reader connections would each see a different DB).
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn from_writer_only(writer: Arc<Mutex<Connection>>) -> Self {
        Self {
            writer,
            readers: None,
        }
    }

    /// Returns a reference to the writer mutex.
    ///
    /// Callers should `.lock()` to obtain a `MutexGuard<Connection>` for
    /// write operations.
    #[must_use]
    pub fn writer(&self) -> &Arc<Mutex<Connection>> {
        &self.writer
    }

    /// Returns a connection for read-only queries.
    ///
    /// In production mode this checks out a connection from the reader pool.
    /// In test-util writer-only mode this locks the writer.
    ///
    /// # Errors
    ///
    /// Returns `DbError::Other` if the pool is exhausted, or `DbError::Poisoned`
    /// if the writer mutex is poisoned (test-util mode).
    pub fn reader(&self) -> Result<ReadConn<'_>, DbError> {
        if let Some(pool) = &self.readers {
            pool.get()
                .map(ReadConn::Pooled)
                .map_err(|e| DbError::Other(format!("reader pool error: {e}")))
        } else {
            self.writer
                .lock()
                .map(ReadConn::Writer)
                .map_err(|_| DbError::Poisoned)
        }
    }
}
