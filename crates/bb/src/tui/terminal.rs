//! A panic-safe terminal guard: enters the alternate screen + raw mode on
//! construction and restores on `Drop`. A panic hook also restores first, so a
//! mid-render panic doesn't leave the user's terminal garbled.

use std::io::{self, Stdout};

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::Terminal;

/// The concrete terminal type the dashboard draws to.
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Owns the terminal's alt-screen/raw-mode state; restores it on drop.
pub struct TerminalGuard {
    pub terminal: Tui,
}

impl TerminalGuard {
    /// Enter the alternate screen + raw mode and build the ratatui terminal.
    ///
    /// # Errors
    /// Returns an [`io::Error`] if the terminal cannot be put into raw mode or
    /// the alternate screen.
    pub fn new() -> io::Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self { terminal })
    }

    /// Drop back to the normal screen so an external command (#90) can use the
    /// terminal. Pair with [`TerminalGuard::resume`].
    ///
    /// # Errors
    /// Propagates the terminal-restore [`io::Error`].
    pub fn suspend(&mut self) -> io::Result<()> {
        restore()
    }

    /// Re-enter the alternate screen + raw mode after a suspended command and
    /// force a full redraw.
    ///
    /// # Errors
    /// Propagates the raw-mode / alternate-screen [`io::Error`].
    pub fn resume(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        self.terminal.clear()?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = restore();
    }
}

/// Leave raw mode + the alternate screen (idempotent enough to call from Drop and
/// the panic hook).
fn restore() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

/// Wrap the current panic hook so the terminal is restored before the panic
/// message prints.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore();
        previous(info);
    }));
}
