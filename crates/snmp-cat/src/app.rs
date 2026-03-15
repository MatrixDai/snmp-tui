use std::time::SystemTime;

use mib_parser::OidTree;
use snmp_client::{OperationType, SnmpRequest, SnmpResponse, SnmpResult, SnmpWorker};
use tokio::sync::mpsc;

use crate::config::{self, ConnectionEntry};
use crate::modal::{ConnectionManagerModal, MibInfoModal, Modal, SearchModal, SetModal};

/// SNMP query strategy determined from MIB object metadata.
enum QueryStrategy {
    /// Scalar OBJECT-TYPE: GET with `.0` appended.
    Scalar,
    /// Instance OID (OBJECT-IDENTITY or leaf without access): GET on exact OID.
    Direct,
    /// Table column or branch: GETNEXT to discover instances.
    Next,
}
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
    TreeReset,

    // Detail panel
    DetailScrollUp,
    DetailScrollDown,
    DetailJumpTop,
    DetailJumpBottom,

    // Results panel
    ResultsScrollUp,
    ResultsScrollDown,
    ResultsJumpTop,
    ResultsJumpBottom,

    // SNMP operations
    SnmpGet,
    SnmpGetNext,
    SnmpWalk,

    // Clipboard
    CopyTreeNode,
    CopyDetail,
    CopyResult,

    // Modal dialogs
    OpenConnectionManager,
    OpenSetModal,
    OpenSearchModal,
    OpenMibInfoModal,
    ToggleHelp,
    ClearResults,
    ModalClose,
    ModalConfirm,
    ModalTabNext,
    ModalTabPrev,
    ModalChar(char),
    ModalBackspace,
    ModalDown,
    ModalUp,

    // Inline search (Detail/Results panels)
    InlineSearchOpen,
    InlineSearchClose,
    InlineSearchChar(char),
    InlineSearchBackspace,
    InlineSearchConfirm,

    // Post-confirmation search navigation (n/N after Enter)
    DetailSearchNext,
    DetailSearchPrev,
    ResultsSearchNext,
    ResultsSearchPrev,

    // Prefix key handling
    /// First `g` press — wait for second key
    PrefixG,

    // App lifecycle
    Quit,
    #[allow(dead_code)]
    Tick,
}

/// Inline search state shared by Detail and Results panels.
///
/// Two-phase search: `active` = typing input, `confirmed` = navigating matches.
pub struct PanelSearch {
    /// Whether the search input bar is active (accepting typed input).
    pub active: bool,
    /// Whether a search has been confirmed (matches highlighted, n/N navigates).
    pub confirmed: bool,
    /// The current search query string.
    pub query: String,
    /// Indices of matching lines (into the rendered lines Vec).
    pub matches: Vec<usize>,
    /// Index into `matches` for the currently highlighted match.
    pub current_match: usize,
}

impl PanelSearch {
    pub fn new() -> Self {
        Self {
            active: false,
            confirmed: false,
            query: String::new(),
            matches: Vec::new(),
            current_match: 0,
        }
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.confirmed = false;
        self.query.clear();
        self.matches.clear();
        self.current_match = 0;
    }

    /// Confirm the search: close input bar but keep matches for n/N navigation.
    pub fn confirm(&mut self) {
        self.active = false;
        if !self.query.is_empty() {
            self.confirmed = true;
        }
    }

    /// Cancel search entirely: close input and clear all state.
    pub fn deactivate(&mut self) {
        self.active = false;
        self.confirmed = false;
        self.query.clear();
        self.matches.clear();
        self.current_match = 0;
    }

    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
    }

    pub fn backspace(&mut self) {
        self.query.pop();
    }

    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = (self.current_match + 1) % self.matches.len();
        }
    }

    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = if self.current_match == 0 {
                self.matches.len() - 1
            } else {
                self.current_match - 1
            };
        }
    }

    /// Update match list from rendered lines. Case-insensitive substring match.
    pub fn update_matches(&mut self, lines: &[ratatui::text::Line]) {
        self.matches.clear();
        if self.query.is_empty() {
            self.current_match = 0;
            return;
        }
        let query_lower = self.query.to_lowercase();
        for (i, line) in lines.iter().enumerate() {
            let plain: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            if plain.to_lowercase().contains(&query_lower) {
                self.matches.push(i);
            }
        }
        // Clamp current_match
        if self.matches.is_empty() || self.current_match >= self.matches.len() {
            self.current_match = 0;
        }
    }

    /// Get the line index of the current match, if any.
    pub fn current_line(&self) -> Option<usize> {
        self.matches.get(self.current_match).copied()
    }
}

