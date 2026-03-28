// Sprint 3A: security and resilience behavior tests
// Runtime coverage kept here; source-policy checks live in static_policy_tests.rs.

mod common;

// ---------------------------------------------------------------------------
// Issue #4: HTTPS pubkey auto-discovery
// ---------------------------------------------------------------------------

/// Tests for `require_https_for_discovery`: HTTPS passes, HTTP fails without
/// env override, HTTP passes with `ALLOW_INSECURE_PUBKEY_DISCOVERY=true`.
///
/// Combined into one test to avoid env-var race conditions between parallel
/// test threads (Rust 2024 edition makes `set_var`/`remove_var` unsafe for
/// this reason).
#[test]
fn issue4_require_https_for_discovery() {
    // Safety: test-only single-threaded access to this env var; no other
    // thread reads ALLOW_INSECURE_PUBKEY_DISCOVERY concurrently.
    unsafe {
        std::env::remove_var("ALLOW_INSECURE_PUBKEY_DISCOVERY");
    }

    // 1. HTTPS URL always passes.
    let result = stophammer::community::require_https_for_discovery("https://primary:8008");
    assert!(result.is_ok(), "HTTPS discovery must be allowed");

    // 2. HTTP URL is rejected without the env override.
    let result = stophammer::community::require_https_for_discovery("http://primary:8008");
    assert!(result.is_err(), "plain HTTP discovery must be rejected");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("HTTPS"),
        "error message must mention HTTPS, got: {msg}"
    );

    // 3. HTTP URL passes with ALLOW_INSECURE_PUBKEY_DISCOVERY=true.
    // SAFETY: test runs single-threaded; no other thread reads this env var.
    unsafe {
        std::env::set_var("ALLOW_INSECURE_PUBKEY_DISCOVERY", "true");
    }
    let result = stophammer::community::require_https_for_discovery("http://primary:8008");
    assert!(
        result.is_ok(),
        "HTTP must be allowed when ALLOW_INSECURE_PUBKEY_DISCOVERY=true"
    );

    // Cleanup.
    // SAFETY: test runs single-threaded; no other thread reads this env var.
    unsafe {
        std::env::remove_var("ALLOW_INSECURE_PUBKEY_DISCOVERY");
    }
}
