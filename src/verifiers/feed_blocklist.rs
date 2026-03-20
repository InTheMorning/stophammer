// Rust guideline compliant (M-MODULE-DOCS) — 2026-03-20

//! Verifier: exact-match feed blocklist by GUID and URL.

use std::collections::BTreeSet;

use crate::verify::{IngestContext, Verifier, VerifyResult};

/// Rejects feeds whose exact GUID or URL appears in the operator-maintained blocklist.
///
/// This is intended for known-bad feeds that should never enter the index,
/// such as copyright-infringing streams or persistent spam feeds. Matching is
/// exact:
/// - GUIDs are normalized to lowercase UUID strings
/// - URLs are compared as exact trimmed strings against both `canonical_url`
///   and `source_url`
#[derive(Debug)]
pub struct FeedBlocklistVerifier {
    blocked_guids: BTreeSet<String>,
    blocked_urls: BTreeSet<String>,
}

impl FeedBlocklistVerifier {
    #[must_use]
    pub const fn new(blocked_guids: BTreeSet<String>, blocked_urls: BTreeSet<String>) -> Self {
        Self {
            blocked_guids,
            blocked_urls,
        }
    }

    /// Reads exact-match feed GUID and URL blocklists from the environment.
    ///
    /// `BLOCKED_FEED_GUIDS` and `BLOCKED_FEED_URLS` are comma-separated lists.
    #[must_use]
    pub fn from_env() -> Self {
        Self::new(
            read_csv_env("BLOCKED_FEED_GUIDS", true),
            read_csv_env("BLOCKED_FEED_URLS", false),
        )
    }
}

impl Verifier for FeedBlocklistVerifier {
    fn name(&self) -> &'static str {
        "feed_blocklist"
    }

    fn verify(&self, ctx: &IngestContext) -> VerifyResult {
        if self.blocked_urls.contains(ctx.request.canonical_url.trim()) {
            return VerifyResult::Fail(format!(
                "blocked canonical url: {}",
                ctx.request.canonical_url
            ));
        }
        if self.blocked_urls.contains(ctx.request.source_url.trim()) {
            return VerifyResult::Fail(format!("blocked source url: {}", ctx.request.source_url));
        }

        let Some(feed_data) = &ctx.request.feed_data else {
            return VerifyResult::Pass;
        };
        let guid = feed_data.feed_guid.trim().to_ascii_lowercase();
        if self.blocked_guids.contains(&guid) {
            return VerifyResult::Fail(format!("blocked feed guid: {}", feed_data.feed_guid));
        }

        VerifyResult::Pass
    }
}

#[must_use]
fn read_csv_env(var: &str, lowercase: bool) -> BTreeSet<String> {
    std::env::var(var)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if lowercase {
                value.to_ascii_lowercase()
            } else {
                value.to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::{IngestFeedData, IngestFeedRequest};
    use rusqlite::Connection;

    fn req(feed_guid: &str, canonical_url: &str, source_url: &str) -> IngestFeedRequest {
        IngestFeedRequest {
            canonical_url: canonical_url.into(),
            source_url: source_url.into(),
            crawl_token: "token".into(),
            http_status: 200,
            content_hash: "hash".into(),
            feed_data: Some(IngestFeedData {
                feed_guid: feed_guid.into(),
                title: "title".into(),
                description: None,
                image_url: None,
                language: None,
                explicit: false,
                itunes_type: None,
                raw_medium: Some("music".into()),
                author_name: None,
                owner_name: None,
                pub_date: None,
                remote_items: vec![],
                persons: vec![],
                entity_ids: vec![],
                links: vec![],
                feed_payment_routes: vec![],
                live_items: vec![],
                tracks: vec![],
            }),
        }
    }

    fn ctx<'a>(request: &'a IngestFeedRequest, db: &'a Connection) -> IngestContext<'a> {
        IngestContext {
            request,
            db,
            existing: None,
        }
    }

    #[test]
    fn blocks_exact_feed_guid() {
        let verifier = FeedBlocklistVerifier::new(
            ["27293ad7-c199-5047-8135-a864fb546492".to_string()]
                .into_iter()
                .collect(),
            BTreeSet::new(),
        );
        let conn = Connection::open_in_memory().expect("open memory db");
        let request = req(
            "27293AD7-C199-5047-8135-A864FB546492",
            "https://example.com/feed.xml",
            "https://example.com/feed.xml",
        );

        let result = verifier.verify(&ctx(&request, &conn));
        assert!(matches!(result, VerifyResult::Fail(msg) if msg.contains("blocked feed guid")));
    }

    #[test]
    fn blocks_exact_canonical_url() {
        let verifier = FeedBlocklistVerifier::new(
            BTreeSet::new(),
            ["https://feeds.podcastindex.org/100retro.xml".to_string()]
                .into_iter()
                .collect(),
        );
        let conn = Connection::open_in_memory().expect("open memory db");
        let request = req(
            "guid",
            "https://feeds.podcastindex.org/100retro.xml",
            "https://source.example/feed.xml",
        );

        let result = verifier.verify(&ctx(&request, &conn));
        assert!(matches!(result, VerifyResult::Fail(msg) if msg.contains("blocked canonical url")));
    }

    #[test]
    fn blocks_exact_source_url() {
        let verifier = FeedBlocklistVerifier::new(
            BTreeSet::new(),
            ["https://source.example/feed.xml".to_string()]
                .into_iter()
                .collect(),
        );
        let conn = Connection::open_in_memory().expect("open memory db");
        let request = req(
            "guid",
            "https://canonical.example/feed.xml",
            "https://source.example/feed.xml",
        );

        let result = verifier.verify(&ctx(&request, &conn));
        assert!(matches!(result, VerifyResult::Fail(msg) if msg.contains("blocked source url")));
    }

    #[test]
    fn passes_when_not_listed() {
        let verifier = FeedBlocklistVerifier::new(BTreeSet::new(), BTreeSet::new());
        let conn = Connection::open_in_memory().expect("open memory db");
        let request = req(
            "guid",
            "https://canonical.example/feed.xml",
            "https://source.example/feed.xml",
        );

        assert!(matches!(
            verifier.verify(&ctx(&request, &conn)),
            VerifyResult::Pass
        ));
    }
}
