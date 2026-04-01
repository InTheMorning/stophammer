#![allow(
    clippy::cast_possible_truncation,
    reason = "dialog layout sizes are bounded by terminal dimensions and then saturated"
)]
#![allow(
    clippy::collapsible_if,
    reason = "the current branching keeps wallet-domain classification easy to audit"
)]
#![allow(
    clippy::if_not_else,
    reason = "the current list-construction branches match the UI empty-state wording"
)]
#![allow(
    clippy::map_err_ignore,
    reason = "CLI argument parsing intentionally returns user-facing strings instead of preserving parse error types"
)]
#![allow(
    clippy::map_unwrap_or,
    reason = "the TUI rendering code uses explicit default labels in several Option formatting sites"
)]
#![allow(
    clippy::match_same_arms,
    reason = "duplicated focus transitions are clearer than over-compressed match arms in keyboard navigation code"
)]
#![allow(
    clippy::needless_pass_by_value,
    reason = "some dialog/title helpers accept owned strings because callers often build them dynamically"
)]
#![allow(
    clippy::similar_names,
    reason = "UI handlers use short status/stat names tied to domain concepts"
)]
#![allow(
    clippy::struct_excessive_bools,
    reason = "section visibility is naturally modeled as a small set of independent toggles"
)]
#![allow(
    clippy::too_many_lines,
    reason = "the wallet review TUI keeps evidence assembly and draw logic inline for operator maintainability"
)]
#![allow(
    clippy::uninlined_format_args,
    reason = "existing TUI/status formatting favors consistency with surrounding code"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use rusqlite::{Connection, OptionalExtension, params};
use stophammer::db::{DEFAULT_DB_PATH, WALLET_CLASS_VALUES};
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

#[derive(Debug)]
struct Args {
    db_path: PathBuf,
    limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Groups,
    SourceWallets,
    Candidates,
    Feeds,
    Evidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionKind {
    Routes,
    PlatformClaims,
    EntityIdClaims,
    ContributorClaims,
    LinkClaims,
    ReleaseClaims,
}

#[derive(Debug, Clone)]
enum EvidenceNode {
    Section(SectionKind),
    ItemHeader(String),
    ItemDetail,
}

#[derive(Debug, Clone)]
struct EvidenceRow {
    node: EvidenceNode,
    lines: Vec<Line<'static>>,
}

#[derive(Debug, Clone)]
struct EvidenceBranch {
    key: String,
    header: String,
    children: Vec<String>,
    default_collapsed: bool,
}

#[derive(Debug, Clone)]
struct SummaryDialog {
    title: String,
    lines: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct SectionState {
    routes: bool,
    platform_claims: bool,
    entity_id_claims: bool,
    contributor_claims: bool,
    link_claims: bool,
    release_claims: bool,
}

impl Default for SectionState {
    fn default() -> Self {
        Self {
            routes: true,
            platform_claims: true,
            entity_id_claims: true,
            contributor_claims: true,
            link_claims: true,
            release_claims: false,
        }
    }
}

impl SectionState {
    fn is_open(self, kind: SectionKind) -> bool {
        match kind {
            SectionKind::Routes => self.routes,
            SectionKind::PlatformClaims => self.platform_claims,
            SectionKind::EntityIdClaims => self.entity_id_claims,
            SectionKind::ContributorClaims => self.contributor_claims,
            SectionKind::LinkClaims => self.link_claims,
            SectionKind::ReleaseClaims => self.release_claims,
        }
    }

    fn toggle(&mut self, kind: SectionKind) {
        match kind {
            SectionKind::Routes => self.routes = !self.routes,
            SectionKind::PlatformClaims => self.platform_claims = !self.platform_claims,
            SectionKind::EntityIdClaims => self.entity_id_claims = !self.entity_id_claims,
            SectionKind::ContributorClaims => self.contributor_claims = !self.contributor_claims,
            SectionKind::LinkClaims => self.link_claims = !self.link_claims,
            SectionKind::ReleaseClaims => self.release_claims = !self.release_claims,
        }
    }
}

#[derive(Debug, Clone)]
struct ReviewGroup {
    label: String,
    source: String,
    evidence_key: String,
    reviews: Vec<stophammer::db::WalletReviewSummary>,
}

#[derive(Debug)]
struct App {
    conn: Connection,
    limit: usize,
    groups: Vec<ReviewGroup>,
    group_wallets: Vec<stophammer::db::WalletAliasPeer>,
    group_state: ListState,
    selected_group: usize,
    source_state: ListState,
    selected_source: usize,
    candidate_state: ListState,
    selected_candidate: usize,
    feed_state: ListState,
    selected_feed: usize,
    evidence_state: ListState,
    selected_evidence: usize,
    source_wallet_detail: Option<stophammer::db::WalletDetail>,
    candidate_wallet_detail: Option<stophammer::db::WalletDetail>,
    candidate_wallets: Vec<stophammer::db::WalletAliasPeer>,
    claim_feeds: Vec<stophammer::db::WalletClaimFeed>,
    candidate_claim_feeds: Vec<stophammer::db::WalletClaimFeed>,
    evidence_rows: Vec<EvidenceRow>,
    sections: SectionState,
    collapsed_item_keys: BTreeSet<String>,
    dialog: Option<SummaryDialog>,
    focus: Focus,
    status: String,
}

#[derive(Debug, Clone)]
struct ReloadSelection {
    group_key: Option<(String, String, String)>,
    main_wallet_id: Option<String>,
    merge_wallet_id: Option<String>,
    feed_guid: Option<String>,
    selected_evidence: usize,
    sections: SectionState,
    collapsed_item_keys: BTreeSet<String>,
    focus: Focus,
}

const CLASS_CONFIDENCES: &[&str] = &["provisional", "reviewed", "high_confidence"];

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
                    .map_err(|_| format!("invalid --limit value: {value}"))?;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: review_wallet_identity_tui [--db PATH] [--limit N]\n\n\
                     Keys:\n\
                     q            Quit\n\
                     Tab          Cycle focus: groups -> source -> feeds -> evidence\n\
                    Left/Right   Move focus\n\
                     [/]          Previous/next wallet to merge\n\
                     Up/Down      Move selection in focused pane\n\
                     Enter/Space  Expand/collapse selected evidence tree item\n\
                     a            Apply reviewed merges now\n\
                     m            Merge selected wallet into the main wallet\n\
                     u            Undo last applied merge batch\n\
                     x            Mark selected wallet as different from the main wallet\n\
                     c            Cycle main wallet class (also sets confidence to reviewed)\n\
                     v            Cycle main wallet confidence\n\
                     z            Revert main wallet operator classification edits\n\
                     r            Reload reviews and details\n\
                     Home/End     Jump to top/bottom in focused pane"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args { db_path, limit })
}

fn group_reviews(reviews: Vec<stophammer::db::WalletReviewSummary>) -> Vec<ReviewGroup> {
    let mut groups: Vec<ReviewGroup> = Vec::new();
    for review in reviews {
        if let Some(group) = groups.iter_mut().find(|group| {
            group.source == review.source && group.evidence_key == review.evidence_key
        }) {
            group.reviews.push(review);
        } else {
            let label = review.evidence_key.clone();
            groups.push(ReviewGroup {
                label,
                source: review.source.clone(),
                evidence_key: review.evidence_key.clone(),
                reviews: vec![review],
            });
        }
    }
    groups
}

impl App {
    fn new(db_path: &PathBuf, limit: usize) -> Result<Self, Box<dyn Error>> {
        let conn = stophammer::db::open_db(db_path);
        let mut app = Self {
            conn,
            limit,
            groups: Vec::new(),
            group_wallets: Vec::new(),
            group_state: ListState::default(),
            selected_group: 0,
            source_state: ListState::default(),
            selected_source: 0,
            candidate_state: ListState::default(),
            selected_candidate: 0,
            feed_state: ListState::default(),
            selected_feed: 0,
            evidence_state: ListState::default(),
            selected_evidence: 0,
            source_wallet_detail: None,
            candidate_wallet_detail: None,
            candidate_wallets: Vec::new(),
            claim_feeds: Vec::new(),
            candidate_claim_feeds: Vec::new(),
            evidence_rows: Vec::new(),
            sections: SectionState::default(),
            collapsed_item_keys: BTreeSet::new(),
            dialog: None,
            focus: Focus::Groups,
            status: String::from("Loading review groups"),
        };
        app.reload()?;
        Ok(app)
    }

