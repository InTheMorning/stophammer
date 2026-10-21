//! ADR 0046 rule 3: `run_migrations` reports at start, with a
//! `tracing::error!` that names this ADR, when the recorded migration version
//! is above the migration count. A correct database reports nothing.

use std::sync::{Arc, Mutex};

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("capture lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Opens the database at `path` and gives the text of each `error` event.
fn errors_of_open(path: &std::path::Path) -> String {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer_buffer = Arc::clone(&buffer);
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .with_ansi(false)
        .with_writer(move || CaptureWriter(Arc::clone(&writer_buffer)))
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        drop(stophammer::db::open_db(path));
    });
    let bytes = buffer.lock().expect("capture lock").clone();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn a_recorded_version_above_the_migration_count_logs_an_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("adr0046.db");
    let conn = stophammer::db::open_db(&path);
    conn.execute(
        "INSERT INTO schema_migrations (version, applied_at) VALUES (100000, 0)",
        [],
    )
    .expect("record a version from a later build");
    drop(conn);

    let errors = errors_of_open(&path);
    assert!(
        errors.contains("ERROR") && errors.contains("ADR 0046"),
        "ADR 0046 rule 3: a recorded version above the count must log an error that names \
         the ADR, got: {errors:?}"
    );
}

#[test]
fn a_correct_database_logs_no_adr_0046_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("adr0046-correct.db");
    drop(stophammer::db::open_db(&path));

    let errors = errors_of_open(&path);
    assert!(
        !errors.contains("ADR 0046"),
        "ADR 0046 rule 3: a database that ran each migration must log no error, got: {errors:?}"
    );
}
