//! The `bb dash` application model — the single source of truth — with a
//! Model-Update-View shape: [`App`] holds state, [`App::update`] is the reducer
//! over [`Msg`], and [`App::view`] renders a frame. Pure and backend-agnostic so
//! it is exercised with `ratatui::backend::TestBackend` in tests.

use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

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
}

/// The dashboard state.
#[derive(Debug, Clone)]
pub struct App {
    /// Whether the user is authenticated for the target host.
    pub authed: bool,
    pub active_tab: Tab,
    pub modal: Option<Modal>,
    pub should_quit: bool,
}

impl App {
    #[must_use]
    pub fn new(authed: bool) -> Self {
        Self {
            authed,
            active_tab: Tab::PullRequests,
            modal: None,
            should_quit: false,
        }
    }

    /// Apply a semantic message to the model.
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Quit => self.should_quit = true,
            Msg::Pop => {
                if self.modal.take().is_none() {
                    self.should_quit = true;
                }
            }
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
            // Motions are no-ops until a list view exists (036+); the bindings are
            // wired now so the keymap grammar is stable.
            Msg::Down | Msg::Up | Msg::Top | Msg::Bottom | Msg::HalfPageDown | Msg::HalfPageUp => {}
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
        frame.render_widget(self.body(), chunks[1]);
        frame.render_widget(
            Paragraph::new(Line::from(Span::raw(
                "j/k move · 1/2/3 sections · ? help · q quit",
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

    fn body(&self) -> Paragraph<'static> {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.active_tab.title());
        let text = if self.authed {
            format!("{} — loading soon…", self.active_tab.title())
        } else {
            "Not logged in — run `bb auth login`".to_owned()
        };
        Paragraph::new(text).block(block)
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