    fn current_group(&self) -> Option<&ReviewGroup> {
        self.groups.get(self.selected_group)
    }

    fn current_source_review(&self) -> Option<&stophammer::db::WalletReviewSummary> {
        self.current_group()
            .and_then(|group| group.reviews.get(self.selected_source))
    }

    fn current_candidate(&self) -> Option<&stophammer::db::WalletAliasPeer> {
        self.candidate_wallets.get(self.selected_candidate)
    }

    fn current_feed(&self) -> Option<&stophammer::db::WalletClaimFeed> {
        self.claim_feeds.get(self.selected_feed)
    }

    fn current_candidate_review(&self) -> Option<&stophammer::db::WalletReviewSummary> {
        let candidate = self.current_candidate()?;
        self.current_group()?
            .reviews
            .iter()
            .find(|review| review.wallet_id == candidate.wallet_id)
    }

    fn capture_reload_selection(&self) -> ReloadSelection {
        ReloadSelection {
            group_key: self.current_group().map(|group| {
                (
                    group.source.clone(),
                    group.evidence_key.clone(),
                    group.label.clone(),
                )
            }),
            main_wallet_id: self
                .current_source_review()
                .map(|review| review.wallet_id.clone()),
            merge_wallet_id: self
                .current_candidate()
                .map(|wallet| wallet.wallet_id.clone()),
            feed_guid: self.current_feed().map(|feed| feed.feed_guid.clone()),
            selected_evidence: self.selected_evidence,
            sections: self.sections,
            collapsed_item_keys: self.collapsed_item_keys.clone(),
            focus: self.focus,
        }
    }

    fn pair_already_reviewed(
        &self,
        source_wallet_id: &str,
        candidate_wallet_id: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let reviewed = self
            .conn
            .query_row(
                "SELECT 1 \
                 FROM wallet_identity_override \
                 WHERE override_type = 'merge' \
                   AND ((wallet_id = ?1 AND target_id = ?2) \
                     OR (wallet_id = ?2 AND target_id = ?1)) \
                 LIMIT 1",
                params![source_wallet_id, candidate_wallet_id],
                |_row| Ok(()),
            )
            .optional()?
            .is_some();
        Ok(reviewed)
    }

