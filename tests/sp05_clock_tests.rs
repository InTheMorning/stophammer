// SP-05 epoch guard — 2026-03-12
//
// Verify that the unix_now() helper exists, is public, and returns
// a sane value under normal conditions.

#[test]
fn unix_now_returns_positive_value() {
    let ts = stophammer::db::unix_now();
    assert!(
        ts > 1_700_000_000,
        "unix_now() should return a timestamp after 2023; got {ts}"
    );
}

#[test]
fn unix_now_is_consistent_across_calls() {
    let a = stophammer::db::unix_now();
    let b = stophammer::db::unix_now();
    // b should be >= a (monotonic within the same second)
    assert!(
        b >= a,
        "unix_now() should be monotonically non-decreasing; got a={a}, b={b}"
    );
    // and the difference should be tiny (under 2 seconds)
    assert!(
        b - a < 2,
        "two consecutive unix_now() calls should be within 2 seconds; got a={a}, b={b}"
    );
}
