use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, FocusedPanel, Message};
use crate::modal::Modal;

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

    // If a modal is active, route all input to the modal handler
    if app.modal.is_some() {
        return handle_modal_key(key, app);
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

    // Help overlay toggle (works everywhere)
    if key.code == KeyCode::Char('?') {
        return Some(Message::ToggleHelp);
    }

    // If help overlay is showing, any other key dismisses it
    if app.show_help {
        return Some(Message::ToggleHelp);
    }

    // Global keys (always active)
    match key.code {
        KeyCode::Char('q') => return Some(Message::Quit),
        KeyCode::Char('o') => return Some(Message::OpenConnectModal),
        KeyCode::Char('c') => return Some(Message::ClearResults),
        KeyCode::Char('/') => return Some(Message::OpenSearchModal),
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

fn handle_modal_key(key: KeyEvent, app: &App) -> Option<Message> {
    match key.code {
        KeyCode::Esc => Some(Message::ModalClose),
        KeyCode::Enter => {
            // In connect modal, if focused on a cycle field, cycle it instead of confirming
            if let Some(Modal::Connect(m)) = &app.modal
                && matches!(
                    m.fields[m.focused_field].kind,
                    crate::modal::FieldKind::Cycle(_)
                )
            {
                return Some(Message::ModalCycle);
            }
            Some(Message::ModalConfirm)
        }
        KeyCode::Tab => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                Some(Message::ModalTabPrev)
            } else {
                Some(Message::ModalTabNext)
            }
        }
        KeyCode::BackTab => Some(Message::ModalTabPrev),
        KeyCode::Backspace => Some(Message::ModalBackspace),
        KeyCode::Down => Some(Message::ModalDown),
        KeyCode::Up => Some(Message::ModalUp),
        KeyCode::Char(c) => {
            // Ctrl+Enter to confirm from any field in connect modal
            if c == '\n' || (c == 'm' && key.modifiers.contains(KeyModifiers::CONTROL)) {
                return Some(Message::ModalConfirm);
            }
            Some(Message::ModalChar(c))
        }
        _ => None,
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
        // SNMP operations
        KeyCode::Char(' ') => Some(Message::SnmpGet),
        KeyCode::Char('n') => Some(Message::SnmpGetNext),
        KeyCode::Char('w') => Some(Message::SnmpWalk),
        KeyCode::Char('s') => Some(Message::OpenSetModal),
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
        KeyCode::Char('y') => Some(Message::CopyResult),
        _ => None,
    }
}