    fn load_candidate_wallets_for_review(
        &self,
        source_review: &stophammer::db::WalletReviewSummary,
        alias: Option<&str>,
        allowed_wallet_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<stophammer::db::WalletAliasPeer>, Box<dyn Error>> {
        let Some(alias) = alias else {
            return Ok(Vec::new());
        };

        let mut candidates = Vec::new();
        for peer in stophammer::db::get_wallet_alias_peers(&self.conn, alias)? {
            if peer.wallet_id == source_review.wallet_id {
                continue;
            }
            if let Some(allowed_wallet_ids) = allowed_wallet_ids
                && !allowed_wallet_ids.contains(&peer.wallet_id)
            {
                continue;
            }
            if self.pair_already_reviewed(&source_review.wallet_id, &peer.wallet_id)? {
                continue;
            }
            candidates.push(peer);
        }
        Ok(candidates)
    }

    fn prune_review_groups(
        &self,
        groups: Vec<ReviewGroup>,
    ) -> Result<Vec<ReviewGroup>, Box<dyn Error>> {
        let mut pruned = Vec::new();
        for mut group in groups {
            if group.source == "cross_wallet_alias" {
                let alias = Some(group.evidence_key.as_str());
                let pending_peer_ids = group
                    .reviews
                    .iter()
                    .map(|review| review.wallet_id.clone())
                    .collect::<BTreeSet<_>>();
                let mut reviews = Vec::new();
                for review in group.reviews {
                    if !self
                        .load_candidate_wallets_for_review(&review, alias, Some(&pending_peer_ids))?
                        .is_empty()
                    {
                        reviews.push(review);
                    }
                }
                group.reviews = reviews;
            }

            if !group.reviews.is_empty() {
                pruned.push(group);
            }
        }
        Ok(pruned)
    }

    fn reload(&mut self) -> Result<(), Box<dyn Error>> {
        let selection = self.capture_reload_selection();
        let reviews = stophammer::db::list_pending_wallet_reviews(&self.conn, self.limit)?;
        self.groups = self.prune_review_groups(group_reviews(reviews))?;
        self.selected_group = selection
            .group_key
            .clone()
            .and_then(|(source, evidence_key, label)| {
                self.groups.iter().position(|group| {
                    group.source == source
                        && group.evidence_key == evidence_key
                        && group.label == label
                })
            })
            .unwrap_or(0);

        if self.groups.is_empty() {
            self.group_state.select(None);
            self.source_state.select(None);
            self.candidate_state.select(None);
            self.feed_state.select(None);
            self.evidence_state.select(None);
            self.source_wallet_detail = None;
            self.candidate_wallet_detail = None;
            self.group_wallets.clear();
            self.candidate_wallets.clear();
            self.claim_feeds.clear();
            self.candidate_claim_feeds.clear();
            self.evidence_rows.clear();
            self.status = "No pending review groups".to_string();
            return Ok(());
        }

        self.group_state.select(Some(self.selected_group));
        self.populate_group_wallets()?;
        self.selected_source = selection
            .main_wallet_id
            .as_deref()
            .and_then(|wallet_id| {
                self.current_group()?
                    .reviews
                    .iter()
                    .position(|review| review.wallet_id == wallet_id)
            })
            .unwrap_or(0);
        self.source_state = ListState::default();
        self.source_state.select(Some(self.selected_source));
        self.load_selected_source()?;

        if let Some(wallet_id) = selection.merge_wallet_id.as_deref()
            && let Some(index) = self
                .candidate_wallets
                .iter()
                .position(|candidate| candidate.wallet_id == wallet_id)
        {
            self.selected_candidate = index;
            self.candidate_state.select(Some(index));
            self.load_selected_candidate()?;
        }

        if let Some(feed_guid) = selection.feed_guid.as_deref()
            && let Some(index) = self
                .claim_feeds
                .iter()
                .position(|feed| feed.feed_guid == feed_guid)
        {
            self.selected_feed = index;
            self.feed_state.select(Some(index));
        }

        self.sections = selection.sections;
        self.collapsed_item_keys = selection.collapsed_item_keys;
        self.rebuild_evidence_rows();
        if !self.evidence_rows.is_empty() {
            self.selected_evidence = selection
                .selected_evidence
                .min(self.evidence_rows.len().saturating_sub(1));
            self.evidence_state.select(Some(self.selected_evidence));
        }
        self.focus = selection.focus;
        self.status = format!("Loaded {} review groups", self.groups.len());
        Ok(())
    }

    fn populate_group_wallets(&mut self) -> Result<(), Box<dyn Error>> {
        self.group_wallets = if let Some(alias) = self
            .current_group()
            .map(|group| group.evidence_key.as_str())
        {
            let peers = stophammer::db::get_wallet_alias_peers(&self.conn, alias)?;
            let by_wallet_id = peers
                .into_iter()
                .map(|peer| (peer.wallet_id.clone(), peer))
                .collect::<std::collections::BTreeMap<_, _>>();
            self.current_group()
                .map(|group| {
                    group
                        .reviews
                        .iter()
                        .filter_map(|review| by_wallet_id.get(&review.wallet_id).cloned())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(())
    }

    fn load_selected_group(&mut self) -> Result<(), Box<dyn Error>> {
        self.populate_group_wallets()?;
        self.selected_source = 0;
        self.source_state = ListState::default();
        if let Some(group) = self.current_group() {
            if group.reviews.is_empty() {
                self.source_state.select(None);
            } else {
                self.source_state.select(Some(0));
            }
        }
        self.load_selected_source()
    }

    fn load_selected_source(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(source_review) = self.current_source_review().cloned() else {
            self.source_wallet_detail = None;
            self.candidate_wallet_detail = None;
            self.candidate_wallets.clear();
            self.claim_feeds.clear();
            self.candidate_claim_feeds.clear();
            self.evidence_rows.clear();
            return Ok(());
        };

        self.source_wallet_detail =
            stophammer::db::get_wallet_detail(&self.conn, &source_review.wallet_id)?;
        self.claim_feeds =
            stophammer::db::get_wallet_claim_feeds(&self.conn, &source_review.wallet_id)?;

        let pending_peer_ids = self
            .current_group()
            .map(|group| {
                group
                    .reviews
                    .iter()
                    .map(|review| review.wallet_id.clone())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();

        self.candidate_wallets = self.load_candidate_wallets_for_review(
            &source_review,
            self.current_group()
                .map(|group| group.evidence_key.as_str()),
            Some(&pending_peer_ids),
        )?;

        self.selected_candidate = 0;
        self.candidate_state = ListState::default();
        if self.candidate_wallets.is_empty() {
            self.candidate_state.select(None);
            self.candidate_wallet_detail = None;
            self.candidate_claim_feeds.clear();
        } else {
            self.candidate_state.select(Some(0));
            self.load_selected_candidate()?;
        }

        self.selected_feed = 0;
        self.feed_state = ListState::default();
        if self.claim_feeds.is_empty() {
            self.feed_state.select(None);
        } else {
            self.feed_state.select(Some(0));
        }

        self.sections = SectionState::default();
        self.collapsed_item_keys.clear();
        self.rebuild_evidence_rows();
        Ok(())
    }

    fn load_selected_candidate(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(candidate_wallet_id) = self
            .current_candidate()
            .map(|candidate| candidate.wallet_id.clone())
        {
            self.candidate_wallet_detail =
                stophammer::db::get_wallet_detail(&self.conn, &candidate_wallet_id)?;
            self.candidate_claim_feeds =
                stophammer::db::get_wallet_claim_feeds(&self.conn, &candidate_wallet_id)?;
        } else {
            self.candidate_wallet_detail = None;
            self.candidate_claim_feeds.clear();
        }
        Ok(())
    }

    fn rebuild_evidence_rows(&mut self) {
        self.evidence_rows = if let Some(feed) = self.current_feed() {
            build_evidence_rows(feed, self.sections, &self.collapsed_item_keys)
        } else {
            vec![EvidenceRow {
                node: EvidenceNode::ItemDetail,
                lines: vec![Line::from("No claim feed selected.")],
            }]
        };
        if self.evidence_rows.is_empty() {
            self.selected_evidence = 0;
            self.evidence_state.select(None);
        } else {
            self.selected_evidence = self.selected_evidence.min(self.evidence_rows.len() - 1);
            self.evidence_state.select(Some(self.selected_evidence));
        }
    }

    fn select_evidence_section(&mut self, kind: SectionKind) {
        if let Some(index) = self
            .evidence_rows
            .iter()
            .position(|row| matches!(row.node, EvidenceNode::Section(row_kind) if row_kind == kind))
        {
            self.selected_evidence = index;
            self.evidence_state.select(Some(index));
        }
    }

    fn select_evidence_item(&mut self, key: &str) {
        if let Some(index) = self.evidence_rows.iter().position(
            |row| matches!(&row.node, EvidenceNode::ItemHeader(row_key) if row_key == key),
        ) {
            self.selected_evidence = index;
            self.evidence_state.select(Some(index));
        }
    }

    fn next_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Groups => Focus::SourceWallets,
            Focus::SourceWallets => Focus::Feeds,
            Focus::Candidates => Focus::Feeds,
            Focus::Feeds => Focus::Evidence,
            Focus::Evidence => Focus::Groups,
        };
    }

    fn previous_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Groups => Focus::Evidence,
            Focus::SourceWallets => Focus::Groups,
            Focus::Candidates => Focus::SourceWallets,
            Focus::Feeds => Focus::SourceWallets,
            Focus::Evidence => Focus::Feeds,
        };
    }

    fn previous_candidate(&mut self) -> Result<(), Box<dyn Error>> {
        if self.selected_candidate > 0 {
            self.selected_candidate -= 1;
            self.candidate_state.select(Some(self.selected_candidate));
            self.load_selected_candidate()?;
        }
        Ok(())
    }

    fn next_candidate(&mut self) -> Result<(), Box<dyn Error>> {
        if self.selected_candidate + 1 < self.candidate_wallets.len() {
            self.selected_candidate += 1;
            self.candidate_state.select(Some(self.selected_candidate));
            self.load_selected_candidate()?;
        }
        Ok(())
    }

    fn move_down(&mut self) -> Result<(), Box<dyn Error>> {
        match self.focus {
            Focus::Groups => {
                if self.selected_group + 1 < self.groups.len() {
                    self.selected_group += 1;
                    self.group_state.select(Some(self.selected_group));
                    self.load_selected_group()?;
                }
            }
            Focus::SourceWallets => {
                if let Some(group) = self.current_group()
                    && self.selected_source + 1 < group.reviews.len()
                {
                    self.selected_source += 1;
                    self.source_state.select(Some(self.selected_source));
                    self.load_selected_source()?;
                }
            }
            Focus::Candidates => {
                if self.selected_candidate + 1 < self.candidate_wallets.len() {
                    self.selected_candidate += 1;
                    self.candidate_state.select(Some(self.selected_candidate));
                    self.load_selected_candidate()?;
                }
            }
            Focus::Feeds => {
                if self.selected_feed + 1 < self.claim_feeds.len() {
                    self.selected_feed += 1;
                    self.feed_state.select(Some(self.selected_feed));
                    self.selected_evidence = 0;
                    self.rebuild_evidence_rows();
                }
            }
            Focus::Evidence => {
                if self.selected_evidence + 1 < self.evidence_rows.len() {
                    self.selected_evidence += 1;
                    self.evidence_state.select(Some(self.selected_evidence));
                }
            }
        }
        Ok(())
    }

    fn move_up(&mut self) -> Result<(), Box<dyn Error>> {
        match self.focus {
            Focus::Groups => {
                if self.selected_group > 0 {
                    self.selected_group -= 1;
                    self.group_state.select(Some(self.selected_group));
                    self.load_selected_group()?;
                }
            }
            Focus::SourceWallets => {
                if self.selected_source > 0 {
                    self.selected_source -= 1;
                    self.source_state.select(Some(self.selected_source));
                    self.load_selected_source()?;
                }
            }
            Focus::Candidates => {
                if self.selected_candidate > 0 {
                    self.selected_candidate -= 1;
                    self.candidate_state.select(Some(self.selected_candidate));
                    self.load_selected_candidate()?;
                }
            }
            Focus::Feeds => {
                if self.selected_feed > 0 {
                    self.selected_feed -= 1;
                    self.feed_state.select(Some(self.selected_feed));
                    self.selected_evidence = 0;
                    self.rebuild_evidence_rows();
                }
            }
            Focus::Evidence => {
                if self.selected_evidence > 0 {
                    self.selected_evidence -= 1;
                    self.evidence_state.select(Some(self.selected_evidence));
                }
            }
        }
        Ok(())
    }

    fn jump_top(&mut self) -> Result<(), Box<dyn Error>> {
        match self.focus {
            Focus::Groups => {
                self.selected_group = 0;
                self.group_state.select(Some(0));
                self.load_selected_group()?;
            }
            Focus::SourceWallets => {
                self.selected_source = 0;
                if self.current_group().is_some() {
                    self.source_state.select(Some(0));
                    self.load_selected_source()?;
                }
            }
            Focus::Candidates => {
                self.selected_candidate = 0;
                if !self.candidate_wallets.is_empty() {
                    self.candidate_state.select(Some(0));
                    self.load_selected_candidate()?;
                }
            }
            Focus::Feeds => {
                self.selected_feed = 0;
                if !self.claim_feeds.is_empty() {
                    self.feed_state.select(Some(0));
                    self.selected_evidence = 0;
                    self.rebuild_evidence_rows();
                }
            }
            Focus::Evidence => {
                self.selected_evidence = 0;
                if !self.evidence_rows.is_empty() {
                    self.evidence_state.select(Some(0));
                }
            }
        }
        Ok(())
    }

    fn jump_bottom(&mut self) -> Result<(), Box<dyn Error>> {
        match self.focus {
            Focus::Groups => {
                if !self.groups.is_empty() {
                    self.selected_group = self.groups.len() - 1;
                    self.group_state.select(Some(self.selected_group));
                    self.load_selected_group()?;
                }
            }
            Focus::SourceWallets => {
                if let Some(group) = self.current_group()
                    && !group.reviews.is_empty()
                {
                    self.selected_source = group.reviews.len() - 1;
                    self.source_state.select(Some(self.selected_source));
                    self.load_selected_source()?;
                }
            }
            Focus::Candidates => {
                if !self.candidate_wallets.is_empty() {
                    self.selected_candidate = self.candidate_wallets.len() - 1;
                    self.candidate_state.select(Some(self.selected_candidate));
                    self.load_selected_candidate()?;
                }
            }
            Focus::Feeds => {
                if !self.claim_feeds.is_empty() {
                    self.selected_feed = self.claim_feeds.len() - 1;
                    self.feed_state.select(Some(self.selected_feed));
                    self.selected_evidence = 0;
                    self.rebuild_evidence_rows();
                }
            }
            Focus::Evidence => {
                if !self.evidence_rows.is_empty() {
                    self.selected_evidence = self.evidence_rows.len() - 1;
                    self.evidence_state.select(Some(self.selected_evidence));
                }
            }
        }
        Ok(())
    }

    fn toggle_selected_section(&mut self) {
        if let Some(node) = self
            .evidence_rows
            .get(self.selected_evidence)
            .map(|row| row.node.clone())
        {
            match node {
                EvidenceNode::Section(kind) => {
                    self.sections.toggle(kind);
                    self.rebuild_evidence_rows();
                    self.select_evidence_section(kind);
                }
                EvidenceNode::ItemHeader(key) => {
                    if !self.collapsed_item_keys.insert(key.clone()) {
                        self.collapsed_item_keys.remove(&key);
                    }
                    self.rebuild_evidence_rows();
                    self.select_evidence_item(&key);
                }
                EvidenceNode::ItemDetail => {}
            }
        }
    }

    fn approve_merge(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(main_review) = self.current_source_review().cloned() else {
            self.status = "No main wallet selected".to_string();
            return Ok(());
        };
        let Some(merge_review) = self.current_candidate_review().cloned() else {
            self.status = "No wallet selected to merge into the main wallet".to_string();
            return Ok(());
        };

        stophammer::db::apply_wallet_identity_review_action(
            &self.conn,
            merge_review.id,
            "merge",
            Some(&main_review.wallet_id),
            None,
        )?;
        let status = format!(
            "Recorded: merge {} into {}",
            short_id(&merge_review.wallet_id),
            short_id(&main_review.wallet_id)
        );
        self.reload()?;
        self.status = status;
        Ok(())
    }

    fn reject_review(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(merge_review) = self.current_candidate_review().cloned() else {
            self.status = "No wallet selected to compare against the main wallet".to_string();
            return Ok(());
        };

        stophammer::db::apply_wallet_identity_review_action(
            &self.conn,
            merge_review.id,
            "do_not_merge",
            None,
            None,
        )?;
        let status = format!(
            "Recorded: do not merge {} into any main wallet in this group",
            short_id(&merge_review.wallet_id)
        );
        self.reload()?;
        self.status = status;
        Ok(())
    }

    fn cycle_main_wallet_class(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(main_review) = self.current_source_review().cloned() else {
            self.status = "No main wallet selected".to_string();
            return Ok(());
        };
        let current_class = self
            .source_wallet_detail
            .as_ref()
            .map(|wallet| wallet.wallet_class.as_str())
            .unwrap_or("unknown");
        let current_index = WALLET_CLASS_VALUES
            .iter()
            .position(|wallet_class| *wallet_class == current_class)
            .unwrap_or(0);
        let wallet_class = WALLET_CLASS_VALUES[(current_index + 1) % WALLET_CLASS_VALUES.len()];

        stophammer::db::set_wallet_force_class(&self.conn, &main_review.wallet_id, wallet_class)?;
        let status = format!(
            "Recorded: set main wallet {} class to {} (confidence reviewed)",
            short_id(&main_review.wallet_id),
            wallet_class
        );
        self.reload()?;
        self.status = status;
        Ok(())
    }

    fn cycle_main_wallet_confidence(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(main_review) = self.current_source_review().cloned() else {
            self.status = "No main wallet selected".to_string();
            return Ok(());
        };
        let current_confidence = self
            .source_wallet_detail
            .as_ref()
            .map(|wallet| wallet.class_confidence.as_str())
            .unwrap_or("provisional");
        let current_index = CLASS_CONFIDENCES
            .iter()
            .position(|confidence| *confidence == current_confidence)
            .unwrap_or(0);
        let class_confidence = CLASS_CONFIDENCES[(current_index + 1) % CLASS_CONFIDENCES.len()];

        stophammer::db::set_wallet_force_confidence(
            &self.conn,
            &main_review.wallet_id,
            class_confidence,
        )?;
        let status = format!(
            "Recorded: set main wallet {} confidence to {}",
            short_id(&main_review.wallet_id),
            class_confidence
        );
        self.reload()?;
        self.status = status;
        Ok(())
    }

    fn revert_main_wallet_edits(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(main_review) = self.current_source_review().cloned() else {
            self.status = "No main wallet selected".to_string();
            return Ok(());
        };

        stophammer::db::revert_wallet_operator_classification(&self.conn, &main_review.wallet_id)?;
        let status = format!(
            "Reverted operator classification edits for main wallet {}",
            short_id(&main_review.wallet_id)
        );
        self.reload()?;
        self.status = status;
        Ok(())
    }

    fn apply_reviewed_merges(&mut self) -> Result<(), Box<dyn Error>> {
        let stats = stophammer::db::backfill_wallet_pass5(&self.conn)?;
        let mut lines = vec![
            format!("operator merges applied: {}", stats.merges_from_overrides),
            format!("heuristic merges applied: {}", stats.merges_from_grouping),
            format!("soft classified: {}", stats.soft_classified),
            format!("split classified: {}", stats.split_classified),
            format!("review items created: {}", stats.review_items_created),
            format!("orphans deleted: {}", stats.orphans_deleted),
        ];
        if let Some(batch_id) = stats.apply_batch_id {
            lines.insert(0, format!("apply batch id: {}", batch_id));
        }
        let status = format!(
            "Applied merges: {} operator, {} heuristic",
            stats.merges_from_overrides, stats.merges_from_grouping
        );
        self.reload()?;
        self.status = status;
        self.dialog = Some(SummaryDialog {
            title: "Apply Summary".to_string(),
            lines,
        });
        Ok(())
    }

    fn undo_last_apply_batch(&mut self) -> Result<(), Box<dyn Error>> {
        let result = stophammer::db::undo_last_wallet_merge_batch(&self.conn)?;
        let Some(stats) = result else {
            self.status = "No applied merge batch to undo".to_string();
            self.dialog = Some(SummaryDialog {
                title: "Undo Summary".to_string(),
                lines: vec!["No applied merge batch to undo.".to_string()],
            });
            return Ok(());
        };

        let status = format!(
            "Undid merge batch {} ({} merges reverted)",
            stats.batch_id, stats.merges_reverted
        );
        self.reload()?;
        self.status = status;
        self.dialog = Some(SummaryDialog {
            title: "Undo Summary".to_string(),
            lines: vec![
                format!("batch id: {}", stats.batch_id),
                format!("merges reverted: {}", stats.merges_reverted),
                "merge materialization was rolled back".to_string(),
            ],
        });
        Ok(())
    }
}

fn section_title(kind: SectionKind) -> &'static str {
    match kind {
        SectionKind::Routes => "Routes",
        SectionKind::PlatformClaims => "Platform Claims",
        SectionKind::EntityIdClaims => "Entity ID Claims",
        SectionKind::ContributorClaims => "Contributor Claims",
        SectionKind::LinkClaims => "Link Claims",
        SectionKind::ReleaseClaims => "Release Claims",
    }
}

fn abbreviate(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    let take = max_chars.saturating_sub(1);
    let truncated: String = value.chars().take(take).collect();
    format!("{truncated}...")
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

fn known_platform_for_endpoint(route_type: &str, normalized_address: &str) -> Option<&'static str> {
    if route_type == "lnaddress" {
        if let Some((_, domain)) = normalized_address.rsplit_once('@') {
            return match domain {
                "getalby.com" => Some("alby"),
                "fountain.fm" => Some("fountain"),
                "wavlake.com" => Some("wavlake"),
                "breez.technology" => Some("breez"),
                _ => None,
            };
        }
    }

    if matches!(route_type, "node" | "keysend") {
        return match normalized_address {
            "03b6f613e88bd874177c28c6ad83b3baba43c4c656f56be1f8df84669556054b79" => {
                Some("fountain")
            }
            "030a58b8653d32b99200a2334cfe913e51dc7d155aa0116c176657a4f1722677a3" => Some("alby"),
            _ => None,
        };
    }

    None
}

fn endpoint_host_badge(route_type: &str, normalized_address: &str) -> String {
    known_platform_for_endpoint(route_type, normalized_address)
        .map(|label| format!(" ({label})"))
        .unwrap_or_default()
}

fn peer_primary_endpoint_line(
    endpoint_preview: &[stophammer::db::WalletEndpointPreview],
) -> String {
    endpoint_preview.first().map_or_else(
        || "(self) - - -".to_string(),
        |endpoint| {
            if endpoint.route_type == "lnaddress" {
                format!("(lnurl) {}", endpoint.normalized_address)
            } else {
                let custom_key = if endpoint.custom_key.is_empty() {
                    "-"
                } else {
                    endpoint.custom_key.as_str()
                };
                let custom_value = if endpoint.custom_value.is_empty() {
                    "-"
                } else {
                    endpoint.custom_value.as_str()
                };
                format!(
                    "({}) {} {} {}",
                    endpoint_owner_label(&endpoint.route_type, &endpoint.normalized_address),
                    chooser_wallet_address(&endpoint.route_type, &endpoint.normalized_address),
                    custom_key,
                    custom_value
                )
            }
        },
    )
}

fn artist_link_preview(wallet: &stophammer::db::WalletDetail) -> String {
    if wallet.artist_links.is_empty() {
        return "-".to_string();
    }

    wallet
        .artist_links
        .iter()
        .take(3)
        .map(|link| format!("{}:{}", short_id(&link.artist_id), link.confidence))
        .collect::<Vec<_>>()
        .join(", ")
}

fn route_types_from_detail(wallet: &stophammer::db::WalletDetail) -> Vec<String> {
    wallet
        .endpoints
        .iter()
        .map(|endpoint| visible_route_type(&endpoint.route_type).to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn route_type_summary(values: &[String]) -> String {
    preview_join(values, 3, 14)
}

fn recipient_name_preview_from_claims(claim_feeds: &[stophammer::db::WalletClaimFeed]) -> String {
    let mut names = BTreeSet::new();
    for feed in claim_feeds {
        for route in &feed.routes {
            if let Some(name) = route.recipient_name.as_deref() {
                let trimmed = name.trim();
                if !trimmed.is_empty() {
                    names.insert(trimmed.to_string());
                }
            }
        }
    }
    preview_join(&names.into_iter().collect::<Vec<_>>(), 4, 22)
}

fn shared_preview(left: &[String], right: &[String], max_items: usize, max_chars: usize) -> String {
    let left_set = left.iter().cloned().collect::<BTreeSet<_>>();
    let shared = right
        .iter()
        .filter(|value| left_set.contains(*value))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    preview_join(&shared, max_items, max_chars)
}

fn block_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn block_border_type(active: bool) -> BorderType {
    if active {
        BorderType::Thick
    } else {
        BorderType::Plain
    }
}

fn styled_title(text: &str, color: Color) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

fn section_line(text: &str, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled("■ ", Style::default().fg(color)),
        Span::styled(
            text.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn meta_line(label: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(Color::DarkGray)),
        Span::styled(value.into(), Style::default().fg(Color::White)),
    ])
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

fn wrap_text_lines(
    text: &str,
    width: usize,
    initial_indent: &str,
    continuation_indent: &str,
) -> Vec<Line<'static>> {
    fn split_token_for_wrap(token: &str, max_width: usize) -> Vec<String> {
        if token.chars().count() <= max_width {
            return vec![token.to_string()];
        }

        let mut parts = Vec::new();
        let mut remaining = token;
        while remaining.chars().count() > max_width {
            let mut split_at = 0usize;
            let candidate: String = remaining.chars().take(max_width).collect();
            for (idx, ch) in candidate.char_indices() {
                if matches!(ch, '/' | '?' | '&' | '=' | ':' | ',' | ';' | '-' | '_') {
                    split_at = idx + ch.len_utf8();
                }
            }
            if split_at == 0 {
                split_at = candidate.len();
            }
            parts.push(remaining[..split_at].to_string());
            remaining = &remaining[split_at..];
        }
        if !remaining.is_empty() {
            parts.push(remaining.to_string());
        }
        parts
    }

    let words = text
        .split_whitespace()
        .flat_map(|word| {
            split_token_for_wrap(word, width.saturating_sub(continuation_indent.len()))
        })
        .collect::<Vec<_>>();
    if words.is_empty() {
        return vec![Line::from(initial_indent.to_string())];
    }

    let mut lines = Vec::new();
    let mut current = String::from(initial_indent);
    let mut current_width = initial_indent.chars().count();
    let continuation_width = continuation_indent.chars().count();

    for word in words {
        let word_width = word.chars().count();
        let needed = if current_width > initial_indent.chars().count() {
            1 + word_width
        } else {
            word_width
        };

        if current_width + needed > width && current_width > initial_indent.chars().count() {
            lines.push(Line::from(current));
            current = String::from(continuation_indent);
            current_width = continuation_width;
        }

        if current_width > initial_indent.chars().count() && current_width > continuation_width {
            current.push(' ');
            current_width += 1;
        }
        current.push_str(&word);
        current_width += word_width;
    }

    lines.push(Line::from(current));
    lines
}

fn display_wallet_address(route_type: &str, normalized_address: &str) -> String {
    if matches!(route_type, "node" | "keysend") {
        abbreviate(normalized_address, 20)
    } else {
        normalized_address.to_string()
    }
}

fn visible_route_type(route_type: &str) -> &str {
    if route_type == "lnaddress" {
        "lnurl"
    } else {
        route_type
    }
}

fn chooser_wallet_address(route_type: &str, normalized_address: &str) -> String {
    if route_type == "lnaddress" {
        normalized_address.to_string()
    } else {
        abbreviate(normalized_address, 20)
    }
}

fn endpoint_owner_label(route_type: &str, normalized_address: &str) -> &'static str {
    known_platform_for_endpoint(route_type, normalized_address).unwrap_or("self")
}

fn wallet_class_style(wallet_class: &str) -> Style {
    let color = match wallet_class {
        "person_artist" => Color::Green,
        "organization_platform" => Color::Cyan,
        "bot_service" => Color::Yellow,
        _ => Color::Gray,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn confidence_style(class_confidence: &str) -> Style {
    let color = match class_confidence {
        "high_confidence" => Color::Green,
        "reviewed" => Color::LightBlue,
        _ => Color::Yellow,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn push_section(
    rows: &mut Vec<EvidenceRow>,
    sections: SectionState,
    kind: SectionKind,
    title: String,
    count: usize,
    collapsed_item_keys: &BTreeSet<String>,
    items: impl IntoIterator<Item = EvidenceBranch>,
) {
    if count == 0 {
        return;
    }
    let marker = if sections.is_open(kind) { "[-]" } else { "[+]" };
    rows.push(EvidenceRow {
        node: EvidenceNode::Section(kind),
        lines: wrap_text_lines(&format!("{marker} {title} ({count})"), 44, "", ""),
    });
    if sections.is_open(kind) {
        for item in items {
            let is_open = collapsed_item_keys.contains(&item.key) == item.default_collapsed;
            let item_marker = if is_open { "  [-] " } else { "  [+] " };
            rows.push(EvidenceRow {
                node: EvidenceNode::ItemHeader(item.key.clone()),
                lines: wrap_text_lines(&item.header, 44, item_marker, "      "),
            });
            if is_open {
                let total = item.children.len();
                for (index, child) in item.children.iter().enumerate() {
                    let is_last = index + 1 == total;
                    rows.push(EvidenceRow {
                        node: EvidenceNode::ItemDetail,
                        lines: wrap_text_lines(
                            child,
                            44,
                            if is_last { "      `- " } else { "      |- " },
                            if is_last { "         " } else { "      |  " },
                        ),
                    });
                }
            }
        }
    }
}

fn evidence_branch(
    key: impl Into<String>,
    header: impl Into<String>,
    children: Vec<String>,
) -> EvidenceBranch {
    EvidenceBranch {
        key: key.into(),
        header: header.into(),
        children,
        default_collapsed: false,
    }
}

fn evidence_route_endpoint_label(route: &stophammer::db::WalletRouteEvidence) -> String {
    if route.route_type == "lnaddress" {
        format!("<{}>", route.address)
    } else {
        known_platform_for_endpoint(&route.route_type, &route.address)
            .unwrap_or("self")
            .to_string()
    }
}

fn evidence_route_header(route: &stophammer::db::WalletRouteEvidence) -> String {
    match (route.track_title.as_deref(), route.track_guid.as_deref()) {
        (Some(title), _) if route.route_scope == "track" && !title.trim().is_empty() => {
            title.trim().to_string()
        }
        (_, Some(track_guid)) if route.route_scope == "track" => {
            format!("[{}]", short_id(track_guid))
        }
        _ => "feed route".to_string(),
    }
}

fn release_claim_collapsed_by_default(claim_type: &str) -> bool {
    let normalized = claim_type.to_ascii_lowercase();
    normalized.contains("description") || normalized.contains("image")
}

fn build_evidence_rows(
    feed: &stophammer::db::WalletClaimFeed,
    sections: SectionState,
    collapsed_item_keys: &BTreeSet<String>,
) -> Vec<EvidenceRow> {
    let mut release_claims_by_type = BTreeMap::<String, Vec<String>>::new();
    for claim in &feed.release_claims {
        release_claims_by_type
            .entry(claim.claim_type.clone())
            .or_default()
            .push(claim.claim_value.clone());
    }

    let mut rows = Vec::new();
    let mut trunks = vec![
        (
            0usize,
            SectionKind::Routes,
            section_title(SectionKind::Routes).to_string(),
            feed.routes.len(),
            feed.routes
                .iter()
                .enumerate()
                .map(|(index, route)| {
                    evidence_branch(
                        format!("routes:{}:{index}:{}", feed.feed_guid, route.route_id),
                        evidence_route_header(route),
                        vec![
                            format!("{} {}", route.split, evidence_route_endpoint_label(route)),
                            format!("fee: {}", route.fee),
                        ],
                    )
                })
                .collect::<Vec<_>>(),
        ),
        (
            1,
            SectionKind::PlatformClaims,
            section_title(SectionKind::PlatformClaims).to_string(),
            feed.platform_claims.len(),
            feed.platform_claims
                .iter()
                .enumerate()
                .map(|(index, claim)| {
                    evidence_branch(
                        format!("platform:{}:{index}:{}", feed.feed_guid, claim.platform_key),
                        format!("platform {}", claim.platform_key),
                        vec![
                            format!("owner: {}", claim.owner_name.as_deref().unwrap_or("-")),
                            format!("url: {}", claim.url.as_deref().unwrap_or("-")),
                        ],
                    )
                })
                .collect::<Vec<_>>(),
        ),
        (
            2,
            SectionKind::EntityIdClaims,
            section_title(SectionKind::EntityIdClaims).to_string(),
            feed.entity_id_claims.len(),
            feed.entity_id_claims
                .iter()
                .enumerate()
                .map(|(index, claim)| {
                    evidence_branch(
                        format!(
                            "entity-id:{}:{index}:{}:{}",
                            feed.feed_guid, claim.entity_type, claim.entity_id
                        ),
                        format!("{} {}", claim.entity_type, claim.entity_id),
                        vec![
                            format!("scheme: {}", claim.scheme),
                            format!("value: {}", claim.value),
                        ],
                    )
                })
                .collect::<Vec<_>>(),
        ),
        (
            3,
            SectionKind::ContributorClaims,
            section_title(SectionKind::ContributorClaims).to_string(),
            feed.contributor_claims.len(),
            feed.contributor_claims
                .iter()
                .enumerate()
                .map(|(index, claim)| {
                    evidence_branch(
                        format!(
                            "contrib:{}:{index}:{}:{}",
                            feed.feed_guid, claim.entity_type, claim.entity_id
                        ),
                        format!("{} {}", claim.entity_type, claim.entity_id),
                        vec![
                            format!("name: {}", claim.name),
                            format!("role: {}", claim.role.as_deref().unwrap_or("-")),
                            format!("href: {}", claim.href.as_deref().unwrap_or("-")),
                        ],
                    )
                })
                .collect::<Vec<_>>(),
        ),
        (
            4,
            SectionKind::LinkClaims,
            section_title(SectionKind::LinkClaims).to_string(),
            feed.link_claims.len(),
            feed.link_claims
                .iter()
                .enumerate()
                .map(|(index, claim)| {
                    evidence_branch(
                        format!(
                            "link:{}:{index}:{}:{}",
                            feed.feed_guid, claim.entity_type, claim.entity_id
                        ),
                        format!("{} {}", claim.entity_type, claim.entity_id),
                        vec![
                            format!("link_type: {}", claim.link_type),
                            format!("url: {}", claim.url),
                        ],
                    )
                })
                .collect::<Vec<_>>(),
        ),
        (
            5,
            SectionKind::ReleaseClaims,
            format!("Release Claims {}", feed.feed_guid),
            feed.release_claims.len(),
            release_claims_by_type
                .into_iter()
                .map(|(claim_type, values)| EvidenceBranch {
                    key: format!("release:{}:{claim_type}", feed.feed_guid),
                    header: claim_type.clone(),
                    children: values,
                    default_collapsed: release_claim_collapsed_by_default(&claim_type),
                })
                .collect::<Vec<_>>(),
        ),
    ];
    trunks.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)));
    for (_, kind, title, count, items) in trunks {
        push_section(
            &mut rows,
            sections,
            kind,
            title,
            count,
            collapsed_item_keys,
            items,
        );
    }
    if rows.is_empty() {
        rows.push(EvidenceRow {
            node: EvidenceNode::ItemDetail,
            lines: vec![Line::from("No evidence for selected feed.")],
        });
    }
    rows
}

fn wallet_card_lines(
    wallet: Option<&stophammer::db::WalletDetail>,
    claim_feeds: &[stophammer::db::WalletClaimFeed],
) -> Vec<Line<'static>> {
    if let Some(wallet) = wallet {
        let aliases = wallet
            .aliases
            .iter()
            .map(|alias| alias.alias.clone())
            .collect::<Vec<_>>();
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    wallet.display_name.clone(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("[{}]", short_id(&wallet.wallet_id)),
                    Style::default().fg(Color::Magenta),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    wallet.wallet_class.clone(),
                    wallet_class_style(&wallet.wallet_class),
                ),
                Span::raw("  "),
                Span::styled(
                    wallet.class_confidence.clone(),
                    confidence_style(&wallet.class_confidence),
                ),
                Span::raw("  "),
                Span::styled(
                    route_type_summary(&route_types_from_detail(wallet)),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            Line::from(""),
            section_line("Endpoints", Color::LightBlue),
        ];
        if wallet.endpoints.is_empty() {
            lines.push(meta_line("Endpoints", "-"));
        } else {
            for endpoint in &wallet.endpoints {
                lines.push(Line::from(vec![
                    Span::styled("• ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        visible_route_type(&endpoint.route_type).to_string(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(endpoint_host_badge(
                        &endpoint.route_type,
                        &endpoint.normalized_address,
                    )),
                    Span::raw(" "),
                    Span::styled(
                        display_wallet_address(&endpoint.route_type, &endpoint.normalized_address),
                        Style::default().fg(Color::White),
                    ),
                ]));
                lines.push(meta_line(
                    "  custom",
                    format!(
                        "key={}  value={}",
                        if endpoint.custom_key.is_empty() {
                            "-"
                        } else {
                            endpoint.custom_key.as_str()
                        },
                        if endpoint.custom_value.is_empty() {
                            "-"
                        } else {
                            endpoint.custom_value.as_str()
                        }
                    ),
                ));
            }
        }
        lines.push(Line::from(""));
        lines.push(section_line("Identity", Color::LightBlue));
        lines.push(meta_line(
            "Recipient names",
            recipient_name_preview_from_claims(claim_feeds),
        ));
        lines.push(meta_line("Aliases", preview_join(&aliases, 4, 24)));
        lines.push(meta_line(
            "Artist links",
            format!(
                "{}  {}",
                wallet.artist_links.len(),
                artist_link_preview(wallet)
            ),
        ));
        lines.push(meta_line(
            "Created",
            format_local_timestamp(wallet.created_at),
        ));
        lines.push(meta_line(
            "Updated",
            format_local_timestamp(wallet.updated_at),
        ));
        lines
    } else {
        vec![Line::from(Span::styled(
            "Unavailable".to_string(),
            Style::default().fg(Color::DarkGray),
        ))]
    }
}

fn build_task_text(app: &App) -> String {
    let mut text = String::new();
    if let Some(group) = app.current_group() {
        let _ = writeln!(
            text,
            "Task: choose a main wallet, then decide whether another wallet should merge into it because the same artist, service, or recipient appears to control both."
        );
        let _ = writeln!(text, "Alias group: {}", group.label);
        let _ = writeln!(text, "Pending wallets in group: {}", group.reviews.len());

        if let (Some(main_wallet), Some(merge_wallet)) = (
            app.source_wallet_detail.as_ref(),
            app.candidate_wallet_detail.as_ref(),
        ) {
            let main_aliases = main_wallet
                .aliases
                .iter()
                .map(|alias| alias.alias.clone())
                .collect::<Vec<_>>();
            let merge_aliases = merge_wallet
                .aliases
                .iter()
                .map(|alias| alias.alias.clone())
                .collect::<Vec<_>>();
            let main_feeds = app
                .claim_feeds
                .iter()
                .map(|feed| feed.title.clone())
                .collect::<Vec<_>>();
            let merge_feeds = app
                .candidate_claim_feeds
                .iter()
                .map(|feed| feed.title.clone())
                .collect::<Vec<_>>();
            let main_artist_ids = main_wallet
                .artist_links
                .iter()
                .map(|link| short_id(&link.artist_id))
                .collect::<Vec<_>>();
            let merge_artist_ids = merge_wallet
                .artist_links
                .iter()
                .map(|link| short_id(&link.artist_id))
                .collect::<Vec<_>>();
            let main_names = app
                .claim_feeds
                .iter()
                .flat_map(|feed| {
                    feed.routes
                        .iter()
                        .filter_map(|route| route.recipient_name.clone())
                })
                .collect::<Vec<_>>();
            let merge_names = app
                .candidate_claim_feeds
                .iter()
                .flat_map(|feed| {
                    feed.routes
                        .iter()
                        .filter_map(|route| route.recipient_name.clone())
                })
                .collect::<Vec<_>>();
            let main_route_types = route_types_from_detail(main_wallet);
            let merge_route_types = route_types_from_detail(merge_wallet);

            let _ = writeln!(
                text,
                "Question: merge '{}' [{}] into main wallet '{}' [{}]?",
                merge_wallet.display_name,
                short_id(&merge_wallet.wallet_id),
                main_wallet.display_name,
                short_id(&main_wallet.wallet_id)
            );
            let _ = writeln!(
                text,
                "Shared clues: recipient_names={}  aliases={}  feeds={}  artist_ids={}",
                shared_preview(&main_names, &merge_names, 3, 18),
                shared_preview(&main_aliases, &merge_aliases, 3, 18),
                shared_preview(&main_feeds, &merge_feeds, 3, 18),
                shared_preview(&main_artist_ids, &merge_artist_ids, 3, 12)
            );
            let _ = writeln!(
                text,
                "Wallet shapes: main={}  merge-in={}  class labels are provisional.",
                route_type_summary(&main_route_types),
                route_type_summary(&merge_route_types)
            );
            let _ = writeln!(
                text,
                "Press 'm' to merge {} into {}. The main wallet survives.",
                short_id(&merge_wallet.wallet_id),
                short_id(&main_wallet.wallet_id)
            );
            let _ = writeln!(
                text,
                "Press 'x' if {} should not merge into any main wallet in this group.",
                short_id(&merge_wallet.wallet_id)
            );
            let _ = writeln!(
                text,
                "Edit main wallet: c=cycle class  v=cycle confidence  z=revert operator edits"
            );
        } else if let Some(source_review) = app.current_source_review() {
            let _ = writeln!(
                text,
                "Question: choose which wallet should merge into main wallet '{}' [{}], if any.",
                source_review.display_name,
                short_id(&source_review.wallet_id)
            );
            let _ = writeln!(
                text,
                "Edit main wallet: c=cycle class  v=cycle confidence  z=revert operator edits"
            );
        }
    } else {
        let _ = writeln!(text, "No review group selected.");
    }
    text
}

fn chooser_summary_lines(
    wallet_id: &str,
    display_name: &str,
    wallet_class: &str,
    class_confidence: &str,
    feed_count: i64,
    endpoint_preview: &[stophammer::db::WalletEndpointPreview],
) -> Vec<Line<'static>> {
    let endpoint_line = peer_primary_endpoint_line(endpoint_preview);
    vec![
        Line::from(format!(
            "{} [{}]",
            abbreviate(display_name, 30),
            short_id(wallet_id)
        )),
        Line::from(Span::styled(
            format!("{wallet_class} ({class_confidence})"),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            endpoint_line,
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            format!("{feed_count} feeds"),
            Style::default().fg(Color::LightBlue),
        )),
    ]
}

fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let header = Paragraph::new(format!(
        "Focus={:?}  groups={}  group-wallet={} of {}  candidates={}  feeds={}  {}",
        app.focus,
        app.groups.len(),
        if app.current_group().is_some() {
            app.selected_source + 1
        } else {
            0
        },
        app.current_group()
            .map(|group| group.reviews.len())
            .unwrap_or(0),
        app.candidate_wallets.len(),
        app.claim_feeds.len(),
        app.status
    ));
    frame.render_widget(header, root[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(24),
            Constraint::Percentage(40),
            Constraint::Percentage(36),
        ])
        .split(root[1]);

    let group_items = app
        .groups
        .iter()
        .map(|group| {
            let title = abbreviate(&group.label, 30);
            let detail = format!("{}  {} wallets", group.source, group.reviews.len());
            ListItem::new(vec![
                Line::from(title),
                Line::from(Span::styled(detail, Style::default().fg(Color::DarkGray))),
            ])
        })
        .collect::<Vec<_>>();
    let group_list = List::new(group_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Review Groups ({})", app.groups.len()))
                .border_style(block_style(app.focus == Focus::Groups))
                .border_type(block_border_type(app.focus == Focus::Groups)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::LightBlue)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    frame.render_stateful_widget(group_list, body[0], &mut app.group_state);

    let center = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(18),
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Min(0),
        ])
        .split(body[1]);

    let task = Paragraph::new(build_task_text(app))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(styled_title("Task", Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(task, center[0]);

    let compare = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(center[1]);

    let source_card = Paragraph::new(Text::from(wallet_card_lines(
        app.source_wallet_detail.as_ref(),
        &app.claim_feeds,
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(styled_title("Main Wallet", Color::Green)),
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(source_card, compare[0]);

    let candidate_card = Paragraph::new(Text::from(wallet_card_lines(
        app.candidate_wallet_detail.as_ref(),
        &app.candidate_claim_feeds,
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(styled_title("Wallet To Merge", Color::Yellow)),
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(candidate_card, compare[1]);

    let source_items = if !app.group_wallets.is_empty() {
        app.group_wallets
            .iter()
            .map(|wallet| {
                ListItem::new(chooser_summary_lines(
                    &wallet.wallet_id,
                    &wallet.display_name,
                    &wallet.wallet_class,
                    &wallet.class_confidence,
                    wallet.feed_count,
                    &wallet.endpoint_preview,
                ))
            })
            .collect::<Vec<_>>()
    } else {
        vec![ListItem::new("No source wallets")]
    };
    let source_list = List::new(source_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(styled_title("Choose Main Wallet", Color::Green))
                .border_style(block_style(app.focus == Focus::SourceWallets))
                .border_type(block_border_type(app.focus == Focus::SourceWallets)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    frame.render_stateful_widget(source_list, center[2], &mut app.source_state);

    let candidate_items = if app.candidate_wallets.is_empty() {
        vec![ListItem::new("No candidates")]
    } else {
        app.candidate_wallets
            .iter()
            .map(|candidate| {
                ListItem::new(chooser_summary_lines(
                    &candidate.wallet_id,
                    &candidate.display_name,
                    &candidate.wallet_class,
                    &candidate.class_confidence,
                    candidate.feed_count,
                    &candidate.endpoint_preview,
                ))
            })
            .collect::<Vec<_>>()
    };
    let candidate_list = List::new(candidate_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(styled_title("Choose Wallet To Merge [ / ]", Color::Yellow))
                .border_style(block_style(app.focus == Focus::Candidates))
                .border_type(block_border_type(app.focus == Focus::Candidates)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    frame.render_stateful_widget(candidate_list, center[3], &mut app.candidate_state);

    let feed_items = if app.claim_feeds.is_empty() {
        vec![ListItem::new("No feeds")]
    } else {
        app.claim_feeds
            .iter()
            .map(|feed| {
                ListItem::new(vec![
                    Line::from(abbreviate(&feed.title, 48)),
                    Line::from(Span::styled(
                        format!(
                            "guid {}  {} routes, {} platforms, {} ids, {} contributors",
                            short_id(&feed.feed_guid),
                            feed.routes.len(),
                            feed.platform_claims.len(),
                            feed.entity_id_claims.len(),
                            feed.contributor_claims.len()
                        ),
                        Style::default().fg(Color::DarkGray),
                    )),
                ])
            })
            .collect::<Vec<_>>()
    };
    let feed_list = List::new(feed_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(styled_title("Evidence Feeds For Source", Color::Cyan))
                .border_style(block_style(app.focus == Focus::Feeds))
                .border_type(block_border_type(app.focus == Focus::Feeds)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    frame.render_stateful_widget(feed_list, center[4], &mut app.feed_state);

    let evidence_items = app
        .evidence_rows
        .iter()
        .map(|row| {
            let style = match row.node {
                EvidenceNode::Section(_) => Style::default().add_modifier(Modifier::BOLD),
                EvidenceNode::ItemHeader(_) => Style::default().fg(Color::White),
                EvidenceNode::ItemDetail => Style::default().fg(Color::Gray),
            };
            let lines = row
                .lines
                .iter()
                .enumerate()
                .map(|(index, line)| {
                    if index == 0 {
                        let content = line
                            .spans
                            .iter()
                            .map(|span| span.content.to_string())
                            .collect::<String>();
                        Line::from(Span::styled(content, style))
                    } else {
                        line.clone()
                    }
                })
                .collect::<Vec<_>>();
            ListItem::new(lines)
        })
        .collect::<Vec<_>>();
    let evidence_list = List::new(evidence_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(styled_title(
                    &app.current_feed().map_or_else(
                        || "Evidence".to_string(),
                        |feed| format!("Evidence {}", feed.title),
                    ),
                    Color::Magenta,
                ))
                .border_style(block_style(app.focus == Focus::Evidence))
                .border_type(block_border_type(app.focus == Focus::Evidence)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Magenta)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    frame.render_stateful_widget(evidence_list, body[2], &mut app.evidence_state);

    let footer = Paragraph::new(
        "Wallet Review TUI  TAB/LEFT/RIGHT: focus  [ / ]: merge target  UP/DOWN: move  ENTER: toggle tree item  A: apply  U: undo batch  M: merge into main  X: do not merge  C: cycle class  V: cycle confidence  Z: revert edits  R: reload  Q: quit",
    );
    frame.render_widget(footer, root[2]);

    if let Some(dialog) = &app.dialog {
        let popup = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Length((dialog.lines.len() as u16).saturating_add(4)),
                Constraint::Percentage(30),
            ])
            .split(frame.area());
        let row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(50),
                Constraint::Percentage(25),
            ])
            .split(popup[1]);
        let dialog_text = dialog
            .lines
            .iter()
            .map(|line| Line::from(line.clone()))
            .collect::<Vec<_>>();
        let dialog_widget = Paragraph::new(dialog_text)
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
        frame.render_widget(dialog_widget, row[1]);
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
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') => {
                    app.dialog = None;
                }
                _ => {}
            }
            continue;
        }
        match key.code {
            KeyCode::Char('q') => return Ok(()),
            KeyCode::Tab | KeyCode::Right => app.next_focus(),
            KeyCode::BackTab | KeyCode::Left => app.previous_focus(),
            KeyCode::Char('[') => app.previous_candidate()?,
            KeyCode::Char(']') => app.next_candidate()?,
            KeyCode::Down => app.move_down()?,
            KeyCode::Up => app.move_up()?,
            KeyCode::Home => app.jump_top()?,
            KeyCode::End => app.jump_bottom()?,
            KeyCode::Enter | KeyCode::Char(' ') if app.focus == Focus::Evidence => {
                app.toggle_selected_section();
            }
            KeyCode::Char('a') => {
                app.apply_reviewed_merges()?;
            }
            KeyCode::Char('m') => {
                app.approve_merge()?;
            }
            KeyCode::Char('u') => {
                app.undo_last_apply_batch()?;
            }
            KeyCode::Char('x') => {
                app.reject_review()?;
            }
            KeyCode::Char('c') => {
                app.cycle_main_wallet_class()?;
            }
            KeyCode::Char('v') => {
                app.cycle_main_wallet_confidence()?;
            }
            KeyCode::Char('z') => {
                app.revert_main_wallet_edits()?;
            }
            KeyCode::Char('r') => {
                app.reload()?;
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
