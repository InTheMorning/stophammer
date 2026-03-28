//! Shared terminal-session helpers for ratatui/crossterm review tools.

use std::io;

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

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
