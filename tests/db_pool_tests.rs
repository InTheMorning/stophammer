// Issue-WAL-POOL — 2026-03-14
//
// Integration tests for the SQLite WAL connection pool.
//
// Verifies:
// 1. Concurrent reader access — multiple threads can read simultaneously.
// 2. WAL snapshot isolation — readers proceed while writer holds the lock.

mod common;

use std::sync::{Arc, Barrier};

/// Opens a file-backed `DbPool` and spawns 4 threads that each acquire a reader
/// connection and run `SELECT 1`. All threads start concurrently via a barrier
/// to maximise overlap.
#[test]
fn concurrent_readers_succeed() {
    let (pool, _dir) = common::test_db_pool();
    let barrier = Arc::new(Barrier::new(4));

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let pool = pool.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let conn = pool
                    .reader()
                    .unwrap_or_else(|e| panic!("thread {i} failed to get reader: {e}"));
                let val: i64 = conn
                    .query_row("SELECT 1", [], |row| row.get(0))
                    .unwrap_or_else(|e| panic!("thread {i} SELECT failed: {e}"));
                assert_eq!(val, 1, "thread {i} expected 1");
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }
}

/// While the writer mutex is held, reader connections must still be usable
/// (WAL snapshot isolation). This test acquires the writer lock first, then
/// verifies that 4 reader threads can proceed concurrently.
#[test]
fn readers_proceed_while_writer_locked() {
    let (pool, _dir) = common::test_db_pool();

    // Insert a row via the writer so readers have something to query.
    {
        let writer = pool.writer().lock().expect("lock writer for seed");
        writer
            .execute_batch("CREATE TABLE pool_test (id INTEGER PRIMARY KEY)")
            .expect("create pool_test table");
        writer
            .execute("INSERT INTO pool_test (id) VALUES (42)", [])
            .expect("insert seed row");
    }

    // Hold the writer lock for the duration of the reader threads.
    let _writer_guard = pool.writer().lock().expect("lock writer");

    let barrier = Arc::new(Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let pool = pool.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let conn = pool
                    .reader()
                    .unwrap_or_else(|e| panic!("reader thread {i} failed: {e}"));
                let val: i64 = conn
                    .query_row("SELECT id FROM pool_test LIMIT 1", [], |row| row.get(0))
                    .unwrap_or_else(|e| panic!("reader thread {i} query failed: {e}"));
                assert_eq!(val, 42, "reader thread {i} should see committed data");
            })
        })
        .collect();

    for h in handles {
        h.join().expect("reader thread panicked");
    }
}

/// Verifies that reader connections cannot write (PRAGMA query_only=ON).
#[test]
fn reader_connections_are_read_only() {
    let (pool, _dir) = common::test_db_pool();

    // Create a table via the writer.
    {
        let writer = pool.writer().lock().expect("lock writer");
        writer
            .execute_batch("CREATE TABLE ro_test (id INTEGER)")
            .expect("create ro_test");
    }

    // Attempt to INSERT via a reader — must fail.
    let reader = pool.reader().expect("get reader");
    let result = reader.execute("INSERT INTO ro_test (id) VALUES (1)", []);
    assert!(
        result.is_err(),
        "reader connection must not allow writes (query_only=ON)"
    );
}

/// The `DbPool` returned by `from_writer_only` (test-util) routes reads
/// through the writer. Verify basic read works.
#[test]
fn writer_only_pool_reads_work() {
    let pool = common::test_db_pool_memory();
    let conn = pool.reader().expect("reader from writer-only pool");
    let val: i64 = conn
        .query_row("SELECT 1", [], |row| row.get(0))
        .expect("SELECT 1 on writer-only pool");
    assert_eq!(val, 1);
}
