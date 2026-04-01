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

use std::collections::BTreeSet;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};
use rusqlite::{Connection, OptionalExtension};
use stophammer::db::DEFAULT_DB_PATH;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

#[derive(Debug)]
struct Args {
    db_path: PathBuf,
    limit: usize,
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
    plan: stophammer::db::ArtistIdentityFeedPlan,
    feed_url: String,
    artists: Vec<ArtistSummary>,
}

#[derive(Debug, Clone)]
struct SummaryDialog {
    title: String,
    lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Reviews,
    MainArtist,
    Evidence,
}

#[derive(Debug)]
struct App {
    conn: Connection,
    limit: usize,
    reviews: Vec<stophammer::db::ArtistIdentityPendingReview>,
    queue_summary: String,
    review_state: ListState,
    artist_state: ListState,
    focus: Focus,
    snapshot: Option<ReviewSnapshot>,
    evidence_scroll: u16,
    status: String,
    dialog: Option<SummaryDialog>,
}

impl App {
    fn new(db_path: &Path, limit: usize) -> Result<Self, Box<dyn Error>> {
        let conn = stophammer::db::open_db(db_path);
        let mut app = Self {
            conn,
            limit,
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
            stophammer::db::list_pending_artist_identity_reviews(&self.conn, self.limit)?;
        self.queue_summary = format_artist_review_summary(
            &stophammer::db::summarize_pending_artist_identity_reviews(&self.conn)?,
        );
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

        let review = stophammer::db::get_artist_identity_review(&self.conn, pending.review_id)?
            .ok_or_else(|| io::Error::other(format!("review missing: {}", pending.review_id)))?;
        let plan =
            stophammer::db::explain_artist_identity_for_feed(&self.conn, &pending.feed_guid)?;
        let feed_url = feed_url_for_guid(&self.conn, &pending.feed_guid)?;
        let artists = load_artist_summaries(&self.conn, &review.artist_ids)?;

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
            plan,
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

        let outcome = stophammer::db::apply_artist_identity_review_action(
            &mut self.conn,
            review_id,
            "merge",
            Some(&target_artist_id),
            None,
        )?;

        self.dialog = Some(SummaryDialog {
            title: "Artist Merge Applied".to_string(),
            lines: vec![
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
        });

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

        let outcome = stophammer::db::apply_artist_identity_review_action(
            &mut self.conn,
            review_id,
            "do_not_merge",
            None,
            None,
        )?;
        self.dialog = Some(SummaryDialog {
            title: "Artist Review Blocked".to_string(),
            lines: vec![
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
        });
        self.reload(Some(review_id), None)?;
        Ok(())
    }

    fn show_queue_summary(&mut self) -> Result<(), Box<dyn Error>> {
        let summary = stophammer::db::summarize_pending_artist_identity_reviews(&self.conn)?;
        let age = stophammer::db::summarize_pending_artist_identity_review_age(&self.conn)?;
        let total: usize = summary.iter().map(|item| item.count).sum();
        let mut lines = vec![
            format!("Total pending artist reviews: {total}"),
            format!("Created in last 24h: {}", age.created_last_24h),
            format!("Older than 7d: {}", age.older_than_7d),
        ];
        if let Some(oldest_created_at) = age.oldest_created_at {
            lines.push(format!(
                "Oldest created_at: {}",
                format_local_timestamp(oldest_created_at)
            ));
        }
        if summary.is_empty() {
            lines.push("No pending artist review sources".to_string());
        } else {
            lines.push(String::new());
            lines.extend(summary.into_iter().map(|item| {
                let share = (item.count.saturating_mul(100)) / total.max(1);
                format!("{}: {} ({}%)", item.source, item.count, share)
            }));
        }
        self.dialog = Some(SummaryDialog {
            title: "Artist Queue Summary".to_string(),
            lines,
        });
        Ok(())
    }

    fn show_feed_hotspots(&mut self) -> Result<(), Box<dyn Error>> {
        let hotspots = stophammer::db::list_pending_review_feed_hotspots(&self.conn, 10)?;
        let mut lines = vec![
            "Top feeds by pending combined review load".to_string(),
            String::new(),
        ];
        if hotspots.is_empty() {
            lines.push("No feed hotspots with pending reviews".to_string());
        } else {
            for feed in hotspots {
                lines.push(format!(
                    "{} [{}] | total={} artist={} wallet={}",
                    feed.title,
                    short_id(&feed.feed_guid),
                    feed.total_review_count,
                    feed.artist_review_count,
                    feed.wallet_review_count
                ));
                lines.push(format!("  {}", abbreviate(&feed.feed_url, 72)));
            }
        }
        self.dialog = Some(SummaryDialog {
            title: "Feed Hotspots".to_string(),
            lines,
        });
        Ok(())
    }

    fn show_operator_overview(&mut self) -> Result<(), Box<dyn Error>> {
        let artist_summary = stophammer::db::summarize_pending_artist_identity_reviews(&self.conn)?;
        let wallet_summary = stophammer::db::summarize_pending_wallet_reviews(&self.conn)?;
        let artist_age = stophammer::db::summarize_pending_artist_identity_review_age(&self.conn)?;
        let wallet_age = stophammer::db::summarize_pending_wallet_review_age(&self.conn)?;
        let hotspots = stophammer::db::list_pending_review_feed_hotspots(&self.conn, 5)?;

        let artist_total: usize = artist_summary.iter().map(|item| item.count).sum();
        let wallet_total: usize = wallet_summary.iter().map(|item| item.count).sum();
        let mut lines = vec![
            format!(
                "Artist reviews: total={} last24h={} older7d={}",
                artist_total, artist_age.created_last_24h, artist_age.older_than_7d
            ),
            format!(
                "Wallet reviews: total={} last24h={} older7d={}",
                wallet_total, wallet_age.created_last_24h, wallet_age.older_than_7d
            ),
        ];
        if let Some(oldest) = artist_age.oldest_created_at {
            lines.push(format!(
                "Oldest artist review: {}",
                format_local_timestamp(oldest)
            ));
        }
        if let Some(oldest) = wallet_age.oldest_created_at {
            lines.push(format!(
                "Oldest wallet review: {}",
                format_local_timestamp(oldest)
            ));
        }
        lines.push(String::new());
        lines.push("Top artist review sources:".to_string());
        if artist_summary.is_empty() {
            lines.push("  none".to_string());
        } else {
            lines.extend(artist_summary.into_iter().take(3).map(|item| {
                let share = (item.count.saturating_mul(100)) / artist_total.max(1);
                format!("  {}: {} ({}%)", item.source, item.count, share)
            }));
        }
        lines.push(String::new());
        lines.push("Top wallet review sources:".to_string());
        if wallet_summary.is_empty() {
            lines.push("  none".to_string());
        } else {
            lines.extend(wallet_summary.into_iter().take(3).map(|item| {
                let share = (item.count.saturating_mul(100)) / wallet_total.max(1);
                format!("  {}: {} ({}%)", item.source, item.count, share)
            }));
        }
        lines.push(String::new());
        lines.push("Hottest feeds:".to_string());
        if hotspots.is_empty() {
            lines.push("  none".to_string());
        } else {
            lines.extend(hotspots.into_iter().map(|feed| {
                format!(
                    "  {} | total={} artist={} wallet={}",
                    feed.title,
                    feed.total_review_count,
                    feed.artist_review_count,
                    feed.wallet_review_count
                )
            }));
        }
        self.dialog = Some(SummaryDialog {
            title: "Operator Overview".to_string(),
            lines,
        });
        Ok(())
    }

    fn show_stale_reviews(&mut self) -> Result<(), Box<dyn Error>> {
        let stale = stophammer::db::list_stale_pending_artist_identity_reviews(
            &self.conn,
            7 * 24 * 60 * 60,
            10,
        )?;
        let mut lines = vec![
            "Pending artist reviews older than 7 days".to_string(),
            String::new(),
        ];
        if stale.is_empty() {
            lines.push("No stale artist reviews".to_string());
        } else {
            lines.extend(stale.into_iter().map(|review| {
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
            }));
        }
        self.dialog = Some(SummaryDialog {
            title: "Stale Artist Reviews".to_string(),
            lines,
        });
        Ok(())
    }

    fn show_recent_reviews(&mut self) -> Result<(), Box<dyn Error>> {
        let recent = stophammer::db::list_recent_pending_artist_identity_reviews(
            &self.conn,
            24 * 60 * 60,
            10,
        )?;
        let mut lines = vec![
            "Pending artist reviews created in the last 24 hours".to_string(),
            String::new(),
        ];
        if recent.is_empty() {
            lines.push("No recent artist reviews".to_string());
        } else {
            lines.extend(recent.into_iter().map(|review| {
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
            }));
        }
        self.dialog = Some(SummaryDialog {
            title: "Recent Artist Reviews".to_string(),
            lines,
        });
        Ok(())
    }

    fn show_help_dialog(&mut self) {
        self.dialog = Some(SummaryDialog {
            title: "Artist Review TUI Help".to_string(),
            lines: vec![
                "Tab / Shift-Tab: cycle focus".to_string(),
                "Up / Down / Home / End: navigate".to_string(),
                "m: merge into selected main artist".to_string(),
                "x: mark review do_not_merge".to_string(),
                "o: operator overview".to_string(),
                "p: review-next playbook".to_string(),
                "s: queue source summary".to_string(),
                "h: hottest feeds".to_string(),
                "t: stale reviews (>7d)".to_string(),
                "y: recent reviews (<24h)".to_string(),
                "n / N: next / previous review with same source".to_string(),
                "r: reload pending reviews".to_string(),
                "Enter / Space / Esc: close dialog".to_string(),
                "q: quit".to_string(),
            ],
        });
    }

    fn show_review_playbook(&mut self) -> Result<(), Box<dyn Error>> {
        let summary = stophammer::db::summarize_pending_artist_identity_reviews(&self.conn)?;
        let age = stophammer::db::summarize_pending_artist_identity_review_age(&self.conn)?;
        let hotspots = stophammer::db::list_pending_review_feed_hotspots(&self.conn, 3)?;
        let total: usize = summary.iter().map(|item| item.count).sum();

        let mut lines = vec![format!("Pending artist reviews: {total}")];
        if total == 0 {
            lines.push("Nothing pending. Reload after the next resolver pass.".to_string());
        } else {
            if age.older_than_7d > 0 {
                lines.push(format!(
                    "1. Clear stale backlog first: {} artist reviews are older than 7 days.",
                    age.older_than_7d
                ));
            } else if age.created_last_24h > 0 {
                lines.push(format!(
                    "1. Fresh churn only: {} artist reviews were created in the last 24 hours.",
                    age.created_last_24h
                ));
            }

            if let Some(top_source) = summary.first() {
                let share = (top_source.count.saturating_mul(100)) / total.max(1);
                lines.push(format!(
                    "2. Main source family: {} ({} pending, {}% of backlog).",
                    top_source.source, top_source.count, share
                ));
            }

            if let Some(feed) = hotspots.first() {
                lines.push(format!(
                    "3. Start with feed hotspot: {} (total={}, artist={}, wallet={}).",
                    feed.title,
                    feed.total_review_count,
                    feed.artist_review_count,
                    feed.wallet_review_count
                ));
            }

            lines.push(
                "4. Use o/s/h/t/y to inspect overview, sources, hotspots, stale, and recent items."
                    .to_string(),
            );
        }

        self.dialog = Some(SummaryDialog {
            title: "Artist Review Playbook".to_string(),
            lines,
        });
        Ok(())
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "manual CLI parsing keeps the review tool dependency-free"
)]
fn parse_args() -> Result<Args, String> {
    let mut db_path = PathBuf::from(DEFAULT_DB_PATH);
    let mut limit = 50usize;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--db requires a path".to_string())?;
                db_path = PathBuf::from(value);
            }
            "--limit" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--limit requires a number".to_string())?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_err| format!("invalid --limit value: {value}"))?;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: review_artist_identity_tui [--db PATH] [--limit N]\n\
                     Interactive artist identity review tool.\n\
                     Lets operators choose a main artist for each pending feed-scoped review,\n\
                     inspect supporting feed evidence, then apply merge or do-not-merge decisions.\n\
                     Keys: Tab/Shift-Tab focus, m merge, x do-not-merge, o overview, p playbook, s queue summary, h feed hotspots, t stale reviews, y recent reviews, n/N same-source jump, ? help, r reload, q quit."
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args { db_path, limit })
}

fn duplicate_name_artist_rows(
    conn: &Connection,
    artist_id: &str,
) -> Result<Option<(String, String, i64)>, rusqlite::Error> {
    conn.query_row(
        "SELECT artist_id, name, created_at FROM artists WHERE artist_id = ?1",
        [artist_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
}

fn feed_rows_for_artist(
    conn: &Connection,
    artist_id: &str,
) -> Result<Vec<(String, String, String)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT f.feed_guid, f.title, f.feed_url
         FROM artist_credit_name acn
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id
         JOIN feeds f ON f.artist_credit_id = ac.id
         WHERE acn.artist_id = ?1
         ORDER BY f.title_lower, f.feed_guid",
    )?;
    stmt.query_map([artist_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?
    .collect::<Result<Vec<_>, _>>()
}

fn feed_url_for_guid(conn: &Connection, feed_guid: &str) -> Result<String, rusqlite::Error> {
    conn.query_row(
        "SELECT feed_url FROM feeds WHERE feed_guid = ?1",
        [feed_guid],
        |row| row.get(0),
    )
}

fn external_ids_for_artist(
    conn: &Connection,
    artist_id: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    Ok(stophammer::db::get_external_ids(conn, "artist", artist_id)?
        .into_iter()
        .map(|row| format!("{}={}", row.scheme, row.value))
        .collect())
}

fn release_count_for_artist(conn: &Connection, artist_id: &str) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(DISTINCT sfr.release_id)
         FROM artist_credit_name acn
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id
         JOIN feeds f ON f.artist_credit_id = ac.id
         JOIN source_feed_release_map sfr ON sfr.feed_guid = f.feed_guid
         WHERE acn.artist_id = ?1",
        [artist_id],
        |row| row.get(0),
    )
}

fn feed_count_for_artist(conn: &Connection, artist_id: &str) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(DISTINCT f.feed_guid)
         FROM artist_credit_name acn
         JOIN artist_credit ac ON ac.id = acn.artist_credit_id
         JOIN feeds f ON f.artist_credit_id = ac.id
         WHERE acn.artist_id = ?1",
        [artist_id],
        |row| row.get(0),
    )
}

