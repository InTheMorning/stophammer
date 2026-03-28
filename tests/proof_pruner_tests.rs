// Proof pruner configuration tests.

/// `prune_interval_from_env` should return default 300 when env var is absent.
/// We cannot safely set env vars in the 2024 edition without `unsafe`, so we
/// verify the public function exists, returns a sensible default, and does not panic.
#[test]
fn prune_interval_default_is_300() {
    // When PROOF_PRUNE_INTERVAL_SECS is not set (which it won't be in test env
    // unless explicitly configured), the function should return 300.
    let interval = stophammer::proof::prune_interval_from_env();
    assert_eq!(
        interval, 300,
        "default prune interval should be 300 seconds"
    );
}

/// The interval must be positive (at least 1).
#[test]
fn prune_interval_is_positive() {
    let interval = stophammer::proof::prune_interval_from_env();
    assert!(interval > 0, "prune interval must be positive");
}
