// Rust guideline compliant (M-MODULE-DOCS) — 2026-03-09

//! Verifier: V4V payment route presence and validity.

use crate::ingest::IngestPaymentRoute;
use crate::verify::{IngestContext, Verifier, VerifyResult};

/// Rejects feeds that do not participate in V4V (value-for-value) payments.
///
/// # Rules
///
/// 1. The feed must have at least one feed-level payment route with a non-empty
///    address and a positive split — this is the fallback wallet for all tracks.
///
/// 2. For each track that declares its own `podcast:value` block (non-empty
///    `payment_routes`), those routes must also contain at least one valid
///    recipient. A track that declares the block but lists no recipients is
///    malformed and is rejected.
///
/// 3. Tracks with no routes of their own are valid — they fall back to the
///    feed-level routes at play time.
///
/// 4. All routes (feed and track level) must have positive splits (`> 0`).
///    A split of zero means the recipient receives nothing and indicates a
///    malformed `podcast:valueRecipient` entry.
///
/// # Fallback model
///
/// ```text
/// play track T
///   └── T has own routes?  yes → pay T's routes
///                          no  → pay feed-level routes
/// ```
pub struct V4VPaymentVerifier;

impl Verifier for V4VPaymentVerifier {
    fn name(&self) -> &'static str { "v4v_payment" }

    fn verify(&self, ctx: &IngestContext) -> VerifyResult {
        let Some(feed_data) = &ctx.request.feed_data else {
            return VerifyResult::Pass; // fetch failed — handled elsewhere
        };

        // ── 1. Feed must have at least one valid feed-level route ─────────────
        if feed_data.feed_payment_routes.is_empty() {
            return VerifyResult::Fail(
                "no feed-level podcast:value block — feed does not participate in V4V".into(),
            );
        }
        if let Err(msg) = validate_routes("feed", &feed_data.feed_payment_routes) {
            return VerifyResult::Fail(msg);
        }

        // ── 2 & 3. Per-track validation ───────────────────────────────────────
        for track in &feed_data.tracks {
            if track.payment_routes.is_empty() {
                // No track-level routes: falls back to feed routes (already validated).
                continue;
            }
            // Track declared its own routes — they must be valid.
            if let Err(msg) = validate_routes(
                &format!("track '{}'", track.track_guid),
                &track.payment_routes,
            ) {
                return VerifyResult::Fail(msg);
            }
        }

        VerifyResult::Pass
    }
}

/// Returns `Ok(())` if `routes` contains at least one recipient with a
/// non-empty address and a positive split, or `Err(reason)` otherwise.
fn validate_routes(context: &str, routes: &[IngestPaymentRoute]) -> Result<(), String> {
    let valid = routes.iter().any(|r| !r.address.is_empty() && r.split > 0);
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{context} podcast:value block has no valid recipient \
             (all routes have empty address or zero split)"
        ))
    }
}
