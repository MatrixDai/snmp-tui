use std::time::SystemTime;

use mib_parser::OidTree;
use snmp_client::{OperationType, SnmpRequest, SnmpResponse, SnmpResult, SnmpWorker};
use tokio::sync::mpsc;

use crate::config;
use crate::modal::{ConnectModal, Modal, SearchModal, SetModal};
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

    // SNMP operations
    SnmpGet,
    SnmpGetNext,
    SnmpWalk,

    // Clipboard
    CopyResult,

    // Modal dialogs
    OpenConnectModal,
    OpenSetModal,
    OpenSearchModal,
    ToggleHelp,
    ClearResults,
    ModalClose,
    ModalConfirm,
    ModalTabNext,
    ModalTabPrev,
    ModalChar(char),
    ModalBackspace,
    ModalCycle,
    ModalDown,
    ModalUp,

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
    /// Viewport height (updated each frame from draw).
    pub viewport_height: usize,
}

impl DetailState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            total_lines: 0,
            viewport_height: 0,
        }
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    pub fn scroll_down(&mut self) {
        if self.viewport_height > 0
            && self.total_lines > self.viewport_height
            && self.scroll_offset < self.total_lines - self.viewport_height
        {
            self.scroll_offset += 1;
        }
    }

    /// Reset scroll when the selected tree node changes.
    pub fn reset_scroll(&mut self) {
        self.scroll_offset = 0;
    }
}

/// A single result entry in the results panel log.
#[derive(Debug, Clone)]
pub struct ResultEntry {
    pub operation: OperationType,
    pub oid: String,
    pub object_name: String,
    pub result: ResultValue,
    pub timestamp: SystemTime,
}

/// The value or error in a result entry.
#[derive(Debug, Clone)]
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
    /// Viewport height (updated each frame from draw).
    pub viewport_height: usize,
    /// Whether auto-scroll is active (scroll to bottom on new entries).
    pub auto_scroll: bool,
}

impl ResultsState {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            scroll_offset: 0,
            total_lines: 0,
            viewport_height: 0,
            auto_scroll: true,
        }
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
            self.auto_scroll = false;
        }
    }

    pub fn scroll_down(&mut self) {
        if self.viewport_height > 0
            && self.total_lines > self.viewport_height
            && self.scroll_offset < self.total_lines - self.viewport_height
        {
            self.scroll_offset += 1;
        }
        // Re-enable auto-scroll if we're at the bottom
        if self.viewport_height > 0
            && (self.total_lines <= self.viewport_height
                || self.scroll_offset >= self.total_lines - self.viewport_height)
        {
            self.auto_scroll = true;
        }
    }

    pub fn jump_bottom(&mut self) {
        if self.viewport_height > 0 && self.total_lines > self.viewport_height {
            self.scroll_offset = self.total_lines - self.viewport_height;
        }
        self.auto_scroll = true;
    }
}

