use std::time::SystemTime;

use mib_parser::OidTree;

use crate::tree_state::TreeState;

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
pub enum Message {
    // Navigation
    FocusNext,
    FocusPrev,

    // Tree navigation
    TreeUp,
    TreeDown,
    TreeExpand,
    TreeCollapse,
    TreeJumpTop,
    TreeJumpBottom,

    // Detail panel
    DetailScrollUp,
    DetailScrollDown,

    // Results panel
    ResultsScrollUp,
    ResultsScrollDown,
    ResultsJumpBottom,

    // Prefix key handling
    /// First `g` press — wait for second key
    PrefixG,

    // App lifecycle
    Quit,
    #[allow(dead_code)]
    Tick,
}

/// State for the detail panel (scrollable view of MIB object metadata).
pub struct DetailState {
    /// Scroll offset (first visible line).
    pub scroll_offset: usize,
    /// Total number of rendered lines (updated each frame).
    pub total_lines: usize,
}

impl DetailState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            total_lines: 0,
        }
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    pub fn scroll_down(&mut self, viewport_height: usize) {
        if self.total_lines > viewport_height
            && self.scroll_offset < self.total_lines - viewport_height
        {
            self.scroll_offset += 1;
        }
    }

    /// Reset scroll when the selected tree node changes.
    pub fn reset_scroll(&mut self) {
        self.scroll_offset = 0;
    }
}

/// Operation type for result entries.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ResultOperation {
    Get,
    GetNext,
    GetBulk,
    Walk,
    Set,
}

impl std::fmt::Display for ResultOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Get => write!(f, "GET"),
            Self::GetNext => write!(f, "GETNEXT"),
            Self::GetBulk => write!(f, "GETBULK"),
            Self::Walk => write!(f, "WALK"),
            Self::Set => write!(f, "SET"),
        }
    }
}

/// A single result entry in the results panel log.
#[derive(Debug, Clone)]
pub struct ResultEntry {
    pub operation: ResultOperation,
    pub oid: String,
    pub target: String,
    pub result: ResultValue,
    pub timestamp: SystemTime,
}

/// The value or error in a result entry.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ResultValue {
    /// Single value result (GET, GETNEXT, SET confirmation).
    Single(String),
    /// Multiple value results (WALK, GETBULK).
    Multiple(Vec<(String, String)>),
    /// Error result.
    Error(String),
}

/// State for the results panel (scrollable log of SNMP query results).
pub struct ResultsState {
    pub entries: Vec<ResultEntry>,
    /// Scroll offset (first visible line in the rendered output).
    pub scroll_offset: usize,
    /// Total number of rendered lines (updated each frame).
    pub total_lines: usize,
    /// Whether auto-scroll is active (scroll to bottom on new entries).
    pub auto_scroll: bool,
}

impl ResultsState {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            scroll_offset: 0,
            total_lines: 0,
            auto_scroll: true,
        }
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
            self.auto_scroll = false;
        }
    }

    pub fn scroll_down(&mut self, viewport_height: usize) {
        if self.total_lines > viewport_height
            && self.scroll_offset < self.total_lines - viewport_height
        {
            self.scroll_offset += 1;
        }
        // Re-enable auto-scroll if we're at the bottom
        if self.total_lines <= viewport_height
            || self.scroll_offset >= self.total_lines - viewport_height
        {
            self.auto_scroll = true;
        }
    }

    pub fn jump_bottom(&mut self, viewport_height: usize) {
        if self.total_lines > viewport_height {
            self.scroll_offset = self.total_lines - viewport_height;
        }
        self.auto_scroll = true;
    }

    #[allow(dead_code)]
    pub fn push_entry(&mut self, entry: ResultEntry, viewport_height: usize) {
        self.entries.push(entry);
        if self.auto_scroll {
            // Will be recalculated on next render; set to max for now
            self.jump_bottom(viewport_height);
        }
    }
}

/// Core application state.
pub struct App {
    pub focused: FocusedPanel,
    pub oid_tree: OidTree,
    pub tree_state: TreeState,
    pub detail_state: DetailState,
    pub results_state: ResultsState,
    pub running: bool,
    /// Track the previously selected tree node to detect changes.
    prev_selected_node: Option<mib_parser::NodeIndex>,
}