fn feed_evidence_row(
    conn: &Connection,
    feed_guid: &str,
    title: String,
    feed_url: String,
) -> Result<FeedEvidenceRow, Box<dyn Error>> {
    let canonical = conn
        .query_row(
            "SELECT release_id, match_type, confidence
             FROM source_feed_release_map WHERE feed_guid = ?1",
            [feed_guid],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;

    let platforms = stophammer::db::get_source_platform_claims_for_feed(conn, feed_guid)?
        .into_iter()
        .map(|claim| claim.platform_key)
        .filter(|value| !value.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let website_links =
        stophammer::db::get_source_entity_links_for_entity(conn, "feed", feed_guid)?
            .into_iter()
            .filter(|claim| claim.link_type == "website")
            .map(|claim| claim.url)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

    let npubs = stophammer::db::get_source_entity_ids_for_entity(conn, "feed", feed_guid)?
        .into_iter()
        .filter(|claim| claim.scheme == "nostr_npub")
        .map(|claim| claim.value)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let publisher_remote_feed_guids =
        stophammer::db::get_feed_remote_items_for_feed(conn, feed_guid)?
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
        canonical_release_id: canonical.as_ref().map(|row| row.0.clone()),
        canonical_match_type: canonical.as_ref().map(|row| row.1.clone()),
        canonical_confidence: canonical.as_ref().map(|row| row.2),
        platforms,
        website_links,
        npubs,
        publisher_remote_feed_guids,
    })
}

