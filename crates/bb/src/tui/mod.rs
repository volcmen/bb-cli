//! `bb dash` — the interactive TUI dashboard (Epic 9).
//!
//! Architecture (DDR 0003): `ratatui` + `crossterm`, **no tokio**. The [`app`]
//! module is a pure Model-Update-View core; [`terminal`] owns the panic-safe
//! screen guard; this module's [`run`] is the blocking input→update→draw loop.
//! Data fetching arrives in spec 035 via a `std::thread` worker.

mod app;
mod keymap;
mod terminal;

use ratatui::crossterm::event::{self, Event, KeyEventKind};

use app::App;

/// Run the dashboard until the user quits, restoring the terminal on the way out
/// (including on panic, via the guard's hook).
///
/// # Errors
/// Propagates terminal setup / draw / input-read [`io`](std::io) errors.
pub fn run(authed: bool) -> anyhow::Result<()> {
    let mut guard = terminal::TerminalGuard::new()?;
    let mut app = App::new(authed);

    while !app.should_quit {
        guard.terminal.draw(|frame| app.view(frame))?;
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                if let Some(msg) = keymap::map_key(key) {
                    app.update(msg);
                }
            }
        }
    }
    Ok(())
}
