// Rust guideline compliant (M-MODULE-DOCS) — 2026-03-09

//! Verifier: V4V payment route presence and validity.

use crate::ingest::IngestPaymentRoute;
use crate::medium;
use crate::verify::{IngestContext, Verifier, VerifyResult};

/// Rejects feeds in which a track resolves to no valid payment route.
///
/// ADR 0048 owns these rules. The gate is coverage, not the presence of a
/// feed-level block.
///
/// # Rules
///
/// 1. A track that declares its own `podcast:value` block is covered by those
///    routes. They must hold at least one valid recipient.
///
/// 2. A track that declares no routes is covered by the feed-level routes.
///    Those must hold at least one valid recipient.
///
/// 3. A feed-level block is therefore required only when a track declares no
///    routes. A feed-level block that the feed declares must still be valid.
///
/// 4. A feed with no tracks must have a valid feed-level block. It has nothing
///    else to cover, and a music feed with no track does not participate.
///
/// 5. A valid recipient has a non-empty address and a positive split.
///
/// 6. A failure for a track names that track.
///
/// # Fallback model
///
/// ```text
/// play track T
///   └── T has own routes?  yes → pay T's routes
///                          no  → pay feed-level routes
/// ```
#[derive(Debug)]
pub struct V4VPaymentVerifier;

impl Verifier for V4VPaymentVerifier {
    fn name(&self) -> &'static str {
        "v4v_payment"
    }

    fn verify(&self, ctx: &IngestContext) -> VerifyResult {
        let Some(feed_data) = &ctx.request.feed_data else {
            return VerifyResult::Pass; // fetch failed — handled elsewhere
        };

        // Publisher and musicL feeds are source-layer containers by design.
        if medium::payment_exempt(feed_data.raw_medium.as_deref()) {
            return VerifyResult::Pass;
        }

        // ── A declared feed-level block must be valid ─────────────────────────
        let feed_routes_valid = if feed_data.feed_payment_routes.is_empty() {
            false
        } else {
            if let Err(msg) = validate_routes("feed", &feed_data.feed_payment_routes) {
                return VerifyResult::Fail(msg);
            }
            true
        };

        // ── A feed with no tracks needs a valid feed-level block ──────────────
        if feed_data.tracks.is_empty() && !feed_routes_valid {
            return VerifyResult::Fail(
                "no feed-level podcast:value block and no tracks — feed does not \
                 participate in V4V"
                    .into(),
            );
        }

        // ── Each track resolves to a valid route ──────────────────────────────
        for track in &feed_data.tracks {
            if track.payment_routes.is_empty() {
                if feed_routes_valid {
                    continue;
                }
                return VerifyResult::Fail(format!(
                    "track '{}' declares no podcast:value block, and the feed has no \
                     feed-level block to fall back to",
                    track.track_guid
                ));
            }
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
