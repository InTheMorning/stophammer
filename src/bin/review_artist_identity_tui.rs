#![allow(
    clippy::elidable_lifetime_names,
    reason = "ratatui helper signatures are clearer when lifetime names mirror widget lifetimes"
)]
#![allow(
    clippy::map_unwrap_or,
    reason = "the TUI formatter uses explicit fallback strings in several small rendering helpers"
)]
#![allow(
    clippy::needless_lifetimes,
    reason = "some helper signatures keep explicit lifetimes to document returned borrows in UI code"
)]
#![allow(
    clippy::too_many_lines,
    reason = "the review TUI keeps drawing and evidence assembly inline to stay operable without framework indirection"
)]
#![allow(
    clippy::vec_init_then_push,
    reason = "incremental construction keeps long ratatui line definitions readable"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};
use stophammer::db::DEFAULT_DB_PATH;
use stophammer::review_backend::{ApiBackend, DbBackend, ReviewBackend};
use stophammer::tui::format_local_timestamp;

#[derive(Debug)]
struct Args {
    backend: BackendArgs,
    limit: usize,
    min_score: Option<u16>,
}

#[derive(Debug)]
enum BackendArgs {
    Db { path: PathBuf },
    Api { base_url: String, admin_token: String },
}

#[derive(Debug, Clone)]
struct FeedEvidenceRow {
    feed_guid: String,
    title: String,
    feed_url: String,
    canonical_release_id: Option<String>,
    canonical_match_type: Option<String>,
    canonical_confidence: Option<i64>,
    platforms: Vec<String>,
    website_links: Vec<String>,
    npubs: Vec<String>,
    publisher_remote_feed_guids: Vec<String>,
}

#[derive(Debug, Clone)]
struct ArtistSummary {
    artist_id: String,
    name: String,
    created_at: i64,
    feed_count: i64,
    release_count: i64,
    external_ids: Vec<String>,
    feeds: Vec<FeedEvidenceRow>,
}

#[derive(Debug, Clone)]
struct ReviewSnapshot {
    pending: stophammer::db::ArtistIdentityPendingReview,
    review: stophammer::db::ArtistIdentityReviewItem,
    /// Loaded lazily on first render of the evidence pane.
    plan: Option<stophammer::db::ArtistIdentityFeedPlan>,
    feed_url: String,
    artists: Vec<ArtistSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Reviews,
    MainArtist,
    Evidence,
}

struct App {
    backend: Box<dyn ReviewBackend>,
    limit: usize,
    min_score: Option<u16>,
    reviews: Vec<stophammer::db::ArtistIdentityPendingReview>,
    queue_summary: String,
    review_state: ListState,
    artist_state: ListState,
    focus: Focus,
    snapshot: Option<ReviewSnapshot>,
    evidence_scroll: u16,
    status: String,
    dialog: Option<stophammer::tui::TextDialog>,
}

impl App {
    fn new(
        backend: Box<dyn ReviewBackend>,
        limit: usize,
        min_score: Option<u16>,
    ) -> Result<Self, Box<dyn Error>> {
        let mut app = Self {
            backend,
            limit,
            min_score,
            reviews: Vec::new(),
            queue_summary: String::new(),
            review_state: ListState::default(),
            artist_state: ListState::default(),
            focus: Focus::Reviews,
            snapshot: None,
            evidence_scroll: 0,
            status: "Loading pending artist identity reviews...".to_string(),
            dialog: None,
        };
        app.reload(None, None)?;
        Ok(app)
    }

    fn reload(
        &mut self,
        preferred_review_id: Option<i64>,
        preferred_artist_id: Option<&str>,
    ) -> Result<(), Box<dyn Error>> {
        self.reviews =
            self.backend
                .list_pending_artist_reviews(self.limit, None, self.min_score)?;
        let mut source_counts = BTreeMap::<String, usize>::new();
        let mut confidence_counts = BTreeMap::<String, usize>::new();
        for review in &self.reviews {
            *source_counts.entry(review.source.clone()).or_default() += 1;
            *confidence_counts
                .entry(review.confidence.clone())
                .or_default() += 1;
        }
        let source_summary = stophammer::tui::format_source_count_summary(
            "artist reviews",
            source_counts
                .iter()
                .map(|(source, count)| (source.as_str(), *count)),
        );
        self.queue_summary = stophammer::tui::format_confidence_band_hint(
            confidence_counts
                .iter()
                .map(|(confidence, count)| (confidence.as_str(), *count)),
        )
        .map_or(source_summary.clone(), |hint| {
            format!("{source_summary} | {hint}")
        });
        if confidence_counts
            .iter()
            .any(|(confidence, count)| confidence == "high_confidence" && *count > 0)
        {
            self.queue_summary.push_str(" | H=list");
        }
        let review_idx = match preferred_review_id {
            Some(review_id) => self
                .reviews
                .iter()
                .position(|review| review.review_id == review_id)
                .unwrap_or(0),
            None => self.review_state.selected().unwrap_or(0),
        };

        if self.reviews.is_empty() {
            self.review_state.select(None);
            self.artist_state.select(None);
            self.snapshot = None;
            self.evidence_scroll = 0;
            self.status = "No pending artist identity reviews.".to_string();
            self.queue_summary = "No pending artist identity reviews".to_string();
            return Ok(());
        }

        self.review_state
            .select(Some(review_idx.min(self.reviews.len().saturating_sub(1))));
        self.load_selected_review(preferred_artist_id)?;
        Ok(())
    }

