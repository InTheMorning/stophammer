// Rust guideline compliant (M-MODULE-DOCS) — 2026-03-09

//! Verifier: crawl token gate.

use sha2::{Sha256, Digest};
use subtle::ConstantTimeEq;

use crate::verify::{IngestContext, Verifier, VerifyResult};

/// Rejects requests with an invalid crawl token.
///
/// This verifier should always be first in the chain — it gates all other
/// checks so that unauthenticated crawlers are rejected before any DB access.
#[derive(Debug)]
pub struct CrawlTokenVerifier {
    pub expected: String,
}

impl Verifier for CrawlTokenVerifier {
    fn name(&self) -> &'static str { "crawl_token" }

    // Issue-1 constant-time crawl token — 2026-03-13
    fn verify(&self, ctx: &IngestContext) -> VerifyResult {
        let h1 = Sha256::digest(ctx.request.crawl_token.as_bytes());
        let h2 = Sha256::digest(self.expected.as_bytes());
        if bool::from(h1.ct_eq(&h2)) {
            VerifyResult::Pass
        } else {
            VerifyResult::Fail("invalid crawl token".into())
        }
    }
}
