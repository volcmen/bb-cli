//! The `bb dash` application model — the single source of truth — with a
//! Model-Update-View shape: [`App`] holds state, [`App::update`] is the reducer
//! over [`Msg`], and [`App::view`] renders a frame. Pure and backend-agnostic so
//! it is exercised with `ratatui::backend::TestBackend` in tests.

use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::api::models::{CommitStatus, PullRequest};
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

/// A modal layer over the main view. The scaffold only needs the help overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    Help,
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
    /// Re-fetch the active section (handled by the event loop, which owns the worker).
    Refresh,
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
    /// In-flight request kinds (drives the spinner).
    pub loading: Vec<RequestKind>,
    /// Transient status/error toast.
    pub status: Option<String>,
    /// Whether the detail pane is open over the list.
    pub detail_open: bool,
    detail: Option<Detail>,
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
            loading: Vec::new(),
            status: None,
            detail_open: false,
            detail: None,
            spinner: 0,
        }
    }

    /// Move the PR selection by `delta` rows, clamped to the loaded set.
    fn move_selection(&mut self, delta: isize) {
        if self.prs.is_empty() {
            return;
        }
        let max = self.prs.len() as isize - 1;
        self.selected = (self.selected as isize).saturating_add(delta).clamp(0, max) as usize;
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
        if self.active_tab == Tab::PullRequests {
            self.prs.get(self.selected).map(|p| p.id)
        } else {
            None
        }
    }

    /// The web URL to open for `o`: the detail PR if open, else the selected row.
    #[must_use]
    pub fn current_url(&self) -> Option<&str> {
        if let Some(d) = &self.detail {
            d.pr.html_url()
        } else {
            self.prs.get(self.selected).and_then(PullRequest::html_url)
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
                    self.done(RequestKind::PrDetail);
                } else {
                    self.should_quit = true;
                }
            }
            Msg::Open => {
                if self.active_tab == Tab::PullRequests && !self.prs.is_empty() {
                    self.detail_open = true;
                    self.detail = None;
                    self.begin(RequestKind::PrDetail);
                }
            }
            // The event loop opens the browser (it owns the Browser seam).
            Msg::OpenBrowser => {}
            Msg::ToggleHelp => {
                self.modal = match self.modal {
                    Some(Modal::Help) => None,
                    _ => Some(Modal::Help),
                };
            }
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
            Msg::Top => self.selected = 0,
            Msg::Bottom => self.selected = self.prs.len().saturating_sub(1),
            Msg::HalfPageDown => self.move_selection(HALF_PAGE),
            Msg::HalfPageUp => self.move_selection(-HALF_PAGE),
            // Refresh is acted on by the event loop (which owns the worker).
            Msg::Refresh => {}
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
        frame.render_widget(
            Paragraph::new(Line::from(Span::raw(
                "j/k move · g/G ends · r refresh · 1/2/3 sections · ? help · q quit",
            ))),
            chunks[2],
        );

        if self.modal == Some(Modal::Help) {
            self.render_help(frame);
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

        // The PR table is the one rich view so far; everything else is a centered
        // message inside the same bordered block.
        if self.authed
            && self.status.is_none()
            && !self.is_loading()
            && self.active_tab == Tab::PullRequests
            && !self.prs.is_empty()
        {
            self.render_pr_table(frame, area, block);
            return;
        }

        let message = if !self.authed {
            "Not logged in — run `bb auth login`".to_owned()
        } else if let Some(status) = &self.status {
            format!("⚠ {status}")
        } else if self.is_loading() {
            format!("{} Loading pull requests…", SPINNER[self.spinner])
        } else {
            match self.active_tab {
                Tab::PullRequests => "No pull requests".to_owned(),
                other => format!("{} — coming soon", other.title()),
            }
        };
        frame.render_widget(Paragraph::new(message).block(block), area);
    }

    fn render_detail(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
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

    fn render_pr_table(&self, frame: &mut Frame, area: ratatui::layout::Rect, block: Block) {
        use ratatui::widgets::{Cell, HighlightSpacing, Row, Table, TableState};

        let header = Row::new(["#", "TITLE", "BRANCH", "STATE"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let rows = self.prs.iter().map(|pr| {
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