    fn load_selected_review(
        &mut self,
        preferred_artist_id: Option<&str>,
    ) -> Result<(), Box<dyn Error>> {
        let Some(review_idx) = self.review_state.selected() else {
            self.snapshot = None;
            self.artist_state.select(None);
            return Ok(());
        };
        let Some(pending) = self.reviews.get(review_idx).cloned() else {
            self.snapshot = None;
            self.artist_state.select(None);
            return Ok(());
        };

        let review = self
            .backend
            .get_artist_review(pending.review_id)?
            .ok_or_else(|| io::Error::other(format!("review missing: {}", pending.review_id)))?;
        let feed_url = self.backend.feed_url(&pending.feed_guid)?;
        let artists = load_artist_summaries(self.backend.as_ref(), &review.artist_ids)?;

        let artist_idx = match preferred_artist_id {
            Some(artist_id) => artists
                .iter()
                .position(|artist| artist.artist_id == artist_id)
                .unwrap_or(0),
            None => self.artist_state.selected().unwrap_or(0),
        };

        self.snapshot = Some(ReviewSnapshot {
            pending: pending.clone(),
            review,
            plan: None, // loaded lazily on first render of the evidence pane
            feed_url,
            artists,
        });

        let artist_count = self
            .snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.artists.len());
        self.artist_state
            .select((artist_count > 0).then_some(artist_idx.min(artist_count.saturating_sub(1))));
        self.evidence_scroll = 0;
        self.status = format!(
            "Loaded review {} for {:?} in feed {:?}.",
            pending.review_id, pending.name_key, pending.title
        );
        Ok(())
    }

    fn ensure_plan_loaded(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(snapshot) = self.snapshot.as_mut() else {
            return Ok(());
        };
        if snapshot.plan.is_some() {
            return Ok(());
        }
        snapshot.plan = Some(
            self.backend
                .explain_artist_identity_for_feed(&snapshot.pending.feed_guid)?,
        );
        Ok(())
    }

    fn current_pending_review(&self) -> Option<&stophammer::db::ArtistIdentityPendingReview> {
        self.review_state
            .selected()
            .and_then(|idx| self.reviews.get(idx))
    }

    fn current_snapshot(&self) -> Option<&ReviewSnapshot> {
        self.snapshot.as_ref()
    }

    fn current_main_artist(&self) -> Option<&ArtistSummary> {
        let idx = self.artist_state.selected()?;
        self.snapshot.as_ref()?.artists.get(idx)
    }

    fn next_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Reviews => Focus::MainArtist,
            Focus::MainArtist => Focus::Evidence,
            Focus::Evidence => Focus::Reviews,
        };
    }

    fn previous_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Reviews => Focus::Evidence,
            Focus::MainArtist => Focus::Reviews,
            Focus::Evidence => Focus::MainArtist,
        };
    }

    fn move_down(&mut self) -> Result<(), Box<dyn Error>> {
        match self.focus {
            Focus::Reviews => {
                if self.reviews.is_empty() {
                    return Ok(());
                }
                let current = self.review_state.selected().unwrap_or(0);
                let next = (current + 1).min(self.reviews.len().saturating_sub(1));
                self.review_state.select(Some(next));
                self.load_selected_review(None)?;
            }
            Focus::MainArtist => {
                let Some(snapshot) = self.snapshot.as_ref() else {
                    return Ok(());
                };
                if snapshot.artists.is_empty() {
                    return Ok(());
                }
                let current = self.artist_state.selected().unwrap_or(0);
                let next = (current + 1).min(snapshot.artists.len().saturating_sub(1));
                self.artist_state.select(Some(next));
                self.evidence_scroll = 0;
            }
            Focus::Evidence => {
                self.evidence_scroll = self.evidence_scroll.saturating_add(1);
            }
        }
        Ok(())
    }

    fn move_up(&mut self) -> Result<(), Box<dyn Error>> {
        match self.focus {
            Focus::Reviews => {
                if self.reviews.is_empty() {
                    return Ok(());
                }
                let current = self.review_state.selected().unwrap_or(0);
                let next = current.saturating_sub(1);
                self.review_state.select(Some(next));
                self.load_selected_review(None)?;
            }
            Focus::MainArtist => {
                if self.snapshot.is_none() {
                    return Ok(());
                }
                let current = self.artist_state.selected().unwrap_or(0);
                self.artist_state.select(Some(current.saturating_sub(1)));
                self.evidence_scroll = 0;
            }
            Focus::Evidence => {
                self.evidence_scroll = self.evidence_scroll.saturating_sub(1);
            }
        }
        Ok(())
    }

    fn jump_top(&mut self) -> Result<(), Box<dyn Error>> {
        match self.focus {
            Focus::Reviews => {
                if !self.reviews.is_empty() {
                    self.review_state.select(Some(0));
                    self.load_selected_review(None)?;
                }
            }
            Focus::MainArtist => {
                if self.snapshot.is_some() {
                    self.artist_state.select(Some(0));
                    self.evidence_scroll = 0;
                }
            }
            Focus::Evidence => {
                self.evidence_scroll = 0;
            }
        }
        Ok(())
    }

    fn jump_bottom(&mut self) -> Result<(), Box<dyn Error>> {
        match self.focus {
            Focus::Reviews => {
                if !self.reviews.is_empty() {
                    self.review_state
                        .select(Some(self.reviews.len().saturating_sub(1)));
                    self.load_selected_review(None)?;
                }
            }
            Focus::MainArtist => {
                if let Some(snapshot) = self.snapshot.as_ref()
                    && !snapshot.artists.is_empty()
                {
                    self.artist_state
                        .select(Some(snapshot.artists.len().saturating_sub(1)));
                    self.evidence_scroll = 0;
                }
            }
            Focus::Evidence => {
                self.evidence_scroll = u16::MAX;
            }
        }
        Ok(())
    }

    fn jump_next_same_source(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(current_review) = self.current_pending_review() else {
            return Ok(());
        };
        let Some(current_index) = self.review_state.selected() else {
            return Ok(());
        };
        let source = current_review.source.clone();
        let next_index = ((current_index + 1)..self.reviews.len())
            .chain(0..current_index)
            .find(|&index| self.reviews[index].source == source);
        if let Some(index) = next_index {
            self.review_state.select(Some(index));
            self.load_selected_review(None)?;
        }
        Ok(())
    }

    fn jump_previous_same_source(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(current_review) = self.current_pending_review() else {
            return Ok(());
        };
        let Some(current_index) = self.review_state.selected() else {
            return Ok(());
        };
        let source = current_review.source.clone();
        let previous_index = (0..current_index)
            .rev()
            .chain(((current_index + 1)..self.reviews.len()).rev())
            .find(|&index| self.reviews[index].source == source);
        if let Some(index) = previous_index {
            self.review_state.select(Some(index));
            self.load_selected_review(None)?;
        }
        Ok(())
    }

    fn jump_next_high_confidence(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(current_index) = self.review_state.selected() else {
            return Ok(());
        };
        let matching = self
            .reviews
            .iter()
            .enumerate()
            .filter(|(_, review)| review.confidence == "high_confidence")
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            self.status = "No HIGH-confidence artist reviews loaded.".to_string();
            return Ok(());
        }
        if matching.len() == 1 && matching[0] == current_index {
            self.status = "Only one HIGH-confidence artist review is loaded.".to_string();
            return Ok(());
        }
        let next_index = matching
            .iter()
            .copied()
            .find(|&index| index > current_index)
            .or_else(|| matching.first().copied());
        if let Some(index) = next_index {
            self.review_state.select(Some(index));
            self.load_selected_review(None)?;
            self.status = format!(
                "Jumped to HIGH-confidence artist review {} of {}.",
                matching
                    .iter()
                    .position(|&candidate| candidate == index)
                    .map_or(1, |position| position + 1),
                matching.len()
            );
        }
        Ok(())
    }

    fn jump_previous_high_confidence(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(current_index) = self.review_state.selected() else {
            return Ok(());
        };
        let matching = self
            .reviews
            .iter()
            .enumerate()
            .filter(|(_, review)| review.confidence == "high_confidence")
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            self.status = "No HIGH-confidence artist reviews loaded.".to_string();
            return Ok(());
        }
        if matching.len() == 1 && matching[0] == current_index {
            self.status = "Only one HIGH-confidence artist review is loaded.".to_string();
            return Ok(());
        }
        let previous_index = matching
            .iter()
            .rev()
            .copied()
            .find(|&index| index < current_index)
            .or_else(|| matching.last().copied());
        if let Some(index) = previous_index {
            self.review_state.select(Some(index));
            self.load_selected_review(None)?;
            self.status = format!(
                "Jumped to HIGH-confidence artist review {} of {}.",
                matching
                    .iter()
                    .position(|&candidate| candidate == index)
                    .map_or(1, |position| position + 1),
                matching.len()
            );
        }
        Ok(())
    }

    fn approve_merge(&mut self) -> Result<(), Box<dyn Error>> {
        let (review_id, feed_guid, review_label) = {
            let Some(snapshot) = self.snapshot.as_ref() else {
                return Ok(());
            };
            (
                snapshot.review.review_id,
                snapshot.review.feed_guid.clone(),
                snapshot.review.name_key.clone(),
            )
        };
        let Some(main_artist) = self.current_main_artist() else {
            return Ok(());
        };
        let target_artist_id = main_artist.artist_id.clone();
        let target_name = main_artist.name.clone();

        let outcome =
            self.backend
                .resolve_artist_review(review_id, "merge", Some(&target_artist_id), None)?;

        self.dialog = Some(stophammer::tui::text_dialog(
            "Artist Merge Applied",
            vec![
                format!("Review {review_id} now targets {target_name} [{target_artist_id}]."),
                format!("Feed: {feed_guid}"),
                format!("Name key: {review_label}"),
                format!("Seed artists: {}", outcome.resolve_stats.seed_artists),
                format!(
                    "Candidate groups: {}",
                    outcome.resolve_stats.candidate_groups
                ),
                format!(
                    "Groups processed: {}",
                    outcome.resolve_stats.groups_processed
                ),
                format!("Merges applied: {}", outcome.resolve_stats.merges_applied),
                format!("Pending reviews: {}", outcome.resolve_stats.pending_reviews),
                format!("Blocked reviews: {}", outcome.resolve_stats.blocked_reviews),
            ],
        ));

        self.reload(Some(review_id), Some(target_artist_id.as_str()))?;
        Ok(())
    }

    fn reject_review(&mut self) -> Result<(), Box<dyn Error>> {
        let (review_id, feed_guid, name_key) = {
            let Some(snapshot) = self.snapshot.as_ref() else {
                return Ok(());
            };
            (
                snapshot.review.review_id,
                snapshot.review.feed_guid.clone(),
                snapshot.review.name_key.clone(),
            )
        };

        let outcome = self
            .backend
            .resolve_artist_review(review_id, "do_not_merge", None, None)?;
        self.dialog = Some(stophammer::tui::text_dialog(
            "Artist Review Blocked",
            vec![
                format!(
                    "Review {review_id} for {:?} was marked do_not_merge.",
                    name_key
                ),
                format!("Feed: {feed_guid}"),
                format!(
                    "Groups processed: {}",
                    outcome.resolve_stats.groups_processed
                ),
                format!("Merges applied: {}", outcome.resolve_stats.merges_applied),
                format!("Pending reviews: {}", outcome.resolve_stats.pending_reviews),
                format!("Blocked reviews: {}", outcome.resolve_stats.blocked_reviews),
            ],
        ));
        self.reload(Some(review_id), None)?;
        Ok(())
    }

    fn show_queue_summary(&mut self) -> Result<(), Box<dyn Error>> {
        let (summary, confidence_summary, score_summary) = self.backend.artist_review_summary()?;
        let conflict_summary = stophammer::tui::summarize_reason_counts(
            self.reviews
                .iter()
                .flat_map(|review| review.conflict_reasons.iter().map(String::as_str)),
        );
        let (age, _wallet_age) = self.backend.review_age_summary()?;
        let total: usize = summary.iter().map(|item| item.count).sum();
        let mut lines = stophammer::tui::build_queue_summary_header_lines(
            "artist reviews",
            total,
            age.created_last_24h,
            age.older_than_7d,
            age.oldest_created_at,
        );
        lines.push(String::new());
        stophammer::tui::push_confidence_summary_section(
            &mut lines,
            "Confidence bands:",
            confidence_summary
                .iter()
                .map(|item| (item.confidence.as_str(), item.count)),
        );
        lines.push(String::new());
        stophammer::tui::push_score_summary_section(
            &mut lines,
            "Score bands:",
            score_summary
                .iter()
                .map(|item| (item.score_band.as_str(), item.count)),
        );
        lines.push(String::new());
        stophammer::tui::push_conflict_summary_section(
            &mut lines,
            "Conflict reasons:",
            conflict_summary
                .iter()
                .map(|(reason, count)| (reason.as_str(), *count)),
        );
        lines.push(String::new());
        lines.extend(stophammer::tui::build_queue_summary_lines(
            summary
                .iter()
                .map(|item| (item.source.as_str(), item.count)),
            total,
            "No pending artist review sources",
            "Use n/N to stay within it.",
        ));
        self.dialog = Some(stophammer::tui::counted_dialog(
            "Artist Queue Summary",
            total,
            lines,
        ));
        Ok(())
    }

    fn show_feed_hotspots(&mut self) -> Result<(), Box<dyn Error>> {
        let hotspots = self.backend.feed_hotspots(10)?;
        let hotspot_count = hotspots.len();
        let lines =
            stophammer::tui::build_feed_hotspot_dialog_lines(&hotspots, short_id, abbreviate);
        self.dialog = Some(stophammer::tui::counted_dialog(
            "Feed Hotspots",
            hotspot_count,
            lines,
        ));
        Ok(())
    }

    fn show_operator_overview(&mut self) -> Result<(), Box<dyn Error>> {
        let (artist_summary, artist_confidence_summary, artist_score_summary) =
            self.backend.artist_review_summary()?;
        let (wallet_summary, wallet_confidence_summary, wallet_score_summary) =
            self.backend.wallet_review_summary()?;
        let artist_conflict_summary = stophammer::tui::summarize_reason_counts(
            self.reviews
                .iter()
                .flat_map(|review| review.conflict_reasons.iter().map(String::as_str)),
        );
        let wallet_conflict_summary = stophammer::tui::summarize_reason_counts(
            self.backend
                .list_pending_wallet_reviews(self.limit, None, self.min_score)?
                .iter()
                .flat_map(|review| review.conflict_reasons.iter().map(String::as_str)),
        );
        let (artist_age, wallet_age) = self.backend.review_age_summary()?;
        let hotspots = self.backend.feed_hotspots(5)?;
        let hotspot_count = hotspots.len();

        let artist_total: usize = artist_summary.iter().map(|item| item.count).sum();
        let wallet_total: usize = wallet_summary.iter().map(|item| item.count).sum();
        let lines = stophammer::tui::build_operator_overview_lines(
            artist_summary
                .iter()
                .map(|item| (item.source.as_str(), item.count)),
            wallet_summary
                .iter()
                .map(|item| (item.source.as_str(), item.count)),
            &hotspots,
            stophammer::tui::OperatorOverviewConfig {
                artist_total,
                artist_age: &artist_age,
                wallet_total,
                wallet_age: &wallet_age,
                artist_dominant_suffix: "Use n/N to stay within it.",
                wallet_dominant_suffix: "Use n/N in the wallet TUI to stay within it.",
            },
            short_id,
            abbreviate,
        );
        let mut lines = lines;
        lines.push(String::new());
        stophammer::tui::push_confidence_summary_section(
            &mut lines,
            "Artist confidence bands:",
            artist_confidence_summary
                .iter()
                .map(|item| (item.confidence.as_str(), item.count)),
        );
        lines.push(String::new());
        stophammer::tui::push_score_summary_section(
            &mut lines,
            "Artist score bands:",
            artist_score_summary
                .iter()
                .map(|item| (item.score_band.as_str(), item.count)),
        );
        lines.push(String::new());
        stophammer::tui::push_conflict_summary_section(
            &mut lines,
            "Artist conflict reasons:",
            artist_conflict_summary
                .iter()
                .map(|(reason, count)| (reason.as_str(), *count)),
        );
        lines.push(String::new());
        stophammer::tui::push_confidence_summary_section(
            &mut lines,
            "Wallet confidence bands:",
            wallet_confidence_summary
                .iter()
                .map(|item| (item.confidence.as_str(), item.count)),
        );
        lines.push(String::new());
        stophammer::tui::push_score_summary_section(
            &mut lines,
            "Wallet score bands:",
            wallet_score_summary
                .iter()
                .map(|item| (item.score_band.as_str(), item.count)),
        );
        lines.push(String::new());
        stophammer::tui::push_conflict_summary_section(
            &mut lines,
            "Wallet conflict reasons:",
            wallet_conflict_summary
                .iter()
                .map(|(reason, count)| (reason.as_str(), *count)),
        );
        self.dialog = Some(stophammer::tui::operator_overview_dialog(
            artist_total,
            wallet_total,
            hotspot_count,
            lines,
        ));
        Ok(())
    }

    fn show_stale_reviews(&mut self) -> Result<(), Box<dyn Error>> {
        let stale = self
            .backend
            .list_stale_artist_reviews(7 * 24 * 60 * 60, 10)?;
        let stale_count = stale.len();
        let lines = stophammer::tui::build_review_subset_lines(
            "Pending artist reviews older than 7 days",
            "No stale artist reviews",
            &stale,
            |review| review.source.as_str(),
            |review| {
                format!(
                    "{} [{}] | review={} | {} | key={} | {} | created {}",
                    review.title,
                    short_id(&review.feed_guid),
                    review.review_id,
                    review.source,
                    abbreviate(&review.evidence_key, 24),
                    review.artist_count,
                    format_local_timestamp(review.created_at)
                )
            },
        );
        self.dialog = Some(stophammer::tui::counted_dialog(
            "Stale Artist Reviews",
            stale_count,
            lines,
        ));
        Ok(())
    }

    fn show_recent_reviews(&mut self) -> Result<(), Box<dyn Error>> {
        let recent = self
            .backend
            .list_recent_artist_reviews(24 * 60 * 60, 10)?;
        let recent_count = recent.len();
        let lines = stophammer::tui::build_review_subset_lines(
            "Pending artist reviews created in the last 24 hours",
            "No recent artist reviews",
            &recent,
            |review| review.source.as_str(),
            |review| {
                format!(
                    "{} [{}] | review={} | {} | key={} | {} | created {}",
                    review.title,
                    short_id(&review.feed_guid),
                    review.review_id,
                    review.source,
                    abbreviate(&review.evidence_key, 24),
                    review.artist_count,
                    format_local_timestamp(review.created_at)
                )
            },
        );
        self.dialog = Some(stophammer::tui::counted_dialog(
            "Recent Artist Reviews",
            recent_count,
            lines,
        ));
        Ok(())
    }

    fn show_high_confidence_reviews(&mut self) {
        let high_confidence = self
            .reviews
            .iter()
            .filter(|review| review.confidence == "high_confidence")
            .collect::<Vec<_>>();
        let count = high_confidence.len();
        let lines = stophammer::tui::build_review_subset_lines(
            "Pending artist reviews marked high confidence",
            "No high-confidence artist reviews",
            &high_confidence,
            |review| review.source.as_str(),
            |review| {
                let score = review
                    .score
                    .map_or_else(|| "unscored".to_string(), |score| format!("score={score}"));
                let conflicts = if review.conflict_reasons.is_empty() {
                    String::new()
                } else {
                    format!(
                        " | conflicts={}",
                        preview_join(&review.conflict_reasons, 2, 18)
                    )
                };
                format!(
                    "{} [{}] | review={} | {} | key={} | {} | {}{}",
                    review.title,
                    short_id(&review.feed_guid),
                    review.review_id,
                    review.source,
                    abbreviate(&review.evidence_key, 24),
                    review.artist_count,
                    score,
                    conflicts,
                )
            },
        );
        self.dialog = Some(stophammer::tui::counted_dialog(
            "High-Confidence Artist Reviews",
            count,
            lines,
        ));
    }

    fn show_help_dialog(&mut self) {
        let mut lines = vec![
            "Tab / Shift-Tab: cycle focus".to_string(),
            "Up / Down / Home / End: navigate".to_string(),
            "m: merge into selected main artist".to_string(),
            "x: mark review do_not_merge".to_string(),
        ];
        lines.extend(stophammer::tui::review_operator_help_lines(
            "n / N: next / previous review with same source family",
        ));
        lines.extend([
            "H: list high-confidence reviews".to_string(),
            "r: reload pending reviews".to_string(),
            "Enter / Space / Esc: close dialog".to_string(),
            "q: quit".to_string(),
        ]);
        self.dialog = Some(stophammer::tui::counted_dialog(
            "Artist Review TUI Help",
            self.reviews.len(),
            lines,
        ));
    }

    fn show_review_playbook(&mut self) -> Result<(), Box<dyn Error>> {
        let (summary, confidence_summary, _score_summary) = self.backend.artist_review_summary()?;
        let (age, _wallet_age) = self.backend.review_age_summary()?;
        let hotspots = self.backend.feed_hotspots(3)?;
        let total: usize = summary.iter().map(|item| item.count).sum();
        let lines = stophammer::tui::build_review_playbook_lines(
            total,
            summary
                .iter()
                .map(|item| (item.source.as_str(), item.count)),
            confidence_summary
                .iter()
                .map(|item| (item.confidence.as_str(), item.count)),
            &hotspots,
            stophammer::tui::ReviewPlaybookConfig {
                review_label_plural: "artist reviews",
                created_last_24h: age.created_last_24h,
                older_than_7d: age.older_than_7d,
                backlog_idle_message: "Nothing pending. Reload after the next resolver pass.",
                dominant_family_walk_template: "   Use n/N to walk the '{}' family quickly before switching heuristics.",
                final_step: "4. Use o/s/h/t/y/H to inspect overview, sources, hotspots, stale, recent, and high-confidence items.",
            },
            short_id,
            abbreviate,
        );

        self.dialog = Some(stophammer::tui::counted_dialog(
            "Artist Review Playbook",
            total,
            lines,
        ));
        Ok(())
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "manual CLI parsing keeps the review tool dependency-free"
)]
fn parse_args() -> Result<Args, String> {
    let mut db_path = None;
    let mut node = std::env::var("NODE").ok();
    let mut admin_token = std::env::var("ADMIN_TOKEN").ok();
    let mut limit = 50usize;
    let mut min_score = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--db requires a path".to_string())?;
                db_path = Some(PathBuf::from(value));
            }
            "--node" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--node requires a base URL".to_string())?;
                node = Some(value);
            }
            "--admin-token" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--admin-token requires a token".to_string())?;
                admin_token = Some(value);
            }
            "--limit" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--limit requires a number".to_string())?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_err| format!("invalid --limit value: {value}"))?;
            }
            "--min-score" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--min-score requires a number".to_string())?;
                min_score = Some(
                    value
                        .parse::<u16>()
                        .map_err(|_err| format!("invalid --min-score value: {value}"))?,
                );
            }
            "--help" | "-h" => {
                println!(
                    "Usage: review_artist_identity_tui [--db PATH | --node URL --admin-token TOKEN] [--limit N] [--min-score N]\n\
                     Interactive artist identity review tool.\n\
                     API mode also reads NODE and ADMIN_TOKEN from the environment.\n\
                     Lets operators choose a main artist for each pending feed-scoped review,\n\
                     inspect supporting feed evidence, then apply merge or do-not-merge decisions.\n\
                     Keys: Tab/Shift-Tab focus, m merge, x do-not-merge, o overview, p playbook, s queue summary, h feed hotspots, t stale reviews, y recent reviews, H HIGH-confidence list, n/N same-source-family jump, g/G HIGH-confidence jump, ? help, r reload, q quit."
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let backend = match (db_path, node) {
        (Some(_), Some(_)) => {
            return Err("choose either --db or --node/ADMIN_TOKEN, not both".to_string())
        }
        (Some(path), None) => BackendArgs::Db { path },
        (None, Some(base_url)) => BackendArgs::Api {
            base_url,
            admin_token: admin_token.ok_or_else(|| {
                "--admin-token or ADMIN_TOKEN is required when using --node/NODE".to_string()
            })?,
        },
        (None, None) => BackendArgs::Db {
            path: PathBuf::from(DEFAULT_DB_PATH),
        },
    };

    Ok(Args {
        backend,
        limit,
        min_score,
    })
}

fn feed_evidence_row(
    backend: &dyn ReviewBackend,
    feed_guid: &str,
    title: String,
    feed_url: String,
) -> Result<FeedEvidenceRow, Box<dyn Error>> {
    let evidence = backend.feed_evidence(feed_guid)?;
    let canonical = evidence.release_maps.first();
    let platforms = evidence
        .platform_claims
        .into_iter()
        .map(|claim| claim.platform_key)
        .filter(|value| !value.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let website_links = evidence
        .entity_links
        .into_iter()
        .filter(|claim| claim.link_type == "website")
        .map(|claim| claim.url)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let npubs = evidence
        .entity_ids
        .into_iter()
        .filter(|claim| claim.scheme == "nostr_npub")
        .map(|claim| claim.value)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let publisher_remote_feed_guids = evidence
        .remote_items
        .into_iter()
        .filter(|item| item.medium.as_deref() == Some("publisher"))
        .map(|item| item.remote_feed_guid)
        .filter(|value| !value.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    Ok(FeedEvidenceRow {
        feed_guid: feed_guid.to_string(),
        title,
        feed_url,
        canonical_release_id: canonical.map(|row| row.release_id.clone()),
        canonical_match_type: canonical.map(|row| row.match_type.clone()),
        canonical_confidence: canonical.map(|row| row.confidence),
        platforms,
        website_links,
        npubs,
        publisher_remote_feed_guids,
    })
}

fn load_artist_summaries(
    backend: &dyn ReviewBackend,
    artist_ids: &[String],
) -> Result<Vec<ArtistSummary>, Box<dyn Error>> {
    let mut artists = Vec::new();
    for artist_id in artist_ids {
        let Some(diagnostics) = backend.artist_diagnostics(artist_id)? else {
            continue;
        };
        let feed_count = diagnostics.feeds.len();
        let release_count = diagnostics.release_count;
        let mut feeds = Vec::new();
        for feed in diagnostics.feeds {
            feeds.push(feed_evidence_row(
                backend,
                &feed.feed_guid,
                feed.title,
                feed.feed_url,
            )?);
        }
        artists.push(ArtistSummary {
            artist_id: diagnostics.artist_id,
            name: diagnostics.name,
            created_at: diagnostics.created_at,
            feed_count: i64::try_from(feed_count).unwrap_or(i64::MAX),
            release_count: i64::try_from(release_count).unwrap_or(i64::MAX),
            external_ids: diagnostics
                .external_ids
                .into_iter()
                .map(|row| format!("{}={}", row.scheme, row.value))
                .collect(),
            feeds,
        });
    }
    Ok(artists)
}

fn abbreviate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let head = keep / 2;
    let tail = keep.saturating_sub(head);
    let prefix = value.chars().take(head).collect::<String>();
    let suffix = value
        .chars()
        .rev()
        .take(tail)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{prefix}…{suffix}")
}

fn short_id(value: &str) -> String {
    abbreviate(value, 12)
}

fn preview_join(values: &[String], max_items: usize, max_chars: usize) -> String {
    stophammer::tui::preview_join(values, max_items, max_chars, abbreviate)
}

fn build_review_items(app: &App) -> Vec<ListItem<'static>> {
    if app.reviews.is_empty() {
        return vec![ListItem::new("No pending reviews")];
    }

    app.reviews
        .iter()
        .map(|review| {
            let (badge, badge_color) = stophammer::tui::recency_badge(review.created_at);
            let same_source_count = app
                .reviews
                .iter()
                .filter(|candidate| candidate.source == review.source)
                .count();
            let mut lines = vec![
                Line::from(vec![Span::styled(
                    format!("{} [{}]", review.title, review.review_id),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(vec![
                    Span::styled(review.name_key.clone(), Style::default().fg(Color::Cyan)),
                    Span::raw("  "),
                    Span::styled(review.source.clone(), Style::default().fg(Color::Yellow)),
                    Span::raw("  "),
                    Span::styled(
                        stophammer::tui::review_confidence_badge(&review.confidence),
                        stophammer::tui::review_confidence_style(&review.confidence),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("family={same_source_count}"),
                        Style::default().fg(Color::LightBlue),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        review.score.map_or_else(
                            || "score=-".to_string(),
                            |score| format!("score={score}"),
                        ),
                        Style::default().fg(Color::Green),
                    ),
                    Span::raw("  "),
                    Span::styled(badge, Style::default().fg(badge_color)),
                    Span::raw("  "),
                    Span::styled(
                        format!("key={}", abbreviate(&review.evidence_key, 18)),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(Span::styled(
                    format!(
                        "{} artists  {}  created {}",
                        review.artist_count,
                        abbreviate(&review.feed_guid, 20),
                        format_local_timestamp(review.created_at)
                    ),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    abbreviate(&review.explanation, 88),
                    Style::default().fg(Color::Gray),
                )),
            ];
            if !review.supporting_sources.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!(
                        "support={}",
                        preview_join(&review.supporting_sources, 3, 42)
                    ),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            if !review.conflict_reasons.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!(
                        "conflicts={}",
                        preview_join(&review.conflict_reasons, 2, 36)
                    ),
                    Style::default().fg(Color::Red),
                )));
            }
            ListItem::new(lines)
        })
        .collect()
}

fn build_artist_items(app: &App) -> Vec<ListItem<'static>> {
    let Some(snapshot) = app.current_snapshot() else {
        return vec![ListItem::new("No artists")];
    };
    if snapshot.artists.is_empty() {
        return vec![ListItem::new("No artists")];
    }

    snapshot
        .artists
        .iter()
        .map(|artist| {
            ListItem::new(vec![
                Line::from(Span::styled(
                    format!("{} [{}]", artist.name, short_id(&artist.artist_id)),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    preview_join(&artist.external_ids, 2, 28),
                    Style::default().fg(Color::LightBlue),
                )),
                Line::from(vec![
                    Span::styled(
                        format!("{} feeds", artist.feed_count),
                        Style::default().fg(Color::Green),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("{} releases", artist.release_count),
                        Style::default().fg(Color::Yellow),
                    ),
                ]),
                Line::from(Span::styled(
                    format!("created {}", format_local_timestamp(artist.created_at)),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        })
        .collect()
}

fn matching_candidate_group<'a>(
    snapshot: &'a ReviewSnapshot,
) -> Option<&'a stophammer::db::ArtistIdentityCandidateGroup> {
    snapshot
        .plan
        .as_ref()?
        .candidate_groups
        .iter()
        .find(|group| {
            group.source == snapshot.review.source
                && group.name_key == snapshot.review.name_key
                && group.evidence_key == snapshot.review.evidence_key
        })
}

fn build_evidence_lines(app: &App) -> Vec<Line<'static>> {
    let Some(snapshot) = app.current_snapshot() else {
        return vec![Line::from("No review selected.")];
    };
    let mut lines = Vec::new();
    let selected_main_id = app
        .current_main_artist()
        .map(|artist| artist.artist_id.clone());

    lines.push(Line::from(vec![
        Span::styled("Task: ", Style::default().fg(Color::Cyan)),
        Span::styled(
            "Choose the canonical main artist for this review group. Press M to merge the other artists into the selected main artist, or X to block this merge.",
            Style::default().fg(Color::White),
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Feed: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(
                "{} [{}]",
                snapshot.pending.title,
                abbreviate(&snapshot.pending.feed_guid, 18)
            ),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Source: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            snapshot.review.source.clone(),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled("Name: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            snapshot.review.name_key.clone(),
            Style::default().fg(Color::Cyan),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Evidence key: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            abbreviate(&snapshot.review.evidence_key, 80),
            Style::default().fg(Color::White),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Seed artists: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            snapshot
                .plan
                .as_ref()
                .map(|plan| {
                    plan.seed_artists
                        .iter()
                        .map(|artist| format!("{} [{}]", artist.name, short_id(&artist.artist_id)))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default(),
            Style::default().fg(Color::LightBlue),
        ),
    ]));

    if let Some(group) = matching_candidate_group(snapshot) {
        lines.push(Line::from(vec![
            Span::styled("Review row: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "status={} override={} target={}",
                    group.review_status.as_deref().unwrap_or("-"),
                    group.override_type.as_deref().unwrap_or("-"),
                    group
                        .target_artist_id
                        .as_deref()
                        .map(short_id)
                        .unwrap_or_else(|| "-".to_string())
                ),
                Style::default().fg(Color::White),
            ),
        ]));
        if let Some(note) = &group.note {
            lines.push(Line::from(vec![
                Span::styled("Note: ", Style::default().fg(Color::DarkGray)),
                Span::styled(note.clone(), Style::default().fg(Color::White)),
            ]));
        }
    }

    for artist in &snapshot.artists {
        let is_main = selected_main_id.as_deref() == Some(artist.artist_id.as_str());
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                if is_main { "MAIN " } else { "ARTIST " },
                Style::default()
                    .fg(if is_main { Color::Green } else { Color::Yellow })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} [{}]", artist.name, artist.artist_id),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Counts: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{} feeds  {} releases",
                    artist.feed_count, artist.release_count
                ),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Created: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format_local_timestamp(artist.created_at),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("External IDs: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                preview_join(&artist.external_ids, 6, 48),
                Style::default().fg(Color::LightBlue),
            ),
        ]));

        for feed in &artist.feeds {
            lines.push(Line::from(vec![
                Span::styled("  feed ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} [{}]", feed.title, abbreviate(&feed.feed_guid, 18)),
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("    url: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    abbreviate(&feed.feed_url, 100),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));

            let mut evidence_bits = Vec::new();
            if let Some(release_id) = &feed.canonical_release_id {
                evidence_bits.push(format!(
                    "release={} ({}/{})",
                    short_id(release_id),
                    feed.canonical_match_type.as_deref().unwrap_or("-"),
                    feed.canonical_confidence
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "-".to_string())
                ));
            }
            if !feed.platforms.is_empty() {
                evidence_bits.push(format!("platforms={}", feed.platforms.join(", ")));
            }
            if !feed.website_links.is_empty() {
                evidence_bits.push(format!(
                    "websites={}",
                    feed.website_links
                        .iter()
                        .map(|url| abbreviate(url, 44))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !feed.npubs.is_empty() {
                evidence_bits.push(format!(
                    "npubs={}",
                    feed.npubs
                        .iter()
                        .map(|npub| abbreviate(npub, 24))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !feed.publisher_remote_feed_guids.is_empty() {
                evidence_bits.push(format!(
                    "publisher={}",
                    feed.publisher_remote_feed_guids
                        .iter()
                        .map(|guid| abbreviate(guid, 18))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if evidence_bits.is_empty() {
                lines.push(Line::from(Span::styled(
                    "    no supporting evidence rows",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for bit in evidence_bits {
                    lines.push(Line::from(vec![
                        Span::styled("    ", Style::default().fg(Color::DarkGray)),
                        Span::styled(bit, Style::default().fg(Color::White)),
                    ]));
                }
            }
        }
    }

    lines
}

fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    let layout = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(10),
        Constraint::Length(2),
    ])
    .split(area);

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "Artist Review TUI",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(app.status.clone(), Style::default().fg(Color::White)),
        ]),
        Line::from(Span::styled(
            app.queue_summary.clone(),
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            app.current_pending_review().map_or_else(
                || "Selected: none".to_string(),
                |review| {
                    let position = app
                        .review_state
                        .selected()
                        .map_or(0, |idx| idx.saturating_add(1));
                    let (family_position, family_total) =
                        artist_source_family_position(app).unwrap_or((0, 0));
                    format!(
                        "Selected {}/{}: {} | review={} feed={} source={} confidence={} score={} family={}/{} key={} artists={} created={}{}",
                        position,
                        app.reviews.len(),
                        abbreviate(&review.title, 28),
                        review.review_id,
                        short_id(&review.feed_guid),
                        review.source,
                        review.confidence,
                        review
                            .score
                            .map_or_else(|| "-".to_string(), |score| score.to_string()),
                        family_position,
                        family_total,
                        abbreviate(&review.evidence_key, 24),
                        review.artist_count,
                        format_local_timestamp(review.created_at),
                        {
                            let mut suffix = String::new();
                            if !review.supporting_sources.is_empty() {
                                let _ = write!(
                                    suffix,
                                    " support={}",
                                    preview_join(&review.supporting_sources, 2, 22)
                                );
                            }
                            if !review.conflict_reasons.is_empty() {
                                let _ = write!(
                                    suffix,
                                    " conflicts={}",
                                    preview_join(&review.conflict_reasons, 2, 18)
                                );
                            }
                            if !review.score_breakdown.is_empty() {
                                let _ = write!(
                                    suffix,
                                    " break={}",
                                    stophammer::tui::preview_score_breakdown(
                                        &review.score_breakdown,
                                        2,
                                        18,
                                        abbreviate,
                                    )
                                );
                            }
                            suffix
                        }
                    )
                },
            ),
            Style::default().fg(Color::DarkGray),
        )),
    ]);
    frame.render_widget(header, layout[0]);

    let body = Layout::horizontal([
        Constraint::Percentage(28),
        Constraint::Percentage(26),
        Constraint::Percentage(46),
    ])
    .split(layout[1]);

    let review_title = app.current_pending_review().map_or_else(
        || "Pending Artist Reviews".to_string(),
        |review| {
            let position = app
                .review_state
                .selected()
                .map_or(0, |idx| idx.saturating_add(1));
            let (family_position, family_total) =
                artist_source_family_position(app).unwrap_or((0, 0));
            format!(
                "Pending Artist Reviews ({}/{}, review={}, {} {}/{}, key={})",
                position,
                app.reviews.len(),
                review.review_id,
                review.source,
                family_position,
                family_total,
                abbreviate(&review.evidence_key, 18)
            )
        },
    );
    let review_list = List::new(build_review_items(app))
        .block(stophammer::tui::titled_block(
            &review_title,
            Color::Cyan,
            app.focus == Focus::Reviews,
            Style::default().fg(Color::DarkGray),
        ))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(32, 96, 160))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    frame.render_stateful_widget(review_list, body[0], &mut app.review_state);

    let middle =
        Layout::vertical([Constraint::Percentage(48), Constraint::Percentage(52)]).split(body[1]);

    let artist_list = List::new(build_artist_items(app))
        .block(stophammer::tui::titled_block(
            "Choose Main Artist",
            Color::Green,
            app.focus == Focus::MainArtist,
            Style::default().fg(Color::DarkGray),
        ))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(28, 110, 70))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    frame.render_stateful_widget(artist_list, middle[0], &mut app.artist_state);

    let context_lines = if let Some(snapshot) = app.current_snapshot() {
        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("Feed URL: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                abbreviate(&snapshot.feed_url, 80),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Artists in review: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                snapshot.review.artist_names.join(", "),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Review ID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                snapshot.review.review_id.to_string(),
                Style::default().fg(Color::White),
            ),
        ]));
        if !snapshot.review.supporting_sources.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Supporting: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    preview_join(&snapshot.review.supporting_sources, 4, 52),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
        if !snapshot.review.conflict_reasons.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Conflicts: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    preview_join(&snapshot.review.conflict_reasons, 4, 52),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }
        if !snapshot.review.score_breakdown.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Score breakdown: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    stophammer::tui::preview_score_breakdown(
                        &snapshot.review.score_breakdown,
                        4,
                        52,
                        abbreviate,
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
        lines
    } else {
        vec![Line::from("No review selected.")]
    };
    let context_title = app.current_snapshot().map_or_else(
        || "Review Context".to_string(),
        |snapshot| {
            format!(
                "Review Context #{} ({}, key={})",
                snapshot.review.review_id,
                snapshot.review.source,
                abbreviate(&snapshot.review.evidence_key, 18)
            )
        },
    );
    let context = Paragraph::new(context_lines)
        .block(stophammer::tui::titled_block(
            &context_title,
            Color::LightBlue,
            false,
            Style::default().fg(Color::DarkGray),
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(context, middle[1]);

    let evidence_title = app.current_snapshot().map_or_else(
        || "Evidence".to_string(),
        |snapshot| {
            format!(
                "Evidence {} ({}, key={})",
                snapshot.pending.title,
                snapshot.review.source,
                abbreviate(&snapshot.review.evidence_key, 18)
            )
        },
    );
    // Load the plan lazily — only when the evidence pane is about to render.
    let _ = app.ensure_plan_loaded();
    let evidence = Paragraph::new(build_evidence_lines(app))
        .block(stophammer::tui::titled_block(
            &evidence_title,
            Color::LightBlue,
            app.focus == Focus::Evidence,
            Style::default().fg(Color::DarkGray),
        ))
        .wrap(Wrap { trim: false })
        .scroll((app.evidence_scroll, 0));
    frame.render_widget(evidence, body[2]);

    let footer = Paragraph::new(stophammer::tui::build_review_footer(
        "tab focus  arrows move  home/end jump  m merge  x block  H high-confidence list",
    ))
    .wrap(Wrap { trim: false });
    frame.render_widget(footer, layout[2]);

    if let Some(dialog) = &app.dialog {
        stophammer::tui::render_text_dialog(frame, area, dialog);
    }
}

fn artist_source_family_position(app: &App) -> Option<(usize, usize)> {
    let review = app.current_pending_review()?;
    let selected = app.review_state.selected()?;
    let matching = app
        .reviews
        .iter()
        .enumerate()
        .filter(|(_, item)| item.source == review.source)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let position = matching
        .iter()
        .position(|&index| index == selected)
        .map(|index| index.saturating_add(1))?;
    Some((position, matching.len()))
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), Box<dyn Error>> {
    terminal.draw(|frame| draw(frame, app))?;
    loop {
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if app.dialog.is_some() {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') => app.dialog = None,
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Tab | KeyCode::Right => app.next_focus(),
                KeyCode::BackTab | KeyCode::Left => app.previous_focus(),
                KeyCode::Down => app.move_down()?,
                KeyCode::Up => app.move_up()?,
                KeyCode::Home => app.jump_top()?,
                KeyCode::End => app.jump_bottom()?,
                KeyCode::Char('m') => app.approve_merge()?,
                KeyCode::Char('x') => app.reject_review()?,
                KeyCode::Char('o') => app.show_operator_overview()?,
                KeyCode::Char('p') => app.show_review_playbook()?,
                KeyCode::Char('s') => app.show_queue_summary()?,
                KeyCode::Char('h') => app.show_feed_hotspots()?,
                KeyCode::Char('t') => app.show_stale_reviews()?,
                KeyCode::Char('y') => app.show_recent_reviews()?,
                KeyCode::Char('H') => app.show_high_confidence_reviews(),
                KeyCode::Char('n') => app.jump_next_same_source()?,
                KeyCode::Char('N') => app.jump_previous_same_source()?,
                KeyCode::Char('g') => app.jump_next_high_confidence()?,
                KeyCode::Char('G') => app.jump_previous_high_confidence()?,
                KeyCode::Char('?') => app.show_help_dialog(),
                KeyCode::Char('r') => {
                    let review_id = app.current_pending_review().map(|review| review.review_id);
                    let artist_id = app
                        .current_main_artist()
                        .map(|artist| artist.artist_id.clone());
                    app.reload(review_id, artist_id.as_deref())?;
                }
                _ => {}
            }
        }
        terminal.draw(|frame| draw(frame, app))?;
    }
}

fn run_tui(args: &Args) -> Result<(), Box<dyn Error>> {
    let mut cleanup = stophammer::tui::TerminalCleanupGuard::enter()?;
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let backend: Box<dyn ReviewBackend> = match &args.backend {
        BackendArgs::Db { path } => Box::new(DbBackend::new(stophammer::db::open_db(path))),
        BackendArgs::Api {
            base_url,
            admin_token,
        } => Box::new(ApiBackend::new(base_url.clone(), admin_token.clone())),
    };
    let mut app = App::new(backend, args.limit, args.min_score)?;
    let result = run_app(&mut terminal, &mut app);
    cleanup.complete(&mut terminal)?;
    result
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args().map_err(io::Error::other)?;
    run_tui(&args)
}
