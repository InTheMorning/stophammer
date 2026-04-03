#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    reason = "internal backend trait — callers are the TUI binaries, not a public API"
)]

use crate::db::{
    self, ArtistIdentityFeedPlan, ArtistIdentityPendingReview, ArtistIdentityPendingReviewSummary,
    ArtistIdentityReviewActionOutcome, ArtistIdentityReviewItem, PendingReviewAgeSummary,
    PendingReviewConfidenceSummary, PendingReviewFeedHotspot, PendingReviewScoreSummary,
    WalletAliasPeer, WalletClaimFeed, WalletDetail, WalletIdentityReviewActionOutcome,
    WalletPendingReviewSummary, WalletReviewSummary,
};
use crate::model;
use reqwest::blocking::Client;
use std::time::Duration;

// ── Structs used by the trait ───────────────────────────────────────────────

#[derive(Debug)]
pub struct FeedEvidence {
    pub release_maps: Vec<model::SourceFeedReleaseMap>,
    pub platform_claims: Vec<model::SourcePlatformClaim>,
    pub entity_links: Vec<model::SourceEntityLink>,
    pub entity_ids: Vec<model::SourceEntityIdClaim>,
    pub remote_items: Vec<model::FeedRemoteItemRaw>,
}

#[derive(Debug)]
pub struct ArtistDiagnostics {
    pub artist_id: String,
    pub name: String,
    pub created_at: i64,
    pub feeds: Vec<ArtistFeedInfo>,
    pub release_count: usize,
    pub external_ids: Vec<model::ExternalId>,
}

#[derive(Debug)]
pub struct ArtistFeedInfo {
    pub feed_guid: String,
    pub title: String,
    pub feed_url: String,
}

// ── Trait ────────────────────────────────────────────────────────────────────

