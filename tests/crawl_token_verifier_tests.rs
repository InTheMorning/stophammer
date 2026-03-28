// Crawl token verifier behavior tests.

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