/// Connection state for the SNMP device.
#[derive(Debug, Clone)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected { host: String, version: String },
    Error(String),
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disconnected => write!(f, "No device"),
            Self::Connecting => write!(f, "Connecting..."),
            Self::Connected { host, version } => write!(f, "{} {}", host, version),
            Self::Error(e) => write!(f, "Error: {}", e),
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
    pub connection: ConnectionState,
    pub running: bool,
    /// Active modal dialog, if any.
    pub modal: Option<Modal>,
    /// Whether an SNMP operation is currently in-flight.
    pub inflight_op: Option<OperationType>,
    /// Track the previously selected tree node to detect changes.
    prev_selected_node: Option<mib_parser::NodeIndex>,
    /// SNMP worker handle for sending requests.
    worker: Option<SnmpWorker>,
    /// Receiver for SNMP responses from the background worker.
    pub response_rx: Option<mpsc::Receiver<SnmpResponse>>,
    /// Pending connect info (host, version) for when connection response arrives.
    pending_connect_info: Option<(String, String)>,
    /// Current connection config values (for pre-filling the connect modal).
    pub connect_host: String,
    pub connect_port: u16,
    pub connect_version: String,
    pub connect_community: String,
    /// Last OID returned by GETNEXT, keyed by the base (tree node) OID.
    /// Used to advance through table rows on repeated GETNEXT presses.
    last_getnext_oid: Option<(mib_parser::Oid, mib_parser::Oid)>,
    /// Show help overlay.
    pub show_help: bool,
    /// Transient status message (e.g., "Copied to clipboard").
    pub status_message: Option<(String, std::time::Instant)>,
    /// Maximum number of WALK result entries before truncation (9.5).
    pub max_walk_entries: usize,
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
            connection: ConnectionState::Disconnected,
            running: true,
            modal: None,
            inflight_op: None,
            prev_selected_node: None,
            worker: None,
            response_rx: None,
            pending_connect_info: None,
            connect_host: String::new(),
            connect_port: 161,
            connect_version: "v2c".to_string(),
            connect_community: "public".to_string(),
            last_getnext_oid: None,
            show_help: false,
            status_message: None,
            max_walk_entries: 500,
        }
    }

    /// Initialize the SNMP worker (must be called from a tokio context).
    pub fn init_worker(&mut self, debug: bool) {
        let debug_log = if debug {
            Some(std::path::PathBuf::from("/tmp/snmp-cat-debug.log"))
        } else {
            None
        };
        let (worker, response_rx) = SnmpWorker::spawn(debug_log);
        self.worker = Some(worker);
        self.response_rx = Some(response_rx);
    }

    /// Send a connect request to the SNMP worker.
    pub fn connect(&mut self, config: snmp_client::SnmpConfig) {
        let host = config.destination();
        let version = config.version.to_string();
        // Save config values for future modal pre-fill
        self.connect_host = config.host.clone();
        self.connect_port = config.port;
        self.connect_version = config.version.to_string();
        self.connect_community = config.community.clone();
        self.connection = ConnectionState::Connecting;
        self.inflight_op = Some(OperationType::Connect);
        if let Some(ref worker) = self.worker {
            let _ = worker.try_send(SnmpRequest::Connect(config));
        }
        // Store host/version for use when response arrives
        self.pending_connect_info = Some((host, version));
    }

    /// Send an SNMP request for the currently selected OID.
    fn send_snmp_request(&mut self, request_fn: impl FnOnce(mib_parser::Oid) -> SnmpRequest) {
        if self.inflight_op.is_some() {
            return; // Don't stack requests
        }
        if !matches!(self.connection, ConnectionState::Connected { .. }) {
            return; // Not connected
        }
        let oid = match self.selected_oid() {
            Some(oid) => oid,
            None => return,
        };
        if let Some(ref worker) = self.worker {
            let request = request_fn(oid);
            let op = match &request {
                SnmpRequest::Get(_) => OperationType::Get,
                SnmpRequest::GetNext(_) => OperationType::GetNext,
                SnmpRequest::Walk(_) => OperationType::Walk,
                _ => return,
            };
            if worker.try_send(request).is_ok() {
                self.inflight_op = Some(op);
            }
        }
    }

    /// Send a GETNEXT that advances from the last returned OID.
    /// If no previous GETNEXT result exists for this node, starts from the base OID.
    fn send_advancing_getnext(&mut self) {
        if self.inflight_op.is_some() {
            return;
        }
        if !matches!(self.connection, ConnectionState::Connected { .. }) {
            return;
        }
        let base_oid = match self.selected_oid() {
            Some(oid) => oid,
            None => return,
        };

        // If we have a previous result whose request was for this base OID
        // (or a sub-OID of it), advance from the last returned OID
        let request_oid = if let Some((_, ref prev_result)) = self.last_getnext_oid {
            if prev_result.components().starts_with(base_oid.components()) {
                // Still within subtree — continue from last returned OID
                prev_result.clone()
            } else {
                // Left the subtree — loop back to start
                base_oid
            }
        } else {
            base_oid
        };

        if let Some(ref worker) = self.worker
            && worker.try_send(SnmpRequest::GetNext(request_oid)).is_ok()
        {
            self.inflight_op = Some(OperationType::GetNext);
        }
    }

    /// Get the OID of the currently selected tree node.
    fn selected_oid(&self) -> Option<mib_parser::Oid> {
        let node_idx = self.tree_state.selected_node()?;
        self.oid_tree.resolve_oid(node_idx)
    }

    /// Handle an SNMP response from the background worker.
    pub fn handle_snmp_response(&mut self, response: SnmpResponse) {
        self.inflight_op = None;

        // Track last GETNEXT result OID for advancing through table rows
        if matches!(
            response.operation,
            OperationType::Get | OperationType::GetNext
        ) && let SnmpResult::Value(ref resp_oid, _) = response.result
        {
            self.last_getnext_oid = Some((response.request_oid.clone(), resp_oid.clone()));
        }

        // Handle connect/disconnect responses specially
        match response.operation {
            OperationType::Connect => {
                match &response.result {
                    SnmpResult::Ok(_) => {
                        if let Some((host, version)) = self.pending_connect_info.take() {
                            self.connection = ConnectionState::Connected { host, version };
                            // 9.1: Persist connection settings to config file
                            config::save_connection_settings(
                                &self.connect_host,
                                self.connect_port,
                                &self.connect_version,
                                &self.connect_community,
                            );
                        }
                    }
                    SnmpResult::Error(e) => {
                        self.connection = ConnectionState::Error(e.clone());
                        self.pending_connect_info = None;
                    }
                    _ => {}
                }
                self.push_result_entry(&response);
                return;
            }
            OperationType::Disconnect => {
                self.connection = ConnectionState::Disconnected;
                self.push_result_entry(&response);
                return;
            }
            _ => {}
        }

        // For GetNext, suppress results that have left the selected subtree
        if response.operation == OperationType::GetNext
            && let SnmpResult::Value(ref resp_oid, _) = response.result
            && let Some(base_oid) = self.selected_oid()
            && !resp_oid.components().starts_with(base_oid.components())
        {
            return;
        }

        self.push_result_entry(&response);
    }

    /// Convert an SnmpResponse to a ResultEntry and push it to the results panel.
    fn push_result_entry(&mut self, response: &SnmpResponse) {
        let object_name = self
            .tree_state
            .selected_node()
            .and_then(|idx| self.oid_tree.get(idx))
            .map(|n| n.name.clone())
            .unwrap_or_default();

        // For Value results, use the response OID (the actual instance OID)
        // rather than the request OID (which may be a base/object-type OID)
        let (display_oid, result) = match &response.result {
            SnmpResult::Value(resp_oid, value) => {
                (resp_oid.to_string(), ResultValue::Single(value.to_string()))
            }
            SnmpResult::MultiValue(pairs) => {
                let total = pairs.len();
                let limit = self.max_walk_entries;
                let mut formatted: Vec<(String, String)> = pairs
                    .iter()
                    .take(limit)
                    .map(|(oid, val)| (oid.to_string(), val.to_string()))
                    .collect();
                if total > limit {
                    formatted.push((
                        String::new(),
                        format!("... ({} more entries truncated)", total - limit),
                    ));
                }
                (
                    response.request_oid.to_string(),
                    ResultValue::Multiple(formatted),
                )
            }
            SnmpResult::Ok(msg) => (
                response.request_oid.to_string(),
                ResultValue::Single(msg.clone()),
            ),
            SnmpResult::Error(e) => (
                response.request_oid.to_string(),
                ResultValue::Error(e.clone()),
            ),
        };

        let entry = ResultEntry {
            operation: response.operation,
            oid: display_oid,
            object_name,
            result,
            timestamp: SystemTime::now(),
        };

        self.results_state.entries.push(entry);
        // Auto-scroll will be applied in draw
    }

    /// Copy the most recent result entry's value to system clipboard.
    fn copy_selected_result(&mut self) {
        if let Some(entry) = self.results_state.entries.last() {
            let text = match &entry.result {
                ResultValue::Single(v) => {
                    if entry.oid.is_empty() {
                        v.clone()
                    } else {
                        format!("{} = {}", entry.oid, v)
                    }
                }
                ResultValue::Multiple(pairs) => pairs
                    .iter()
                    .map(|(oid, val)| format!("{} = {}", oid, val))
                    .collect::<Vec<_>>()
                    .join("\n"),
                ResultValue::Error(e) => format!("{} -> {}", entry.oid, e),
            };
            // Use OSC 52 escape sequence to set clipboard via terminal emulator.
            // This works in most modern terminals including over SSH.
            use base64::Engine;
            use std::io::Write;
            let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
            let osc52 = format!("\x1b]52;c;{}\x07", encoded);
            let result = std::io::stdout()
                .write_all(osc52.as_bytes())
                .and_then(|_| std::io::stdout().flush());
            match result {
                Ok(()) => {
                    self.status_message =
                        Some(("Copied to clipboard".to_string(), std::time::Instant::now()));
                }
                Err(_) => {
                    self.status_message = Some((
                        "Failed to copy to clipboard".to_string(),
                        std::time::Instant::now(),
                    ));
                }
            }
        }
    }

    /// Open the SET modal for the currently selected OID.
    fn open_set_modal(&mut self) {
        let node_idx = match self.tree_state.selected_node() {
            Some(idx) => idx,
            None => return,
        };
        let node = match self.oid_tree.get(node_idx) {
            Some(n) => n,
            None => return,
        };
        let oid = self
            .oid_tree
            .resolve_oid(node_idx)
            .map(|o| o.to_string())
            .unwrap_or_default();
        let name = node.name.clone();
        let syntax = node.mib_object.as_ref().and_then(|m| m.syntax.clone());
        // A node is scalar-ish if it has no children (leaf node)
        let is_scalar = node.children.is_empty();
        self.modal = Some(Modal::Set(SetModal::new(oid, name, syntax, is_scalar)));
    }

    /// Handle SET modal confirmation.
    fn confirm_set(&mut self) {
        let (oid_str, value) = {
            let set_modal = match &self.modal {
                Some(Modal::Set(m)) => m,
                _ => return,
            };
            let value = match set_modal.build_value() {
                Some(v) => v,
                None => return,
            };
            (set_modal.effective_oid(), value)
        };

        if !matches!(self.connection, ConnectionState::Connected { .. }) {
            return;
        }

        // Parse OID string to Oid
        let components: Vec<u32> = oid_str.split('.').filter_map(|p| p.parse().ok()).collect();
        if components.is_empty() {
            return;
        }
        let oid = mib_parser::Oid::new(components);

        if let Some(ref worker) = self.worker {
            let request = SnmpRequest::Set { oid, value };
            if worker.try_send(request).is_ok() {
                self.inflight_op = Some(OperationType::Set);
            }
        }
        self.modal = None;
    }

    /// Handle search modal confirmation — navigate to the selected result.
    fn confirm_search(&mut self) {
        let target = {
            let search_modal = match &self.modal {
                Some(Modal::Search(m)) => m,
                _ => return,
            };
            search_modal.selected_node()
        };

        if let Some(node_idx) = target {
            self.tree_state.navigate_to(node_idx, &self.oid_tree);
        }
        self.modal = None;
    }

    /// Process a message and update application state.
    pub fn update(&mut self, msg: Message) {
        // Clear pending_g on any message that isn't PrefixG
        if !matches!(msg, Message::PrefixG) {
            self.tree_state.pending_g = false;
        }

        // Handle modal messages
        match &msg {
            Message::ModalClose => {
                self.modal = None;
                return;
            }
            Message::ModalConfirm => {
                match &self.modal {
                    Some(Modal::Connect(m)) => {
                        if let Some(config) = m.build_config() {
                            self.connect(config);
                        }
                        self.modal = None;
                    }
                    Some(Modal::Set(_)) => self.confirm_set(),
                    Some(Modal::Search(_)) => self.confirm_search(),
                    None => {}
                }
                return;
            }
            _ => {}
        }

        // Route input to modal if active
        if self.modal.is_some() {
            match msg {
                Message::ModalTabNext => {
                    if let Some(Modal::Connect(m)) = &mut self.modal {
                        m.focus_next();
                    }
                }
                Message::ModalTabPrev => {
                    if let Some(Modal::Connect(m)) = &mut self.modal {
                        m.focus_prev();
                    }
                }
                Message::ModalChar(c) => match &mut self.modal {
                    Some(Modal::Connect(m)) => m.type_char(c),
                    Some(Modal::Set(m)) => m.type_char(c),
                    Some(Modal::Search(m)) => {
                        let tree = &self.oid_tree;
                        m.type_char(c, tree);
                    }
                    None => {}
                },
                Message::ModalBackspace => match &mut self.modal {
                    Some(Modal::Connect(m)) => m.backspace(),
                    Some(Modal::Set(m)) => m.backspace(),
                    Some(Modal::Search(m)) => {
                        let tree = &self.oid_tree;
                        m.backspace(tree);
                    }
                    None => {}
                },
                Message::ModalCycle => {
                    if let Some(Modal::Connect(m)) = &mut self.modal {
                        m.cycle_field();
                    }
                }
                Message::ModalDown => {
                    if let Some(Modal::Search(m)) = &mut self.modal {
                        m.select_next();
                    }
                }
                Message::ModalUp => {
                    if let Some(Modal::Search(m)) = &mut self.modal {
                        m.select_prev();
                    }
                }
                _ => {}
            }
            return;
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
                self.detail_state.scroll_down();
            }
            Message::ResultsScrollUp => {
                self.results_state.scroll_up();
            }
            Message::ResultsScrollDown => {
                self.results_state.scroll_down();
            }
            Message::ResultsJumpBottom => {
                self.results_state.jump_bottom();
            }
            Message::SnmpGet => {
                // Smart GET: use GETNEXT to auto-discover the first instance
                // (handles both scalar .0 and table column .1 correctly)
                self.send_snmp_request(SnmpRequest::GetNext);
            }
            Message::SnmpGetNext => {
                // Advancing GETNEXT: continue from last returned OID if available
                self.send_advancing_getnext();
            }
            Message::SnmpWalk => {
                self.send_snmp_request(SnmpRequest::Walk);
            }
            Message::OpenConnectModal => {
                self.modal = Some(Modal::Connect(ConnectModal::new(
                    &self.connect_host,
                    self.connect_port,
                    &self.connect_version,
                    &self.connect_community,
                )));
            }
            Message::OpenSetModal => {
                self.open_set_modal();
            }
            Message::OpenSearchModal => {
                self.modal = Some(Modal::Search(SearchModal::new()));
            }
            Message::CopyResult => {
                self.copy_selected_result();
            }
            Message::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            Message::ClearResults => {
                self.results_state.entries.clear();
                self.results_state.scroll_offset = 0;
                self.results_state.total_lines = 0;
                self.results_state.auto_scroll = true;
            }
            Message::PrefixG => {
                self.tree_state.pending_g = true;
            }
            Message::Quit => {
                self.running = false;
            }
            Message::Tick => {}
            // Modal messages are handled above before this match block
            _ => {}
        }

        // Reset state when selected node changes
        let current_node = self.tree_state.selected_node();
        if current_node != self.prev_selected_node {
            self.detail_state.reset_scroll();
            self.last_getnext_oid = None;
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
        state.viewport_height = 10;

        state.scroll_down();
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
        state.viewport_height = 10;
        assert!(state.auto_scroll);

        state.total_lines = 30;
        state.scroll_offset = 5;
        state.scroll_up();
        assert!(!state.auto_scroll);

        state.jump_bottom();
        assert!(state.auto_scroll);
        assert_eq!(state.scroll_offset, 20);
    }

    #[test]
    fn connection_state_display() {
        assert_eq!(format!("{}", ConnectionState::Disconnected), "No device");
        assert_eq!(
            format!(
                "{}",
                ConnectionState::Connected {
                    host: "192.168.1.1:161".to_string(),
                    version: "v2c".to_string()
                }
            ),
            "192.168.1.1:161 v2c"
        );
    }

    #[test]
    fn result_entry_push() {
        let tree = make_test_tree();
        let mut app = App::new(tree);
        app.connection = ConnectionState::Connected {
            host: "10.0.0.1:161".to_string(),
            version: "v2c".to_string(),
        };

        let response = SnmpResponse::error(
            OperationType::Get,
            Oid::new(vec![1, 3, 6, 1]),
            "timeout".to_string(),
        );
        app.push_result_entry(&response);

        assert_eq!(app.results_state.entries.len(), 1);
        assert!(matches!(
            app.results_state.entries[0].result,
            ResultValue::Error(_)
        ));
    }
}
