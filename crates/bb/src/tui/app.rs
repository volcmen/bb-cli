//! The `bb dash` application model — the single source of truth — with a
//! Model-Update-View shape: [`App`] holds state, [`App::update`] is the reducer
//! over [`Msg`], and [`App::view`] renders a frame. Pure and backend-agnostic so
//! it is exercised with `ratatui::backend::TestBackend` in tests.

use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use std::collections::HashSet;

use crate::api::models::{CommitStatus, Issue, Pipeline, PipelineStep, PullRequest, User};
use crate::render::sanitize;

use super::worker::{RequestKind, Response};

/// State backing the PR detail pane.
#[derive(Debug, Clone)]
struct Detail {
    pr: PullRequest,
    checks: Vec<CommitStatus>,
    scroll: u16,
    max_scroll: u16,
}

/// Spinner frames advanced by [`Msg::Tick`] while a request is in flight.
const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

/// The top-level sections (number keys `1/2/3` jump to them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    PullRequests,
    Issues,
    Pipelines,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::PullRequests, Tab::Issues, Tab::Pipelines];

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    fn title(self) -> &'static str {
        match self {
            Tab::PullRequests => "Pull Requests",
            Tab::Issues => "Issues",
            Tab::Pipelines => "Pipelines",
        }
    }
}

/// A modal layer over the main view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    Help,
    /// A y/N confirmation guarding a destructive action.
    Confirm {
        action: PendingAction,
        prompt: String,
    },
    /// A text-input modal for composing a comment.
    Comment {
        id: u64,
        buffer: String,
    },
}

/// A destructive action awaiting confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    Merge(u64),
    Decline(u64),
}

/// How raw key input should be interpreted, given the active modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputContext {
    Normal,
    Help,
    Confirm,
    Comment,
    /// Typing into the in-list fuzzy filter (`/`).
    Filter,
}

/// Semantic events produced by [`keymap`](super::keymap) from raw key input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    Quit,
    /// `Esc`: pop one modal layer, or quit at the root.
    Pop,
    ToggleHelp,
    SelectTab(usize),
    NextTab,
    PrevTab,
    Down,
    Up,
    Top,
    Bottom,
    HalfPageDown,
    HalfPageUp,
    /// Open the detail pane for the selected row (Enter / `l`).
    Open,
    /// Open the current PR in the browser (`o`) — acted on by the event loop.
    OpenBrowser,
    /// Approve / un-approve toggle (`a`) — acted on by the event loop.
    Approve,
    /// Merge (`m`) — opens a confirm modal.
    Merge,
    /// Decline (`x`) — opens a confirm modal.
    Decline,
    /// Comment (`C`) — opens an input modal.
    Comment,
    /// Confirm the pending destructive action (`y` in a confirm modal) — loop-handled.
    ConfirmYes,
    /// Submit the comment input (`Enter` in a comment modal) — loop-handled.
    Submit,
    /// Type a character into the comment input.
    InsertChar(char),
    /// Delete the last character of the comment input.
    Backspace,
    /// Re-fetch the active section (handled by the event loop, which owns the worker).
    Refresh,
    /// Open the in-list fuzzy filter (`/`).
    StartFilter,
    /// Type a character into the filter query.
    FilterChar(char),
    /// Delete the last character of the filter query.
    FilterBackspace,
    /// Stop editing the filter but keep it applied (Enter).
    ApplyFilter,
    /// Clear and close the filter (Esc).
    ClearFilter,
    /// Timer tick — advances the spinner (and later drives auto-refresh).
    Tick,
}

/// Rows moved by a half-page motion (`Ctrl-d`/`Ctrl-u`) until a real viewport
/// height is threaded through.
const HALF_PAGE: isize = 10;

/// The dashboard state.
#[derive(Debug, Clone)]
pub struct App {
    /// Whether the user is authenticated for the target host.
    pub authed: bool,
    pub active_tab: Tab,
    pub modal: Option<Modal>,
    pub should_quit: bool,
    /// Loaded pull requests (populated by [`App::apply_response`]).
    pub prs: Vec<PullRequest>,
    /// Index of the highlighted PR row.
    pub selected: usize,
    /// Loaded issues.
    pub issues: Vec<Issue>,
    /// Index of the highlighted issue row.
    pub issue_selected: usize,
    /// Whether the issue list has been fetched yet (lazy on first switch).
    pub issues_loaded: bool,
    /// Whether the repo's issue tracker is disabled.
    pub issues_disabled: bool,
    issue_detail: Option<Issue>,
    /// Loaded pipelines.
    pub pipelines: Vec<Pipeline>,
    /// Index of the highlighted pipeline row.
    pub pipeline_selected: usize,
    /// Whether the pipeline list has been fetched yet (lazy on first switch).
    pub pipelines_loaded: bool,
    pipeline_detail: Option<(Pipeline, Vec<PipelineStep>)>,
    /// The current fuzzy-filter query (empty = no filter).
    filter_query: String,
    /// Whether the filter input is focused (capturing keys).
    filtering: bool,
    /// In-flight request kinds (drives the spinner).
    pub loading: Vec<RequestKind>,
    /// Transient status/error toast.
    pub status: Option<String>,
    /// Whether the detail pane is open over the list.
    pub detail_open: bool,
    detail: Option<Detail>,
    /// PR ids you've approved this session (drives the `a` toggle optimistically).
    my_approved: HashSet<u64>,
    spinner: usize,
}

impl App {
    #[must_use]
    pub fn new(authed: bool) -> Self {
        Self {
            authed,
            active_tab: Tab::PullRequests,
            modal: None,
            should_quit: false,
            prs: Vec::new(),
            selected: 0,
            issues: Vec::new(),
            issue_selected: 0,
            issues_loaded: false,
            issues_disabled: false,
            issue_detail: None,
            pipelines: Vec::new(),
            pipeline_selected: 0,
            pipelines_loaded: false,
            pipeline_detail: None,
            filter_query: String::new(),
            filtering: false,
            loading: Vec::new(),
            status: None,
            detail_open: false,
            detail: None,
            my_approved: HashSet::new(),
            spinner: 0,
        }
    }

    /// The PR an action (approve/merge/decline/comment) targets — only on the PR
    /// section (those actions don't apply to issues).
    #[must_use]
    pub fn action_target_id(&self) -> Option<u64> {
        if self.active_tab != Tab::PullRequests {
            return None;
        }
        if self.detail_open {
            self.detail_pr_id()
        } else {
            self.selected_pr_id()
        }
    }

    /// Length of the active section's **visible** (filtered) list.
    fn active_len(&self) -> usize {
        self.visible_indices().len()
    }

