// Rust guideline compliant (M-MODULE-DOCS) — 2026-03-09

//! Verifier: podcast:medium tag must be "music" or "publisher".

use crate::verify::{IngestContext, Verifier, VerifyResult};

/// Rejects feeds where `podcast:medium` is not "music" or "publisher".
///
/// Publisher feeds (`medium=publisher`) are parent feeds that list child music
/// feeds via `<podcast:remoteItem>`. They carry no tracks or payment routes
/// but provide artist/label identity and feed grouping signals.
#[derive(Debug)]
pub struct MediumMusicVerifier;

impl Verifier for MediumMusicVerifier {
    fn name(&self) -> &'static str {
        "medium_music"
    }

    fn verify(&self, ctx: &IngestContext) -> VerifyResult {
        match ctx
            .request
            .feed_data
            .as_ref()
            .and_then(|f| f.raw_medium.as_deref())
        {
            Some("music") => VerifyResult::Pass,
            Some("publisher") => {
                // Publisher feeds must reference at least one music child feed
                // to be worth ingesting — otherwise they are empty shells.
                let has_music_child = ctx
                    .request
                    .feed_data
                    .as_ref()
                    .is_some_and(|f| {
                        f.remote_items
                            .iter()
                            .any(|ri| ri.medium.as_deref() == Some("music"))
                    });
                if has_music_child {
                    VerifyResult::Pass
                } else {
                    VerifyResult::Fail(
                        "publisher feed has no remoteItem with medium='music'".into(),
                    )
                }
            }
            Some(other) => {
                VerifyResult::Fail(format!("medium is '{other}', expected 'music' or 'publisher'"))
            }
            None => VerifyResult::Fail("podcast:medium absent — must be 'music' or 'publisher'".into()),
        }
    }
}
