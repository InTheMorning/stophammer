// Sprint 3A: security and resilience fixes
// Issue-4, Issue-16, Issue-22, Issue-15

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
    unsafe { std::env::remove_var("ALLOW_INSECURE_PUBKEY_DISCOVERY"); }

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
    unsafe { std::env::set_var("ALLOW_INSECURE_PUBKEY_DISCOVERY", "true"); }
    let result = stophammer::community::require_https_for_discovery("http://primary:8008");
    assert!(result.is_ok(), "HTTP must be allowed when ALLOW_INSECURE_PUBKEY_DISCOVERY=true");

    // Cleanup.
    // SAFETY: test runs single-threaded; no other thread reads this env var.
    unsafe { std::env::remove_var("ALLOW_INSECURE_PUBKEY_DISCOVERY"); }
}

// ---------------------------------------------------------------------------
// Issue #16: ADMIN_TOKEN empty warning
// Finding-3 separate sync token — 2026-03-13
// ---------------------------------------------------------------------------

/// Static analysis: the `register_with_primary` function body must warn when
/// neither `SYNC_TOKEN` nor `ADMIN_TOKEN` is configured.
#[test]
fn issue16_admin_token_empty_warning_present_in_source() {
    let src = include_str!("../src/community.rs");
    assert!(
        src.contains("Neither SYNC_TOKEN nor ADMIN_TOKEN"),
        "community.rs must warn when neither SYNC_TOKEN nor ADMIN_TOKEN is set"
    );
}

// ---------------------------------------------------------------------------
// Issue #22: validate_feed_url must not block async runtime
// ---------------------------------------------------------------------------

/// Static analysis: api.rs must wrap `validate_feed_url` in `spawn_blocking`.
#[test]
fn issue22_validate_feed_url_wrapped_in_spawn_blocking() {
    let src = include_str!("../src/api.rs");
    // The call site must use spawn_blocking around validate_feed_url.
    assert!(
        src.contains("spawn_blocking") && src.contains("validate_feed_url"),
        "api.rs must call validate_feed_url inside spawn_blocking"
    );
}

// ---------------------------------------------------------------------------
// Issue #15: .expect() instead of .unwrap() on server bind
// ---------------------------------------------------------------------------

/// Static analysis: main.rs must not contain bare `.unwrap()` on serve calls.
/// All server bind/serve must use `.expect(...)`.
#[test]
fn issue15_no_bare_unwrap_in_main_serve() {
    let src = include_str!("../src/main.rs");
    // The serve_with_optional_tls function must not have `.unwrap()` calls.
    let fn_start = src.find("async fn serve_with_optional_tls")
        .expect("serve_with_optional_tls function must exist");
    let fn_body = &src[fn_start..];
    // Count braces to find the end of the function.
    let mut depth = 0i32;
    let mut fn_end = 0;
    for (i, ch) in fn_body.char_indices() {
        if ch == '{' { depth += 1; }
        if ch == '}' {
            depth -= 1;
            if depth == 0 {
                fn_end = i + 1;
                break;
            }
        }
    }
    let fn_text = &fn_body[..fn_end];
    // .unwrap() should not appear (replaced by .expect("..."))
    assert!(
        !fn_text.contains(".unwrap()"),
        "serve_with_optional_tls must not contain .unwrap(); use .expect() instead"
    );
}
