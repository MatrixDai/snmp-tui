use mib_parser::OidTree;

/// Which panel currently has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    Tree,
    Detail,
    Results,
}

impl FocusedPanel {
    pub fn next(self) -> Self {
        match self {
            Self::Tree => Self::Detail,
            Self::Detail => Self::Results,
            Self::Results => Self::Tree,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Tree => Self::Results,
            Self::Detail => Self::Tree,
            Self::Results => Self::Detail,
        }
    }
}

/// Messages that drive application state changes.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Variants used in later milestones
pub enum Message {
    // Navigation
    FocusNext,
    FocusPrev,

    // App lifecycle
    Quit,
    Tick,
}

/// Core application state.
pub struct App {
    pub focused: FocusedPanel,
    pub oid_tree: OidTree,
    pub running: bool,
}

impl App {
    pub fn new(oid_tree: OidTree) -> Self {
        Self {
            focused: FocusedPanel::Tree,
            oid_tree,
            running: true,
        }
    }

    /// Process a message and update application state.
    pub fn update(&mut self, msg: Message) {
        match msg {
            Message::FocusNext => {
                self.focused = self.focused.next();
            }
            Message::FocusPrev => {
                self.focused = self.focused.prev();
            }
            Message::Quit => {
                self.running = false;
            }
            Message::Tick => {}
        }
    }
}
