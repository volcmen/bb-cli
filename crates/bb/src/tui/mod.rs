//! `bb dash` — the interactive TUI dashboard (Epic 9).
//!
//! Architecture (DDR 0003): `ratatui` + `crossterm`, **no tokio**. [`app`] is a
//! pure Model-Update-View core; [`terminal`] owns the panic-safe screen guard;
//! [`worker`] is a `std::thread` data fetcher (spec 035); this module's [`run`] is
//! the blocking loop that merges input, worker responses, and a tick timer.

mod app;
pub(crate) mod config;
mod keymap;
mod terminal;
mod worker;

use std::sync::Arc;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::commands::issue::query::IssueFilter;
use crate::commands::pr::query::PrFilter;
use crate::core::{Browser, RepoId, Transport};

use app::{App, Msg, PendingAction, Tab};
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
    browser: Arc<dyn Browser>,
    dash_config: config::DashConfig,
    config_warnings: Vec<String>,
) -> anyhow::Result<()> {
    let mut guard = terminal::TerminalGuard::new()?;
    let mut app = App::new(authed);
    app.theme = dash_config.theme;
    app.active_tab = dash_config.default_tab;
    if !config_warnings.is_empty() {
        app.status = Some(format!("config: {}", config_warnings.join("; ")));
    }

    let pr_filter = PrFilter {
        state: "OPEN".to_owned(),
        base: None,
        limit: 30,
    };
    let issue_filter = IssueFilter {
        state: None,
        limit: 30,
    };
    let pipeline_limit = 30usize;
    // Auto-refresh cadence for running pipelines (the tick is ~120ms), from config
    // and bounded so it never hammers the API.
    let mut ticks_since_poll = 0u32;
    let poll_every_ticks = u32::try_from(dash_config.refresh_secs * 1000 / 120)
        .unwrap_or(40)
        .max(1);

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
                    if let Some(msg) = keymap::map_key(key, app.input_context()) {
                        dispatch(
                            &mut app,
                            worker.as_ref(),
                            &pr_filter,
                            &issue_filter,
                            pipeline_limit,
                            &browser,
                            msg,
                        );
                    }
                }
            }
        } else {
            app.update(Msg::Tick);
            ticks_since_poll += 1;
            // Live pipeline refresh: while any run is in progress, re-fetch on a
            // bounded cadence; stops automatically once all are terminal.
            if ticks_since_poll >= poll_every_ticks {
                ticks_since_poll = 0;
                if app.pipelines_active() {
                    if let Some(worker) = &worker {
                        app.begin(RequestKind::Pipelines);
                        worker.send(Request::Pipelines(pipeline_limit));
                    }
                }
            }
        }

        // Lazily load a section the first time it's shown.
        if app.needs_issue_load() {
            if let Some(worker) = &worker {
                app.begin(RequestKind::Issues);
                worker.send(Request::Issues(issue_filter.clone()));
            }
        }
        if app.needs_pipeline_load() {
            if let Some(worker) = &worker {
                app.begin(RequestKind::Pipelines);
                worker.send(Request::Pipelines(pipeline_limit));
            }
        }

        // Drain any worker responses without blocking. A completed mutation
        // (ActionDone) triggers an auto-refresh of the list (and the detail pane,
        // if open) so the UI reflects the new state.
        if let Some(worker) = &worker {
            while let Ok(response) = worker.rx.try_recv() {
                let refresh = matches!(response, worker::Response::ActionDone(_));
                let detail_id = if refresh { app.detail_pr_id() } else { None };
                app.apply_response(response);
                if refresh {
                    app.begin(RequestKind::Prs);
                    worker.send(Request::Prs(pr_filter.clone()));
                    if let Some(id) = detail_id {
                        app.begin(RequestKind::PrDetail);
                        worker.send(Request::PrDetail(id));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Route a UI message: worker- and Browser-touching messages are acted on here
/// (the loop owns those seams); everything else folds into the model.
fn dispatch(
    app: &mut App,
    worker: Option<&Worker>,
    pr_filter: &PrFilter,
    issue_filter: &IssueFilter,
    pipeline_limit: usize,
    browser: &Arc<dyn Browser>,
    msg: Msg,
) {
    // Any key press dismisses a lingering toast/error before acting on it.
    app.status = None;
    match msg {
        Msg::Refresh => {
            if let Some(worker) = worker {
                match app.active_tab {
                    Tab::Issues => {
                        app.begin(RequestKind::Issues);
                        worker.send(Request::Issues(issue_filter.clone()));
                    }
                    Tab::Pipelines => {
                        app.begin(RequestKind::Pipelines);
                        worker.send(Request::Pipelines(pipeline_limit));
                    }
                    Tab::PullRequests => {
                        app.begin(RequestKind::Prs);
                        worker.send(Request::Prs(pr_filter.clone()));
                    }
                }
            }
        }
        Msg::Open => match app.active_tab {
            Tab::Issues => {
                if let Some(id) = app.selected_issue_id() {
                    app.update(Msg::Open);
                    if let Some(worker) = worker {
                        worker.send(Request::IssueDetail(id));
                    }
                }
            }
            Tab::Pipelines => {
                if let Some(build) = app.selected_pipeline_build() {
                    app.update(Msg::Open);
                    if let Some(worker) = worker {
                        worker.send(Request::PipelineDetail(build));
                    }
                }
            }
            Tab::PullRequests => {
                if let Some(id) = app.selected_pr_id() {
                    app.update(Msg::Open);
                    if let Some(worker) = worker {
                        worker.send(Request::PrDetail(id));
                    }
                }
            }
        },
        Msg::OpenBrowser => {
            if let Some(url) = app.current_url() {
                let _ = browser.browse(url);
            }
        }
        Msg::Approve => {
            if let Some(id) = app.action_target_id() {
                let now_approved = app.toggle_self_approved(id);
                if let Some(worker) = worker {
                    app.begin(RequestKind::Action);
                    worker.send(if now_approved {
                        Request::Approve(id)
                    } else {
                        Request::Unapprove(id)
                    });
                }
            }
        }
        Msg::ConfirmYes => {
            if let Some(action) = app.take_pending_confirm() {
                if let Some(worker) = worker {
                    app.begin(RequestKind::Action);
                    worker.send(match action {
                        PendingAction::Merge(id) => Request::Merge(id),
                        PendingAction::Decline(id) => Request::Decline(id),
                    });
                }
            }
        }
        Msg::Submit => {
            if let Some((id, body)) = app.take_comment() {
                if !body.trim().is_empty() {
                    if let Some(worker) = worker {
                        app.begin(RequestKind::Action);
                        worker.send(Request::Comment(id, body));
                    }
                }
            }
        }
        other => app.update(other),
    }
}
