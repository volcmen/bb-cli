//! `bb dash` — the interactive TUI dashboard (Epic 9).
//!
//! Architecture (DDR 0003): `ratatui` + `crossterm`, **no tokio**. [`app`] is a
//! pure Model-Update-View core; [`terminal`] owns the panic-safe screen guard;
//! [`worker`] is a `std::thread` data fetcher (spec 035); this module's [`run`] is
//! the blocking loop that merges input, worker responses, and a tick timer.

mod app;
mod keymap;
mod terminal;
mod worker;

use std::sync::Arc;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::commands::pr::query::PrFilter;
use crate::core::{RepoId, Transport};

use app::{App, Msg};
use worker::{Request, RequestKind, Worker};

/// How often the loop wakes to advance the spinner when no input arrives.
const TICK: Duration = Duration::from_millis(120);

/// Run the dashboard until the user quits, restoring the terminal on the way out
/// (including on panic, via the guard's hook).
///
/// When authenticated against a resolvable `repo`, a background [`Worker`] is
/// spawned and an initial PR load is kicked off; otherwise the UI still opens
/// (showing a login / no-repo hint).
///
/// # Errors
/// Propagates terminal setup / draw / input-read [`io`](std::io) errors.
pub fn run(
    authed: bool,
    repo: Option<RepoId>,
    transport: Arc<dyn Transport>,
    header: Option<String>,
) -> anyhow::Result<()> {
    let mut guard = terminal::TerminalGuard::new()?;
    let mut app = App::new(authed);

    let pr_filter = PrFilter {
        state: "OPEN".to_owned(),
        base: None,
        limit: 30,
    };

    let worker = match (authed, repo) {
        (true, Some(repo)) => {
            let worker = Worker::spawn(transport, header, repo);
            app.begin(RequestKind::Prs);
            worker.send(Request::Prs(pr_filter.clone()));
            Some(worker)
        }
        (true, None) => {
            app.status = Some("no Bitbucket repository here — pass -R WORKSPACE/SLUG".to_owned());
            None
        }
        (false, _) => None,
    };

    while !app.should_quit {
        guard.terminal.draw(|frame| app.view(frame))?;

        // Input source (with the tick as its timeout); a timeout means "tick".
        if event::poll(TICK)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if let Some(msg) = keymap::map_key(key) {
                        // Refresh re-issues the section's request through the worker;
                        // everything else folds into the model.
                        if msg == Msg::Refresh {
                            if let Some(worker) = &worker {
                                app.begin(RequestKind::Prs);
                                worker.send(Request::Prs(pr_filter.clone()));
                            }
                        } else {
                            app.update(msg);
                        }
                    }
                }
            }
        } else {
            app.update(Msg::Tick);
        }

        // Drain any worker responses without blocking.
        if let Some(worker) = &worker {
            while let Ok(response) = worker.rx.try_recv() {
                app.apply_response(response);
            }
        }
    }
    Ok(())
}
