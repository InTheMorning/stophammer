//! Shared terminal-session helpers for ratatui/crossterm review tools.

use std::collections::BTreeMap;
use std::io;

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

/// Restores raw-mode and alternate-screen terminal state on both normal exit
/// and panic unwinding.
#[derive(Debug)]
pub struct TerminalCleanupGuard {
    raw_mode: bool,
    alternate_screen: bool,
}

impl TerminalCleanupGuard {
    /// Enters raw mode and the alternate screen for an interactive TUI session.
    ///
    /// # Errors
    ///
    /// Returns an `io::Error` if the terminal state cannot be prepared.
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        Ok(Self {
            raw_mode: true,
            alternate_screen: true,
        })
    }

    /// Restores terminal state after a clean TUI exit and shows the cursor.
    ///
    /// # Errors
    ///
    /// Returns an `io::Error` if terminal cleanup fails.
    pub fn complete<W: io::Write>(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<W>>,
    ) -> io::Result<()> {
        if self.raw_mode {
            disable_raw_mode()?;
            self.raw_mode = false;
        }
        if self.alternate_screen {
            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
            self.alternate_screen = false;
        }
        terminal.show_cursor()
    }
}

impl Drop for TerminalCleanupGuard {
    fn drop(&mut self) {
        if self.raw_mode {
            let _ = disable_raw_mode();
        }
        if self.alternate_screen {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, LeaveAlternateScreen);
        }
    }
}

/// Summarizes the dominant `source` within a review subset.
#[must_use]
pub fn dominant_source_summary<'a>(sources: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut total = 0usize;
    for source in sources {
        total = total.saturating_add(1);
        *counts.entry(source).or_default() += 1;
    }
    let (source, count) = counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)))?;
    let share = (count.saturating_mul(100)) / total.max(1);
    Some(format!("Top source in this subset: {source} ({count}, {share}%)"))
}

/// Formats the compact queue summary line shared by review TUIs.
#[must_use]
pub fn format_source_count_summary<'a>(
    label: &str,
    items: impl IntoIterator<Item = (&'a str, usize)>,
) -> String {
    let items = items
        .into_iter()
        .map(|(source, count)| (source.to_string(), count))
        .collect::<Vec<_>>();
    if items.is_empty() {
        return format!("No pending {label}");
    }

    let total: usize = items.iter().map(|(_, count)| *count).sum();
    let dominant = items.first().map(|(source, count)| {
        let share = (count.saturating_mul(100)) / total.max(1);
        format!("top={source}({share}%)")
    });
    let details = items
        .iter()
        .take(3)
        .map(|(source, count)| format!("{source}={count}"))
        .collect::<Vec<_>>()
        .join(", ");
    dominant.map_or_else(
        || format!("Pending {label}: {total} ({details})"),
        |dominant| format!("Pending {label}: {total} ({dominant}; {details})"),
    )
}

/// Formats a dominant-source hint for operator dialogs.
#[must_use]
pub fn format_dominant_family_hint(source: &str, share: usize, suffix: &str) -> String {
    format!("Dominant family: {source} ({share}%). {suffix}")
}

/// Formats a standard counted dialog title shared by review TUIs.
#[must_use]
pub fn format_counted_dialog_title(label: &str, count: usize) -> String {
    format!("{label} ({count})")
}

/// Formats the operator overview dialog title shared by review TUIs.
#[must_use]
pub fn format_operator_overview_title(
    artist_total: usize,
    wallet_total: usize,
    hotspot_count: usize,
) -> String {
    format!("Operator Overview (artist={artist_total} wallet={wallet_total} hotspots={hotspot_count})")
}

/// Appends a ranked source-family section shared by operator dialogs.
pub fn push_source_family_section<'a>(
    lines: &mut Vec<String>,
    heading: &str,
    items: impl IntoIterator<Item = (&'a str, usize)>,
    total: usize,
    max_items: usize,
    dominant_suffix: &str,
) {
    let items = items
        .into_iter()
        .map(|(source, count)| (source.to_string(), count))
        .collect::<Vec<_>>();
    lines.push(heading.to_string());
    if items.is_empty() {
        lines.push("  none".to_string());
        return;
    }
    if let Some((source, count)) = items.first() {
        let share = (count.saturating_mul(100)) / total.max(1);
        if share >= 50 {
            lines.push(format!(
                "  {}",
                format_dominant_family_hint(source, share, dominant_suffix),
            ));
        }
    }
    lines.extend(items.into_iter().take(max_items).map(|(source, count)| {
        let share = (count.saturating_mul(100)) / total.max(1);
        format!("  {source}: {count} ({share}%)")
    }));
}

