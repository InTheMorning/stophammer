// Rust guideline compliant (M-MODULE-DOCS) — 2026-03-09

//! Crawl token comparison.
//!
//! ADR 0051 section 4: the node checks the crawl token first, outside the
//! configurable verifier chain. [`crate::verify::VerifierChain::authenticate`]
//! calls [`token_matches`]. No `VERIFIER_CHAIN` entry names this check.

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Returns `true` when `provided` equals `expected`.
///
/// The comparison hashes both values and compares the digests in constant
/// time, so neither the length nor the content of the secret leaks through
/// timing.
// Issue-1 constant-time crawl token — 2026-03-13
#[must_use]
pub fn token_matches(provided: &str, expected: &str) -> bool {
    let h1 = Sha256::digest(provided.as_bytes());
    let h2 = Sha256::digest(expected.as_bytes());
    bool::from(h1.ct_eq(&h2))
}
