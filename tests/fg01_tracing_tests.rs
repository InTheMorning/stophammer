// FG-01 structured logging — 2026-03-13
//
// Verifies that no `eprintln!` or `println!` calls remain in src/ files.
// All output must go through `tracing::` macros for structured, level-filtered logging.

/// Scans the given source text for raw `eprintln!(` or `println!(` calls,
/// ignoring occurrences inside comments and string literals (best-effort).
fn has_raw_print_macros(source: &str) -> Vec<(usize, String)> {
    let mut hits = Vec::new();
    for (line_no, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        // Skip comment-only lines
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        if trimmed.contains("eprintln!(") || trimmed.contains("println!(") {
            hits.push((line_no + 1, line.to_string()));
        }
    }
    hits
}

#[test]
fn no_eprintln_or_println_in_main_rs() {
    let source = include_str!("../src/main.rs");
    let hits = has_raw_print_macros(source);
    assert!(hits.is_empty(), "main.rs still contains raw print macros:\n{hits:#?}");
}

#[test]
fn no_eprintln_or_println_in_api_rs() {
    let source = include_str!("../src/api.rs");
    let hits = has_raw_print_macros(source);
    assert!(hits.is_empty(), "api.rs still contains raw print macros:\n{hits:#?}");
}

#[test]
fn no_eprintln_or_println_in_community_rs() {
    let source = include_str!("../src/community.rs");
    let hits = has_raw_print_macros(source);
    assert!(hits.is_empty(), "community.rs still contains raw print macros:\n{hits:#?}");
}

#[test]
fn no_eprintln_or_println_in_apply_rs() {
    let source = include_str!("../src/apply.rs");
    let hits = has_raw_print_macros(source);
    assert!(hits.is_empty(), "apply.rs still contains raw print macros:\n{hits:#?}");
}

#[test]
fn no_eprintln_or_println_in_tls_rs() {
    let source = include_str!("../src/tls.rs");
    let hits = has_raw_print_macros(source);
    assert!(hits.is_empty(), "tls.rs still contains raw print macros:\n{hits:#?}");
}

#[test]
fn no_eprintln_or_println_in_search_rs() {
    let source = include_str!("../src/search.rs");
    let hits = has_raw_print_macros(source);
    assert!(hits.is_empty(), "search.rs still contains raw print macros:\n{hits:#?}");
}

#[test]
fn no_eprintln_or_println_in_verify_rs() {
    let source = include_str!("../src/verify.rs");
    let hits = has_raw_print_macros(source);
    assert!(hits.is_empty(), "verify.rs still contains raw print macros:\n{hits:#?}");
}
