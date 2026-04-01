#![allow(
    clippy::elidable_lifetime_names,
    reason = "ratatui helper signatures are clearer when lifetime names mirror widget lifetimes"
)]
#![allow(
    clippy::map_unwrap_or,
    reason = "several TUI formatting paths read more directly with map(...).unwrap_or_else(...)"
)]
#![allow(
    clippy::too_many_lines,
    reason = "evidence rendering is intentionally kept in a few large layout functions for maintainability"
)]
#![allow(
    clippy::uninlined_format_args,
    reason = "existing TUI/status formatting favors consistency with surrounding code"
)]

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};
use rusqlite::{Connection, params, params_from_iter};
use stophammer::db::DEFAULT_DB_PATH;
use stophammer::tui::format_local_timestamp;

use stophammer::model::{
    Feed, ResolvedEntitySourceByFeed, ResolvedExternalIdByFeed, SourceContributorClaim,
    SourceEntityIdClaim, SourceEntityLink, SourceItemEnclosure, SourcePlatformClaim,
    SourceReleaseClaim, Track,
};

#[derive(Debug)]
struct Args {
    db_path: PathBuf,
    limit: usize,
}

#[derive(Debug, Clone)]
struct FeedQueueRow {
    feed_guid: String,
    title: String,
    feed_url: String,
    track_count: i64,
    source_claim_count: i64,
    resolved_count: i64,
    contributor_count: i64,
    entity_id_count: i64,
    link_count: i64,
    release_count: i64,
    platform_count: i64,
    enclosure_count: i64,
}

#[derive(Debug, Clone)]
struct FeedClaimSnapshot {
    feed: Feed,
    tracks: Vec<Track>,
    contributor_claims: Vec<SourceContributorClaim>,
    entity_id_claims: Vec<SourceEntityIdClaim>,
    link_claims: Vec<SourceEntityLink>,
    release_claims: Vec<SourceReleaseClaim>,
    enclosures: Vec<SourceItemEnclosure>,
    platform_claims: Vec<SourcePlatformClaim>,
    resolved_external_ids: Vec<ResolvedExternalIdByFeed>,
    resolved_entity_sources: Vec<ResolvedEntitySourceByFeed>,
}

#[derive(Debug, Clone, Copy, Default)]
struct ClaimFamilyTotals {
    contributors: usize,
    entity_ids: usize,
    links: usize,
    releases: usize,
    platforms: usize,
    enclosures: usize,
}

fn claim_family_rows(totals: ClaimFamilyTotals) -> Vec<(&'static str, usize)> {
    vec![
        ("contributors", totals.contributors),
        ("entity_ids", totals.entity_ids),
        ("links", totals.links),
        ("releases", totals.releases),
        ("platforms", totals.platforms),
        ("enclosures", totals.enclosures),
    ]
}

fn dominant_claim_family(totals: ClaimFamilyTotals) -> Option<(&'static str, usize, usize)> {
    let rows = claim_family_rows(totals);
    let total = rows.iter().map(|(_, count)| *count).sum::<usize>();
    let (label, count) = rows.into_iter().max_by_key(|(_, count)| *count)?;
    (count > 0).then_some((label, count, (count * 100) / total.max(1)))
}

fn queue_claim_family_summary(totals: ClaimFamilyTotals) -> Option<String> {
    dominant_claim_family(totals)
        .map(|(label, count, share)| format!("top claim family={label} ({count}, {share}%)"))
}

fn dominant_feed_claim_family(feed: &FeedQueueRow) -> Option<(&'static str, i64, i64)> {
    let rows = [
        ("contributors", feed.contributor_count),
        ("entity_ids", feed.entity_id_count),
        ("links", feed.link_count),
        ("releases", feed.release_count),
        ("platforms", feed.platform_count),
        ("enclosures", feed.enclosure_count),
    ];
    let total = rows.iter().map(|(_, count)| *count).sum::<i64>();
    let (label, count) = rows.into_iter().max_by_key(|(_, count)| *count)?;
    (count > 0).then_some((label, count, (count * 100) / total.max(1)))
}

fn dominant_feed_claim_family_summary(feed: &FeedQueueRow) -> String {
    dominant_feed_claim_family(feed).map_or_else(
        || "top=no-claims".to_string(),
        |(label, _count, share)| format!("top={label}({share}%)"),
    )
}

fn dominant_track_claim_family(
    snapshot: &FeedClaimSnapshot,
    track_guid: &str,
) -> Option<(&'static str, usize, usize)> {
    let rows = [
        (
            "contributors",
            snapshot
                .contributor_claims
                .iter()
                .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
                .count(),
        ),
        (
            "entity_ids",
            snapshot
                .entity_id_claims
                .iter()
                .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
                .count(),
        ),
        (
            "links",
            snapshot
                .link_claims
                .iter()
                .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
                .count(),
        ),
        (
            "releases",
            snapshot
                .release_claims
                .iter()
                .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
                .count(),
        ),
        (
            "enclosures",
            snapshot
                .enclosures
                .iter()
                .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
                .count(),
        ),
    ];
    let total = rows.iter().map(|(_, count)| *count).sum::<usize>();
    let (label, count) = rows.into_iter().max_by_key(|(_, count)| *count)?;
    (count > 0).then_some((label, count, (count * 100) / total.max(1)))
}

fn dominant_track_claim_family_summary(snapshot: &FeedClaimSnapshot, track_guid: &str) -> String {
    dominant_track_claim_family(snapshot, track_guid).map_or_else(
        || "top=no-claims".to_string(),
        |(label, _count, share)| format!("top={label}({share}%)"),
    )
}

fn feed_family_subset_summary(feeds: &[FeedQueueRow]) -> Option<String> {
    let mut counts = BTreeMap::<&'static str, usize>::new();
    for feed in feeds {
        if let Some((label, _, _)) = dominant_feed_claim_family(feed) {
            *counts.entry(label).or_default() += 1;
        }
    }
    let total = counts.values().sum::<usize>();
    let (label, count) = counts.into_iter().max_by_key(|(_, count)| *count)?;
    Some(format!(
        "dominant feed family in this subset: {label} ({count}/{total})"
    ))
}