pub trait ReviewBackend {
    // --- Artist identity reviews (read) ---
    fn list_pending_artist_reviews(
        &self,
        limit: usize,
        confidence: Option<&str>,
        min_score: Option<u16>,
    ) -> anyhow::Result<Vec<ArtistIdentityPendingReview>>;
    fn list_stale_artist_reviews(
        &self,
        min_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<ArtistIdentityPendingReview>>;
    fn list_recent_artist_reviews(
        &self,
        max_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<ArtistIdentityPendingReview>>;
    fn get_artist_review(&self, id: i64) -> anyhow::Result<Option<ArtistIdentityReviewItem>>;
    fn explain_artist_identity_for_feed(
        &self,
        feed_guid: &str,
    ) -> anyhow::Result<ArtistIdentityFeedPlan>;

    // --- Artist identity reviews (write) ---
    fn resolve_artist_review(
        &mut self,
        id: i64,
        action: &str,
        target: Option<&str>,
        note: Option<&str>,
    ) -> anyhow::Result<ArtistIdentityReviewActionOutcome>;

    // --- Wallet identity reviews (read) ---
    fn list_pending_wallet_reviews(&self, limit: usize)
    -> anyhow::Result<Vec<WalletReviewSummary>>;
    fn list_stale_wallet_reviews(
        &self,
        min_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<WalletReviewSummary>>;
    fn list_recent_wallet_reviews(
        &self,
        max_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<WalletReviewSummary>>;
    fn get_wallet_alias_peers(&self, alias: &str) -> anyhow::Result<Vec<WalletAliasPeer>>;
    fn get_wallet_detail(&self, id: &str) -> anyhow::Result<Option<WalletDetail>>;
    fn get_wallet_claim_feeds(&self, id: &str) -> anyhow::Result<Vec<WalletClaimFeed>>;

    // --- Wallet identity reviews (write) ---
    fn resolve_wallet_review(
        &mut self,
        id: i64,
        action: &str,
        target_id: Option<&str>,
        value: Option<&str>,
    ) -> anyhow::Result<WalletIdentityReviewActionOutcome>;
    fn set_wallet_force_class(&mut self, id: &str, class: &str) -> anyhow::Result<()>;
    fn set_wallet_force_confidence(&mut self, id: &str, confidence: &str) -> anyhow::Result<()>;
    fn revert_wallet_classification(&mut self, id: &str) -> anyhow::Result<()>;
    fn apply_wallet_merges(&mut self) -> anyhow::Result<db::WalletRefreshStats>;
    fn undo_last_wallet_batch(&mut self) -> anyhow::Result<Option<db::WalletUndoStats>>;

    // --- Summaries / dashboard ---
    fn artist_review_summary(
        &self,
    ) -> anyhow::Result<(
        Vec<ArtistIdentityPendingReviewSummary>,
        Vec<PendingReviewConfidenceSummary>,
        Vec<PendingReviewScoreSummary>,
    )>;
    fn wallet_review_summary(
        &self,
    ) -> anyhow::Result<(
        Vec<WalletPendingReviewSummary>,
        Vec<PendingReviewConfidenceSummary>,
        Vec<PendingReviewScoreSummary>,
    )>;
    fn review_age_summary(
        &self,
    ) -> anyhow::Result<(PendingReviewAgeSummary, PendingReviewAgeSummary)>;
    fn feed_hotspots(&self, limit: usize) -> anyhow::Result<Vec<PendingReviewFeedHotspot>>;

    // --- Evidence lookups ---
    fn feed_url(&self, feed_guid: &str) -> anyhow::Result<String>;
    fn artist_diagnostics(&self, artist_id: &str) -> anyhow::Result<Option<ArtistDiagnostics>>;
    fn feed_evidence(&self, feed_guid: &str) -> anyhow::Result<FeedEvidence>;
}

// ── DbBackend ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct DbBackend {
    conn: rusqlite::Connection,
}

impl DbBackend {
    pub fn new(conn: rusqlite::Connection) -> Self {
        Self { conn }
    }
}

impl ReviewBackend for DbBackend {
    fn list_pending_artist_reviews(
        &self,
        limit: usize,
        confidence: Option<&str>,
        min_score: Option<u16>,
    ) -> anyhow::Result<Vec<ArtistIdentityPendingReview>> {
        let mut reviews = db::list_pending_artist_identity_reviews(&self.conn, limit)?;
        db::filter_pending_artist_reviews(&mut reviews, confidence, min_score);
        Ok(reviews)
    }

    fn list_stale_artist_reviews(
        &self,
        min_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<ArtistIdentityPendingReview>> {
        Ok(db::list_stale_pending_artist_identity_reviews(
            &self.conn,
            min_age_secs,
            limit,
        )?)
    }

    fn list_recent_artist_reviews(
        &self,
        max_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<ArtistIdentityPendingReview>> {
        Ok(db::list_recent_pending_artist_identity_reviews(
            &self.conn,
            max_age_secs,
            limit,
        )?)
    }

    fn get_artist_review(&self, id: i64) -> anyhow::Result<Option<ArtistIdentityReviewItem>> {
        Ok(db::get_artist_identity_review(&self.conn, id)?)
    }

    fn explain_artist_identity_for_feed(
        &self,
        feed_guid: &str,
    ) -> anyhow::Result<ArtistIdentityFeedPlan> {
        Ok(db::explain_artist_identity_for_feed(&self.conn, feed_guid)?)
    }

    fn resolve_artist_review(
        &mut self,
        id: i64,
        action: &str,
        target: Option<&str>,
        note: Option<&str>,
    ) -> anyhow::Result<ArtistIdentityReviewActionOutcome> {
        Ok(db::apply_artist_identity_review_action(
            &mut self.conn,
            id,
            action,
            target,
            note,
        )?)
    }

    fn list_pending_wallet_reviews(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<WalletReviewSummary>> {
        Ok(db::list_pending_wallet_reviews(&self.conn, limit)?)
    }

    fn list_stale_wallet_reviews(
        &self,
        min_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<WalletReviewSummary>> {
        Ok(db::list_stale_pending_wallet_reviews(
            &self.conn,
            min_age_secs,
            limit,
        )?)
    }

    fn list_recent_wallet_reviews(
        &self,
        max_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<WalletReviewSummary>> {
        Ok(db::list_recent_pending_wallet_reviews(
            &self.conn,
            max_age_secs,
            limit,
        )?)
    }

    fn get_wallet_alias_peers(&self, alias: &str) -> anyhow::Result<Vec<WalletAliasPeer>> {
        Ok(db::get_wallet_alias_peers(&self.conn, alias)?)
    }

    fn get_wallet_detail(&self, id: &str) -> anyhow::Result<Option<WalletDetail>> {
        Ok(db::get_wallet_detail(&self.conn, id)?)
    }

    fn get_wallet_claim_feeds(&self, id: &str) -> anyhow::Result<Vec<WalletClaimFeed>> {
        Ok(db::get_wallet_claim_feeds(&self.conn, id)?)
    }

    fn resolve_wallet_review(
        &mut self,
        id: i64,
        action: &str,
        target_id: Option<&str>,
        value: Option<&str>,
    ) -> anyhow::Result<WalletIdentityReviewActionOutcome> {
        Ok(db::apply_wallet_identity_review_action(
            &self.conn, id, action, target_id, value,
        )?)
    }

    fn set_wallet_force_class(&mut self, id: &str, class: &str) -> anyhow::Result<()> {
        Ok(db::set_wallet_force_class(&self.conn, id, class)?)
    }

    fn set_wallet_force_confidence(&mut self, id: &str, confidence: &str) -> anyhow::Result<()> {
        Ok(db::set_wallet_force_confidence(&self.conn, id, confidence)?)
    }

    fn revert_wallet_classification(&mut self, id: &str) -> anyhow::Result<()> {
        Ok(db::revert_wallet_operator_classification(&self.conn, id)?)
    }

    fn apply_wallet_merges(&mut self) -> anyhow::Result<db::WalletRefreshStats> {
        Ok(db::backfill_wallet_pass5(&self.conn)?)
    }

    fn undo_last_wallet_batch(&mut self) -> anyhow::Result<Option<db::WalletUndoStats>> {
        Ok(db::undo_last_wallet_merge_batch(&self.conn)?)
    }

    fn artist_review_summary(
        &self,
    ) -> anyhow::Result<(
        Vec<ArtistIdentityPendingReviewSummary>,
        Vec<PendingReviewConfidenceSummary>,
        Vec<PendingReviewScoreSummary>,
    )> {
        let reviews = db::list_pending_artist_identity_reviews(
            &self.conn,
            db::max_pending_review_scan_limit(),
        )?;
        let (summary, confidence, score, _conflict) =
            db::summarize_artist_pending_review_subset(&reviews);
        Ok((summary, confidence, score))
    }

    fn wallet_review_summary(
        &self,
    ) -> anyhow::Result<(
        Vec<WalletPendingReviewSummary>,
        Vec<PendingReviewConfidenceSummary>,
        Vec<PendingReviewScoreSummary>,
    )> {
        let reviews =
            db::list_pending_wallet_reviews(&self.conn, db::max_pending_review_scan_limit())?;
        let (summary, confidence, score, _conflict) =
            db::summarize_wallet_pending_review_subset(&reviews);
        Ok((summary, confidence, score))
    }

    fn review_age_summary(
        &self,
    ) -> anyhow::Result<(PendingReviewAgeSummary, PendingReviewAgeSummary)> {
        let artist_reviews = db::list_pending_artist_identity_reviews(
            &self.conn,
            db::max_pending_review_scan_limit(),
        )?;
        let wallet_reviews =
            db::list_pending_wallet_reviews(&self.conn, db::max_pending_review_scan_limit())?;
        let artist_age =
            db::summarize_pending_review_age_subset(artist_reviews.iter().map(|r| r.created_at));
        let wallet_age =
            db::summarize_pending_review_age_subset(wallet_reviews.iter().map(|r| r.created_at));
        Ok((artist_age, wallet_age))
    }

    fn feed_hotspots(&self, limit: usize) -> anyhow::Result<Vec<PendingReviewFeedHotspot>> {
        let artist_reviews = db::list_pending_artist_identity_reviews(
            &self.conn,
            db::max_pending_review_scan_limit(),
        )?;
        let wallet_reviews =
            db::list_pending_wallet_reviews(&self.conn, db::max_pending_review_scan_limit())?;
        Ok(db::summarize_pending_review_hotspots_subset(
            &self.conn,
            &artist_reviews,
            &wallet_reviews,
            limit,
        )?)
    }

    fn feed_url(&self, feed_guid: &str) -> anyhow::Result<String> {
        let url = self.conn.query_row(
            "SELECT feed_url FROM feeds WHERE feed_guid = ?1",
            rusqlite::params![feed_guid],
            |row| row.get(0),
        )?;
        Ok(url)
    }

    fn artist_diagnostics(&self, artist_id: &str) -> anyhow::Result<Option<ArtistDiagnostics>> {
        use rusqlite::OptionalExtension;
        let artist: Option<(String, String, i64)> = self
            .conn
            .query_row(
                "SELECT artist_id, name, created_at FROM artists WHERE artist_id = ?1",
                rusqlite::params![artist_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let Some((id, name, created_at)) = artist else {
            return Ok(None);
        };

        let mut stmt = self.conn.prepare(
            "SELECT f.feed_guid, f.title, f.feed_url \
             FROM feeds f JOIN artist_feeds af ON f.feed_guid = af.feed_guid \
             WHERE af.artist_id = ?1 ORDER BY f.title",
        )?;
        let feeds = stmt
            .query_map(rusqlite::params![id], |row| {
                Ok(ArtistFeedInfo {
                    feed_guid: row.get(0)?,
                    title: row.get(1)?,
                    feed_url: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let releases = db::get_releases_for_artist(&self.conn, &id)?;
        let external_id_rows = db::get_external_ids(&self.conn, "artist", &id)?;
        let external_ids = external_id_rows
            .into_iter()
            .map(|row| model::ExternalId {
                scheme: row.scheme,
                value: row.value,
            })
            .collect();

        Ok(Some(ArtistDiagnostics {
            artist_id: id,
            name,
            created_at,
            feeds,
            release_count: releases.len(),
            external_ids,
        }))
    }

    fn feed_evidence(&self, feed_guid: &str) -> anyhow::Result<FeedEvidence> {
        let release_maps = db::get_source_feed_release_maps_for_feed(&self.conn, feed_guid)?;
        let platform_claims = db::get_source_platform_claims_for_feed(&self.conn, feed_guid)?;
        let entity_links = db::get_source_entity_links_for_entity(&self.conn, "feed", feed_guid)?;
        let entity_ids = db::get_source_entity_ids_for_entity(&self.conn, "feed", feed_guid)?;
        let remote_items = db::get_feed_remote_items_for_feed(&self.conn, feed_guid)?;

        Ok(FeedEvidence {
            release_maps,
            platform_claims,
            entity_links,
            entity_ids,
            remote_items,
        })
    }
}

// ── ApiBackend ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ApiBackend {
    client: Client,
    base_url: String,
    admin_token: String,
}

impl ApiBackend {
    pub fn new(base_url: String, admin_token: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
            base_url,
            admin_token,
        }
    }

    fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let res = self
            .client
            .get(&url)
            .header("X-Admin-Token", &self.admin_token)
            .send()?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().unwrap_or_default();
            anyhow::bail!("GET {path} failed ({status}): {body}");
        }
        Ok(serde_json::from_str(&res.text()?)?)
    }

    fn post_json<T: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> anyhow::Result<R> {
        let url = format!("{}{}", self.base_url, path);
        let res = self
            .client
            .post(&url)
            .header("X-Admin-Token", &self.admin_token)
            .json(body)
            .send()?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().unwrap_or_default();
            anyhow::bail!("POST {path} failed ({status}): {body}");
        }
        Ok(serde_json::from_str(&res.text()?)?)
    }

    fn post_empty<R: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<R> {
        let url = format!("{}{}", self.base_url, path);
        let res = self
            .client
            .post(&url)
            .header("X-Admin-Token", &self.admin_token)
            .send()?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().unwrap_or_default();
            anyhow::bail!("POST {path} failed ({status}): {body}");
        }
        Ok(serde_json::from_str(&res.text()?)?)
    }
}

impl ReviewBackend for ApiBackend {
    fn list_pending_artist_reviews(
        &self,
        limit: usize,
        confidence: Option<&str>,
        min_score: Option<u16>,
    ) -> anyhow::Result<Vec<ArtistIdentityPendingReview>> {
        use std::fmt::Write;
        let mut query = format!("?limit={limit}");
        if let Some(c) = confidence {
            let _ = write!(query, "&confidence={c}");
        }
        if let Some(s) = min_score {
            let _ = write!(query, "&min_score={s}");
        }
        let res: crate::api::PendingArtistIdentityReviewsResponse =
            self.get(&format!("/admin/artist-identity/reviews/pending{query}"))?;
        Ok(res.reviews)
    }

    fn list_stale_artist_reviews(
        &self,
        min_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<ArtistIdentityPendingReview>> {
        let days = min_age_secs / (24 * 60 * 60);
        let res: crate::api::PendingArtistIdentityReviewsResponse = self.get(&format!(
            "/admin/artist-identity/reviews/pending/stale?min_age_days={days}&limit={limit}"
        ))?;
        Ok(res.reviews)
    }

    fn list_recent_artist_reviews(
        &self,
        max_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<ArtistIdentityPendingReview>> {
        let days = max_age_secs / (24 * 60 * 60);
        let res: crate::api::PendingArtistIdentityReviewsResponse = self.get(&format!(
            "/admin/artist-identity/reviews/pending/recent?max_age_days={days}&limit={limit}"
        ))?;
        Ok(res.reviews)
    }

    fn get_artist_review(&self, id: i64) -> anyhow::Result<Option<ArtistIdentityReviewItem>> {
        let res: crate::api::ArtistIdentityReviewResponse =
            self.get(&format!("/admin/artist-identity/reviews/{id}"))?;
        Ok(res.review)
    }

    fn explain_artist_identity_for_feed(
        &self,
        feed_guid: &str,
    ) -> anyhow::Result<ArtistIdentityFeedPlan> {
        let res: crate::api::AdminFeedDiagnosticsResponse =
            self.get(&format!("/v1/diagnostics/feeds/{feed_guid}"))?;
        Ok(res.artist_identity_plan)
    }

    fn resolve_artist_review(
        &mut self,
        id: i64,
        action: &str,
        target: Option<&str>,
        note: Option<&str>,
    ) -> anyhow::Result<ArtistIdentityReviewActionOutcome> {
        let req = crate::api::ResolveArtistIdentityReviewRequest {
            action: action.to_string(),
            target_artist_id: target.map(str::to_string),
            note: note.map(str::to_string),
        };
        let res: crate::api::ResolveArtistIdentityReviewResponse = self.post_json(
            &format!("/admin/artist-identity/reviews/{id}/resolve"),
            &req,
        )?;
        Ok(ArtistIdentityReviewActionOutcome {
            review: res.review,
            resolve_stats: res.resolve_stats,
        })
    }

    fn list_pending_wallet_reviews(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<WalletReviewSummary>> {
        let res: crate::api::PendingWalletIdentityReviewsResponse = self.get(&format!(
            "/admin/wallet-identity/reviews/pending?limit={limit}"
        ))?;
        Ok(res.reviews)
    }

    fn list_stale_wallet_reviews(
        &self,
        min_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<WalletReviewSummary>> {
        let days = min_age_secs / (24 * 60 * 60);
        let res: crate::api::PendingWalletIdentityReviewsResponse = self.get(&format!(
            "/admin/wallet-identity/reviews/pending/stale?min_age_days={days}&limit={limit}"
        ))?;
        Ok(res.reviews)
    }

    fn list_recent_wallet_reviews(
        &self,
        max_age_secs: i64,
        limit: usize,
    ) -> anyhow::Result<Vec<WalletReviewSummary>> {
        let days = max_age_secs / (24 * 60 * 60);
        let res: crate::api::PendingWalletIdentityReviewsResponse = self.get(&format!(
            "/admin/wallet-identity/reviews/pending/recent?max_age_days={days}&limit={limit}"
        ))?;
        Ok(res.reviews)
    }

    fn get_wallet_alias_peers(&self, alias: &str) -> anyhow::Result<Vec<WalletAliasPeer>> {
        // The wallet diagnostics endpoint returns alias_peers for a wallet. We
        // search by alias here, which isn't a 1:1 match. The diagnostics
        // response already aggregates peers across all aliases for a wallet, so
        // we look up the wallet that owns this alias and pull from that.
        //
        // This is an O(1)-extra-request path vs the DB path; the TUI only calls
        // it for aliases already visible on the detail screen.
        let res: crate::api::AdminWalletDiagnosticsResponse =
            self.get(&format!("/v1/diagnostics/wallets/{alias}"))?;
        Ok(res.alias_peers)
    }

    fn get_wallet_detail(&self, id: &str) -> anyhow::Result<Option<WalletDetail>> {
        match self.get::<crate::api::AdminWalletDiagnosticsResponse>(&format!(
            "/v1/diagnostics/wallets/{id}"
        )) {
            Ok(res) => Ok(Some(res.wallet)),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("404") {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn get_wallet_claim_feeds(&self, id: &str) -> anyhow::Result<Vec<WalletClaimFeed>> {
        let res: crate::api::AdminWalletDiagnosticsResponse =
            self.get(&format!("/v1/diagnostics/wallets/{id}"))?;
        Ok(res.claim_feeds)
    }

    fn resolve_wallet_review(
        &mut self,
        id: i64,
        action: &str,
        target_id: Option<&str>,
        value: Option<&str>,
    ) -> anyhow::Result<WalletIdentityReviewActionOutcome> {
        let req = crate::api::ResolveWalletIdentityReviewRequest {
            action: action.to_string(),
            target_wallet_id: target_id.map(str::to_string),
            target_artist_id: None,
            value: value.map(str::to_string),
        };
        let res: crate::api::ResolveWalletIdentityReviewResponse = self.post_json(
            &format!("/admin/wallet-identity/reviews/{id}/resolve"),
            &req,
        )?;
        Ok(WalletIdentityReviewActionOutcome { review: res.review })
    }

    fn set_wallet_force_class(&mut self, id: &str, class: &str) -> anyhow::Result<()> {
        let req = crate::api::WalletForceClassRequest {
            class: class.to_string(),
        };
        let _: () = self.post_json(&format!("/admin/wallets/{id}/force-class"), &req)?;
        Ok(())
    }

    fn set_wallet_force_confidence(&mut self, id: &str, confidence: &str) -> anyhow::Result<()> {
        let req = crate::api::WalletForceConfidenceRequest {
            confidence: confidence.to_string(),
        };
        let _: () = self.post_json(&format!("/admin/wallets/{id}/force-confidence"), &req)?;
        Ok(())
    }

    fn revert_wallet_classification(&mut self, id: &str) -> anyhow::Result<()> {
        let _: () = self.post_empty(&format!("/admin/wallets/{id}/revert-classification"))?;
        Ok(())
    }

    fn apply_wallet_merges(&mut self) -> anyhow::Result<db::WalletRefreshStats> {
        self.post_empty("/admin/wallets/apply-merges")
    }

    fn undo_last_wallet_batch(&mut self) -> anyhow::Result<Option<db::WalletUndoStats>> {
        self.post_empty("/admin/wallets/undo-last-batch")
    }

    fn artist_review_summary(
        &self,
    ) -> anyhow::Result<(
        Vec<ArtistIdentityPendingReviewSummary>,
        Vec<PendingReviewConfidenceSummary>,
        Vec<PendingReviewScoreSummary>,
    )> {
        let res: crate::api::PendingArtistIdentityReviewSummaryResponse =
            self.get("/admin/artist-identity/reviews/pending/summary")?;
        Ok((res.summary, res.confidence_summary, res.score_summary))
    }

    fn wallet_review_summary(
        &self,
    ) -> anyhow::Result<(
        Vec<WalletPendingReviewSummary>,
        Vec<PendingReviewConfidenceSummary>,
        Vec<PendingReviewScoreSummary>,
    )> {
        let res: crate::api::PendingWalletIdentityReviewSummaryResponse =
            self.get("/admin/wallet-identity/reviews/pending/summary")?;
        Ok((res.summary, res.confidence_summary, res.score_summary))
    }

    fn review_age_summary(
        &self,
    ) -> anyhow::Result<(PendingReviewAgeSummary, PendingReviewAgeSummary)> {
        let res: crate::api::PendingReviewDashboardResponse =
            self.get("/admin/reviews/dashboard")?;
        Ok((
            res.age_summary.artist_identity,
            res.age_summary.wallet_identity,
        ))
    }

    fn feed_hotspots(&self, limit: usize) -> anyhow::Result<Vec<PendingReviewFeedHotspot>> {
        let res: crate::api::PendingReviewFeedHotspotsResponse =
            self.get(&format!("/admin/reviews/feeds/hotspots?limit={limit}"))?;
        Ok(res.feeds)
    }

    fn feed_url(&self, feed_guid: &str) -> anyhow::Result<String> {
        let res: crate::api::AdminFeedDiagnosticsResponse =
            self.get(&format!("/v1/diagnostics/feeds/{feed_guid}"))?;
        Ok(res.feed_url)
    }

    fn artist_diagnostics(&self, artist_id: &str) -> anyhow::Result<Option<ArtistDiagnostics>> {
        let res: crate::api::AdminArtistDiagnosticsResponse =
            match self.get(&format!("/v1/diagnostics/artists/{artist_id}")) {
                Ok(r) => r,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("404") {
                        return Ok(None);
                    }
                    return Err(e);
                }
            };

        let feeds = res
            .feeds
            .into_iter()
            .map(|f| ArtistFeedInfo {
                feed_guid: f.feed_guid,
                title: f.title,
                feed_url: f.feed_url,
            })
            .collect();

        Ok(Some(ArtistDiagnostics {
            artist_id: res.artist.artist_id,
            name: res.artist.name,
            created_at: res.artist.created_at,
            feeds,
            release_count: res.releases.len(),
            external_ids: res.external_ids,
        }))
    }

    fn feed_evidence(&self, feed_guid: &str) -> anyhow::Result<FeedEvidence> {
        let res: crate::api::FeedEvidenceResponse =
            self.get(&format!("/admin/sources/feeds/{feed_guid}/evidence"))?;
        Ok(FeedEvidence {
            release_maps: res.release_maps,
            platform_claims: res.platform_claims,
            entity_links: res.entity_links,
            entity_ids: res.entity_ids,
            remote_items: res.remote_items,
        })
    }
}