/// Builds the body lines for a queue-summary dialog shared by review TUIs.
#[must_use]
pub fn build_queue_summary_lines<'a>(
    items: impl IntoIterator<Item = (&'a str, usize)>,
    total: usize,
    empty_message: &str,
    dominant_suffix: &str,
) -> Vec<String> {
    let items = items
        .into_iter()
        .map(|(source, count)| (source.to_string(), count))
        .collect::<Vec<_>>();
    if items.is_empty() {
        return vec![empty_message.to_string()];
    }

    let mut lines = Vec::new();
    if let Some((source, count)) = items.first() {
        let share = (count.saturating_mul(100)) / total.max(1);
        if share >= 50 {
            lines.push(format_dominant_family_hint(source, share, dominant_suffix));
            lines.push(String::new());
        }
    }
    lines.extend(items.into_iter().map(|(source, count)| {
        let share = (count.saturating_mul(100)) / total.max(1);
        format!("{source}: {count} ({share}%)")
    }));
    lines
}

/// Builds lines for stale/recent review-subset dialogs while leaving row formatting local.
#[must_use]
pub fn build_review_subset_lines<T>(
    description: &str,
    empty_message: &str,
    items: &[T],
    source_of: impl Fn(&T) -> &str,
    format_row: impl Fn(&T) -> String,
) -> Vec<String> {
    let mut lines = vec![description.to_string(), String::new()];
    if items.is_empty() {
        lines.push(empty_message.to_string());
        return lines;
    }

    if let Some(summary) = dominant_source_summary(items.iter().map(source_of)) {
        lines.push(summary);
        lines.push(String::new());
    }
    lines.extend(items.iter().map(format_row));
    lines
}

/// Appends formatted feed-hotspot lines shared by operator dialogs.
pub fn push_feed_hotspot_lines(
    lines: &mut Vec<String>,
    hotspots: &[crate::db::PendingReviewFeedHotspot],
    row_prefix: &str,
    url_prefix: &str,
    short_id: impl Fn(&str) -> String,
    abbreviate: impl Fn(&str, usize) -> String,
) {
    let total_hotspot_load: usize = hotspots.iter().map(|feed| feed.total_review_count).sum();
    for feed in hotspots {
        let share = if total_hotspot_load > 0 {
            (feed.total_review_count * 100) / total_hotspot_load
        } else {
            0
        };
        lines.push(format!(
            "{row_prefix}{} [{}] | total={} ({}%) artist={} wallet={}",
            feed.title,
            short_id(&feed.feed_guid),
            feed.total_review_count,
            share,
            feed.artist_review_count,
            feed.wallet_review_count
        ));
        lines.push(format!("{url_prefix}{}", abbreviate(&feed.feed_url, 72)));
    }
}

/// Builds the body lines for a feed-hotspot dialog shared by review TUIs.
#[must_use]
pub fn build_feed_hotspot_dialog_lines(
    hotspots: &[crate::db::PendingReviewFeedHotspot],
    short_id: impl Fn(&str) -> String,
    abbreviate: impl Fn(&str, usize) -> String,
) -> Vec<String> {
    let mut lines = vec![
        "Top feeds by pending combined review load".to_string(),
        String::new(),
    ];
    if hotspots.is_empty() {
        lines.push("No feed hotspots with pending reviews".to_string());
    } else {
        push_feed_hotspot_lines(&mut lines, hotspots, "", "  ", short_id, abbreviate);
    }
    lines
}

/// Parameters for a shared review-playbook dialog body.
#[derive(Debug, Clone, Copy)]
pub struct ReviewPlaybookConfig<'a> {
    pub review_label_plural: &'a str,
    pub created_last_24h: usize,
    pub older_than_7d: usize,
    pub backlog_idle_message: &'a str,
    pub dominant_family_walk_template: &'a str,
    pub final_step: &'a str,
}

