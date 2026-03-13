use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, FocusedPanel, Message};

/// Poll for crossterm events and convert to application Messages.
/// Returns None if no event is available within the timeout.
pub fn poll_event(timeout: Duration, app: &App) -> Option<Message> {
    if event::poll(timeout).ok()?
        && let Event::Key(key) = event::read().ok()?
    {
        return handle_key_event(key, app);
    }
    None
}

fn handle_key_event(key: KeyEvent, app: &App) -> Option<Message> {
    // Ignore key release events
    if key.kind != crossterm::event::KeyEventKind::Press {
        return None;
    }

    // Handle `gg` sequence: if `g` was pending and we get another `g`, jump to top
    if app.tree_state.pending_g
        && app.focused == FocusedPanel::Tree
        && key.code == KeyCode::Char('g')
    {
        return Some(Message::TreeJumpTop);
    }
    // If pending_g but not a second `g`, fall through to normal handling
    // (pending_g will be cleared in update)

    // Global keys (always active)
    match key.code {
        KeyCode::Char('q') => return Some(Message::Quit),
        KeyCode::Tab => {
            return if key.modifiers.contains(KeyModifiers::SHIFT) {
                Some(Message::FocusPrev)
            } else {
                Some(Message::FocusNext)
            };
        }
        KeyCode::BackTab => return Some(Message::FocusPrev),
        _ => {}
    }

    // Panel-specific keys
    match app.focused {
        FocusedPanel::Tree => handle_tree_key(key),
        FocusedPanel::Detail => handle_detail_key(key),
        FocusedPanel::Results => handle_results_key(key),
    }
}

fn handle_tree_key(key: KeyEvent) -> Option<Message> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Message::TreeDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Message::TreeUp),
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => Some(Message::TreeExpand),
        KeyCode::Char('h') | KeyCode::Left => Some(Message::TreeCollapse),
        KeyCode::Char('G') => Some(Message::TreeJumpBottom),
        KeyCode::Char('g') => Some(Message::PrefixG),
        _ => None,
    }
}

fn handle_detail_key(key: KeyEvent) -> Option<Message> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Message::DetailScrollDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Message::DetailScrollUp),
        _ => None,
    }
}

fn handle_results_key(key: KeyEvent) -> Option<Message> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Message::ResultsScrollDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Message::ResultsScrollUp),
        KeyCode::Char('G') => Some(Message::ResultsJumpBottom),
        _ => None,
    }
}