    /// Indices into the active section's list that match the fuzzy filter (all
    /// indices when the filter is empty), in list order.
    fn visible_indices(&self) -> Vec<usize> {
        let q = self.filter_query.to_lowercase();
        match self.active_tab {
            Tab::PullRequests => self
                .prs
                .iter()
                .enumerate()
                .filter(|(_, p)| fuzzy_match(&pr_haystack(p), &q))
                .map(|(i, _)| i)
                .collect(),
            Tab::Issues => self
                .issues
                .iter()
                .enumerate()
                .filter(|(_, i)| fuzzy_match(&issue_haystack(i), &q))
                .map(|(i, _)| i)
                .collect(),
            Tab::Pipelines => self
                .pipelines
                .iter()
                .enumerate()
                .filter(|(_, p)| fuzzy_match(&pipeline_haystack(p), &q))
                .map(|(i, _)| i)
                .collect(),
        }
    }

    /// The highlighted PR, mapped through the visible filter (PR section only).
    fn active_pr(&self) -> Option<&PullRequest> {
        if self.active_tab != Tab::PullRequests {
            return None;
        }
        self.visible_indices()
            .get(self.selected)
            .and_then(|&i| self.prs.get(i))
    }

    fn active_issue(&self) -> Option<&Issue> {
        if self.active_tab != Tab::Issues {
            return None;
        }
        self.visible_indices()
            .get(self.issue_selected)
            .and_then(|&i| self.issues.get(i))
    }

    fn active_pipeline(&self) -> Option<&Pipeline> {
        if self.active_tab != Tab::Pipelines {
            return None;
        }
        self.visible_indices()
            .get(self.pipeline_selected)
            .and_then(|&i| self.pipelines.get(i))
    }

    /// The active section's selection index.
    fn active_sel(&self) -> usize {
        match self.active_tab {
            Tab::PullRequests => self.selected,
            Tab::Issues => self.issue_selected,
            Tab::Pipelines => self.pipeline_selected,
        }
    }

    /// Set the active section's selection (clamped to its list).
    fn set_active_sel(&mut self, value: usize) {
        let clamped = value.min(self.active_len().saturating_sub(1));
        match self.active_tab {
            Tab::PullRequests => self.selected = clamped,
            Tab::Issues => self.issue_selected = clamped,
            Tab::Pipelines => self.pipeline_selected = clamped,
        }
    }

    /// Whether the pipeline list still needs its initial (lazy) load.
    #[must_use]
    pub fn needs_pipeline_load(&self) -> bool {
        self.active_tab == Tab::Pipelines
            && self.authed
            && !self.pipelines_loaded
            && !self.loading.contains(&RequestKind::Pipelines)
    }

    /// Whether any visible pipeline is still running (drives auto-refresh polling).
    #[must_use]
    pub fn pipelines_active(&self) -> bool {
        self.active_tab == Tab::Pipelines
            && self
                .pipelines
                .iter()
                .any(|p| p.state_name() != "COMPLETED" && !p.state_name().is_empty())
    }

    /// The build number of the highlighted pipeline, if the Pipelines section is active.
    #[must_use]
    pub fn selected_pipeline_build(&self) -> Option<u64> {
        self.active_pipeline().and_then(|p| p.build_number)
    }

    /// Whether the issue list still needs its initial (lazy) load.
    #[must_use]
    pub fn needs_issue_load(&self) -> bool {
        self.active_tab == Tab::Issues
            && self.authed
            && !self.issues_loaded
            && !self.loading.contains(&RequestKind::Issues)
    }

    /// The id of the highlighted issue, if the Issues section is active.
    #[must_use]
    pub fn selected_issue_id(&self) -> Option<u64> {
        self.active_issue().map(|i| i.id)
    }

    /// The id of the PR shown in the detail pane, if open.
    #[must_use]
    pub fn detail_pr_id(&self) -> Option<u64> {
        self.detail.as_ref().map(|d| d.pr.id)
    }

    /// Optimistically flip your approval for `id`, returning the new state
    /// (`true` = now approved → send Approve; `false` = send Unapprove).
    pub fn toggle_self_approved(&mut self, id: u64) -> bool {
        if self.my_approved.remove(&id) {
            false
        } else {
            self.my_approved.insert(id);
            true
        }
    }

    /// Take the pending confirm action and close the modal (loop dispatches it).
    pub fn take_pending_confirm(&mut self) -> Option<PendingAction> {
        if let Some(Modal::Confirm { action, .. }) = &self.modal {
            let action = *action;
            self.modal = None;
            Some(action)
        } else {
            None
        }
    }

    /// Take the composed comment `(id, body)` and close the modal.
    pub fn take_comment(&mut self) -> Option<(u64, String)> {
        if let Some(Modal::Comment { id, buffer }) = &self.modal {
            let out = (*id, buffer.clone());
            self.modal = None;
            Some(out)
        } else {
            None
        }
    }

    /// How the next key should be interpreted (modal- and filter-aware).
    #[must_use]
    pub fn input_context(&self) -> InputContext {
        match &self.modal {
            Some(Modal::Help) => InputContext::Help,
            Some(Modal::Confirm { .. }) => InputContext::Confirm,
            Some(Modal::Comment { .. }) => InputContext::Comment,
            None if self.filtering => InputContext::Filter,
            None => InputContext::Normal,
        }
    }

    /// Move the active section's selection by `delta` rows, clamped to its list.
    fn move_selection(&mut self, delta: isize) {
        if self.active_len() == 0 {
            return;
        }
        let max = self.active_len() as isize - 1;
        let next = (self.active_sel() as isize)
            .saturating_add(delta)
            .clamp(0, max);
        self.set_active_sel(next as usize);
    }

    /// Scroll the detail body by `delta` lines, clamped to its content.
    fn scroll_detail(&mut self, delta: isize) {
        if let Some(d) = &mut self.detail {
            let max = i32::from(d.max_scroll);
            d.scroll = i32::from(d.scroll)
                .saturating_add(delta as i32)
                .clamp(0, max) as u16;
        }
    }

    /// The id of the highlighted PR, if the PR section is active and non-empty.
    #[must_use]
    pub fn selected_pr_id(&self) -> Option<u64> {
        self.active_pr().map(|p| p.id)
    }

    /// The web URL to open for `o`: the open detail's PR/issue, else the active
    /// section's selected row.
    #[must_use]
    pub fn current_url(&self) -> Option<&str> {
        if self.detail_open {
            return match self.active_tab {
                Tab::Issues => self.issue_detail.as_ref().and_then(Issue::html_url),
                _ => self.detail.as_ref().and_then(|d| d.pr.html_url()),
            };
        }
        match self.active_tab {
            Tab::PullRequests => self.active_pr().and_then(PullRequest::html_url),
            Tab::Issues => self.active_issue().and_then(Issue::html_url),
            Tab::Pipelines => None,
        }
    }