/// Builds the shared backlog/source/hotspot guidance body for review playbooks.
#[must_use]
pub fn build_review_playbook_lines<'a>(
    total: usize,
    source_items: impl IntoIterator<Item = (&'a str, usize)>,
    hotspots: &[crate::db::PendingReviewFeedHotspot],
    config: ReviewPlaybookConfig<'_>,
    short_id: impl Fn(&str) -> String,
    abbreviate: impl Fn(&str, usize) -> String,
) -> Vec<String> {
    let source_items = source_items
        .into_iter()
        .map(|(source, count)| (source.to_string(), count))
        .collect::<Vec<_>>();

    let mut lines = vec![format!("Pending {}: {total}", config.review_label_plural)];
    if total == 0 {
        lines.push(config.backlog_idle_message.to_string());
        return lines;
    }

    if config.older_than_7d > 0 {
        lines.push(format!(
            "1. Clear stale backlog first: {} {} are older than 7 days.",
            config.older_than_7d, config.review_label_plural
        ));
    } else if config.created_last_24h > 0 {
        lines.push(format!(
            "1. Fresh churn only: {} {} were created in the last 24 hours.",
            config.created_last_24h, config.review_label_plural
        ));
    }

    if let Some((source, count)) = source_items.first() {
        let share = (count.saturating_mul(100)) / total.max(1);
        lines.push(format!(
            "2. Main source family: {source} ({count} pending, {share}% of backlog)."
        ));
        if share >= 50 {
            lines.push(
                config
                    .dominant_family_walk_template
                    .replace("{}", source.as_str()),
            );
        }
    }

    if let Some(feed) = hotspots.first() {
        let total_hotspot_load: usize = hotspots.iter().map(|item| item.total_review_count).sum();
        let share = if total_hotspot_load > 0 {
            (feed.total_review_count * 100) / total_hotspot_load
        } else {
            0
        };
        lines.push(format!(
            "3. Start with feed hotspot: {} [{}] (total={}, {}% of hotspot load, artist={}, wallet={}).",
            feed.title,
            short_id(&feed.feed_guid),
            feed.total_review_count,
            share,
            feed.artist_review_count,
            feed.wallet_review_count
        ));
        lines.push(format!("   {}", abbreviate(&feed.feed_url, 72)));
    }

    lines.push(config.final_step.to_string());
    lines
}

/// Shared operator shortcut lines used by review-TUI help dialogs.
#[must_use]
pub fn review_operator_help_lines(same_family_line: &str) -> Vec<String> {
    vec![
        "o: operator overview".to_string(),
        "p: review-next playbook".to_string(),
        "s: queue source summary".to_string(),
        "h: hottest feeds".to_string(),
        "t: stale reviews (>7d)".to_string(),
        "y: recent reviews (<24h)".to_string(),
        same_family_line.to_string(),
    ]
}

/// Builds the shared footer legend used by review TUIs.
#[must_use]
pub fn build_review_footer(prefix: &str) -> String {
    format!(
        "{prefix}  n/N same-family  o overview  p playbook  s summary  h hotspots  t stale  y recent  ? help  r reload  q quit"
    )
}

/// Formats a Unix timestamp in local wall-clock time for TUI surfaces.
#[must_use]
pub fn format_local_timestamp(timestamp: i64) -> String {
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

/// Builds the age/total preamble for queue-summary dialogs.
#[must_use]
pub fn build_queue_summary_header_lines(
    total_label: &str,
    total: usize,
    created_last_24h: usize,
    older_than_7d: usize,
    oldest_created_at: Option<i64>,
) -> Vec<String> {
    let mut lines = vec![
        format!("Total pending {total_label}: {total}"),
        format!("Created in last 24h: {created_last_24h}"),
        format!("Older than 7d: {older_than_7d}"),
    ];
    if let Some(oldest_created_at) = oldest_created_at {
        lines.push(format!(
            "Oldest created_at: {}",
            format_local_timestamp(oldest_created_at)
        ));
    }
    lines
}

/// Builds the shared age/totals preamble for operator overview dialogs.
#[must_use]
pub fn build_operator_overview_header_lines(
    artist_total: usize,
    artist_age: &crate::db::PendingReviewAgeSummary,
    wallet_total: usize,
    wallet_age: &crate::db::PendingReviewAgeSummary,
) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Artist reviews: total={artist_total} last24h={} older7d={}",
            artist_age.created_last_24h, artist_age.older_than_7d
        ),
        format!(
            "Wallet reviews: total={wallet_total} last24h={} older7d={}",
            wallet_age.created_last_24h, wallet_age.older_than_7d
        ),
    ];
    if let Some(oldest) = artist_age.oldest_created_at {
        lines.push(format!("Oldest artist review: {}", format_local_timestamp(oldest)));
    }
    if let Some(oldest) = wallet_age.oldest_created_at {
        lines.push(format!("Oldest wallet review: {}", format_local_timestamp(oldest)));
    }
    lines.push(String::new());
    lines
}

