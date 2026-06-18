//! The vim-native keymap grammar (spec 034 / design "Keymap standard"). Maps a
//! raw key event to a semantic [`Msg`]. Kept data-driven and pure so 042 can
//! override it and `?` can render it.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::Msg;

/// Translate a key press into a [`Msg`], or `None` if unbound.
#[must_use]
pub fn map_key(key: KeyEvent) -> Option<Msg> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('q') => Some(Msg::Quit),
        KeyCode::Char('c') if ctrl => Some(Msg::Quit),
        KeyCode::Esc => Some(Msg::Pop),
        KeyCode::Char('?') => Some(Msg::ToggleHelp),
        KeyCode::Char('1') => Some(Msg::SelectTab(0)),
        KeyCode::Char('2') => Some(Msg::SelectTab(1)),
        KeyCode::Char('3') => Some(Msg::SelectTab(2)),
        KeyCode::Tab => Some(Msg::NextTab),
        KeyCode::BackTab => Some(Msg::PrevTab),
        KeyCode::Char('j') | KeyCode::Down => Some(Msg::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(Msg::Up),
        KeyCode::Char('g') => Some(Msg::Top),
        KeyCode::Char('G') => Some(Msg::Bottom),
        KeyCode::Char('d') if ctrl => Some(Msg::HalfPageDown),
        KeyCode::Char('u') if ctrl => Some(Msg::HalfPageUp),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn core_bindings() {
        assert_eq!(map_key(key(KeyCode::Char('q'))), Some(Msg::Quit));
        assert_eq!(map_key(ctrl('c')), Some(Msg::Quit));
        assert_eq!(map_key(key(KeyCode::Esc)), Some(Msg::Pop));
        assert_eq!(map_key(key(KeyCode::Char('?'))), Some(Msg::ToggleHelp));
        assert_eq!(map_key(key(KeyCode::Char('2'))), Some(Msg::SelectTab(1)));
        assert_eq!(map_key(key(KeyCode::Char('j'))), Some(Msg::Down));
        assert_eq!(map_key(key(KeyCode::Down)), Some(Msg::Down));
        assert_eq!(map_key(key(KeyCode::Char('G'))), Some(Msg::Bottom));
        assert_eq!(map_key(ctrl('d')), Some(Msg::HalfPageDown));
        assert_eq!(map_key(key(KeyCode::Tab)), Some(Msg::NextTab));
    }

    #[test]
    fn unbound_key_is_none() {
        assert_eq!(map_key(key(KeyCode::Char('z'))), None);
        // plain 'd' (no ctrl) is unbound for now
        assert_eq!(map_key(key(KeyCode::Char('d'))), None);
    }
}
