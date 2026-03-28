//! Verifier-chain configuration tests.
//!
//! Unknown verifier names must fail closed rather than being skipped.

use stophammer::verify::{ChainSpec, build_chain};

#[test]
#[should_panic(expected = "unknown verifier 'bogus_verifier' in VERIFIER_CHAIN")]
fn build_chain_panics_on_unknown_verifier_name() {
    let spec = ChainSpec {
        names: vec!["crawl_token".to_string(), "bogus_verifier".to_string()],
    };
    let _ = build_chain(&spec, "test-token".to_string());
}

#[test]
#[should_panic(expected = "unknown verifier 'typo_hash' in VERIFIER_CHAIN")]
fn build_chain_panics_on_typo_verifier_name() {
    let spec = ChainSpec {
        names: vec!["typo_hash".to_string()],
    };
    let _ = build_chain(&spec, "test-token".to_string());
}

#[test]
fn build_chain_succeeds_with_all_valid_names() {
    let spec = ChainSpec {
        names: vec![
            "crawl_token".to_string(),
            "content_hash".to_string(),
            "medium_music".to_string(),
            "feed_guid".to_string(),
            "v4v_payment".to_string(),
            "payment_route_sum".to_string(),
            "enclosure_type".to_string(),
        ],
    };
    // Should not panic
    let _ = build_chain(&spec, "test-token".to_string());
}

#[test]
fn build_chain_succeeds_with_empty_chain() {
    let spec = ChainSpec { names: vec![] };
    // Empty chain is valid (no verifiers configured)
    let _ = build_chain(&spec, "test-token".to_string());
}