    /// Mark a request kind as in flight (UI sent a [`Request`](super::worker::Request)).
    pub fn begin(&mut self, kind: RequestKind) {
        if !self.loading.contains(&kind) {
            self.loading.push(kind);
        }
    }

    fn done(&mut self, kind: RequestKind) {
        self.loading.retain(|k| *k != kind);
    }

    /// Fold a worker [`Response`] into the model.
    pub fn apply_response(&mut self, response: Response) {
        match response {
            Response::Prs(prs) => {
                // Persist the selection across a refresh by PR id when possible.
                let prev_id = self.prs.get(self.selected).map(|p| p.id);
                self.prs = prs;
                self.selected = prev_id
                    .and_then(|id| self.prs.iter().position(|p| p.id == id))
                    .unwrap_or(0)
                    .min(self.prs.len().saturating_sub(1));
                self.done(RequestKind::Prs);
            }
            Response::PrDetail { pr, checks } => {
                if self.detail_open {
                    // Approximate scroll bound by the description's line count; the
                    // paragraph wraps, so this is a floor that still lets long
                    // bodies scroll (a real viewport bound can refine it later).
                    let max_scroll =
                        u16::try_from(pr.body().unwrap_or("").lines().count()).unwrap_or(u16::MAX);
                    self.detail = Some(Detail {
                        pr: *pr,
                        checks,
                        scroll: 0,
                        max_scroll,
                    });
                }
                self.done(RequestKind::PrDetail);
            }
            Response::Issues(issues) => {
                let prev_id = self.issues.get(self.issue_selected).map(|i| i.id);
                self.issues = issues;
                self.issue_selected = prev_id
                    .and_then(|id| self.issues.iter().position(|i| i.id == id))
                    .unwrap_or(0)
                    .min(self.issues.len().saturating_sub(1));
                self.issues_loaded = true;
                self.issues_disabled = false;
                self.done(RequestKind::Issues);
            }
            Response::IssueDetail(issue) => {
                if self.detail_open {
                    self.issue_detail = Some(*issue);
                }
                self.done(RequestKind::IssueDetail);
            }
            Response::IssuesDisabled => {
                self.issues_disabled = true;
                self.issues_loaded = true;
                self.detail_open = false;
                self.issue_detail = None;
                self.done(RequestKind::Issues);
                self.done(RequestKind::IssueDetail);
            }
            Response::Pipelines(pipelines) => {
                let prev = self
                    .pipelines
                    .get(self.pipeline_selected)
                    .and_then(|p| p.uuid.clone());
                self.pipelines = pipelines;
                self.pipeline_selected = prev
                    .and_then(|uuid| {
                        self.pipelines
                            .iter()
                            .position(|p| p.uuid == Some(uuid.clone()))
                    })
                    .unwrap_or(0)
                    .min(self.pipelines.len().saturating_sub(1));
                self.pipelines_loaded = true;
                self.done(RequestKind::Pipelines);
            }
            Response::PipelineDetail { pipeline, steps } => {
                if self.detail_open {
                    self.pipeline_detail = Some((*pipeline, steps));
                }
                self.done(RequestKind::PipelineDetail);
            }
            Response::ActionDone(message) => {
                self.done(RequestKind::Action);
                self.status = Some(message);
            }
            Response::Error(message, kind) => {
                self.done(kind);
                self.status = Some(message);
            }
        }
    }

    /// Whether any request is in flight.
    #[must_use]
    pub fn is_loading(&self) -> bool {
        !self.loading.is_empty()
    }

