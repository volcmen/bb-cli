//! The vim-native keymap grammar (spec 034 / design "Keymap standard"). Maps a
//! raw key event to a semantic [`Msg`], honoring the active [`InputContext`] so a
//! confirm/comment modal captures input. Kept data-driven and pure so 042 can
//! override it and `?` can render it.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{InputContext, Msg};

/// Translate a key press into a [`Msg`], or `None` if unbound, given the modal
/// context the key is typed in.
#[must_use]
pub fn map_key(key: KeyEvent, ctx: InputContext) -> Option<Msg> {
    match ctx {
        InputContext::Comment => comment_key(key),
        InputContext::Confirm => confirm_key(key),
        InputContext::Help => help_key(key),
        InputContext::Normal => normal_key(key),
    }
}

/// In a comment modal: type into the buffer, Enter submits, Esc cancels.
fn comment_key(key: KeyEvent) -> Option<Msg> {
    match key.code {
        KeyCode::Esc => Some(Msg::Pop),
        KeyCode::Enter => Some(Msg::Submit),
        KeyCode::Backspace => Some(Msg::Backspace),
        KeyCode::Char(c) => Some(Msg::InsertChar(c)),
        _ => None,
    }
}

/// In a confirm modal: `y` accepts, anything else cancels.
fn confirm_key(key: KeyEvent) -> Option<Msg> {
    match key.code {
        KeyCode::Char('y' | 'Y') => Some(Msg::ConfirmYes),
        _ => Some(Msg::Pop),
    }
}

/// In the help overlay: `?` toggles, anything else closes it.
fn help_key(key: KeyEvent) -> Option<Msg> {
    match key.code {
        KeyCode::Char('?') => Some(Msg::ToggleHelp),
        _ => Some(Msg::Pop),
    }
}

/// The normal (no-modal) grammar.
fn normal_key(key: KeyEvent) -> Option<Msg> {
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
        KeyCode::Char('r') => Some(Msg::Refresh),
        KeyCode::Enter | KeyCode::Char('l') => Some(Msg::Open),
        KeyCode::Char('o') => Some(Msg::OpenBrowser),
        KeyCode::Char('a') => Some(Msg::Approve),
        KeyCode::Char('m') => Some(Msg::Merge),
        KeyCode::Char('x') => Some(Msg::Decline),
        KeyCode::Char('C') => Some(Msg::Comment),
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
    fn normal_bindings() {
        let n = InputContext::Normal;
        assert_eq!(map_key(key(KeyCode::Char('q')), n), Some(Msg::Quit));
        assert_eq!(map_key(ctrl('c'), n), Some(Msg::Quit));
        assert_eq!(map_key(key(KeyCode::Char('?')), n), Some(Msg::ToggleHelp));
        assert_eq!(map_key(key(KeyCode::Char('j')), n), Some(Msg::Down));
        assert_eq!(map_key(key(KeyCode::Enter), n), Some(Msg::Open));
        assert_eq!(map_key(key(KeyCode::Char('a')), n), Some(Msg::Approve));
        assert_eq!(map_key(key(KeyCode::Char('m')), n), Some(Msg::Merge));
        assert_eq!(map_key(key(KeyCode::Char('x')), n), Some(Msg::Decline));
        assert_eq!(map_key(key(KeyCode::Char('C')), n), Some(Msg::Comment));
    }

    #[test]
    fn confirm_only_y_accepts() {
        let c = InputContext::Confirm;
        assert_eq!(map_key(key(KeyCode::Char('y')), c), Some(Msg::ConfirmYes));
        assert_eq!(map_key(key(KeyCode::Char('n')), c), Some(Msg::Pop));
        assert_eq!(map_key(key(KeyCode::Esc), c), Some(Msg::Pop));
    }

    #[test]
    fn comment_captures_text() {
        let c = InputContext::Comment;
        assert_eq!(
            map_key(key(KeyCode::Char('h')), c),
            Some(Msg::InsertChar('h'))
        );
        assert_eq!(map_key(key(KeyCode::Backspace), c), Some(Msg::Backspace));
        assert_eq!(map_key(key(KeyCode::Enter), c), Some(Msg::Submit));
        assert_eq!(map_key(key(KeyCode::Esc), c), Some(Msg::Pop));
        // 'q' is text here, not quit.
        assert_eq!(
            map_key(key(KeyCode::Char('q')), c),
            Some(Msg::InsertChar('q'))
        );
    }

    #[test]
    fn unbound_normal_key_is_none() {
        assert_eq!(map_key(key(KeyCode::Char('z')), InputContext::Normal), None);
    }
}
