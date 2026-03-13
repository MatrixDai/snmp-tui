use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::app::Message;

/// Poll for crossterm events and convert to application Messages.
/// Returns None if no event is available within the timeout.
pub fn poll_event(timeout: Duration) -> Option<Message> {
    if event::poll(timeout).ok()?
        && let Event::Key(key) = event::read().ok()?
    {
        return handle_key_event(key);
    }
    None
}

fn handle_key_event(key: KeyEvent) -> Option<Message> {
    // Ignore key release events (crossterm sends both press and release on some platforms)
    if key.kind != crossterm::event::KeyEventKind::Press {
        return None;
    }

    match key.code {
        KeyCode::Char('q') => Some(Message::Quit),
        KeyCode::Tab => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                Some(Message::FocusPrev)
            } else {
                Some(Message::FocusNext)
            }
        }
        KeyCode::BackTab => Some(Message::FocusPrev),
        _ => None,
    }
}