    /// Apply a semantic message to the model.
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Quit => self.should_quit = true,
            Msg::Pop => {
                // Precedence: a modal closes first, then the detail pane, then quit.
                if self.modal.take().is_some() {
                } else if self.detail_open {
                    self.detail_open = false;
                    self.detail = None;
                    self.issue_detail = None;
                    self.pipeline_detail = None;
                    self.done(RequestKind::PrDetail);
                    self.done(RequestKind::IssueDetail);
                    self.done(RequestKind::PipelineDetail);
                } else {
                    self.should_quit = true;
                }
            }
            Msg::Open => match self.active_tab {
                Tab::PullRequests if !self.prs.is_empty() => {
                    self.detail_open = true;
                    self.detail = None;
                    self.begin(RequestKind::PrDetail);
                }
                Tab::Issues if !self.issues.is_empty() => {
                    self.detail_open = true;
                    self.issue_detail = None;
                    self.begin(RequestKind::IssueDetail);
                }
                Tab::Pipelines if !self.pipelines.is_empty() => {
                    self.detail_open = true;
                    self.pipeline_detail = None;
                    self.begin(RequestKind::PipelineDetail);
                }
                _ => {}
            },
            Msg::ToggleHelp => {
                self.modal = if matches!(self.modal, Some(Modal::Help)) {
                    None
                } else {
                    Some(Modal::Help)
                };
            }
            Msg::Merge => {
                if let Some(id) = self.action_target_id() {
                    self.modal = Some(Modal::Confirm {
                        action: PendingAction::Merge(id),
                        prompt: format!("Merge PR #{id}? (y/N)"),
                    });
                }
            }
            Msg::Decline => {
                if let Some(id) = self.action_target_id() {
                    self.modal = Some(Modal::Confirm {
                        action: PendingAction::Decline(id),
                        prompt: format!("Decline PR #{id}? (y/N)"),
                    });
                }
            }
            Msg::Comment => {
                if let Some(id) = self.action_target_id() {
                    self.modal = Some(Modal::Comment {
                        id,
                        buffer: String::new(),
                    });
                }
            }
            Msg::InsertChar(c) => {
                if let Some(Modal::Comment { buffer, .. }) = &mut self.modal {
                    buffer.push(c);
                }
            }
            Msg::Backspace => {
                if let Some(Modal::Comment { buffer, .. }) = &mut self.modal {
                    buffer.pop();
                }
            }
            Msg::StartFilter => {
                self.filtering = true;
                self.filter_query.clear();
                self.set_active_sel(0);
            }
            Msg::FilterChar(c) => {
                self.filter_query.push(c);
                self.set_active_sel(0);
            }
            Msg::FilterBackspace => {
                self.filter_query.pop();
                self.set_active_sel(0);
            }
            Msg::ApplyFilter => self.filtering = false,
            Msg::ClearFilter => {
                self.filter_query.clear();
                self.filtering = false;
                self.set_active_sel(0);
            }
            // Acted on by the event loop (worker / Browser seam).
            Msg::Approve | Msg::OpenBrowser | Msg::ConfirmYes | Msg::Submit | Msg::Refresh => {}
            Msg::SelectTab(n) => {
                if let Some(tab) = Tab::ALL.get(n) {
                    self.active_tab = *tab;
                }
            }
            Msg::NextTab => {
                let next = (self.active_tab.index() + 1) % Tab::ALL.len();
                self.active_tab = Tab::ALL[next];
            }
            Msg::PrevTab => {
                let len = Tab::ALL.len();
                let prev = (self.active_tab.index() + len - 1) % len;
                self.active_tab = Tab::ALL[prev];
            }
            Msg::Tick => {
                if self.is_loading() {
                    self.spinner = (self.spinner + 1) % SPINNER.len();
                }
            }
            // Motions scroll the detail body when it's open, else move the list.
            Msg::Down if self.detail_open => self.scroll_detail(1),
            Msg::Up if self.detail_open => self.scroll_detail(-1),
            Msg::Top if self.detail_open => {
                if let Some(d) = &mut self.detail {
                    d.scroll = 0;
                }
            }
            Msg::Bottom if self.detail_open => {
                if let Some(d) = &mut self.detail {
                    d.scroll = d.max_scroll;
                }
            }
            Msg::HalfPageDown if self.detail_open => self.scroll_detail(HALF_PAGE),
            Msg::HalfPageUp if self.detail_open => self.scroll_detail(-HALF_PAGE),
            Msg::Down => self.move_selection(1),
            Msg::Up => self.move_selection(-1),
            Msg::Top => self.set_active_sel(0),
            Msg::Bottom => self.set_active_sel(self.active_len().saturating_sub(1)),
            Msg::HalfPageDown => self.move_selection(HALF_PAGE),
            Msg::HalfPageUp => self.move_selection(-HALF_PAGE),
        }
    }

    /// Render the current frame: a tab bar, the active section's placeholder, a
    /// footer hint, and the help overlay when open.
    pub fn view(&self, frame: &mut Frame) {
        let chunks = Layout::vertical([
            Constraint::Length(1), // tab bar
            Constraint::Min(1),    // body
            Constraint::Length(1), // footer
        ])
        .split(frame.area());

        frame.render_widget(self.tab_bar(), chunks[0]);
        self.render_body(frame, chunks[1]);
        let footer = if self.filtering || !self.filter_query.is_empty() {
            let cursor = if self.filtering { "_" } else { "" };
            Line::from(Span::styled(
                format!("/{}{cursor}", self.filter_query),
                Style::default().fg(Color::Cyan),
            ))
        } else if let Some(status) = &self.status {
            Line::from(Span::styled(
                status.clone(),
                Style::default().fg(Color::Yellow),
            ))
        } else {
            Line::from(Span::raw(
                "j/k move · / filter · ↵ open · a approve · m merge · x decline · C comment · r refresh · ? help · q quit",
            ))
        };
        frame.render_widget(Paragraph::new(footer), chunks[2]);

        match &self.modal {
            Some(Modal::Help) => self.render_help(frame),
            Some(Modal::Confirm { prompt, .. }) => render_modal(frame, "Confirm", prompt, 50, 20),
            Some(Modal::Comment { id, buffer }) => {
                let body = format!("{buffer}\n\n(Enter to submit · Esc to cancel)");
                render_modal(frame, &format!("Comment on PR #{id}"), &body, 70, 40);
            }
            None => {}
        }
    }

    fn tab_bar(&self) -> Line<'static> {
        let mut spans = Vec::new();
        for (i, tab) in Tab::ALL.iter().enumerate() {
            let label = format!(" {}:{} ", i + 1, tab.title());
            let style = if *tab == self.active_tab {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::default()
            };
            spans.push(Span::styled(label, style));
        }
        Line::from(spans)
    }

    fn render_body(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        if self.detail_open {
            self.render_detail(frame, area);
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.active_tab.title());

        // A populated section renders its table; otherwise a centered message
        // inside the same bordered block. (Toasts/errors live in the footer so
        // they never replace the list.)
        if !self.authed {
            let p = Paragraph::new("Not logged in — run `bb auth login`").block(block);
            frame.render_widget(p, area);
            return;
        }
        let visible = self.visible_indices();
        let empty_msg = |base: &str| {
            if self.filter_query.is_empty() {
                base.to_owned()
            } else {
                "No matches".to_owned()
            }
        };
        let message = match self.active_tab {
            Tab::PullRequests => {
                if self.is_loading() && self.prs.is_empty() {
                    format!("{} Loading pull requests…", SPINNER[self.spinner])
                } else if visible.is_empty() {
                    empty_msg("No pull requests")
                } else {
                    return self.render_pr_table(frame, area, block, &visible);
                }
            }
            Tab::Issues => {
                if self.issues_disabled {
                    "Issue tracker not enabled for this repository".to_owned()
                } else if self.is_loading() && self.issues.is_empty() {
                    format!("{} Loading issues…", SPINNER[self.spinner])
                } else if visible.is_empty() {
                    empty_msg("No issues")
                } else {
                    return self.render_issue_table(frame, area, block, &visible);
                }
            }
            Tab::Pipelines => {
                if self.is_loading() && self.pipelines.is_empty() {
                    format!("{} Loading pipelines…", SPINNER[self.spinner])
                } else if visible.is_empty() {
                    empty_msg("No pipelines")
                } else {
                    return self.render_pipeline_table(frame, area, block, &visible);
                }
            }
        };
        frame.render_widget(Paragraph::new(message).block(block), area);
    }

    fn render_pipeline_table(
        &self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        block: Block,
        visible: &[usize],
    ) {
        use ratatui::widgets::{Cell, HighlightSpacing, Row, Table, TableState};

        let header = Row::new(["#", "STATE", "RESULT", "REF", "CREATED"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let rows = visible.iter().map(|&i| {
            let p = &self.pipelines[i];
            let build = p
                .build_number
                .map_or_else(|| "?".to_owned(), |n| format!("#{n}"));
            let ref_name = p
                .target
                .as_ref()
                .and_then(|t| t.ref_name.as_deref())
                .unwrap_or_default();
            let created = p.created_on.as_deref().unwrap_or_default();
            Row::new([
                Cell::from(build),
                Cell::from(state_cell(p.state_name())),
                Cell::from(state_cell(p.result_name())),
                Cell::from(sanitize(ref_name)),
                Cell::from(created.get(0..10).unwrap_or(created).to_owned()),
            ])
        });
        let widths = [
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Length(11),
            Constraint::Fill(1),
            Constraint::Length(10),
        ];
        let table = Table::new(rows, widths)
            .header(header)
            .block(block)
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_spacing(HighlightSpacing::Always);
        let mut state = TableState::default().with_selected(Some(self.pipeline_selected));
        frame.render_stateful_widget(table, area, &mut state);
    }

    fn render_issue_table(
        &self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        block: Block,
        visible: &[usize],
    ) {
        use ratatui::widgets::{Cell, HighlightSpacing, Row, Table, TableState};

        let header = Row::new(["#", "TITLE", "KIND", "PRIORITY", "STATE"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let rows = visible.iter().map(|&idx| {
            let i = &self.issues[idx];
            Row::new([
                Cell::from(format!("#{}", i.id)),
                Cell::from(sanitize(i.title.as_deref().unwrap_or_default())),
                Cell::from(i.kind.clone().unwrap_or_default()),
                Cell::from(i.priority.clone().unwrap_or_default()),
                Cell::from(i.state.clone().unwrap_or_default()),
            ])
        });
        let widths = [
            Constraint::Length(6),
            Constraint::Fill(1),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(12),
        ];
        let table = Table::new(rows, widths)
            .header(header)
            .block(block)
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_spacing(HighlightSpacing::Always);
        let mut state = TableState::default().with_selected(Some(self.issue_selected));
        frame.render_stateful_widget(table, area, &mut state);
    }

    fn render_detail(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        if self.active_tab == Tab::Issues {
            self.render_issue_detail(frame, area);
            return;
        }
        if self.active_tab == Tab::Pipelines {
            self.render_pipeline_detail(frame, area);
            return;
        }
        let Some(d) = &self.detail else {
            let msg = format!("{} Loading pull request…", SPINNER[self.spinner]);
            let block = Block::default().borders(Borders::ALL).title("Pull Request");
            frame.render_widget(Paragraph::new(msg).block(block), area);
            return;
        };

        let title = format!(
            "#{} {}",
            d.pr.id,
            sanitize(d.pr.title.as_deref().unwrap_or_default())
        );

        let approvals = d.pr.approvals();
        let approved_by: Vec<String> = approvals.iter().map(|u| u.label()).collect();
        let reviewers: Vec<Span> =
            d.pr.reviewers
                .iter()
                .map(|r| {
                    let approved = approvals
                        .iter()
                        .any(|a| a.uuid == r.uuid && r.uuid.is_some());
                    let mark = if approved { "✔" } else { "⧗" };
                    Span::raw(format!("{mark} {}  ", r.label()))
                })
                .collect();

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(vec![
            Span::raw("State: "),
            state_cell(d.pr.state.as_deref().unwrap_or_default()),
        ]));
        if let Some(author) = &d.pr.author {
            lines.push(Line::raw(format!("Author: {}", author.label())));
        }
        lines.push(Line::raw(format!(
            "Branch: {} → {}",
            sanitize(d.pr.source.branch_name()),
            sanitize(d.pr.destination.branch_name()),
        )));
        if !reviewers.is_empty() {
            let mut spans = vec![Span::raw("Reviewers: ")];
            spans.extend(reviewers);
            lines.push(Line::from(spans));
        } else if !approved_by.is_empty() {
            lines.push(Line::raw(format!(
                "Approved by: {}",
                approved_by.join(", ")
            )));
        }
        lines.push(Line::raw(""));
        for raw in d.pr.body().unwrap_or("(no description)").lines() {
            lines.push(Line::raw(sanitize(raw)));
        }
        lines.push(Line::raw(""));
        if self.is_loading() {
            lines.push(Line::from(vec![Span::raw(format!(
                "Checks: {} loading…",
                SPINNER[self.spinner]
            ))]));
        } else if d.checks.is_empty() {
            lines.push(Line::raw("Checks: none reported"));
        } else {
            lines.push(Line::raw("Checks:"));
            for c in &d.checks {
                let name = c.name.as_deref().or(c.key.as_deref()).unwrap_or("check");
                lines.push(Line::from(vec![
                    Span::raw(format!("  {name}  ")),
                    state_cell(c.state.as_deref().unwrap_or_default()),
                ]));
            }
        }

        let block = Block::default().borders(Borders::ALL).title(title);
        let para = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((d.scroll, 0));
        frame.render_widget(para, area);
    }

    fn render_pipeline_detail(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let Some((p, steps)) = &self.pipeline_detail else {
            let msg = format!("{} Loading pipeline…", SPINNER[self.spinner]);
            let block = Block::default().borders(Borders::ALL).title("Pipeline");
            frame.render_widget(Paragraph::new(msg).block(block), area);
            return;
        };

        let build = p
            .build_number
            .map_or_else(|| "?".to_owned(), |n| format!("#{n}"));
        let mut lines: Vec<Line> = vec![Line::from(vec![
            Span::raw("State: "),
            state_cell(p.state_name()),
            Span::raw("  "),
            state_cell(p.result_name()),
        ])];
        if let Some(ref_name) = p.target.as_ref().and_then(|t| t.ref_name.as_deref()) {
            lines.push(Line::raw(format!("Ref: {}", sanitize(ref_name))));
        }
        lines.push(Line::raw(""));
        lines.push(Line::raw("Steps:"));
        if steps.is_empty() {
            lines.push(Line::raw("  (none)"));
        } else {
            for step in steps {
                let name = step.name.as_deref().unwrap_or("step");
                let state = step
                    .state
                    .as_ref()
                    .and_then(|s| s.name.as_deref())
                    .unwrap_or_default();
                lines.push(Line::from(vec![
                    Span::raw(format!("  {name}  ")),
                    state_cell(state),
                ]));
            }
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!("Pipeline {build}"));
        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn render_issue_detail(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let Some(i) = &self.issue_detail else {
            let msg = format!("{} Loading issue…", SPINNER[self.spinner]);
            let block = Block::default().borders(Borders::ALL).title("Issue");
            frame.render_widget(Paragraph::new(msg).block(block), area);
            return;
        };

        let title = format!(
            "#{} {}",
            i.id,
            sanitize(i.title.as_deref().unwrap_or_default())
        );
        let mut lines: Vec<Line> = vec![
            Line::from(vec![
                Span::raw("State: "),
                state_cell(i.state.as_deref().unwrap_or_default()),
            ]),
            Line::raw(format!(
                "Kind: {}    Priority: {}",
                i.kind.as_deref().unwrap_or("—"),
                i.priority.as_deref().unwrap_or("—"),
            )),
        ];
        if let Some(reporter) = &i.reporter {
            lines.push(Line::raw(format!("Reporter: {}", reporter.label())));
        }
        lines.push(Line::raw(""));
        let body = i
            .content
            .as_ref()
            .and_then(|c| c.raw.as_deref())
            .unwrap_or("(no description)");
        for raw in body.lines() {
            lines.push(Line::raw(sanitize(raw)));
        }

        let block = Block::default().borders(Borders::ALL).title(title);
        let para = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(para, area);
    }

    fn render_pr_table(
        &self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        block: Block,
        visible: &[usize],
    ) {
        use ratatui::widgets::{Cell, HighlightSpacing, Row, Table, TableState};

        let header = Row::new(["#", "TITLE", "BRANCH", "STATE"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let rows = visible.iter().map(|&i| {
            let pr = &self.prs[i];
            Row::new([
                Cell::from(format!("#{}", pr.id)),
                Cell::from(sanitize(pr.title.as_deref().unwrap_or_default())),
                Cell::from(sanitize(pr.source.branch_name())),
                Cell::from(state_cell(pr.state.as_deref().unwrap_or_default())),
            ])
        });
        let widths = [
            Constraint::Length(6),
            Constraint::Fill(1),
            Constraint::Length(24),
            Constraint::Length(10),
        ];
        let table = Table::new(rows, widths)
            .header(header)
            .block(block)
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_spacing(HighlightSpacing::Always);

        let mut state = TableState::default().with_selected(Some(self.selected));
        frame.render_stateful_widget(table, area, &mut state);
    }

    fn render_help(&self, frame: &mut Frame) {
        let area = centered(frame.area(), 60, 40);
        let help = Paragraph::new(vec![
            Line::from("Keys"),
            Line::from("  j / k        move down / up"),
            Line::from("  g / G        top / bottom"),
            Line::from("  Ctrl-d / -u  half page"),
            Line::from("  1 / 2 / 3    sections"),
            Line::from("  Tab          next section"),
            Line::from("  ?            toggle this help"),
            Line::from("  Esc          back / quit"),
            Line::from("  q            quit"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Help"));
        frame.render_widget(Clear, area);
        frame.render_widget(help, area);
    }
}

/// Case-insensitive subsequence match (`query`'s chars appear in order in
/// `haystack`). An empty query matches everything.
fn fuzzy_match(haystack: &str, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    let mut needles = query_lower.chars().peekable();
    for h in haystack.to_lowercase().chars() {
        match needles.peek() {
            Some(&n) if n == h => {
                needles.next();
            }
            Some(_) => {}
            None => break,
        }
    }
    needles.peek().is_none()
}

fn pr_haystack(pr: &PullRequest) -> String {
    format!(
        "{} {} {}",
        pr.title.as_deref().unwrap_or(""),
        pr.source.branch_name(),
        pr.author.as_ref().map_or(String::new(), User::label),
    )
}

fn issue_haystack(issue: &Issue) -> String {
    format!(
        "{} {} {}",
        issue.title.as_deref().unwrap_or(""),
        issue.kind.as_deref().unwrap_or(""),
        issue.priority.as_deref().unwrap_or(""),
    )
}

fn pipeline_haystack(p: &Pipeline) -> String {
    format!(
        "{} {} {}",
        p.build_number.map_or(String::new(), |n| n.to_string()),
        p.state_name(),
        p.target
            .as_ref()
            .and_then(|t| t.ref_name.as_deref())
            .unwrap_or(""),
    )
}

/// A PR/CI state rendered as a colored span.
fn state_cell(state: &str) -> Span<'static> {
    let color = match state {
        "OPEN" | "SUCCESSFUL" => Color::Green,
        "MERGED" => Color::Cyan,
        "INPROGRESS" => Color::Yellow,
        "DECLINED" | "SUPERSEDED" | "FAILED" | "STOPPED" => Color::Red,
        _ => Color::Gray,
    };
    Span::styled(state.to_owned(), Style::default().fg(color))
}

/// Render a centered modal box (`Clear`ed background) with a title and body.
fn render_modal(frame: &mut Frame, title: &str, body: &str, pct_x: u16, pct_y: u16) {
    let area = centered(frame.area(), pct_x, pct_y);
    let para = Paragraph::new(body.to_owned())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title.to_owned()),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(para, area);
}

/// A rectangle `pct_x`% × `pct_y`% of `area`, centered.
fn centered(area: ratatui::layout::Rect, pct_x: u16, pct_y: u16) -> ratatui::layout::Rect {
    let v = Layout::vertical([
        Constraint::Percentage((100 - pct_y) / 2),
        Constraint::Percentage(pct_y),
        Constraint::Percentage((100 - pct_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - pct_x) / 2),
        Constraint::Percentage(pct_x),
        Constraint::Percentage((100 - pct_x) / 2),
    ])
    .split(v[1])[1]
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    #[test]
    fn quit_sets_should_quit() {
        let mut app = App::new(true);
        app.update(Msg::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn esc_at_root_quits_but_pops_a_modal_first() {
        let mut app = App::new(true);
        app.update(Msg::ToggleHelp);
        assert_eq!(app.modal, Some(Modal::Help));
        // Esc in a modal pops it, does NOT quit.
        app.update(Msg::Pop);
        assert_eq!(app.modal, None);
        assert!(!app.should_quit);
        // Esc at root quits.
        app.update(Msg::Pop);
        assert!(app.should_quit);
    }

    #[test]
    fn toggle_help_is_idempotent_pair() {
        let mut app = App::new(true);
        app.update(Msg::ToggleHelp);
        assert_eq!(app.modal, Some(Modal::Help));
        app.update(Msg::ToggleHelp);
        assert_eq!(app.modal, None);
    }

    #[test]
    fn tab_selection_and_cycling() {
        let mut app = App::new(true);
        app.update(Msg::SelectTab(2));
        assert_eq!(app.active_tab, Tab::Pipelines);
        app.update(Msg::NextTab); // wraps to PullRequests
        assert_eq!(app.active_tab, Tab::PullRequests);
        app.update(Msg::PrevTab); // wraps to Pipelines
        assert_eq!(app.active_tab, Tab::Pipelines);
        // Out-of-range tab index is ignored.
        app.update(Msg::SelectTab(9));
        assert_eq!(app.active_tab, Tab::Pipelines);
    }

    #[test]
    fn response_prs_populates_rows_and_clears_loading() {
        let mut app = App::new(true);
        app.begin(RequestKind::Prs);
        assert!(app.is_loading());
        let prs: Vec<PullRequest> =
            serde_json::from_str(r#"[{"id":7,"title":"T","state":"OPEN"}]"#).unwrap();
        app.apply_response(Response::Prs(prs));
        assert_eq!(app.prs.len(), 1);
        assert_eq!(app.prs[0].id, 7);
        assert!(!app.is_loading());
    }

    #[test]
    fn response_error_sets_status_and_clears_loading() {
        let mut app = App::new(true);
        app.begin(RequestKind::Prs);
        app.apply_response(Response::Error("boom".to_owned(), RequestKind::Prs));
        assert_eq!(app.status.as_deref(), Some("boom"));
        assert!(!app.is_loading());
    }

    #[test]
    fn tick_advances_spinner_only_while_loading() {
        let mut app = App::new(true);
        let before = app.spinner;
        app.update(Msg::Tick);
        assert_eq!(app.spinner, before, "idle tick must not advance");
        app.begin(RequestKind::Prs);
        app.update(Msg::Tick);
        assert_ne!(app.spinner, before, "loading tick advances the spinner");
    }

    fn prs(n: u64) -> Vec<PullRequest> {
        (1..=n)
            .map(|i| {
                serde_json::from_str(&format!(
                    r#"{{"id":{i},"title":"PR {i}","state":"OPEN",
                        "source":{{"branch":{{"name":"feat/{i}"}}}}}}"#
                ))
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn selection_moves_and_clamps_at_bounds() {
        let mut app = App::new(true);
        app.apply_response(Response::Prs(prs(3)));
        assert_eq!(app.selected, 0);
        app.update(Msg::Up); // clamps at 0
        assert_eq!(app.selected, 0);
        app.update(Msg::Down);
        app.update(Msg::Down);
        assert_eq!(app.selected, 2);
        app.update(Msg::Down); // clamps at last
        assert_eq!(app.selected, 2);
        app.update(Msg::Top);
        assert_eq!(app.selected, 0);
        app.update(Msg::Bottom);
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn refresh_preserves_selection_by_id() {
        let mut app = App::new(true);
        app.apply_response(Response::Prs(prs(3)));
        app.update(Msg::Bottom); // select id 3 (index 2)
        assert_eq!(app.prs[app.selected].id, 3);
        // A refresh returns the same set in a different order; selection follows id 3.
        let mut reordered = prs(3);
        reordered.reverse();
        app.apply_response(Response::Prs(reordered));
        assert_eq!(app.prs[app.selected].id, 3);
    }

    fn detail_response(id: u64) -> Response {
        let pr = serde_json::from_str(&format!(
            r#"{{"id":{id},"title":"Detail {id}","state":"OPEN",
                "description":"line one\nline two",
                "source":{{"branch":{{"name":"feat/{id}"}}}},
                "destination":{{"branch":{{"name":"main"}}}},
                "author":{{"display_name":"Dev"}}}}"#
        ))
        .unwrap();
        Response::PrDetail {
            pr: Box::new(pr),
            checks: Vec::new(),
        }
    }

    #[test]
    fn open_then_esc_round_trips_preserving_selection() {
        let mut app = App::new(true);
        app.apply_response(Response::Prs(prs(3)));
        app.update(Msg::Down); // select index 1
        assert_eq!(app.selected, 1);
        app.update(Msg::Open);
        assert!(app.detail_open && app.is_loading());
        app.apply_response(detail_response(2));
        assert!(app.detail_open && !app.is_loading());
        // Esc closes the detail (not quit) and the list selection is intact.
        app.update(Msg::Pop);
        assert!(!app.detail_open);
        assert!(!app.should_quit);
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn detail_body_scroll_clamps() {
        let mut app = App::new(true);
        app.apply_response(Response::Prs(prs(1)));
        app.update(Msg::Open);
        app.apply_response(detail_response(1)); // 2 description lines → max_scroll 2
        app.update(Msg::Up); // clamps at 0
                             // Drive well past the end; scroll must clamp, not overflow.
        for _ in 0..20 {
            app.update(Msg::Down);
        }
        let scroll = app.detail.as_ref().unwrap().scroll;
        let max = app.detail.as_ref().unwrap().max_scroll;
        assert_eq!(scroll, max, "scroll should clamp to max_scroll");
    }

    #[test]
    fn detail_view_renders_title_and_branches() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(true);
        app.apply_response(Response::Prs(prs(1)));
        app.update(Msg::Open);
        app.apply_response(detail_response(1));
        terminal.draw(|f| app.view(f)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(text.contains("Detail 1"), "buffer: {text}");
        assert!(
            text.contains("feat/1") && text.contains("main"),
            "buffer: {text}"
        );
        assert!(text.contains("Dev"), "buffer: {text}");
    }

    #[test]
    fn merge_opens_confirm_and_does_not_act_until_confirmed() {
        let mut app = App::new(true);
        app.apply_response(Response::Prs(prs(1)));
        app.update(Msg::Merge);
        // Only a confirm modal is opened — no request can have been dispatched.
        assert!(matches!(
            app.modal,
            Some(Modal::Confirm {
                action: PendingAction::Merge(1),
                ..
            })
        ));
        // Esc cancels without acting.
        app.update(Msg::Pop);
        assert!(app.modal.is_none() && !app.should_quit);
        // Re-open and "confirm": take_pending_confirm yields the action + closes.
        app.update(Msg::Merge);
        assert_eq!(app.take_pending_confirm(), Some(PendingAction::Merge(1)));
        assert!(app.modal.is_none());
    }

    #[test]
    fn comment_modal_captures_input_and_yields_body() {
        let mut app = App::new(true);
        app.apply_response(Response::Prs(prs(1)));
        app.update(Msg::Comment);
        app.update(Msg::InsertChar('h'));
        app.update(Msg::InsertChar('i'));
        app.update(Msg::Backspace);
        app.update(Msg::InsertChar('o'));
        assert_eq!(app.take_comment(), Some((1, "ho".to_owned())));
        assert!(app.modal.is_none());
    }

    #[test]
    fn approve_toggle_flips() {
        let mut app = App::new(true);
        assert!(app.toggle_self_approved(7)); // now approved
        assert!(!app.toggle_self_approved(7)); // now un-approved
    }

    #[test]
    fn input_context_tracks_modal() {
        let mut app = App::new(true);
        assert_eq!(app.input_context(), InputContext::Normal);
        app.apply_response(Response::Prs(prs(1)));
        app.update(Msg::Comment);
        assert_eq!(app.input_context(), InputContext::Comment);
        app.update(Msg::Pop);
        app.update(Msg::Merge);
        assert_eq!(app.input_context(), InputContext::Confirm);
    }

    fn issues(n: u64) -> Vec<Issue> {
        (1..=n)
            .map(|i| {
                serde_json::from_str(&format!(
                    r#"{{"id":{i},"title":"Issue {i}","state":"new","kind":"bug",
                        "priority":"major","content":{{"raw":"body {i}"}}}}"#
                ))
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn per_tab_selection_is_independent() {
        let mut app = App::new(true);
        app.apply_response(Response::Prs(prs(3)));
        app.apply_response(Response::Issues(issues(4)));
        // On the PR tab, motion moves the PR selection.
        app.update(Msg::Down);
        assert_eq!(app.selected, 1);
        assert_eq!(app.issue_selected, 0);
        // Switch to Issues; motion moves the issue selection, PR untouched.
        app.update(Msg::SelectTab(1));
        app.update(Msg::Bottom);
        assert_eq!(app.issue_selected, 3);
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn issues_disabled_sets_flag_and_renders_screen() {
        let mut app = App::new(true);
        app.update(Msg::SelectTab(1));
        app.begin(RequestKind::Issues);
        app.apply_response(Response::IssuesDisabled);
        assert!(app.issues_disabled && app.issues_loaded && !app.is_loading());

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.view(f)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("Issue tracker not enabled"));
    }

    #[test]
    fn issue_detail_opens_and_renders() {
        let mut app = App::new(true);
        app.update(Msg::SelectTab(1));
        app.apply_response(Response::Issues(issues(2)));
        app.update(Msg::Open);
        assert!(app.detail_open && app.is_loading());
        let issue: Issue = serde_json::from_str(
            r#"{"id":1,"title":"Issue 1","state":"new","kind":"bug","content":{"raw":"the body"}}"#,
        )
        .unwrap();
        app.apply_response(Response::IssueDetail(Box::new(issue)));
        assert!(!app.is_loading());

        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.view(f)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(text.contains("Issue 1"), "buffer: {text}");
        assert!(text.contains("the body"), "buffer: {text}");
    }

    #[test]
    fn issue_table_renders_rows() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(true);
        app.update(Msg::SelectTab(1));
        app.apply_response(Response::Issues(issues(2)));
        terminal.draw(|f| app.view(f)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(
            text.contains("#1") && text.contains("Issue 1"),
            "buffer: {text}"
        );
        assert!(text.contains("bug"), "buffer: {text}");
    }

    #[test]
    fn pr_actions_disabled_on_issue_tab() {
        let mut app = App::new(true);
        app.apply_response(Response::Issues(issues(2)));
        app.update(Msg::SelectTab(1));
        // action_target_id is None on the Issues tab (PR actions don't apply).
        assert_eq!(app.action_target_id(), None);
    }

    fn pipelines(states: &[(&str, &str)]) -> Vec<Pipeline> {
        states
            .iter()
            .enumerate()
            .map(|(i, (state, result))| {
                serde_json::from_str(&format!(
                    r#"{{"build_number":{},"uuid":"{{p{i}}}",
                        "state":{{"name":"{state}","result":{{"name":"{result}"}}}},
                        "target":{{"ref_name":"main"}},"created_on":"2026-06-18T00:00:00Z"}}"#,
                    i + 1
                ))
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn pipelines_active_drives_polling() {
        let mut app = App::new(true);
        app.update(Msg::SelectTab(2)); // Pipelines
                                       // A running pipeline → polling needed.
        app.apply_response(Response::Pipelines(pipelines(&[("IN_PROGRESS", "")])));
        assert!(app.pipelines_active());
        // All terminal → polling stops.
        app.apply_response(Response::Pipelines(pipelines(&[(
            "COMPLETED",
            "SUCCESSFUL",
        )])));
        assert!(!app.pipelines_active());
    }

    #[test]
    fn pipeline_table_and_detail_render() {
        let mut app = App::new(true);
        app.update(Msg::SelectTab(2));
        app.apply_response(Response::Pipelines(pipelines(&[("COMPLETED", "FAILED")])));

        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.view(f)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(text.contains("#1"), "buffer: {text}");
        assert!(text.contains("FAILED"), "buffer: {text}");

        // Open detail.
        app.update(Msg::Open);
        let pipeline: Pipeline =
            serde_json::from_str(r#"{"build_number":1,"state":{"name":"COMPLETED"}}"#).unwrap();
        let step: PipelineStep =
            serde_json::from_str(r#"{"name":"Build","state":{"name":"COMPLETED"}}"#).unwrap();
        app.apply_response(Response::PipelineDetail {
            pipeline: Box::new(pipeline),
            steps: vec![step],
        });
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| app.view(f)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(text.contains("Pipeline #1"), "buffer: {text}");
        assert!(text.contains("Build"), "buffer: {text}");
    }

    #[test]
    fn fuzzy_filter_narrows_and_clears() {
        let mut app = App::new(true);
        // Three PRs: "PR 1" (feat/1), "PR 2" (feat/2), "PR 3" (feat/3).
        app.apply_response(Response::Prs(prs(3)));
        app.update(Msg::StartFilter);
        assert_eq!(app.input_context(), InputContext::Filter);
        // Type "2" → only "PR 2" / "feat/2" matches.
        app.update(Msg::FilterChar('2'));
        assert_eq!(app.active_len(), 1);
        assert_eq!(app.selected_pr_id(), Some(2));
        // Clearing restores all rows.
        app.update(Msg::ClearFilter);
        assert!(!app.filtering);
        assert_eq!(app.active_len(), 3);
    }

    #[test]
    fn fuzzy_filter_subsequence_and_no_matches() {
        assert!(fuzzy_match("Fix the parser", "fxp")); // subsequence
        assert!(!fuzzy_match("Fix the parser", "zzz"));
        let mut app = App::new(true);
        app.apply_response(Response::Prs(prs(2)));
        app.update(Msg::StartFilter);
        app.update(Msg::FilterChar('z')); // matches nothing
        assert_eq!(app.active_len(), 0);
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.view(f)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("No matches"));
    }

    #[test]
    fn pr_table_renders_rows() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(true);
        app.apply_response(Response::Prs(prs(2)));
        terminal.draw(|f| app.view(f)).unwrap();
        let text = buffer_text(terminal.backend());
        assert!(
            text.contains("#1") && text.contains("PR 1"),
            "buffer: {text}"
        );
        assert!(text.contains("feat/2"), "buffer: {text}");
    }

    #[test]
    fn empty_pr_state_renders_message() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = App::new(true);
        terminal.draw(|f| app.view(f)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("No pull requests"));
    }

    #[test]
    fn view_renders_without_panicking() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = App::new(true);
        terminal.draw(|f| app.view(f)).unwrap();
        // The active section title appears in the buffer.
        let text = buffer_text(terminal.backend());
        assert!(text.contains("Pull Requests"), "buffer: {text}");
    }

    #[test]
    fn unauthed_view_shows_login_hint() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = App::new(false);
        terminal.draw(|f| app.view(f)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("Not logged in"));
    }

    #[test]
    fn help_overlay_renders() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(true);
        app.update(Msg::ToggleHelp);
        terminal.draw(|f| app.view(f)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("Help"));
    }

    fn buffer_text(backend: &TestBackend) -> String {
        backend
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }
}