fn current_family_position(app: &App) -> Option<(&'static str, usize, usize)> {
    let current_idx = app.feed_state.selected()?;
    let (label, _, _) = dominant_feed_claim_family(app.feeds.get(current_idx)?)?;
    let family_indices = app
        .feeds
        .iter()
        .enumerate()
        .filter_map(|(idx, feed)| {
            dominant_feed_claim_family(feed)
                .and_then(|(candidate, _, _)| (candidate == label).then_some(idx))
        })
        .collect::<Vec<_>>();
    let position = family_indices
        .iter()
        .position(|idx| *idx == current_idx)?
        .saturating_add(1);
    Some((label, position, family_indices.len()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Feeds,
    Tracks,
    Evidence,
}

#[derive(Debug)]
struct App {
    conn: Connection,
    limit: usize,
    feeds: Vec<FeedQueueRow>,
    queue_claim_family_totals: ClaimFamilyTotals,
    feed_state: ListState,
    track_state: ListState,
    focus: Focus,
    snapshot: Option<FeedClaimSnapshot>,
    evidence_scroll: u16,
    status: String,
    dialog: Option<stophammer::tui::TextDialog>,
}

impl App {
    fn new(db_path: &Path, limit: usize) -> Result<Self, Box<dyn Error>> {
        let conn = stophammer::db::open_db(db_path);
        let mut app = Self {
            conn,
            limit,
            feeds: Vec::new(),
            queue_claim_family_totals: ClaimFamilyTotals::default(),
            feed_state: ListState::default(),
            track_state: ListState::default(),
            focus: Focus::Feeds,
            snapshot: None,
            evidence_scroll: 0,
            status: "Loading source-claims review queue...".to_string(),
            dialog: None,
        };
        app.reload(None, None)?;
        Ok(app)
    }

    fn reload(
        &mut self,
        preferred_feed_guid: Option<&str>,
        preferred_track_guid: Option<&str>,
    ) -> Result<(), Box<dyn Error>> {
        self.feeds = list_claim_review_feeds(&self.conn, self.limit)?;
        self.queue_claim_family_totals = load_queue_claim_family_totals(
            &self.conn,
            &self
                .feeds
                .iter()
                .map(|feed| feed.feed_guid.clone())
                .collect::<Vec<_>>(),
        )?;
        if self.feeds.is_empty() {
            self.feed_state.select(None);
            self.track_state.select(None);
            self.snapshot = None;
            self.status = "No feeds with source claims or resolved promotions.".to_string();
            return Ok(());
        }

        let selected_feed_idx = preferred_feed_guid
            .and_then(|feed_guid| self.feeds.iter().position(|row| row.feed_guid == feed_guid))
            .or_else(|| self.feed_state.selected())
            .unwrap_or(0)
            .min(self.feeds.len().saturating_sub(1));
        self.feed_state.select(Some(selected_feed_idx));
        self.load_selected_feed(preferred_track_guid)?;
        Ok(())
    }

    fn load_selected_feed(
        &mut self,
        preferred_track_guid: Option<&str>,
    ) -> Result<(), Box<dyn Error>> {
        let Some(feed_idx) = self.feed_state.selected() else {
            self.snapshot = None;
            self.track_state.select(None);
            return Ok(());
        };
        let Some(queue_row) = self.feeds.get(feed_idx) else {
            self.snapshot = None;
            self.track_state.select(None);
            return Ok(());
        };
        let feed_guid = queue_row.feed_guid.clone();
        let snapshot = load_feed_claim_snapshot(&self.conn, &feed_guid)?;
        let track_idx = preferred_track_guid
            .and_then(|track_guid| {
                snapshot
                    .tracks
                    .iter()
                    .position(|track| track.track_guid == track_guid)
            })
            .or_else(|| self.track_state.selected())
            .unwrap_or(0);
        let track_count = snapshot.tracks.len();
        self.track_state
            .select((track_count > 0).then_some(track_idx.min(track_count.saturating_sub(1))));
        self.snapshot = Some(snapshot);
        self.evidence_scroll = 0;
        self.status = format!(
            "Loaded feed {:?} with {} source claims and {} resolved promotion rows.",
            queue_row.title, queue_row.source_claim_count, queue_row.resolved_count
        );
        Ok(())
    }

    fn current_feed_row(&self) -> Option<&FeedQueueRow> {
        self.feed_state
            .selected()
            .and_then(|idx| self.feeds.get(idx))
    }

    fn current_snapshot(&self) -> Option<&FeedClaimSnapshot> {
        self.snapshot.as_ref()
    }

    fn current_track(&self) -> Option<&Track> {
        let idx = self.track_state.selected()?;
        self.snapshot.as_ref()?.tracks.get(idx)
    }

    fn next_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Feeds => Focus::Tracks,
            Focus::Tracks => Focus::Evidence,
            Focus::Evidence => Focus::Feeds,
        };
    }

    fn previous_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Feeds => Focus::Evidence,
            Focus::Tracks => Focus::Feeds,
            Focus::Evidence => Focus::Tracks,
        };
    }

    fn show_help_dialog(&mut self) {
        self.dialog = Some(stophammer::tui::text_dialog(
            "Source Claims Review TUI Help",
            vec![
                "Tab / Left / Right: cycle focus".to_string(),
                "Up / Down / Home / End: navigate".to_string(),
                "o: operator overview for the current source-claims queue".to_string(),
                "p: review-next playbook for source-claims backlog".to_string(),
                "s: feed summary for the selected source-claims bundle".to_string(),
                "t: selected-track claim mix by claim family".to_string(),
                "h: top source-claims hotspots".to_string(),
                "c: current-feed conflict summary".to_string(),
                "m: selected-feed claim mix by claim family".to_string(),
                "n / N: jump to next / previous feed with the same dominant claim family".to_string(),
                "r: reload current feed and track selection".to_string(),
                "? : show this help dialog".to_string(),
                "Enter / Space / Esc: close dialog".to_string(),
                "q: quit".to_string(),
            ],
        ));
    }

    fn current_feed_family_label(&self) -> Option<&'static str> {
        let feed = self.current_feed_row()?;
        dominant_feed_claim_family(feed).map(|(label, _, _)| label)
    }

    fn jump_same_family(&mut self, forward: bool) -> Result<(), Box<dyn Error>> {
        let Some(target_family) = self.current_feed_family_label() else {
            return Ok(());
        };
        if self.feeds.len() < 2 {
            return Ok(());
        }
        let current_idx = self.feed_state.selected().unwrap_or(0);
        let len = self.feeds.len();

        for offset in 1..len {
            let idx = if forward {
                (current_idx + offset) % len
            } else {
                (current_idx + len - offset) % len
            };
            let Some((label, _, _)) = dominant_feed_claim_family(&self.feeds[idx]) else {
                continue;
            };
            if label == target_family {
                self.feed_state.select(Some(idx));
                self.load_selected_feed(None)?;
                self.status = format!(
                    "Jumped to {}-dominant feed {:?}.",
                    target_family, self.feeds[idx].title
                );
                break;
            }
        }
        Ok(())
    }

    fn show_summary_dialog(&mut self) {
        let Some(feed_row) = self.current_feed_row() else {
            self.dialog = Some(stophammer::tui::text_dialog(
                "Source Claims Feed Summary (0)",
                vec!["No feed selected.".to_string()],
            ));
            return;
        };
        let Some(snapshot) = self.current_snapshot() else {
            self.dialog = Some(stophammer::tui::text_dialog(
                format!("Source Claims Feed Summary [{}]", short_id(&feed_row.feed_guid)),
                vec!["No source-claims snapshot loaded.".to_string()],
            ));
            return;
        };

        let feed_contributor_claims = snapshot
            .contributor_claims
            .iter()
            .filter(|claim| claim.entity_type == "feed")
            .count();
        let feed_entity_id_claims = snapshot
            .entity_id_claims
            .iter()
            .filter(|claim| claim.entity_type == "feed")
            .count();
        let feed_link_claims = snapshot
            .link_claims
            .iter()
            .filter(|claim| claim.entity_type == "feed")
            .count();
        let feed_release_claims = snapshot
            .release_claims
            .iter()
            .filter(|claim| claim.entity_type == "feed")
            .count();
        let track_contributor_claims = snapshot
            .contributor_claims
            .iter()
            .filter(|claim| claim.entity_type == "track")
            .count();
        let track_entity_id_claims = snapshot
            .entity_id_claims
            .iter()
            .filter(|claim| claim.entity_type == "track")
            .count();
        let track_link_claims = snapshot
            .link_claims
            .iter()
            .filter(|claim| claim.entity_type == "track")
            .count();
        let track_release_claims = snapshot
            .release_claims
            .iter()
            .filter(|claim| claim.entity_type == "track")
            .count();
        let track_enclosures = snapshot
            .enclosures
            .iter()
            .filter(|claim| claim.entity_type == "track")
            .count();
        let conflict_lines = feed_conflict_lines(snapshot);
        let conflict_count = conflict_lines
            .iter()
            .filter(|line| *line != "no obvious feed-level claim conflicts detected")
            .count();
        let selected_track_summary = self.current_track().map_or_else(
            || "No track selected.".to_string(),
            |track| {
                format!(
                    "Selected track: {} [{}] with {} claim rows.",
                    track.title,
                    short_id(&track.track_guid),
                    count_track_claims(snapshot, &track.track_guid)
                )
            },
        );

        self.dialog = Some(stophammer::tui::text_dialog(
            format!(
                "Source Claims Feed Summary [{}]",
                short_id(&feed_row.feed_guid)
            ),
            vec![
                format!("Feed: {}", feed_row.title),
                format!("URL: {}", abbreviate(&feed_row.feed_url, 80)),
                current_family_position(self).map_or_else(
                    || "Family: none".to_string(),
                    |(label, position, total)| format!("Family: {label} ({position}/{total})"),
                ),
                format!(
                    "Tracks: {}  Source claims: {}  Resolved overlays: {}",
                    snapshot.tracks.len(),
                    feed_row.source_claim_count,
                    feed_row.resolved_count
                ),
                String::new(),
                "Feed-scoped claims:".to_string(),
                format!(
                    "  contributors={} ids={} links={} releases={} platforms={}",
                    feed_contributor_claims,
                    feed_entity_id_claims,
                    feed_link_claims,
                    feed_release_claims,
                    snapshot.platform_claims.len()
                ),
                "Track-scoped claims:".to_string(),
                format!(
                    "  contributors={} ids={} links={} releases={} enclosures={}",
                    track_contributor_claims,
                    track_entity_id_claims,
                    track_link_claims,
                    track_release_claims,
                    track_enclosures
                ),
                "Resolved overlays:".to_string(),
                format!(
                    "  external_ids={} entity_sources={}",
                    snapshot.resolved_external_ids.len(),
                    snapshot.resolved_entity_sources.len()
                ),
                "Conflicts:".to_string(),
                format!("  detected={conflict_count}"),
                selected_track_summary,
            ],
        ));
    }

    fn show_track_claim_mix_dialog(&mut self) {
        let Some(snapshot) = self.current_snapshot() else {
            self.dialog = Some(stophammer::tui::text_dialog(
                "Track Claim Mix (0)",
                vec!["No feed snapshot loaded.".to_string()],
            ));
            return;
        };
        let Some(track) = self.current_track() else {
            self.dialog = Some(stophammer::tui::text_dialog(
                "Track Claim Mix (0)",
                vec!["No track selected.".to_string()],
            ));
            return;
        };

        let contributor_count = snapshot
            .contributor_claims
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track.track_guid)
            .count();
        let entity_id_count = snapshot
            .entity_id_claims
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track.track_guid)
            .count();
        let link_count = snapshot
            .link_claims
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track.track_guid)
            .count();
        let release_count = snapshot
            .release_claims
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track.track_guid)
            .count();
        let enclosure_count = snapshot
            .enclosures
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track.track_guid)
            .count();
        let rows = [
            ("contributors", contributor_count),
            ("entity_ids", entity_id_count),
            ("links", link_count),
            ("releases", release_count),
            ("enclosures", enclosure_count),
        ];
        let total = rows.iter().map(|(_, count)| *count).sum::<usize>();

        let mut lines = vec![
            format!("Track: {} [{}]", track.title, short_id(&track.track_guid)),
            format!("Feed: {} [{}]", snapshot.feed.title, short_id(&snapshot.feed.feed_guid)),
            format!("Total track claim rows: {total}"),
        ];
        if let Some((label, _count, share)) = dominant_track_claim_family(snapshot, &track.track_guid)
        {
            lines.push(format!("Dominant family: {label} ({share}%)"));
        }
        lines.push(String::new());
        lines.push("Track claim families:".to_string());
        lines.extend(rows.into_iter().filter(|(_, count)| *count > 0).map(|(label, count)| {
            let share = (count * 100) / total.max(1);
            format!("  {label}: {count} ({share}%)")
        }));
        if total == 0 {
            lines.push("  no track-scoped claim rows".to_string());
        }

        self.dialog = Some(stophammer::tui::text_dialog(
            format!("Track Claim Mix [{}]", short_id(&track.track_guid)),
            lines,
        ));
    }

    fn show_operator_overview_dialog(&mut self) {
        let feed_count = self.feeds.len();
        if feed_count == 0 {
            self.dialog = Some(stophammer::tui::text_dialog(
                "Source Claims Operator Overview (0)",
                vec!["No feeds with source claims or resolved overlays are queued.".to_string()],
            ));
            return;
        }

        let total_tracks: i64 = self.feeds.iter().map(|feed| feed.track_count).sum();
        let total_claims: i64 = self.feeds.iter().map(|feed| feed.source_claim_count).sum();
        let total_resolved: i64 = self.feeds.iter().map(|feed| feed.resolved_count).sum();
        let claim_family_totals = self.queue_claim_family_totals;
        let top_claim_feed = self
            .feeds
            .iter()
            .max_by(|a, b| {
                a.source_claim_count
                    .cmp(&b.source_claim_count)
                    .then_with(|| b.title.cmp(&a.title))
            })
            .expect("non-empty feeds");
        let top_resolved_feed = self
            .feeds
            .iter()
            .max_by(|a, b| {
                a.resolved_count
                    .cmp(&b.resolved_count)
                    .then_with(|| b.title.cmp(&a.title))
            })
            .expect("non-empty feeds");

        let claim_hotspots = self
            .feeds
            .iter()
            .take(5)
            .map(|feed| {
                format!(
                    "  {} [{}] claims={} resolved={} tracks={}",
                    feed.title,
                    short_id(&feed.feed_guid),
                    feed.source_claim_count,
                    feed.resolved_count,
                    feed.track_count
                )
            })
            .collect::<Vec<_>>();

        let mut lines = vec![
            format!(
                "Feeds queued: {}  Tracks: {}  Source claims: {}  Resolved overlays: {}",
                feed_count, total_tracks, total_claims, total_resolved
            ),
            String::new(),
            format!(
                "Top feed by source claims: {} [{}] ({} claims, {} resolved).",
                top_claim_feed.title,
                short_id(&top_claim_feed.feed_guid),
                top_claim_feed.source_claim_count,
                top_claim_feed.resolved_count
            ),
            format!(
                "Top feed by resolved overlays: {} [{}] ({} resolved, {} claims).",
                top_resolved_feed.title,
                short_id(&top_resolved_feed.feed_guid),
                top_resolved_feed.resolved_count,
                top_resolved_feed.source_claim_count
            ),
            String::new(),
            "Claim hotspots:".to_string(),
        ];
        if let Some(summary) = feed_family_subset_summary(&self.feeds) {
            lines.push(summary);
            lines.push(String::new());
        }
        lines.extend(claim_hotspots);
        lines.push(String::new());
        lines.push("Claim family mix:".to_string());
        let family_rows = claim_family_rows(claim_family_totals);
        let total_family_claims = family_rows.iter().map(|(_, count)| *count).sum::<usize>();
        lines.extend(family_rows.into_iter().filter(|(_, count)| *count > 0).map(
            |(label, count)| {
                let share = (count * 100) / total_family_claims.max(1);
                format!("  {label}: {count} ({share}%)")
            },
        ));

        self.dialog = Some(stophammer::tui::text_dialog(
            format!("Source Claims Operator Overview ({feed_count})"),
            lines,
        ));
    }

    fn show_hotspots_dialog(&mut self) {
        let mut lines = vec![
            "Top feeds by source-claims load".to_string(),
            String::new(),
        ];
        if self.feeds.is_empty() {
            lines.push("No source-claims hotspots queued.".to_string());
        } else {
            if let Some(summary) = feed_family_subset_summary(
                &self.feeds.iter().take(10).cloned().collect::<Vec<_>>(),
            ) {
                lines.push(summary);
                lines.push(String::new());
            }
            let total_claim_load: i64 = self.feeds.iter().map(|feed| feed.source_claim_count).sum();
            lines.extend(self.feeds.iter().take(10).map(|feed| {
                let share = if total_claim_load > 0 {
                    (feed.source_claim_count * 100) / total_claim_load
                } else {
                    0
                };
                format!(
                    "{} [{}] | claims={} ({}%) resolved={} tracks={} {}",
                    feed.title,
                    short_id(&feed.feed_guid),
                    feed.source_claim_count,
                    share,
                    feed.resolved_count,
                    feed.track_count,
                    dominant_feed_claim_family_summary(feed)
                )
            }));
        }
        self.dialog = Some(stophammer::tui::text_dialog(
            format!(
                "Source Claims Hotspots ({})",
                self.feeds.len().min(10)
            ),
            lines,
        ));
    }

    fn show_review_playbook(&mut self) {
        let feed_count = self.feeds.len();
        let total_claim_load: i64 = self.feeds.iter().map(|feed| feed.source_claim_count).sum();
        let total_resolved_load: i64 = self.feeds.iter().map(|feed| feed.resolved_count).sum();
        let total_tracks: i64 = self.feeds.iter().map(|feed| feed.track_count).sum();
        let claim_family_totals = self.queue_claim_family_totals;
        let mut lines = vec![
            format!(
                "Queued feeds: {}  Tracks: {}  Source claims: {}  Resolved overlays: {}",
                feed_count, total_tracks, total_claim_load, total_resolved_load
            ),
        ];
        if let Some(summary) = feed_family_subset_summary(&self.feeds) {
            lines.push(summary);
        }
        lines.push(String::new());
        if self.feeds.is_empty() {
            lines.push("Backlog idle: no feeds with source claims or resolved overlays are queued.".to_string());
            self.dialog = Some(stophammer::tui::text_dialog(
                "Source Claims Playbook (0)",
                lines,
            ));
            return;
        }

        let top_feed = &self.feeds[0];
        let top_share = if total_claim_load > 0 {
            (top_feed.source_claim_count * 100) / total_claim_load
        } else {
            0
        };
        lines.push(format!(
            "1. Start with {} [{}] (claims={}, resolved={}, tracks={}, {}).",
            top_feed.title,
            short_id(&top_feed.feed_guid),
            top_feed.source_claim_count,
            top_feed.resolved_count,
            top_feed.track_count,
            dominant_feed_claim_family_summary(top_feed)
        ));
        lines.push(format!("   {}", abbreviate(&top_feed.feed_url, 72)));
        if top_share >= 50 {
            lines.push(format!(
                "2. This one feed holds {}% of current source-claims load; stay on it until the evidence story is clear.",
                top_share
            ));
        } else {
            lines.push(
                "2. Use h to compare the top feeds before drilling into track evidence."
                    .to_string(),
            );
        }
        if let Some((label, count, share)) = dominant_claim_family(claim_family_totals) {
            lines.push(format!(
                "3. Dominant claim family: {label} ({count} rows, {share}% of queued claim load)."
            ));
            let hint = match label {
                "contributors" => {
                    "Start by reading role/name disagreement and group-name drift in the evidence pane."
                }
                "entity_ids" => {
                    "Check whether the queue is mostly external-ID drift before trusting canonical overlays."
                }
                "links" => {
                    "Look for website/social link disagreement before assuming artist or platform identity splits."
                }
                "releases" => {
                    "Inspect release-title and release-type rows before focusing on track-level claim noise."
                }
                "platforms" => {
                    "Platform claims dominate; compare owner names and URLs before trusting publisher/platform lineage."
                }
                "enclosures" => {
                    "Media variants dominate; inspect enclosure rows before treating differences as identity issues."
                }
                _ => "Inspect the dominant claim family first before drilling into secondary evidence.",
            };
            lines.push(format!("   {hint}"));
        }
        lines.push(
            "4. Use s on the selected feed, then inspect track-scoped claim rows in the evidence pane."
                .to_string(),
        );
        lines.push(
            "5. Use o for backlog totals and q/r to leave or refresh after source data changes."
                .to_string(),
        );

        self.dialog = Some(stophammer::tui::text_dialog(
            format!("Source Claims Playbook ({feed_count})"),
            lines,
        ));
    }

    fn show_conflicts_dialog(&mut self) {
        let Some(feed_row) = self.current_feed_row() else {
            self.dialog = Some(stophammer::tui::text_dialog(
                "Feed Conflicts (0)",
                vec!["No feed selected.".to_string()],
            ));
            return;
        };
        let Some(snapshot) = self.current_snapshot() else {
            self.dialog = Some(stophammer::tui::text_dialog(
                format!("Feed Conflicts [{}]", short_id(&feed_row.feed_guid)),
                vec!["No feed snapshot loaded.".to_string()],
            ));
            return;
        };

        let conflicts = feed_conflict_lines(snapshot);
        let count = conflicts
            .iter()
            .filter(|line| *line != "no obvious feed-level claim conflicts detected")
            .count();
        let mut lines = vec![
            format!("Feed: {} [{}]", feed_row.title, short_id(&feed_row.feed_guid)),
            format!("URL: {}", abbreviate(&feed_row.feed_url, 80)),
            String::new(),
        ];
        if count == 0 {
            lines.push("No obvious feed-level claim conflicts detected.".to_string());
        } else {
            lines.push(format!("Detected conflicts: {count}"));
            lines.push(String::new());
            lines.extend(conflicts);
        }

        self.dialog = Some(stophammer::tui::text_dialog(
            format!("Feed Conflicts [{}]", short_id(&feed_row.feed_guid)),
            lines,
        ));
    }

    fn show_claim_mix_dialog(&mut self) {
        let Some(feed_row) = self.current_feed_row() else {
            self.dialog = Some(stophammer::tui::text_dialog(
                "Claim Mix (0)",
                vec!["No feed selected.".to_string()],
            ));
            return;
        };
        let Some(snapshot) = self.current_snapshot() else {
            self.dialog = Some(stophammer::tui::text_dialog(
                format!("Claim Mix [{}]", short_id(&feed_row.feed_guid)),
                vec!["No feed snapshot loaded.".to_string()],
            ));
            return;
        };

        let contributor_feed = snapshot
            .contributor_claims
            .iter()
            .filter(|claim| claim.entity_type == "feed")
            .count();
        let contributor_track = snapshot
            .contributor_claims
            .iter()
            .filter(|claim| claim.entity_type == "track")
            .count();
        let id_feed = snapshot
            .entity_id_claims
            .iter()
            .filter(|claim| claim.entity_type == "feed")
            .count();
        let id_track = snapshot
            .entity_id_claims
            .iter()
            .filter(|claim| claim.entity_type == "track")
            .count();
        let link_feed = snapshot
            .link_claims
            .iter()
            .filter(|claim| claim.entity_type == "feed")
            .count();
        let link_track = snapshot
            .link_claims
            .iter()
            .filter(|claim| claim.entity_type == "track")
            .count();
        let release_feed = snapshot
            .release_claims
            .iter()
            .filter(|claim| claim.entity_type == "feed")
            .count();
        let release_track = snapshot
            .release_claims
            .iter()
            .filter(|claim| claim.entity_type == "track")
            .count();
        let enclosure_track = snapshot
            .enclosures
            .iter()
            .filter(|claim| claim.entity_type == "track")
            .count();
        let platform_feed = snapshot.platform_claims.len();

        let family_rows = vec![
            ("contributors", contributor_feed, contributor_track),
            ("entity_ids", id_feed, id_track),
            ("links", link_feed, link_track),
            ("releases", release_feed, release_track),
            ("platforms", platform_feed, 0),
            ("enclosures", 0, enclosure_track),
        ];
        let total_source_claims = family_rows
            .iter()
            .map(|(_, feed_count, track_count)| feed_count + track_count)
            .sum::<usize>();

        let mut lines = vec![
            format!("Feed: {} [{}]", feed_row.title, short_id(&feed_row.feed_guid)),
            format!("URL: {}", abbreviate(&feed_row.feed_url, 80)),
            format!(
                "Tracks: {}  Total source claim rows: {}",
                snapshot.tracks.len(),
                total_source_claims
            ),
            String::new(),
            "Claim families:".to_string(),
        ];
        lines.extend(family_rows.into_iter().filter_map(|(label, feed_count, track_count)| {
            let total = feed_count + track_count;
            (total > 0).then(|| {
                let share = (total * 100) / total_source_claims.max(1);
                format!(
                    "  {label}: total={total} ({share}%) feed={feed_count} track={track_count}"
                )
            })
        }));

        self.dialog = Some(stophammer::tui::text_dialog(
            format!("Claim Mix [{}]", short_id(&feed_row.feed_guid)),
            lines,
        ));
    }

    fn move_down(&mut self) -> Result<(), Box<dyn Error>> {
        match self.focus {
            Focus::Feeds => {
                if self.feeds.is_empty() {
                    return Ok(());
                }
                let next = (self.feed_state.selected().unwrap_or(0) + 1)
                    .min(self.feeds.len().saturating_sub(1));
                self.feed_state.select(Some(next));
                self.load_selected_feed(None)?;
            }
            Focus::Tracks => {
                let Some(snapshot) = self.snapshot.as_ref() else {
                    return Ok(());
                };
                if snapshot.tracks.is_empty() {
                    return Ok(());
                }
                let next = (self.track_state.selected().unwrap_or(0) + 1)
                    .min(snapshot.tracks.len().saturating_sub(1));
                self.track_state.select(Some(next));
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
            Focus::Feeds => {
                if self.feeds.is_empty() {
                    return Ok(());
                }
                let next = self.feed_state.selected().unwrap_or(0).saturating_sub(1);
                self.feed_state.select(Some(next));
                self.load_selected_feed(None)?;
            }
            Focus::Tracks => {
                if self.snapshot.is_none() {
                    return Ok(());
                }
                let next = self.track_state.selected().unwrap_or(0).saturating_sub(1);
                self.track_state.select(Some(next));
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
            Focus::Feeds => {
                if !self.feeds.is_empty() {
                    self.feed_state.select(Some(0));
                    self.load_selected_feed(None)?;
                }
            }
            Focus::Tracks => {
                if self.snapshot.is_some() {
                    self.track_state.select(Some(0));
                    self.evidence_scroll = 0;
                }
            }
            Focus::Evidence => self.evidence_scroll = 0,
        }
        Ok(())
    }

    fn jump_bottom(&mut self) -> Result<(), Box<dyn Error>> {
        match self.focus {
            Focus::Feeds => {
                if !self.feeds.is_empty() {
                    self.feed_state
                        .select(Some(self.feeds.len().saturating_sub(1)));
                    self.load_selected_feed(None)?;
                }
            }
            Focus::Tracks => {
                if let Some(snapshot) = self.snapshot.as_ref()
                    && !snapshot.tracks.is_empty()
                {
                    self.track_state
                        .select(Some(snapshot.tracks.len().saturating_sub(1)));
                    self.evidence_scroll = 0;
                }
            }
            Focus::Evidence => self.evidence_scroll = u16::MAX,
        }
        Ok(())
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "manual CLI parsing keeps the review tool dependency-free"
)]
fn parse_args() -> Result<Args, String> {
    let mut db_path = PathBuf::from(DEFAULT_DB_PATH);
    let mut limit = 200usize;

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
                     "Usage: review_source_claims_tui [--db PATH] [--limit N]\n\
                     Interactive feed-scoped source claims and canonical promotions inspector.\n\
                     Shows source claim families, track-level claim evidence, and the current\n\
                     resolved promotions/provenance overlays for each feed.\n\
                     Keys: Tab/Left/Right focus, Up/Down/Home/End navigate, o overview, p playbook, s summary, t track mix, h hotspots, c conflicts, m claim mix, n/N same-family jump, ? help, r reload, q quit."
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args { db_path, limit })
}