/// State for the detail panel (scrollable view of MIB object metadata).
pub struct DetailState {
    /// Scroll offset (first visible line).
    pub scroll_offset: usize,
    /// Total number of rendered lines (updated each frame).
    pub total_lines: usize,
    /// Viewport height (updated each frame from draw).
    pub viewport_height: usize,
    /// Inline search state.
    pub search: PanelSearch,
}

impl DetailState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            total_lines: 0,
            viewport_height: 0,
            search: PanelSearch::new(),
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

    pub fn jump_top(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn jump_bottom(&mut self) {
        if self.viewport_height > 0 && self.total_lines > self.viewport_height {
            self.scroll_offset = self.total_lines - self.viewport_height;
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
    /// Inline search state.
    pub search: PanelSearch,
}

impl ResultsState {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            scroll_offset: 0,
            total_lines: 0,
            viewport_height: 0,
            auto_scroll: true,
            search: PanelSearch::new(),
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

    pub fn jump_top(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = false;
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
    Validating { host: String, version: String },
    Connected { host: String, version: String },
    Error(String),
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disconnected => write!(f, "No device"),
            Self::Connecting => write!(f, "Connecting..."),
            Self::Validating { .. } => write!(f, "Validating..."),
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
    /// Last OID returned by GETNEXT, keyed by the base (tree node) OID.
    /// Used to advance through table rows on repeated GETNEXT presses.
    last_getnext_oid: Option<(mib_parser::Oid, mib_parser::Oid)>,
    /// Whether `g` was pressed as a prefix key (for `gg` command).
    pub pending_g: bool,
    /// Show help overlay.
    pub show_help: bool,
    /// Transient status message (e.g., "Copied to clipboard").
    pub status_message: Option<(String, std::time::Instant)>,
    /// Maximum number of WALK result entries before truncation.
    pub max_walk_entries: usize,
    /// Global timeout from config.
    pub timeout_ms: u64,
    /// Global retries from config.
    pub retries: u32,
    /// Whether we are waiting for a validation GET response.
    pub pending_validation: bool,
    /// The connection entry being connected (for saving after validation).
    pub pending_connection_entry: Option<ConnectionEntry>,
    /// Saved connections from config (for connection manager).
    pub connections: Vec<ConnectionEntry>,
    /// Last used connection alias.
    pub last_connection: Option<String>,
}

impl App {
    pub fn new(oid_tree: OidTree, app_config: &config::AppConfig) -> Self {
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
            last_getnext_oid: None,
            pending_g: false,
            show_help: false,
            status_message: None,
            max_walk_entries: app_config.max_walk_entries,
            timeout_ms: app_config.timeout_ms,
            retries: app_config.retries,
            pending_validation: false,
            pending_connection_entry: None,
            connections: app_config.connections.clone(),
            last_connection: app_config.last_connection.clone(),
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

    /// Open the connection manager modal.
    pub fn open_connection_manager(&mut self, is_startup: bool) {
        self.modal = Some(Modal::ConnectionManager(ConnectionManagerModal::new(
            self.connections.clone(),
            self.last_connection.clone(),
            is_startup,
        )));
    }

    /// Send a connect request to the SNMP worker.
    pub fn connect(&mut self, config: snmp_client::SnmpConfig) {
        let host = config.destination();
        let version = config.version.to_string();
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

    /// Determine the SNMP query strategy based on MIB object metadata.
    fn query_strategy(&self) -> QueryStrategy {
        let node_idx = match self.tree_state.selected_node() {
            Some(idx) => idx,
            None => return QueryStrategy::Next,
        };
        let node = match self.oid_tree.get(node_idx) {
            Some(n) => n,
            None => return QueryStrategy::Next,
        };

        // Branch nodes (MODULE-IDENTITY, table entries, OID branches) always use GETNEXT/WALK
        if !node.children.is_empty() {
            return QueryStrategy::Next;
        }

        // Leaf nodes: determine strategy from MIB metadata
        if let Some(ref mib_obj) = node.mib_object {
            if mib_obj.access.is_some() {
                // OBJECT-TYPE: scalar or table column?
                if self.is_table_column(node_idx) {
                    QueryStrategy::Next
                } else {
                    QueryStrategy::Scalar // append .0
                }
            } else {
                // OBJECT-IDENTITY or similar: direct GET on exact OID
                QueryStrategy::Direct
            }
        } else {
            QueryStrategy::Direct // leaf without MIB data
        }
    }

    /// Check if a node is a table column (parent has INDEX clause).
    fn is_table_column(&self, node_idx: mib_parser::NodeIndex) -> bool {
        if let Some(node) = self.oid_tree.get(node_idx)
            && let Some(parent_idx) = node.parent
            && let Some(parent) = self.oid_tree.get(parent_idx)
            && let Some(ref mib_obj) = parent.mib_object
        {
            return mib_obj.index_clause.is_some();
        }
        false
    }

    /// Send a GET request with `.0` appended (for scalar OBJECT-TYPEs).
    fn send_scalar_get(&mut self) {
        if self.inflight_op.is_some() {
            return;
        }
        if !matches!(self.connection, ConnectionState::Connected { .. }) {
            return;
        }
        let oid = match self.selected_oid() {
            Some(oid) => oid,
            None => return,
        };
        // Append .0 for scalar instance
        let mut components = oid.components().to_vec();
        components.push(0);
        let scalar_oid = mib_parser::Oid::new(components);
        if let Some(ref worker) = self.worker
            && worker.try_send(SnmpRequest::Get(scalar_oid)).is_ok()
        {
            self.inflight_op = Some(OperationType::Get);
        }
    }

    /// Handle an SNMP response from the background worker.
    pub fn handle_snmp_response(&mut self, response: SnmpResponse) {
        self.inflight_op = None;

        // Handle validation GET response
        if self.pending_validation
            && matches!(
                response.operation,
                OperationType::Get | OperationType::GetNext
            )
        {
            self.pending_validation = false;
            match &response.result {
                SnmpResult::Value(_, _) => {
                    if let ConnectionState::Validating {
                        ref host,
                        ref version,
                    } = self.connection
                    {
                        let host = host.clone();
                        let version = version.clone();
                        self.connection = ConnectionState::Connected { host, version };
                        // Save connection to config
                        if let Some(ref entry) = self.pending_connection_entry {
                            config::save_connection(entry);
                            // Update local connections list
                            if let Some(existing) =
                                self.connections.iter_mut().find(|c| c.alias == entry.alias)
                            {
                                *existing = entry.clone();
                            } else {
                                self.connections.push(entry.clone());
                            }
                            self.last_connection = Some(entry.alias.clone());
                        }
                        self.pending_connection_entry = None;
                    }
                    self.push_result_entry(&response);
                }
                SnmpResult::Error(e) => {
                    self.connection = ConnectionState::Error(format!("Validation failed: {}", e));
                    self.pending_connect_info = None;
                    self.pending_connection_entry = None;
                    self.push_result_entry(&response);
                }
                _ => {}
            }
            return;
        }

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
                            self.connection = ConnectionState::Validating { host, version };
                            // Send validation GET sysDescr.0
                            let sys_descr = mib_parser::Oid::new(vec![1, 3, 6, 1, 2, 1, 1, 1, 0]);
                            if let Some(ref worker) = self.worker {
                                let _ = worker.try_send(SnmpRequest::Get(sys_descr));
                                self.pending_validation = true;
                                self.inflight_op = Some(OperationType::Get);
                            }
                        }
                    }
                    SnmpResult::Error(e) => {
                        self.connection = ConnectionState::Error(e.clone());
                        self.pending_connect_info = None;
                        self.pending_connection_entry = None;
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
    ///
    /// Header displays numeric OID; value lines display resolved name with type prefix.
    fn push_result_entry(&mut self, response: &SnmpResponse) {
        let (display_oid, object_name, result) = match &response.result {
            SnmpResult::Value(resp_oid, value) => {
                let name = self.oid_tree.resolve_name(resp_oid);
                let formatted = format!("{}: {}", value.type_name(), value);
                (resp_oid.to_string(), name, ResultValue::Single(formatted))
            }
            SnmpResult::MultiValue(pairs) => {
                let total = pairs.len();
                let limit = self.max_walk_entries;
                let mut formatted: Vec<(String, String)> = pairs
                    .iter()
                    .take(limit)
                    .map(|(oid, val)| {
                        let name = self.oid_tree.resolve_name(oid);
                        let typed_val = format!("{}: {}", val.type_name(), val);
                        (name, typed_val)
                    })
                    .collect();
                if total > limit {
                    formatted.push((
                        String::new(),
                        format!("... ({} more entries truncated)", total - limit),
                    ));
                }
                (
                    response.request_oid.to_string(),
                    String::new(),
                    ResultValue::Multiple(formatted),
                )
            }
            SnmpResult::Ok(msg) => (
                response.request_oid.to_string(),
                String::new(),
                ResultValue::Single(msg.clone()),
            ),
            SnmpResult::Error(e) => (
                response.request_oid.to_string(),
                String::new(),
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

    /// Copy text to system clipboard via OSC 52 escape sequence.
    /// Works in most modern terminals including over SSH.
    fn copy_to_clipboard(&mut self, text: &str) {
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

    /// Copy selected tree node as "name (OID)".
    fn copy_tree_node(&mut self) {
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
        let name = if node.name.is_empty() {
            format!("{}", node.subid)
        } else {
            node.name.clone()
        };
        let text = format!("{} ({})", name, oid);
        self.copy_to_clipboard(&text);
    }

    /// Copy the full detail panel content as plain text.
    fn copy_detail(&mut self) {
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
        let name = if node.name.is_empty() {
            format!("{}", node.subid)
        } else {
            node.name.clone()
        };

        let mut lines = vec![format!("Name: {}", name), format!("OID: {}", oid)];

        if let Some(ref mib_obj) = node.mib_object {
            lines.push(format!("Module: {}", mib_obj.module));
            if let Some(ref syntax) = mib_obj.syntax {
                lines.push(format!("Syntax: {:?}", syntax));
            }
            if let Some(ref access) = mib_obj.access {
                lines.push(format!("Access: {:?}", access));
            }
            if let Some(ref status) = mib_obj.status {
                lines.push(format!("Status: {:?}", status));
            }
            if let Some(ref index_clause) = mib_obj.index_clause {
                lines.push(format!("Index: {}", index_clause.join(", ")));
            }
            if let Some(ref desc) = mib_obj.description {
                lines.push(format!("Description: {}", desc));
            }
        }

        let text = lines.join("\n");
        self.copy_to_clipboard(&text);
    }

    /// Copy the most recent result entry's value to system clipboard.
    fn copy_selected_result(&mut self) {
        if let Some(entry) = self.results_state.entries.last() {
            let text = match &entry.result {
                ResultValue::Single(v) => {
                    if entry.object_name.is_empty() {
                        v.clone()
                    } else {
                        format!("{} = {}", entry.object_name, v)
                    }
                }
                ResultValue::Multiple(pairs) => pairs
                    .iter()
                    .map(|(name, val)| format!("{} = {}", name, val))
                    .collect::<Vec<_>>()
                    .join("\n"),
                ResultValue::Error(e) => format!("{} -> {}", entry.oid, e),
            };
            self.copy_to_clipboard(&text);
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

    /// Handle connection manager confirm — connect to selected or save from edit view.
    fn confirm_connection_manager(&mut self) {
        let mgr = match &mut self.modal {
            Some(Modal::ConnectionManager(m)) => m,
            _ => return,
        };

        if mgr.edit_view.is_some() {
            // Edit view: save connection and go back to list
            let edit_view = mgr.edit_view.as_ref().unwrap();
            let entry = match edit_view.build_connection_entry() {
                Some(e) => e,
                None => return,
            };

            // If editing an existing connection, delete old alias first (handles renames)
            if let Some(ref original_alias) = mgr.editing_original_alias
                && original_alias != &entry.alias
            {
                config::delete_connection(original_alias);
                // Update local list: remove old
                mgr.connections.retain(|c| c.alias != *original_alias);
            }

            // Save to config
            config::save_connection(&entry);

            // Update local connections list
            if let Some(existing) = mgr.connections.iter_mut().find(|c| c.alias == entry.alias) {
                *existing = entry;
            } else {
                mgr.connections.push(entry);
            }

            // Close edit view, go back to list
            mgr.edit_view = None;
            mgr.editing_index = None;
            mgr.editing_original_alias = None;

            // Also sync connections back to app
            self.connections = match &self.modal {
                Some(Modal::ConnectionManager(m)) => m.connections.clone(),
                _ => self.connections.clone(),
            };
        } else {
            // List view: connect to selected
            let entry = match mgr.selected_entry() {
                Some(e) => e.clone(),
                None => return,
            };
            let snmp_config = entry.to_snmp_config(self.timeout_ms, self.retries);
            self.pending_connection_entry = Some(entry);
            self.connect(snmp_config);
            self.modal = None;
        }
    }

    /// Process a message and update application state.
    pub fn update(&mut self, msg: Message) {
        // Clear pending_g on any message that isn't PrefixG
        if !matches!(msg, Message::PrefixG) {
            self.pending_g = false;
        }

        // Handle modal messages
        match &msg {
            Message::ModalClose => {
                match &mut self.modal {
                    // ConnectionManager: layered Esc
                    Some(Modal::ConnectionManager(m)) => {
                        if m.edit_view.is_some() {
                            m.edit_view = None;
                            m.editing_index = None;
                        } else if m.is_startup {
                            self.running = false;
                        } else {
                            self.modal = None;
                        }
                        return;
                    }
                    // MibInfo: Esc navigates back through layers
                    Some(Modal::MibInfo(m)) => {
                        if let Some(ref mut ov) = m.object_view {
                            if ov.search_active {
                                ov.deactivate_search();
                            } else {
                                m.close_object_view();
                            }
                        } else if m.search_active {
                            m.deactivate_search();
                        } else {
                            self.modal = None;
                        }
                        return;
                    }
                    _ => {
                        self.modal = None;
                        return;
                    }
                }
            }
            Message::ModalConfirm => {
                match &mut self.modal {
                    Some(Modal::ConnectionManager(_)) => {
                        self.confirm_connection_manager();
                    }
                    Some(Modal::Set(_)) => self.confirm_set(),
                    Some(Modal::Search(_)) => self.confirm_search(),
                    Some(Modal::MibInfo(m)) => {
                        if m.search_active {
                            m.search_active = false;
                        } else if m.object_view.is_none() {
                            let tree = &self.oid_tree;
                            m.open_object_view(tree);
                        }
                    }
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
                    if let Some(Modal::ConnectionManager(mgr)) = &mut self.modal
                        && let Some(ref mut edit) = mgr.edit_view
                    {
                        edit.focus_next();
                    }
                }
                Message::ModalTabPrev => {
                    if let Some(Modal::ConnectionManager(mgr)) = &mut self.modal
                        && let Some(ref mut edit) = mgr.edit_view
                    {
                        edit.focus_prev();
                    }
                }
                Message::ModalChar(c) => match &mut self.modal {
                    Some(Modal::ConnectionManager(mgr)) => {
                        if let Some(ref mut edit) = mgr.edit_view {
                            edit.type_char(c);
                        } else {
                            match c {
                                'j' => mgr.scroll_down(),
                                'k' => mgr.scroll_up(),
                                'n' => mgr.open_new(),
                                'e' => mgr.open_edit(),
                                'd' => mgr.delete_selected(),
                                _ => {}
                            }
                        }
                    }
                    Some(Modal::Set(m)) => m.type_char(c),
                    Some(Modal::Search(m)) => {
                        let tree = &self.oid_tree;
                        m.type_char(c, tree);
                    }
                    Some(Modal::MibInfo(m)) => {
                        if let Some(ref mut ov) = m.object_view {
                            if ov.search_active {
                                ov.search_char(c);
                            } else {
                                match c {
                                    'j' => ov.scroll_down(),
                                    'k' => ov.scroll_up(),
                                    '/' => ov.activate_search(),
                                    _ => {}
                                }
                            }
                        } else if m.search_active {
                            m.search_char(c);
                        } else {
                            match c {
                                'j' => m.scroll_down(),
                                'k' => m.scroll_up(),
                                '/' => m.activate_search(),
                                _ => {}
                            }
                        }
                    }
                    None => {}
                },
                Message::ModalBackspace => match &mut self.modal {
                    Some(Modal::ConnectionManager(mgr)) => {
                        if let Some(ref mut edit) = mgr.edit_view {
                            edit.backspace();
                        }
                    }
                    Some(Modal::Set(m)) => m.backspace(),
                    Some(Modal::Search(m)) => {
                        let tree = &self.oid_tree;
                        m.backspace(tree);
                    }
                    Some(Modal::MibInfo(m)) => {
                        if let Some(ref mut ov) = m.object_view {
                            if ov.search_active {
                                ov.search_backspace();
                            }
                        } else if m.search_active {
                            m.search_backspace();
                        }
                    }
                    None => {}
                },
                Message::ModalDown => match &mut self.modal {
                    Some(Modal::Search(m)) => m.select_next(),
                    Some(Modal::ConnectionManager(mgr)) => {
                        if let Some(ref mut edit) = mgr.edit_view {
                            edit.arrow_down();
                        } else {
                            mgr.scroll_down();
                        }
                    }
                    Some(Modal::MibInfo(m)) => {
                        if let Some(ref mut ov) = m.object_view {
                            ov.scroll_down();
                        } else {
                            m.scroll_down();
                        }
                    }
                    _ => {}
                },
                Message::ModalUp => match &mut self.modal {
                    Some(Modal::Search(m)) => m.select_prev(),
                    Some(Modal::ConnectionManager(mgr)) => {
                        if let Some(ref mut edit) = mgr.edit_view {
                            edit.arrow_up();
                        } else {
                            mgr.scroll_up();
                        }
                    }
                    Some(Modal::MibInfo(m)) => {
                        if let Some(ref mut ov) = m.object_view {
                            ov.scroll_up();
                        } else {
                            m.scroll_up();
                        }
                    }
                    _ => {}
                },
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
            Message::TreeReset => {
                self.tree_state.reset(&self.oid_tree);
            }
            Message::DetailScrollUp => {
                self.detail_state.scroll_up();
            }
            Message::DetailScrollDown => {
                self.detail_state.scroll_down();
            }
            Message::DetailJumpTop => {
                self.detail_state.jump_top();
            }
            Message::DetailJumpBottom => {
                self.detail_state.jump_bottom();
            }
            Message::ResultsScrollUp => {
                self.results_state.scroll_up();
            }
            Message::ResultsScrollDown => {
                self.results_state.scroll_down();
            }
            Message::ResultsJumpTop => {
                self.results_state.jump_top();
            }
            Message::ResultsJumpBottom => {
                self.results_state.jump_bottom();
            }
            Message::SnmpGet => match self.query_strategy() {
                QueryStrategy::Scalar => self.send_scalar_get(),
                QueryStrategy::Direct => self.send_snmp_request(SnmpRequest::Get),
                QueryStrategy::Next => self.send_snmp_request(SnmpRequest::GetNext),
            },
            Message::SnmpGetNext => match self.query_strategy() {
                QueryStrategy::Scalar => self.send_scalar_get(),
                QueryStrategy::Direct => self.send_snmp_request(SnmpRequest::Get),
                QueryStrategy::Next => self.send_advancing_getnext(),
            },
            Message::SnmpWalk => match self.query_strategy() {
                QueryStrategy::Scalar => self.send_scalar_get(),
                QueryStrategy::Direct => self.send_snmp_request(SnmpRequest::Get),
                QueryStrategy::Next => self.send_snmp_request(SnmpRequest::Walk),
            },
            Message::OpenConnectionManager => {
                self.open_connection_manager(false);
            }
            Message::OpenSetModal => {
                self.open_set_modal();
            }
            Message::OpenSearchModal => {
                self.modal = Some(Modal::Search(SearchModal::new()));
            }
            Message::OpenMibInfoModal => {
                self.modal = Some(Modal::MibInfo(MibInfoModal::new(&self.oid_tree)));
            }
            Message::CopyTreeNode => {
                self.copy_tree_node();
            }
            Message::CopyDetail => {
                self.copy_detail();
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
            Message::InlineSearchOpen => match self.focused {
                FocusedPanel::Detail => self.detail_state.search.activate(),
                FocusedPanel::Results => self.results_state.search.activate(),
                _ => {}
            },
            Message::InlineSearchClose => match self.focused {
                FocusedPanel::Detail => self.detail_state.search.deactivate(),
                FocusedPanel::Results => self.results_state.search.deactivate(),
                _ => {}
            },
            Message::InlineSearchConfirm => match self.focused {
                FocusedPanel::Detail => self.detail_state.search.confirm(),
                FocusedPanel::Results => self.results_state.search.confirm(),
                _ => {}
            },
            Message::InlineSearchChar(c) => match self.focused {
                FocusedPanel::Detail => self.detail_state.search.type_char(c),
                FocusedPanel::Results => self.results_state.search.type_char(c),
                _ => {}
            },
            Message::InlineSearchBackspace => match self.focused {
                FocusedPanel::Detail => self.detail_state.search.backspace(),
                FocusedPanel::Results => self.results_state.search.backspace(),
                _ => {}
            },
            Message::DetailSearchNext => {
                self.detail_state.search.next_match();
                if let Some(line) = self.detail_state.search.current_line() {
                    self.detail_state.scroll_offset =
                        line.saturating_sub(self.detail_state.viewport_height / 2);
                }
            }
            Message::DetailSearchPrev => {
                self.detail_state.search.prev_match();
                if let Some(line) = self.detail_state.search.current_line() {
                    self.detail_state.scroll_offset =
                        line.saturating_sub(self.detail_state.viewport_height / 2);
                }
            }
            Message::ResultsSearchNext => {
                self.results_state.search.next_match();
                if let Some(line) = self.results_state.search.current_line() {
                    self.results_state.scroll_offset =
                        line.saturating_sub(self.results_state.viewport_height / 2);
                    self.results_state.auto_scroll = false;
                }
            }
            Message::ResultsSearchPrev => {
                self.results_state.search.prev_match();
                if let Some(line) = self.results_state.search.current_line() {
                    self.results_state.scroll_offset =
                        line.saturating_sub(self.results_state.viewport_height / 2);
                    self.results_state.auto_scroll = false;
                }
            }
            Message::PrefixG => {
                self.pending_g = true;
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

    fn make_test_config() -> config::AppConfig {
        config::AppConfig::default()
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
        let config = make_test_config();
        let mut app = App::new(tree, &config);

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
        assert_eq!(
            format!(
                "{}",
                ConnectionState::Validating {
                    host: "10.0.0.1:161".to_string(),
                    version: "v2c".to_string()
                }
            ),
            "Validating..."
        );
    }

    #[test]
    fn result_entry_push() {
        let tree = make_test_tree();
        let config = make_test_config();
        let mut app = App::new(tree, &config);
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