/// Configuration for the shared operator-overview body.
#[derive(Debug, Clone, Copy)]
pub struct OperatorOverviewConfig<'a> {
    pub artist_total: usize,
    pub artist_age: &'a crate::db::PendingReviewAgeSummary,
    pub wallet_total: usize,
    pub wallet_age: &'a crate::db::PendingReviewAgeSummary,
    pub artist_dominant_suffix: &'a str,
    pub wallet_dominant_suffix: &'a str,
}

/// Builds the full operator-overview dialog body shared by review TUIs.
#[must_use]
pub fn build_operator_overview_lines<'a>(
    artist_items: impl IntoIterator<Item = (&'a str, usize)>,
    wallet_items: impl IntoIterator<Item = (&'a str, usize)>,
    hotspots: &[crate::db::PendingReviewFeedHotspot],
    config: OperatorOverviewConfig<'_>,
    short_id: impl Fn(&str) -> String,
    abbreviate: impl Fn(&str, usize) -> String,
) -> Vec<String> {
    let mut lines = build_operator_overview_header_lines(
        config.artist_total,
        config.artist_age,
        config.wallet_total,
        config.wallet_age,
    );
    push_source_family_section(
        &mut lines,
        "Top artist review sources:",
        artist_items,
        config.artist_total,
        3,
        config.artist_dominant_suffix,
    );
    lines.push(String::new());
    push_source_family_section(
        &mut lines,
        "Top wallet review sources:",
        wallet_items,
        config.wallet_total,
        3,
        config.wallet_dominant_suffix,
    );
    lines.push(String::new());
    lines.push("Hottest feeds:".to_string());
    if hotspots.is_empty() {
        lines.push("  none".to_string());
    } else {
        push_feed_hotspot_lines(&mut lines, hotspots, "  ", "    ", short_id, abbreviate);
    }
    lines
}

/// Simple text dialog payload shared by interactive review TUIs.
#[derive(Debug, Clone)]
pub struct TextDialog {
    pub title: String,
    pub lines: Vec<String>,
}

/// Builds a plain text dialog payload shared by review TUIs.
#[must_use]
pub fn text_dialog(title: impl Into<String>, lines: Vec<String>) -> TextDialog {
    TextDialog {
        title: title.into(),
        lines,
    }
}

/// Builds a standard counted dialog payload shared by review TUIs.
#[must_use]
pub fn counted_dialog(label: &str, count: usize, lines: Vec<String>) -> TextDialog {
    TextDialog {
        title: format_counted_dialog_title(label, count),
        lines,
    }
}

/// Builds the standard operator-overview dialog payload shared by review TUIs.
#[must_use]
pub fn operator_overview_dialog(
    artist_total: usize,
    wallet_total: usize,
    hotspot_count: usize,
    lines: Vec<String>,
) -> TextDialog {
    TextDialog {
        title: format_operator_overview_title(artist_total, wallet_total, hotspot_count),
        lines,
    }
}

/// Centers a rectangle inside `area` by percentage.
#[must_use]
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
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

/// Renders a centered modal text dialog with consistent styling.
pub fn render_text_dialog(frame: &mut Frame<'_>, area: Rect, dialog: &TextDialog) {
    let dialog_area = centered_rect(68, 45, area);
    frame.render_widget(Clear, dialog_area);
    let dialog_text = dialog
        .lines
        .iter()
        .cloned()
        .map(Line::from)
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
                .title(Line::styled(
                    dialog.title.clone(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, dialog_area);
}