fn list_claim_review_feeds(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<FeedQueueRow>, Box<dyn Error>> {
    let mut stmt = conn.prepare(
        "WITH track_counts AS (
             SELECT feed_guid, COUNT(*) AS track_count
             FROM tracks
             GROUP BY feed_guid
         ),
         source_counts AS (
             SELECT feed_guid, COUNT(*) AS source_claim_count
             FROM (
                 SELECT feed_guid FROM source_contributor_claims
                 UNION ALL
                 SELECT feed_guid FROM source_entity_ids
                 UNION ALL
                 SELECT feed_guid FROM source_entity_links
                 UNION ALL
                 SELECT feed_guid FROM source_release_claims
                 UNION ALL
                 SELECT feed_guid FROM source_item_enclosures
                 UNION ALL
                 SELECT feed_guid FROM source_platform_claims
             )
             GROUP BY feed_guid
         ),
         contributor_counts AS (
             SELECT feed_guid, COUNT(*) AS contributor_count
             FROM source_contributor_claims
             GROUP BY feed_guid
         ),
         entity_id_counts AS (
             SELECT feed_guid, COUNT(*) AS entity_id_count
             FROM source_entity_ids
             GROUP BY feed_guid
         ),
         link_counts AS (
             SELECT feed_guid, COUNT(*) AS link_count
             FROM source_entity_links
             GROUP BY feed_guid
         ),
         release_counts2 AS (
             SELECT feed_guid, COUNT(*) AS release_count
             FROM source_release_claims
             GROUP BY feed_guid
         ),
         enclosure_counts AS (
             SELECT feed_guid, COUNT(*) AS enclosure_count
             FROM source_item_enclosures
             GROUP BY feed_guid
         ),
         platform_counts AS (
             SELECT feed_guid, COUNT(*) AS platform_count
             FROM source_platform_claims
             GROUP BY feed_guid
         ),
         resolved_counts AS (
             SELECT feed_guid, COUNT(*) AS resolved_count
             FROM (
                 SELECT feed_guid FROM resolved_external_ids_by_feed
                 UNION ALL
                 SELECT feed_guid FROM resolved_entity_sources_by_feed
             )
             GROUP BY feed_guid
         )
         SELECT
             f.feed_guid,
             f.title,
             f.feed_url,
             COALESCE(tc.track_count, 0),
             COALESCE(sc.source_claim_count, 0),
             COALESCE(rc.resolved_count, 0),
             COALESCE(cc.contributor_count, 0),
             COALESCE(ic.entity_id_count, 0),
             COALESCE(lc.link_count, 0),
             COALESCE(rc2.release_count, 0),
             COALESCE(pc.platform_count, 0),
             COALESCE(ec.enclosure_count, 0)
         FROM feeds f
         LEFT JOIN track_counts tc ON tc.feed_guid = f.feed_guid
         LEFT JOIN source_counts sc ON sc.feed_guid = f.feed_guid
         LEFT JOIN contributor_counts cc ON cc.feed_guid = f.feed_guid
         LEFT JOIN entity_id_counts ic ON ic.feed_guid = f.feed_guid
         LEFT JOIN link_counts lc ON lc.feed_guid = f.feed_guid
         LEFT JOIN release_counts2 rc2 ON rc2.feed_guid = f.feed_guid
         LEFT JOIN platform_counts pc ON pc.feed_guid = f.feed_guid
         LEFT JOIN enclosure_counts ec ON ec.feed_guid = f.feed_guid
         LEFT JOIN resolved_counts rc ON rc.feed_guid = f.feed_guid
         WHERE COALESCE(sc.source_claim_count, 0) > 0 OR COALESCE(rc.resolved_count, 0) > 0
         ORDER BY COALESCE(sc.source_claim_count, 0) DESC,
                  COALESCE(rc.resolved_count, 0) DESC,
                  f.title_lower,
                  f.feed_guid
         LIMIT ?1",
    )?;

    let limit_i64 = i64::try_from(limit)
        .map_err(|_err| io::Error::other("feed review limit exceeds SQLite integer range"))?;
    let rows = stmt
        .query_map(params![limit_i64], |row| {
            Ok(FeedQueueRow {
                feed_guid: row.get(0)?,
                title: row.get(1)?,
                feed_url: row.get(2)?,
                track_count: row.get(3)?,
                source_claim_count: row.get(4)?,
                resolved_count: row.get(5)?,
                contributor_count: row.get(6)?,
                entity_id_count: row.get(7)?,
                link_count: row.get(8)?,
                release_count: row.get(9)?,
                platform_count: row.get(10)?,
                enclosure_count: row.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn load_feed_claim_snapshot(
    conn: &Connection,
    feed_guid: &str,
) -> Result<FeedClaimSnapshot, Box<dyn Error>> {
    let feed = stophammer::db::get_feed(conn, feed_guid)?
        .ok_or_else(|| io::Error::other(format!("feed missing: {feed_guid}")))?;
    Ok(FeedClaimSnapshot {
        feed,
        tracks: stophammer::db::get_tracks_for_feed(conn, feed_guid)?,
        contributor_claims: stophammer::db::get_source_contributor_claims_for_feed(
            conn, feed_guid,
        )?,
        entity_id_claims: stophammer::db::get_source_entity_ids_for_feed(conn, feed_guid)?,
        link_claims: stophammer::db::get_source_entity_links_for_feed(conn, feed_guid)?,
        release_claims: stophammer::db::get_source_release_claims_for_feed(conn, feed_guid)?,
        enclosures: stophammer::db::get_source_item_enclosures_for_feed(conn, feed_guid)?,
        platform_claims: stophammer::db::get_source_platform_claims_for_feed(conn, feed_guid)?,
        resolved_external_ids: stophammer::db::get_resolved_external_ids_for_feed(conn, feed_guid)?,
        resolved_entity_sources: stophammer::db::get_resolved_entity_sources_for_feed(
            conn, feed_guid,
        )?,
    })
}

fn count_rows_for_feed_guids(
    conn: &Connection,
    table: &str,
    feed_guids: &[String],
) -> Result<usize, Box<dyn Error>> {
    if feed_guids.is_empty() {
        return Ok(0);
    }
    let placeholders = std::iter::repeat_n("?", feed_guids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE feed_guid IN ({placeholders})");
    let count = conn.query_row(&sql, params_from_iter(feed_guids.iter()), |row| {
        row.get::<_, i64>(0)
    })?;
    Ok(usize::try_from(count).map_err(|_err| io::Error::other("row count exceeded usize"))?)
}

fn load_queue_claim_family_totals(
    conn: &Connection,
    feed_guids: &[String],
) -> Result<ClaimFamilyTotals, Box<dyn Error>> {
    Ok(ClaimFamilyTotals {
        contributors: count_rows_for_feed_guids(conn, "source_contributor_claims", feed_guids)?,
        entity_ids: count_rows_for_feed_guids(conn, "source_entity_ids", feed_guids)?,
        links: count_rows_for_feed_guids(conn, "source_entity_links", feed_guids)?,
        releases: count_rows_for_feed_guids(conn, "source_release_claims", feed_guids)?,
        platforms: count_rows_for_feed_guids(conn, "source_platform_claims", feed_guids)?,
        enclosures: count_rows_for_feed_guids(conn, "source_item_enclosures", feed_guids)?,
    })
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

fn feed_conflict_lines(snapshot: &FeedClaimSnapshot) -> Vec<String> {
    let mut lines = Vec::new();

    let mut website_by_entity: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for link in &snapshot.link_claims {
        if link.link_type == "website" {
            website_by_entity
                .entry((link.entity_type.clone(), link.entity_id.clone()))
                .or_default()
                .push(link.url.clone());
        }
    }
    for ((entity_type, entity_id), urls) in website_by_entity {
        let unique = urls.into_iter().collect::<std::collections::BTreeSet<_>>();
        if unique.len() > 1 {
            lines.push(format!(
                "multiple websites for {entity_type} {}: {}",
                short_id(&entity_id),
                preview_join(&unique.into_iter().collect::<Vec<_>>(), 4, 36)
            ));
        }
    }

    let mut ids_by_scheme_entity: BTreeMap<(String, String, String), Vec<String>> = BTreeMap::new();
    for claim in &snapshot.entity_id_claims {
        ids_by_scheme_entity
            .entry((
                claim.entity_type.clone(),
                claim.entity_id.clone(),
                claim.scheme.clone(),
            ))
            .or_default()
            .push(claim.value.clone());
    }
    for ((entity_type, entity_id, scheme), values) in ids_by_scheme_entity {
        let unique = values
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        if unique.len() > 1 {
            lines.push(format!(
                "multiple {scheme} values for {entity_type} {}: {}",
                short_id(&entity_id),
                preview_join(&unique.into_iter().collect::<Vec<_>>(), 4, 32)
            ));
        }
    }

    let owner_names = snapshot
        .platform_claims
        .iter()
        .filter_map(|claim| claim.owner_name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    if owner_names.len() > 1 {
        lines.push(format!(
            "multiple platform owner names: {}",
            preview_join(&owner_names.into_iter().collect::<Vec<_>>(), 5, 28)
        ));
    }

    if lines.is_empty() {
        lines.push("no obvious feed-level claim conflicts detected".to_string());
    }
    lines
}

fn build_feed_items(app: &App) -> Vec<ListItem<'static>> {
    if app.feeds.is_empty() {
        return vec![ListItem::new("No feeds")];
    }
    app.feeds
        .iter()
        .map(|feed| {
            ListItem::new(vec![
                Line::from(Span::styled(
                    format!("{} [{}]", feed.title, short_id(&feed.feed_guid)),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(vec![
                    Span::styled(
                        format!("{} claims", feed.source_claim_count),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("{} resolved", feed.resolved_count),
                        Style::default().fg(Color::Green),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("{} tracks", feed.track_count),
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
                Line::from(Span::styled(
                    dominant_feed_claim_family_summary(feed),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    abbreviate(&feed.feed_url, 40),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        })
        .collect()
}

fn build_track_items(app: &App) -> Vec<ListItem<'static>> {
    let Some(snapshot) = app.current_snapshot() else {
        return vec![ListItem::new("No tracks")];
    };
    if snapshot.tracks.is_empty() {
        return vec![ListItem::new("No tracks")];
    }

    snapshot
        .tracks
        .iter()
        .map(|track| {
            let track_claims = count_track_claims(snapshot, &track.track_guid);
            ListItem::new(vec![
                Line::from(Span::styled(
                    format!("{} [{}]", track.title, short_id(&track.track_guid)),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(vec![
                    Span::styled(
                        format!("{} claim rows", track_claims),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        track
                            .pub_date
                            .map(format_local_timestamp)
                            .unwrap_or_else(|| "-".to_string()),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(Span::styled(
                    dominant_track_claim_family_summary(snapshot, &track.track_guid),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    track
                        .enclosure_url
                        .as_deref()
                        .map(|url| abbreviate(url, 40))
                        .unwrap_or_else(|| "-".to_string()),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        })
        .collect()
}

fn count_track_claims(snapshot: &FeedClaimSnapshot, track_guid: &str) -> usize {
    snapshot
        .contributor_claims
        .iter()
        .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
        .count()
        + snapshot
            .entity_id_claims
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
            .count()
        + snapshot
            .link_claims
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
            .count()
        + snapshot
            .release_claims
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
            .count()
        + snapshot
            .enclosures
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
            .count()
}

fn push_section(lines: &mut Vec<Line<'static>>, title: &str, accent: Color) {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        title.to_string(),
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )));
}

fn push_detail(lines: &mut Vec<Line<'static>>, label: &str, value: String) {
    lines.push(Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(Color::DarkGray)),
        Span::styled(value, Style::default().fg(Color::White)),
    ]));
}

fn build_evidence_lines(app: &App) -> Vec<Line<'static>> {
    let Some(snapshot) = app.current_snapshot() else {
        return vec![Line::from("No feed selected.")];
    };
    let selected_track_guid = app.current_track().map(|track| track.track_guid.clone());
    let mut lines = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("Feed: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{} [{}]", snapshot.feed.title, snapshot.feed.feed_guid),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    push_detail(&mut lines, "url", abbreviate(&snapshot.feed.feed_url, 110));
    push_detail(
        &mut lines,
        "updated",
        format_local_timestamp(snapshot.feed.updated_at),
    );
    push_detail(
        &mut lines,
        "medium",
        snapshot
            .feed
            .raw_medium
            .clone()
            .unwrap_or_else(|| "-".to_string()),
    );
    push_detail(
        &mut lines,
        "description",
        snapshot
            .feed
            .description
            .clone()
            .map(|value| abbreviate(&value, 120))
            .unwrap_or_else(|| "-".to_string()),
    );

    push_section(&mut lines, "Conflicts", Color::LightRed);
    for conflict in feed_conflict_lines(snapshot) {
        lines.push(Line::from(vec![
            Span::styled("• ", Style::default().fg(Color::LightRed)),
            Span::styled(conflict, Style::default().fg(Color::White)),
        ]));
    }

    push_section(&mut lines, "Resolved Promotions", Color::Green);
    if snapshot.resolved_external_ids.is_empty() && snapshot.resolved_entity_sources.is_empty() {
        lines.push(Line::from(Span::styled(
            "no resolved promotion/provenance overlays",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for row in &snapshot.resolved_external_ids {
            lines.push(Line::from(vec![
                Span::styled("external ", Style::default().fg(Color::Green)),
                Span::styled(
                    format!(
                        "{} {} {}={}",
                        row.entity_type,
                        short_id(&row.entity_id),
                        row.scheme,
                        abbreviate(&row.value, 42)
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
        for row in &snapshot.resolved_entity_sources {
            lines.push(Line::from(vec![
                Span::styled("source ", Style::default().fg(Color::Green)),
                Span::styled(
                    format!(
                        "{} {} {} trust={} {}",
                        row.entity_type,
                        short_id(&row.entity_id),
                        row.source_type,
                        row.trust_level,
                        row.source_url
                            .as_deref()
                            .map(|url| abbreviate(url, 48))
                            .unwrap_or_else(|| "-".to_string())
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
    }

    push_section(&mut lines, "Feed Claims", Color::Cyan);
    push_detail(
        &mut lines,
        "platform claims",
        snapshot.platform_claims.len().to_string(),
    );
    for claim in &snapshot.platform_claims {
        lines.push(Line::from(vec![
            Span::styled("platform ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!(
                    "{} owner={} {}",
                    claim.platform_key,
                    claim.owner_name.as_deref().unwrap_or("-"),
                    claim
                        .url
                        .as_deref()
                        .map(|url| abbreviate(url, 46))
                        .unwrap_or_else(|| "-".to_string())
                ),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    for claim in snapshot
        .contributor_claims
        .iter()
        .filter(|claim| claim.entity_type == "feed")
    {
        lines.push(Line::from(vec![
            Span::styled("contrib ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!(
                    "{} role={} group={}",
                    claim.name,
                    claim.role_norm.as_deref().unwrap_or("-"),
                    claim.group_name.as_deref().unwrap_or("-")
                ),
                Style::default().fg(Color::White),
            ),
        ]));
    }
    for claim in snapshot
        .entity_id_claims
        .iter()
        .filter(|claim| claim.entity_type == "feed")
    {
        lines.push(Line::from(vec![
            Span::styled("id ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{}={}", claim.scheme, abbreviate(&claim.value, 52)),
                Style::default().fg(Color::White),
            ),
        ]));
    }
    for claim in snapshot
        .link_claims
        .iter()
        .filter(|claim| claim.entity_type == "feed")
    {
        lines.push(Line::from(vec![
            Span::styled("link ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{} {}", claim.link_type, abbreviate(&claim.url, 64)),
                Style::default().fg(Color::White),
            ),
        ]));
    }
    for claim in snapshot
        .release_claims
        .iter()
        .filter(|claim| claim.entity_type == "feed")
    {
        lines.push(Line::from(vec![
            Span::styled("release ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!(
                    "{}: {}",
                    claim.claim_type,
                    abbreviate(&claim.claim_value, 58)
                ),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    push_section(&mut lines, "Track Claims", Color::Yellow);
    if let Some(track_guid) = selected_track_guid {
        if let Some(track) = app.current_track() {
            push_detail(
                &mut lines,
                "selected track",
                format!("{} [{}]", track.title, track.track_guid),
            );
        }

        let mut any = false;
        for claim in snapshot
            .contributor_claims
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
        {
            any = true;
            lines.push(Line::from(vec![
                Span::styled("contrib ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!(
                        "{} role={} href={}",
                        claim.name,
                        claim.role_norm.as_deref().unwrap_or("-"),
                        claim
                            .href
                            .as_deref()
                            .map(|value| abbreviate(value, 34))
                            .unwrap_or_else(|| "-".to_string())
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
        for claim in snapshot
            .entity_id_claims
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
        {
            any = true;
            lines.push(Line::from(vec![
                Span::styled("id ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("{}={}", claim.scheme, abbreviate(&claim.value, 54)),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
        for claim in snapshot
            .link_claims
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
        {
            any = true;
            lines.push(Line::from(vec![
                Span::styled("link ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("{} {}", claim.link_type, abbreviate(&claim.url, 62)),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
        for claim in snapshot
            .release_claims
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
        {
            any = true;
            lines.push(Line::from(vec![
                Span::styled("release ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!(
                        "{}: {}",
                        claim.claim_type,
                        abbreviate(&claim.claim_value, 56)
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
        for claim in snapshot
            .enclosures
            .iter()
            .filter(|claim| claim.entity_type == "track" && claim.entity_id == track_guid)
        {
            any = true;
            lines.push(Line::from(vec![
                Span::styled("enclosure ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!(
                        "{} mime={} primary={}",
                        abbreviate(&claim.url, 44),
                        claim.mime_type.as_deref().unwrap_or("-"),
                        claim.is_primary
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
        if !any {
            lines.push(Line::from(Span::styled(
                "no track-scoped claims for selected track",
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "select a track to inspect track-scoped claims",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines
}

fn header_context_line(app: &App) -> Line<'static> {
    let mut spans = vec![Span::styled(
        "Source Claims Review TUI",
        Style::default()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::BOLD),
    )];

    if let Some(feed) = app.current_feed_row() {
        let feed_position = app.feed_state.selected().unwrap_or(0).saturating_add(1);
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!(
                "feed {}/{} {} [{}] claims={} resolved={}",
                feed_position,
                app.feeds.len(),
                abbreviate(&feed.title, 28),
                short_id(&feed.feed_guid),
                feed.source_claim_count,
                feed.resolved_count
            ),
            Style::default().fg(Color::White),
        ));
        if let Some((label, position, total)) = current_family_position(app) {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("family {label} {position}/{total}"),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    if let Some(snapshot) = app.current_snapshot()
        && !snapshot.tracks.is_empty()
    {
        let track_position = app.track_state.selected().unwrap_or(0).saturating_add(1);
        if let Some(track) = app.current_track() {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!(
                    "track {}/{} {} [{}] claims={} {}",
                    track_position,
                    snapshot.tracks.len(),
                    abbreviate(&track.title, 24),
                    short_id(&track.track_guid),
                    count_track_claims(snapshot, &track.track_guid),
                    dominant_track_claim_family_summary(snapshot, &track.track_guid)
                ),
                Style::default().fg(Color::Yellow),
            ));
        }
    }

    Line::from(spans)
}

fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(2),
    ])
    .split(area);

    let header = Paragraph::new(vec![
        header_context_line(app),
        Line::from(Span::styled(
            queue_claim_family_summary(app.queue_claim_family_totals).map_or_else(
                || app.status.clone(),
                |summary| format!("{}  |  {}", app.status, summary),
            ),
            Style::default().fg(Color::DarkGray),
        )),
    ]);
    frame.render_widget(header, layout[0]);

    let body = Layout::horizontal([
        Constraint::Percentage(28),
        Constraint::Percentage(24),
        Constraint::Percentage(48),
    ])
    .split(layout[1]);

    let feed_title = if app.feeds.is_empty() {
        "Feeds".to_string()
    } else {
        let position = app.feed_state.selected().unwrap_or(0).saturating_add(1);
        current_family_position(app).map_or_else(
            || format!("Feeds ({position}/{})", app.feeds.len()),
            |(label, family_position, family_total)| {
                format!(
                    "Feeds ({position}/{}, {label} {family_position}/{family_total})",
                    app.feeds.len()
                )
            },
        )
    };
    let feed_list = List::new(build_feed_items(app))
        .block(stophammer::tui::titled_block(
            &feed_title,
            Color::LightBlue,
            app.focus == Focus::Feeds,
            Style::default().fg(Color::DarkGray),
        ))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(32, 96, 160))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    frame.render_stateful_widget(feed_list, body[0], &mut app.feed_state);

    let track_title = app.current_snapshot().map_or_else(
        || "Tracks".to_string(),
        |snapshot| {
            if snapshot.tracks.is_empty() {
                "Tracks (0)".to_string()
            } else {
                let position = app.track_state.selected().unwrap_or(0).saturating_add(1);
                app.current_track().map_or_else(
                    || format!("Tracks ({position}/{})", snapshot.tracks.len()),
                    |track| {
                        format!(
                            "Tracks ({position}/{}, {})",
                            snapshot.tracks.len(),
                            dominant_track_claim_family_summary(snapshot, &track.track_guid)
                        )
                    },
                )
            }
        },
    );
    let track_list = List::new(build_track_items(app))
        .block(stophammer::tui::titled_block(
            &track_title,
            Color::Yellow,
            app.focus == Focus::Tracks,
            Style::default().fg(Color::DarkGray),
        ))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(126, 94, 18))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    frame.render_stateful_widget(track_list, body[1], &mut app.track_state);

    let evidence_title = app.current_feed_row().map_or_else(
        || "Evidence".to_string(),
        |feed| {
            format!(
                "Evidence {} (claims={} resolved={} {})",
                feed.title,
                feed.source_claim_count,
                feed.resolved_count,
                dominant_feed_claim_family_summary(feed)
            )
        },
    );
    let evidence = Paragraph::new(build_evidence_lines(app))
        .block(stophammer::tui::titled_block(
            &evidence_title,
            Color::Green,
            app.focus == Focus::Evidence,
            Style::default().fg(Color::DarkGray),
        ))
        .wrap(Wrap { trim: false })
        .scroll((app.evidence_scroll, 0));
    frame.render_widget(evidence, body[2]);

    let footer = Paragraph::new(stophammer::tui::build_footer(&[
        "TAB/LEFT/RIGHT: focus",
        "UP/DOWN: move",
        "HOME/END: jump",
        "O: overview",
        "P: playbook",
        "S: summary",
        "T: track-mix",
        "H: hotspots",
        "C: conflicts",
        "M: mix",
        "N: same-family",
        "?: help",
        "R: reload",
        "Q: quit",
    ]));
    frame.render_widget(footer, layout[2]);

    if let Some(dialog) = &app.dialog {
        stophammer::tui::render_text_dialog(frame, area, dialog);
    }
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
            KeyCode::Char('o') => app.show_operator_overview_dialog(),
            KeyCode::Char('p') => app.show_review_playbook(),
            KeyCode::Char('s') => app.show_summary_dialog(),
            KeyCode::Char('t') => app.show_track_claim_mix_dialog(),
            KeyCode::Char('h') => app.show_hotspots_dialog(),
            KeyCode::Char('c') => app.show_conflicts_dialog(),
            KeyCode::Char('m') => app.show_claim_mix_dialog(),
            KeyCode::Char('n') => app.jump_same_family(true)?,
            KeyCode::Char('N') => app.jump_same_family(false)?,
            KeyCode::Char('?') => app.show_help_dialog(),
            KeyCode::Char('r') => {
                let feed_guid = app.current_feed_row().map(|row| row.feed_guid.clone());
                let track_guid = app.current_track().map(|track| track.track_guid.clone());
                app.reload(feed_guid.as_deref(), track_guid.as_deref())?;
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
