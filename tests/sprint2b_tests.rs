// Sprint 2B tests: Poisoned mutex propagation + spawn_db helper
//
// Issue #5 — Poisoned mutex must produce HTTP 500, not silent recovery.
// Issue #14 — spawn_db helper must correctly map DbError → ApiError.

mod common;

use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use stophammer::db;

// ── Issue #5: poisoned mutex produces an error, not silent recovery ─────────

#[test]
fn poisoned_db_mutex_returns_db_error() {
    let conn = common::test_db();
    let db: db::Db = Arc::new(Mutex::new(conn));
    let pool = common::wrap_pool(Arc::clone(&db));

    // Poison the mutex by panicking inside a lock scope.
    let db2 = Arc::clone(&db);
    let _ = std::thread::spawn(move || {
        let _guard = db2.lock().expect("lock for poisoning");
        panic!("intentional panic to poison the mutex");
    })
    .join();

    // The mutex is now poisoned. Verify lock returns an error.
    assert!(db.lock().is_err(), "mutex should be poisoned after a panic");

    // Verify that the Poisoned variant exists and formats correctly.
    let err = db::DbError::Poisoned;
    assert!(
        format!("{err}").contains("poisoned"),
        "Poisoned error message should contain 'poisoned': got '{err}'",
    );
}

#[test]
fn poisoned_db_error_converts_to_500() {
    use stophammer::api::ApiError;

    let err: ApiError = db::DbError::Poisoned.into();
    assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        err.message.contains("poisoned"),
        "ApiError message should mention 'poisoned': got '{}'",
        err.message,
    );
}

// ── Issue #5: apply_single_event propagates poisoned mutex ──────────────────

#[tokio::test]
async fn apply_single_event_with_poisoned_mutex_returns_error() {
    use stophammer::apply;
    use stophammer::event;

    let conn = common::test_db();
    let db: db::Db = Arc::new(Mutex::new(conn));
    let pool = common::wrap_pool(Arc::clone(&db));

    // Poison the mutex.
    let db2 = Arc::clone(&db);
    let _ = std::thread::spawn(move || {
        let _guard = db2.lock().expect("lock for poisoning");
        panic!("intentional panic to poison the mutex");
    })
    .join();

    // Create a minimal dummy event with valid payload_json (inner struct
    // format) so that the payload-integrity deserialization step succeeds and
    // the test actually reaches the poisoned-mutex lock.
    let inner = event::ArtistUpsertedPayload {
        artist: stophammer::model::Artist {
            artist_id: "test-artist".into(),
            name: "Test Artist".into(),
            name_lower: "test artist".into(),
            sort_name: None,
            type_id: None,
            area: None,
            img_url: None,
            url: None,
            begin_year: None,
            end_year: None,
            created_at: 0,
            updated_at: 0,
        },
    };
    let payload_json = serde_json::to_string(&inner).expect("serialize inner");
    let ev = event::Event {
        event_id: "test-event-id".into(),
        event_type: event::EventType::ArtistUpserted,
        payload: event::EventPayload::ArtistUpserted(inner),
        subject_guid: "test-artist".into(),
        signed_by: "deadbeef".into(),
        signature: "badsig".into(),
        seq: 1,
        created_at: 0,
        warnings: vec![],
        payload_json,
    };

    let result = apply::apply_single_event(&pool, &ev);
    assert!(result.is_err(), "should return Err when mutex is poisoned");

    // Use match to avoid needing Debug on ApplyOutcome.
    match result {
        Err(err) => assert!(
            format!("{err}").contains("poisoned"),
            "error should mention 'poisoned': got '{err}'",
        ),
        Ok(_) => panic!("expected Err but got Ok"),
    }
}

// ── Issue #14: spawn_db helper maps errors correctly ────────────────────────

#[tokio::test]
async fn spawn_db_success() {
    let conn = common::test_db();
    let db: db::Db = Arc::new(Mutex::new(conn));
    let pool = common::wrap_pool(Arc::clone(&db));

    let result = stophammer::api::spawn_db(pool, |_conn| Ok(42i32)).await;
    assert!(result.is_ok(), "should succeed");
    assert_eq!(result.unwrap_or(0), 42);
}

#[tokio::test]
async fn spawn_db_propagates_db_error() {
    let conn = common::test_db();
    let db: db::Db = Arc::new(Mutex::new(conn));
    let pool = common::wrap_pool(Arc::clone(&db));

    let result: Result<i32, _> =
        stophammer::api::spawn_db(pool, |_conn| Err(db::DbError::Poisoned)).await;

    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn spawn_db_propagates_poisoned_mutex() {
    let conn = common::test_db();
    let db: db::Db = Arc::new(Mutex::new(conn));
    let pool = common::wrap_pool(Arc::clone(&db));

    // Poison the mutex.
    let db2 = Arc::clone(&db);
    let _ = std::thread::spawn(move || {
        let _guard = db2.lock().expect("lock for poisoning");
        panic!("intentional panic to poison the mutex");
    })
    .join();

    let result = stophammer::api::spawn_db(pool, |_conn| Ok(42i32)).await;
    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        err.message.contains("poisoned"),
        "should mention 'poisoned': got '{}'",
        err.message,
    );
}

#[tokio::test]
async fn spawn_db_mut_success() {
    let conn = common::test_db();
    let db: db::Db = Arc::new(Mutex::new(conn));
    let pool = common::wrap_pool(Arc::clone(&db));

    let result = stophammer::api::spawn_db_mut(pool, |_conn| Ok(99i32)).await;
    assert!(result.is_ok(), "should succeed");
    assert_eq!(result.unwrap_or(0), 99);
}

#[tokio::test]
async fn spawn_db_mut_propagates_poisoned_mutex() {
    let conn = common::test_db();
    let db: db::Db = Arc::new(Mutex::new(conn));
    let pool = common::wrap_pool(Arc::clone(&db));

    // Poison the mutex.
    let db2 = Arc::clone(&db);
    let _ = std::thread::spawn(move || {
        let _guard = db2.lock().expect("lock for poisoning");
        panic!("intentional panic to poison the mutex");
    })
    .join();

    let result = stophammer::api::spawn_db_mut(pool, |_conn| Ok(42i32)).await;
    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        err.message.contains("poisoned"),
        "should mention 'poisoned': got '{}'",
        err.message,
    );
}
