use std::time::SystemTime;

use mib_parser::OidTree;
use snmp_client::{OperationType, SnmpRequest, SnmpResponse, SnmpResult, SnmpWorker};
use tokio::sync::mpsc;

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

    // Modal dialogs
    OpenConnectModal,
    OpenSetModal,
    OpenSearchModal,
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
    pub target: String,
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

    /// Get the OID of the currently selected tree node.
    fn selected_oid(&self) -> Option<mib_parser::Oid> {
        let node_idx = self.tree_state.selected_node()?;
        self.oid_tree.resolve_oid(node_idx)
    }

    /// Handle an SNMP response from the background worker.
    pub fn handle_snmp_response(&mut self, response: SnmpResponse) {
        self.inflight_op = None;

        // Handle connect/disconnect responses specially
        match response.operation {
            OperationType::Connect => {
                match &response.result {
                    SnmpResult::Ok(_) => {
                        if let Some((host, version)) = self.pending_connect_info.take() {
                            self.connection = ConnectionState::Connected { host, version };
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

        self.push_result_entry(&response);
    }

    /// Convert an SnmpResponse to a ResultEntry and push it to the results panel.
    fn push_result_entry(&mut self, response: &SnmpResponse) {
        let target = match &self.connection {
            ConnectionState::Connected { host, .. } => host.clone(),
            _ => "N/A".to_string(),
        };

        let result = match &response.result {
            SnmpResult::Value(oid, value) => ResultValue::Single(format!("{} = {}", oid, value)),
            SnmpResult::MultiValue(pairs) => {
                let formatted: Vec<(String, String)> = pairs
                    .iter()
                    .map(|(oid, val)| (oid.to_string(), val.to_string()))
                    .collect();
                ResultValue::Multiple(formatted)
            }
            SnmpResult::Ok(msg) => ResultValue::Single(msg.clone()),
            SnmpResult::Error(e) => ResultValue::Error(e.clone()),
        };

        let entry = ResultEntry {
            operation: response.operation,
            oid: response.request_oid.to_string(),
            target,
            result,
            timestamp: SystemTime::now(),
        };

        self.results_state.entries.push(entry);
        // Auto-scroll will be applied in draw
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
                self.send_snmp_request(SnmpRequest::Get);
            }
            Message::SnmpGetNext => {
                self.send_snmp_request(SnmpRequest::GetNext);
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
