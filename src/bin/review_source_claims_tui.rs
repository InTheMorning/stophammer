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
use rusqlite::{Connection, params};
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
    feed_state: ListState,
    track_state: ListState,
    focus: Focus,
    snapshot: Option<FeedClaimSnapshot>,
    evidence_scroll: u16,
    status: String,
}

impl App {
    fn new(db_path: &Path, limit: usize) -> Result<Self, Box<dyn Error>> {
        let conn = stophammer::db::open_db(db_path);
        let mut app = Self {
            conn,
            limit,
            feeds: Vec::new(),
            feed_state: ListState::default(),
            track_state: ListState::default(),
            focus: Focus::Feeds,
            snapshot: None,
            evidence_scroll: 0,
            status: "Loading source-claims review queue...".to_string(),
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
                     resolved promotions/provenance overlays for each feed."
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
             COALESCE(rc.resolved_count, 0)
         FROM feeds f
         LEFT JOIN track_counts tc ON tc.feed_guid = f.feed_guid
         LEFT JOIN source_counts sc ON sc.feed_guid = f.feed_guid
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

fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    let layout = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(10),
        Constraint::Length(2),
    ])
    .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "Source Claims Review TUI",
            Style::default()
                .fg(Color::LightBlue)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(app.status.clone(), Style::default().fg(Color::White)),
    ]));
    frame.render_widget(header, layout[0]);

    let body = Layout::horizontal([
        Constraint::Percentage(28),
        Constraint::Percentage(24),
        Constraint::Percentage(48),
    ])
    .split(layout[1]);

    let feed_list = List::new(build_feed_items(app))
        .block(stophammer::tui::titled_block(
            "Feeds",
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

    let track_list = List::new(build_track_items(app))
        .block(stophammer::tui::titled_block(
            "Tracks",
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
        |feed| format!("Evidence {}", feed.title),
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
        "R: reload",
        "Q: quit",
    ]));
    frame.render_widget(footer, layout[2]);
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
        match key.code {
            KeyCode::Char('q') => return Ok(()),
            KeyCode::Tab | KeyCode::Right => app.next_focus(),
            KeyCode::BackTab | KeyCode::Left => app.previous_focus(),
            KeyCode::Down => app.move_down()?,
            KeyCode::Up => app.move_up()?,
            KeyCode::Home => app.jump_top()?,
            KeyCode::End => app.jump_bottom()?,
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
