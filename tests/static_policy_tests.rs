//! Static policy checks that are hard to express as runtime behavior tests.
//!
//! These are intentionally centralized here so source-scanning tests are easy
//! to find, easy to replace later, and do not clutter higher-signal behavioral
//! suites.

/// Scans the given source text for raw `eprintln!(` or `println!(` calls,
/// ignoring comment-only lines.
fn has_raw_print_macros(source: &str) -> Vec<(usize, String)> {
    let mut hits = Vec::new();
    for (line_no, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        if trimmed.contains("eprintln!(") || trimmed.contains("println!(") {
            hits.push((line_no + 1, line.to_string()));
        }
    }
    hits
}

fn assert_no_raw_prints(path: &str, source: &str) {
    let hits = has_raw_print_macros(source);
    assert!(
        hits.is_empty(),
        "{path} still contains raw print macros:\n{hits:#?}"
    );
}

#[test]
fn core_runtime_files_do_not_use_raw_print_macros() {
    assert_no_raw_prints("src/main.rs", include_str!("../src/main.rs"));
    assert_no_raw_prints("src/api.rs", include_str!("../src/api.rs"));
    assert_no_raw_prints("src/community.rs", include_str!("../src/community.rs"));
    assert_no_raw_prints("src/apply.rs", include_str!("../src/apply.rs"));
    assert_no_raw_prints("src/tls.rs", include_str!("../src/tls.rs"));
    assert_no_raw_prints("src/search.rs", include_str!("../src/search.rs"));
    assert_no_raw_prints("src/verify.rs", include_str!("../src/verify.rs"));
}

#[test]
fn crawl_token_verifier_uses_constant_time_hashed_comparison() {
    let src = include_str!("../src/verifiers/crawl_token.rs");

    assert!(
        src.contains("ct_eq"),
        "crawl_token verifier must use constant-time comparison"
    );
    assert!(
        src.contains("Sha256"),
        "crawl_token verifier must hash tokens before comparing"
    );
    assert!(
        !src.contains("crawl_token == self.expected") && !src.contains("self.expected == ctx"),
        "crawl_token verifier must not use direct string equality"
    );
}

#[test]
fn feed_url_validation_is_offloaded_from_async_runtime() {
    let src = include_str!("../src/api.rs");
    assert!(
        src.contains("spawn_blocking") && src.contains("validate_feed_url"),
        "api.rs must call validate_feed_url inside spawn_blocking"
    );
}

#[test]
fn serve_with_optional_tls_does_not_use_unwrap() {
    let src = include_str!("../src/main.rs");
    let fn_start = src
        .find("async fn serve_with_optional_tls")
        .expect("serve_with_optional_tls function must exist");
    let fn_body = &src[fn_start..];
    let mut depth = 0i32;
    let mut fn_end = 0;
    for (i, ch) in fn_body.char_indices() {
        if ch == '{' {
            depth += 1;
        }
        if ch == '}' {
            depth -= 1;
            if depth == 0 {
                fn_end = i + 1;
                break;
            }
        }
    }
    let fn_text = &fn_body[..fn_end];
    assert!(
        !fn_text.contains(".unwrap()"),
        "serve_with_optional_tls must not contain .unwrap(); use explicit error handling"
    );
}

#[test]
fn sse_handler_source_does_not_reintroduce_busy_sleep_polling() {
    let src = include_str!("../src/api.rs");
    assert!(
        !src.contains("from_millis(100)"),
        "api.rs should not reintroduce the old 100ms SSE busy-sleep pattern"
    );
}
