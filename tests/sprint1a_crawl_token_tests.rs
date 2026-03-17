// Sprint 1A: Issue-1 constant-time crawl token comparison
// Issue-1 constant-time crawl token — 2026-03-13

mod common;

use stophammer::verifiers::crawl_token::CrawlTokenVerifier;
use stophammer::verify::{IngestContext, Verifier, VerifyResult};

/// Correct token must pass (basic sanity).
#[test]
fn crawl_token_correct_passes() {
    let conn = common::test_db();
    let verifier = CrawlTokenVerifier {
        expected: "secret-crawl-token".into(),
    };
    let req = make_req("secret-crawl-token");
    let ctx = IngestContext {
        request: &req,
        db: &conn,
        existing: None,
    };
    assert!(matches!(verifier.verify(&ctx), VerifyResult::Pass));
}

/// Wrong token must fail.
#[test]
fn crawl_token_wrong_fails() {
    let conn = common::test_db();
    let verifier = CrawlTokenVerifier {
        expected: "secret-crawl-token".into(),
    };
    let req = make_req("wrong-token");
    let ctx = IngestContext {
        request: &req,
        db: &conn,
        existing: None,
    };
    assert!(matches!(verifier.verify(&ctx), VerifyResult::Fail(_)));
}

/// Empty token must fail.
#[test]
fn crawl_token_empty_fails() {
    let conn = common::test_db();
    let verifier = CrawlTokenVerifier {
        expected: "secret-crawl-token".into(),
    };
    let req = make_req("");
    let ctx = IngestContext {
        request: &req,
        db: &conn,
        existing: None,
    };
    assert!(matches!(verifier.verify(&ctx), VerifyResult::Fail(_)));
}

/// Two different tokens of the same length must fail — validates that the
/// comparison is not short-circuiting on length alone.
#[test]
fn crawl_token_same_length_different_content_fails() {
    let conn = common::test_db();
    let verifier = CrawlTokenVerifier {
        expected: "aaaa".into(),
    };
    let req = make_req("bbbb");
    let ctx = IngestContext {
        request: &req,
        db: &conn,
        existing: None,
    };
    assert!(matches!(verifier.verify(&ctx), VerifyResult::Fail(_)));
}

/// The source code must use `ct_eq` (constant-time comparison), not `==`.
/// This is a structural test that reads the source file and verifies the
/// timing-safe pattern is present.
#[test]
fn crawl_token_source_uses_constant_time_eq() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/verifiers/crawl_token.rs"
    ))
    .expect("read crawl_token.rs source");

    assert!(
        src.contains("ct_eq"),
        "crawl_token.rs must use constant-time comparison (ct_eq), found direct == instead"
    );
    assert!(
        src.contains("Sha256"),
        "crawl_token.rs must hash tokens with SHA-256 before comparing"
    );
    assert!(
        !src.contains("crawl_token == self.expected") && !src.contains("self.expected == ctx"),
        "crawl_token.rs must not use direct string equality"
    );
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
        feed_data: None,
    }
}
