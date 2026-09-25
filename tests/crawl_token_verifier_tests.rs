// Crawl token check behavior tests (ADR 0051 section 4).

use stophammer::verify::VerifierChain;

const EXPECTED: &str = "secret-crawl-token";

fn chain(expected: &str) -> VerifierChain {
    VerifierChain::new(expected.into(), vec![])
}

/// Correct token must pass (basic sanity).
#[test]
fn crawl_token_correct_passes() {
    let result = chain(EXPECTED).authenticate(&make_req(EXPECTED));
    assert!(result.is_ok(), "the correct token must pass: {result:?}");
}

/// Wrong token must fail with the historical reason text.
#[test]
fn crawl_token_wrong_fails() {
    let err = chain(EXPECTED)
        .authenticate(&make_req("wrong-token"))
        .expect_err("a wrong token must fail");
    assert_eq!(
        err.0, "[crawl_token] invalid crawl token",
        "the reason text must stay stable for crawlers"
    );
}

/// Empty token must fail.
#[test]
fn crawl_token_empty_fails() {
    assert!(
        chain(EXPECTED).authenticate(&make_req("")).is_err(),
        "an empty token must fail"
    );
}

/// Two different tokens of the same length must fail — validates that the
/// comparison is not short-circuiting on length alone.
#[test]
fn crawl_token_same_length_different_content_fails() {
    assert!(
        chain("aaaa").authenticate(&make_req("bbbb")).is_err(),
        "a same-length different token must fail"
    );
}

/// The shared comparison agrees with the chain check.
#[test]
fn token_matches_compares_exactly() {
    use stophammer::verifiers::crawl_token::token_matches;
    assert!(token_matches("abc", "abc"), "equal tokens must match");
    assert!(!token_matches("abc", "abcd"), "a prefix must not match");
    assert!(!token_matches("", "abc"), "an empty token must not match");
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn make_req(token: &str) -> stophammer::ingest::IngestFeedRequest {
    stophammer::ingest::IngestFeedRequest {
        canonical_url: "https://example.com/feed.xml".into(),
        source_url: "https://example.com/feed.xml".into(),
        crawl_token: token.into(),
        http_status: 200,
        content_hash: "abc123".into(),
        force_reingest: false,
        redirects: vec![],
        feed_data: None,
    }
}