impl App {
    pub fn new(oid_tree: OidTree) -> Self {
        let tree_state = TreeState::new(&oid_tree);
        Self {
            focused: FocusedPanel::Tree,
            oid_tree,
            tree_state,
            detail_state: DetailState::new(),
            results_state: ResultsState::new(),
            running: true,
            prev_selected_node: None,
        }
    }

    /// Process a message and update application state.
    pub fn update(&mut self, msg: Message) {
        // Clear pending_g on any message that isn't PrefixG
        if !matches!(msg, Message::PrefixG) {
            self.tree_state.pending_g = false;
        }

        match msg {
            Message::FocusNext => {
                self.focused = self.focused.next();
            }
            Message::FocusPrev => {
                self.focused = self.focused.prev();
            }
            Message::TreeUp => {
                self.tree_state.move_up();
            }
            Message::TreeDown => {
                self.tree_state.move_down();
            }
            Message::TreeExpand => {
                self.tree_state.expand(&self.oid_tree);
            }
            Message::TreeCollapse => {
                self.tree_state.collapse(&self.oid_tree);
            }
            Message::TreeJumpTop => {
                self.tree_state.jump_top();
            }
            Message::TreeJumpBottom => {
                self.tree_state.jump_bottom();
            }
            Message::DetailScrollUp => {
                self.detail_state.scroll_up();
            }
            Message::DetailScrollDown => {
                // viewport_height not known here; scroll_down will cap at total_lines
                self.detail_state.scroll_down(usize::MAX);
            }
            Message::ResultsScrollUp => {
                self.results_state.scroll_up();
            }
            Message::ResultsScrollDown => {
                self.results_state.scroll_down(usize::MAX);
            }
            Message::ResultsJumpBottom => {
                self.results_state.jump_bottom(usize::MAX);
            }
            Message::PrefixG => {
                self.tree_state.pending_g = true;
            }
            Message::Quit => {
                self.running = false;
            }
            Message::Tick => {}
        }

        // Reset detail scroll when selected node changes
        let current_node = self.tree_state.selected_node();
        if current_node != self.prev_selected_node {
            self.detail_state.reset_scroll();
            self.prev_selected_node = current_node;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mib_parser::Oid;

    fn make_test_tree() -> OidTree {
        let mut tree = OidTree::new();
        tree.insert(&Oid::new(vec![1]), "iso");
        tree.insert(&Oid::new(vec![1, 3]), "org");
        tree.insert(&Oid::new(vec![1, 3, 6]), "dod");
        tree.sort_children();
        tree
    }

    #[test]
    fn detail_state_scroll() {
        let mut state = DetailState::new();
        state.total_lines = 20;

        state.scroll_down(10);
        assert_eq!(state.scroll_offset, 1);

        state.scroll_up();
        assert_eq!(state.scroll_offset, 0);

        // Can't scroll above 0
        state.scroll_up();
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn detail_state_reset_on_node_change() {
        let tree = make_test_tree();
        let mut app = App::new(tree);

        // Expand iso to show org
        app.update(Message::TreeExpand);
        // Set some scroll offset
        app.detail_state.scroll_offset = 5;

        // Move down to org — changes selected node, should reset detail scroll
        app.update(Message::TreeDown);
        assert_eq!(app.detail_state.scroll_offset, 0);
    }

    #[test]
    fn results_state_auto_scroll() {
        let mut state = ResultsState::new();
        assert!(state.auto_scroll);

        state.total_lines = 30;
        state.scroll_up(); // No-op at 0, but disables auto_scroll... only if offset > 0
        // scroll_up does nothing when offset is 0
        assert!(state.auto_scroll); // still true since offset didn't change

        state.scroll_offset = 5;
        state.scroll_up();
        assert!(!state.auto_scroll);

        state.jump_bottom(10);
        assert!(state.auto_scroll);
        assert_eq!(state.scroll_offset, 20);
    }

    #[test]
    fn result_operation_display() {
        assert_eq!(format!("{}", ResultOperation::Get), "GET");
        assert_eq!(format!("{}", ResultOperation::Walk), "WALK");
        assert_eq!(format!("{}", ResultOperation::Set), "SET");
    }
}
