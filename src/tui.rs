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

/// Simple text dialog payload shared by interactive review TUIs.
#[derive(Debug, Clone)]
pub struct TextDialog {
    pub title: String,
    pub lines: Vec<String>,
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