fn load_artist_summaries(
    conn: &Connection,
    artist_ids: &[String],
) -> Result<Vec<ArtistSummary>, Box<dyn Error>> {
    let mut artists = Vec::new();
    for artist_id in artist_ids {
        let Some((artist_id, name, created_at)) = duplicate_name_artist_rows(conn, artist_id)?
        else {
            continue;
        };
        let mut feeds = Vec::new();
        for (feed_guid, title, feed_url) in feed_rows_for_artist(conn, &artist_id)? {
            feeds.push(feed_evidence_row(conn, &feed_guid, title, feed_url)?);
        }
        artists.push(ArtistSummary {
            artist_id: artist_id.clone(),
            name,
            created_at,
            feed_count: feed_count_for_artist(conn, &artist_id)?,
            release_count: release_count_for_artist(conn, &artist_id)?,
            external_ids: external_ids_for_artist(conn, &artist_id)?,
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
    if values.is_empty() {
        return "-".to_string();
    }
    values
        .iter()
        .take(max_items)
        .map(|value| abbreviate(value, max_chars))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_local_timestamp(timestamp: i64) -> String {
    let Ok(dt) = OffsetDateTime::from_unix_timestamp(timestamp) else {
        return timestamp.to_string();
    };
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let local = dt.to_offset(offset);
    local
        .format(&format_description!(
            "[year]-[month]-[day] [hour]:[minute] [offset_hour sign:mandatory]:[offset_minute]"
        ))
        .unwrap_or_else(|_| timestamp.to_string())
}

fn focus_block<'a>(title: &'a str, is_focused: bool, accent: Color) -> Block<'a> {
    let mut block = Block::default().borders(Borders::ALL);
    if is_focused {
        block = block.border_type(BorderType::Thick).border_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    } else {
        block = block.border_style(Style::default().fg(Color::DarkGray));
    }
    block.title(Span::styled(
        format!(" {title} "),
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    ))
}

fn styled_title(title: &str, color: Color) -> Span<'static> {
    Span::styled(
        format!(" {title} "),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn build_review_items(app: &App) -> Vec<ListItem<'static>> {
    if app.reviews.is_empty() {
        return vec![ListItem::new("No pending reviews")];
    }

    app.reviews
        .iter()
        .map(|review| {
            let (badge, badge_color) = recency_badge(review.created_at);
            let same_source_count = app
                .reviews
                .iter()
                .filter(|candidate| candidate.source == review.source)
                .count();
            ListItem::new(vec![
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
                        format!("family={same_source_count}"),
                        Style::default().fg(Color::LightBlue),
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
            ])
        })
        .collect()
}

fn recency_badge(timestamp: i64) -> (&'static str, Color) {
    let age_secs = OffsetDateTime::now_utc().unix_timestamp() - timestamp;
    if age_secs >= 7 * 24 * 60 * 60 {
        ("STALE", Color::Red)
    } else if age_secs <= 24 * 60 * 60 {
        ("FRESH", Color::Green)
    } else {
        ("MID", Color::Yellow)
    }
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
    snapshot.plan.candidate_groups.iter().find(|group| {
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
                .seed_artists
                .iter()
                .map(|artist| format!("{} [{}]", artist.name, short_id(&artist.artist_id)))
                .collect::<Vec<_>>()
                .join(", "),
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
                        "Selected {}/{}: {} | review={} feed={} source={} family={}/{} key={} artists={} created={}",
                        position,
                        app.reviews.len(),
                        abbreviate(&review.title, 28),
                        review.review_id,
                        short_id(&review.feed_guid),
                        review.source,
                        family_position,
                        family_total,
                        abbreviate(&review.evidence_key, 24),
                        review.artist_count,
                        format_local_timestamp(review.created_at)
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
        .block(focus_block(
            &review_title,
            app.focus == Focus::Reviews,
            Color::Cyan,
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
        .block(focus_block(
            "Choose Main Artist",
            app.focus == Focus::MainArtist,
            Color::Green,
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
        .block(focus_block(&context_title, false, Color::LightBlue))
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
    let evidence = Paragraph::new(build_evidence_lines(app))
        .block(focus_block(
            &evidence_title,
            app.focus == Focus::Evidence,
            Color::LightBlue,
        ))
        .wrap(Wrap { trim: false })
        .scroll((app.evidence_scroll, 0));
    frame.render_widget(evidence, body[2]);

    let footer = Paragraph::new(
        "tab focus  arrows move  home/end jump  n/N same-source  m merge  x block  o overview  p playbook  s summary  h hotspots  t stale  y recent  ? help  r reload  q quit",
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(footer, layout[2]);

    if let Some(dialog) = &app.dialog {
        let dialog_area = centered_rect(68, 45, area);
        frame.render_widget(Clear, dialog_area);
        let dialog_text = dialog
            .lines
            .iter()
            .map(|line| Line::from(line.clone()))
            .collect::<Vec<_>>();
        let widget = Paragraph::new(dialog_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Thick)
                    .border_style(
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    )
                    .title(styled_title(&dialog.title, Color::White)),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, dialog_area);
    }
}

fn format_artist_review_summary(
    summary: &[stophammer::db::ArtistIdentityPendingReviewSummary],
) -> String {
    if summary.is_empty() {
        return "No pending artist review sources".to_string();
    }

    let total: usize = summary.iter().map(|item| item.count).sum();
    let details = summary
        .iter()
        .take(3)
        .map(|item| format!("{}={}", item.source, item.count))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Pending artist reviews: {total} ({details})")
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

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    let horizontal = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1]);
    horizontal[1]
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), Box<dyn Error>> {
    loop {
        terminal.draw(|frame| draw(frame, app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
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
            continue;
        }
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
            KeyCode::Char('n') => app.jump_next_same_source()?,
            KeyCode::Char('N') => app.jump_previous_same_source()?,
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
}

fn run_tui(args: &Args) -> Result<(), Box<dyn Error>> {
    let mut cleanup = stophammer::tui::TerminalCleanupGuard::enter()?;
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(&args.db_path, args.limit)?;
    let result = run_app(&mut terminal, &mut app);
    cleanup.complete(&mut terminal)?;
    result
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args().map_err(io::Error::other)?;
    run_tui(&args)
}
