use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::time::SystemTime;

use mib_parser::OidTree;
use snmp_client::{OperationType, SnmpRequest, SnmpResponse, SnmpResult, SnmpWorker};
use tokio::sync::mpsc;

use crate::config::{self, ConnectionEntry};
use crate::modal::{
    ConnectionManagerModal, MibFileEntry, MibFileStatus, MibManagerModal, MibManagerView, Modal,
    SearchModal, SetModal, SetNodeKind, TableColumnSelectModal, TableViewModal,
};

/// Append a warning message to the debug log file.
fn debug_log_warning(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open("/tmp/snmp-tui-debug.log")
    {
        let _ = writeln!(f, "[WARN] {}", msg);
    }
}

/// Actions dispatched from MIB Manager modal that need app-level execution.
/// Extracted before releasing the modal borrow to avoid borrow conflicts.
enum MibAction {
    ReloadAll,
    UnloadFile(PathBuf),
    LoadPath(String),
}

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
    SnmpTableQuery,
    TableRefresh,

    // Clipboard / Export
    CopyTreeNode,
    CopyDetail,
    CopyResult,
    ExportResults,

    // Modal dialogs
    OpenConnectionManager,
    OpenSetModal,
    OpenSearchModal,
    OpenMibManager,
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
    ModalLeft,
    ModalRight,
    ModalJumpTop,
    ModalJumpBottom,
    ModalPageUp,
    ModalPageDown,

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

    // Panel resizing
    PanelGrow,
    PanelShrink,

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
    Validating {
        alias: String,
        host: String,
        version: String,
    },
    Connected {
        alias: String,
        host: String,
        version: String,
    },
    Error(String),
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disconnected => write!(f, "No device"),
            Self::Connecting => write!(f, "Connecting..."),
            Self::Validating { .. } => write!(f, "Validating..."),
            Self::Connected { host, version, .. } => write!(f, "{} {}", host, version),
            Self::Error(e) => write!(f, "Error: {}", e),
        }
    }
}

/// Core application state.
pub struct App {
    pub focused: FocusedPanel,
    pub oid_tree: OidTree,
    /// Canonical list of MIB files tracked across modal open/close cycles.
    pub mib_files: Vec<MibFileEntry>,
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
    /// Debug mode — warnings are written to /tmp/snmp-tui-debug.log instead of stderr.
    pub debug: bool,
    /// Whether we are waiting for a table walk response to populate the modal.
    pub pending_table_query: bool,
    /// Entry node index saved when table walk is launched (for response parsing).
    pub pending_table_entry_idx: Option<mib_parser::NodeIndex>,
    /// Entry OID saved when column-select opens (used to fire Walk on confirm).
    pub pending_table_entry_oid: Option<mib_parser::Oid>,
    /// Columns selected by user in TableColumnSelectModal (used in walk response parsing).
    pub pending_table_columns: Vec<(u32, String)>,
    /// Tree panel width as percentage of total width (15–70).
    pub tree_width_percent: u16,
    /// Detail panel height as percentage of right-side height (15–85).
    pub detail_height_percent: u16,
    /// Index of the current in-progress streaming walk entry in results_state.entries.
    pub walk_in_progress_idx: Option<usize>,
    /// Accumulated results for in-progress table walk (streaming).
    pending_table_walk_results: Vec<(mib_parser::Oid, snmp_client::SnmpValue)>,
}

impl App {
    pub fn new(
        oid_tree: OidTree,
        mib_files: Vec<MibFileEntry>,
        app_config: &config::AppConfig,
    ) -> Self {
        let tree_state = TreeState::new(&oid_tree);
        Self {
            focused: FocusedPanel::Tree,
            oid_tree,
            mib_files,
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
            debug: app_config.debug,
            pending_table_query: false,
            pending_table_entry_idx: None,
            pending_table_entry_oid: None,
            pending_table_columns: Vec::new(),
            tree_width_percent: 30,
            detail_height_percent: 30,
            walk_in_progress_idx: None,
            pending_table_walk_results: Vec::new(),
        }
    }

    /// Initialize the SNMP worker (must be called from a tokio context).
    pub fn init_worker(&mut self, debug: bool) {
        let debug_log = if debug {
            Some(std::path::PathBuf::from("/tmp/snmp-tui-debug.log"))
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

    /// Rebuild the OID tree from `self.mib_files`, updating statuses and resetting tree state.
    pub fn rebuild_oid_tree(&mut self) {
        let paths: Vec<PathBuf> = self.mib_files.iter().map(|e| e.path.clone()).collect();
        let (all_modules, warnings) = mib_parser::load_mibs_tolerant(&paths);

        for warning in &warnings {
            if self.debug {
                debug_log_warning(warning);
            }
        }

        // Build a map from path string → error status from warning messages.
        // Warning format: "Failed to read {path}: {err}" or "Skipping {path} (parse error): {err}"
        let mut error_map: HashMap<String, MibFileStatus> = HashMap::new();
        for warning in &warnings {
            if let Some(rest) = warning.strip_prefix("Failed to read ")
                && let Some(colon_pos) = rest.find(": ")
            {
                error_map.insert(
                    rest[..colon_pos].to_string(),
                    MibFileStatus::ReadError(rest[colon_pos + 2..].to_string()),
                );
            } else if let Some(rest) = warning.strip_prefix("Skipping ")
                && let Some(parse_pos) = rest.find(" (parse error): ")
            {
                error_map.insert(
                    rest[..parse_pos].to_string(),
                    MibFileStatus::ParseError(rest[parse_pos + 16..].to_string()),
                );
            }
        }

        // Build per-path module info from successfully parsed modules.
        let mut path_modules: HashMap<String, (Vec<String>, usize)> = HashMap::new();
        for module in &all_modules {
            let entry = path_modules
                .entry(module.source_file.clone())
                .or_insert((Vec::new(), 0));
            if !entry.0.contains(&module.name) {
                entry.0.push(module.name.clone());
            }
            entry.1 += module.objects.len();
        }

        // Update MibFileEntry statuses.
        for entry in &mut self.mib_files {
            let path_str = entry.path.display().to_string();
            if let Some(err_status) = error_map.get(&path_str) {
                entry.status = err_status.clone();
                entry.modules = Vec::new();
                entry.object_count = 0;
            } else if let Some((modules, count)) = path_modules.get(&path_str) {
                entry.status = MibFileStatus::Loaded;
                entry.modules = modules.clone();
                entry.object_count = *count;
            }
        }

        // Build new tree.
        self.oid_tree = match mib_parser::build_tree_from_modules(&all_modules) {
            Ok(tree) => tree,
            Err(e) => {
                if self.debug {
                    debug_log_warning(&format!("Failed to build MIB tree: {}", e));
                }
                mib_parser::OidTree::new()
            }
        };

        // Reset tree state and clear pending navigation state.
        self.tree_state = crate::tree_state::TreeState::new(&self.oid_tree);
        self.prev_selected_node = None;
        self.last_getnext_oid = None;
        self.pending_table_entry_idx = None;
        self.walk_in_progress_idx = None;
        self.pending_table_walk_results.clear();
    }

    /// Reload all MIB files (file list unchanged).
    pub fn mib_reload_all(&mut self) {
        self.rebuild_oid_tree();
    }

    /// Remove a MIB file from the tracked list and rebuild.
    pub fn mib_unload_file(&mut self, path: &PathBuf) {
        self.mib_files.retain(|e| &e.path != path);
        self.rebuild_oid_tree();
    }

    /// Add a path (file or directory) to the tracked list and rebuild.
    pub fn mib_load_path(&mut self, path_str: &str) {
        let path = PathBuf::from(path_str.trim());
        // Build canonical-path set for existing files to detect symlink duplicates.
        let existing_canonicals: HashSet<PathBuf> = self
            .mib_files
            .iter()
            .map(|e| e.path.canonicalize().unwrap_or_else(|_| e.path.clone()))
            .collect();

        if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    let file_path = entry.path();
                    if file_path.is_file() {
                        let canonical = file_path
                            .canonicalize()
                            .unwrap_or_else(|_| file_path.clone());
                        if !existing_canonicals.contains(&canonical) {
                            self.mib_files.push(MibFileEntry {
                                path: file_path,
                                modules: Vec::new(),
                                object_count: 0,
                                status: MibFileStatus::ParseError("Pending".to_string()),
                                is_bundled: false,
                            });
                        }
                    }
                }
            }
        } else if !path.as_os_str().is_empty() {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !existing_canonicals.contains(&canonical) {
                self.mib_files.push(MibFileEntry {
                    path,
                    modules: Vec::new(),
                    object_count: 0,
                    status: MibFileStatus::ParseError("Pending".to_string()),
                    is_bundled: false,
                });
            }
        }
        self.rebuild_oid_tree();
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

    /// Get the entry node OID if selected node is a TABLE or ENTRY.
    /// Returns None if the node is not a table-like structure.
    fn table_entry_node(&self) -> Option<(mib_parser::NodeIndex, mib_parser::Oid)> {
        let selected_idx = self.tree_state.selected_node()?;
        let node = self.oid_tree.get(selected_idx)?;
        let selected_oid = self.oid_tree.resolve_oid(selected_idx)?;

        // Case 1: Selected node is an ENTRY/ROW (has index_clause) → use directly
        if let Some(ref mib_obj) = node.mib_object
            && mib_obj.index_clause.is_some()
        {
            return Some((selected_idx, selected_oid));
        }

        // Case 2: Find any child node with index_clause (entry node)
        for &child_idx in &node.children {
            if let Some(child_node) = self.oid_tree.get(child_idx)
                && let Some(child_mib) = &child_node.mib_object
                && child_mib.index_clause.is_some()
                && let Some(entry_oid) = self.oid_tree.resolve_oid(child_idx)
            {
                return Some((child_idx, entry_oid));
            }
        }

        None
    }

    /// Open the table view modal and send a walk request for the entry OID.
    /// If the entry node has MIB children (columns), show a column selection modal first.
    /// Otherwise, proceed directly to walk.
    fn open_table_view(&mut self) {
        if self.inflight_op.is_some() {
            return;
        }
        if !matches!(self.connection, ConnectionState::Connected { .. }) {
            return;
        }

        let (entry_idx, entry_oid) = match self.table_entry_node() {
            Some(pair) => pair,
            None => return,
        };

        let title = self
            .oid_tree
            .get(entry_idx)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| entry_oid.to_string());

        // Collect MIB columns from entry node's children
        let mib_columns: Vec<(u32, String)> = if let Some(entry_node) = self.oid_tree.get(entry_idx)
        {
            let mut cols: Vec<(u32, String)> = entry_node
                .children
                .iter()
                .filter_map(|&ci| self.oid_tree.get(ci).map(|n| (n.subid, n.name.clone())))
                .collect();
            cols.sort_by_key(|&(subid, _)| subid);
            cols
        } else {
            vec![]
        };

        // Save entry info for use when Walk is confirmed/launched
        self.pending_table_entry_idx = Some(entry_idx);
        self.pending_table_entry_oid = Some(entry_oid.clone());

        if mib_columns.len() < 10 {
            // Few or no columns — skip column select modal, use all available columns
            if !mib_columns.is_empty() {
                self.pending_table_columns = mib_columns;
            }
            self.modal = Some(Modal::TableView(TableViewModal::new_loading(
                title,
                Some(entry_idx),
                Some(entry_oid.clone()),
            )));
            if let Some(ref worker) = self.worker
                && worker.try_send(SnmpRequest::Walk(entry_oid)).is_ok()
            {
                self.inflight_op = Some(OperationType::Walk);
                self.pending_table_query = true;
            }
        } else {
            // Show column selection modal (>= 10 columns)
            self.modal = Some(Modal::TableColumnSelect(TableColumnSelectModal::new(
                title,
                mib_columns,
            )));
        }
    }

    /// Confirm column selection and launch the walk.
    fn confirm_table_column_select(&mut self) {
        let (title, selected) = if let Some(Modal::TableColumnSelect(m)) = &mut self.modal {
            if m.checked_count() == 0 {
                m.error = Some("Select at least one column".to_string());
                return;
            }
            (m.title.clone(), m.selected_columns())
        } else {
            return;
        };

        let entry_idx = self.pending_table_entry_idx;
        let entry_oid = match self.pending_table_entry_oid.take() {
            Some(oid) => oid,
            None => return,
        };

        self.pending_table_columns = selected;

        // Switch to loading modal
        self.modal = Some(Modal::TableView(TableViewModal::new_loading(
            title,
            entry_idx,
            Some(entry_oid.clone()),
        )));

        if let Some(ref worker) = self.worker
            && worker.try_send(SnmpRequest::Walk(entry_oid)).is_ok()
        {
            self.inflight_op = Some(OperationType::Walk);
            self.pending_table_query = true;
        }
    }

    /// Refresh the table view by re-sending the walk request.
    fn refresh_table_view(&mut self) {
        if self.inflight_op.is_some() {
            return;
        }
        if !matches!(self.connection, ConnectionState::Connected { .. }) {
            return;
        }

        let (entry_idx, entry_oid, columns) = if let Some(Modal::TableView(modal)) = &self.modal {
            match (&modal.entry_idx, &modal.entry_oid) {
                (Some(idx), Some(oid)) => (*idx, oid.clone(), modal.columns.clone()),
                _ => return,
            }
        } else {
            return;
        };

        // Restore pending state so handle_table_walk_response can use it
        self.pending_table_entry_idx = Some(entry_idx);
        self.pending_table_entry_oid = Some(entry_oid.clone());
        if !columns.is_empty() {
            self.pending_table_columns = columns;
        }

        // Reset modal to loading
        if let Some(Modal::TableView(modal)) = &mut self.modal {
            modal.reset_to_loading();
        }

        // Send walk request
        if let Some(ref worker) = self.worker
            && worker.try_send(SnmpRequest::Walk(entry_oid)).is_ok()
        {
            self.inflight_op = Some(OperationType::Walk);
            self.pending_table_query = true;
        }
    }

    /// Parse a walk response and populate the table view modal.
    fn handle_table_walk_response(&mut self, response: &SnmpResponse) {
        let entry_idx = match self.pending_table_entry_idx.take() {
            Some(idx) => idx,
            None => return,
        };

        let entry_oid = match self.oid_tree.resolve_oid(entry_idx) {
            Some(oid) => oid,
            None => return,
        };

        let entry_node = match self.oid_tree.get(entry_idx) {
            Some(node) => node,
            None => return,
        };

        // Extract walk data
        let pairs = match &response.result {
            snmp_client::SnmpResult::MultiValue(pairs) => pairs,
            snmp_client::SnmpResult::Error(e) => {
                self.pending_table_columns.clear();
                if let Some(Modal::TableView(modal)) = &mut self.modal {
                    modal.set_error(e.clone());
                }
                return;
            }
            _ => {
                self.pending_table_columns.clear();
                return;
            }
        };

        // Parse OIDs into (row_index, col_subid, value)
        let mut row_map: std::collections::BTreeMap<
            String,
            std::collections::HashMap<u32, String>,
        > = std::collections::BTreeMap::new();
        let mut col_subids: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();

        for (oid, value) in pairs {
            // Strip entry_oid prefix to get <col_subid>.<row_index...>
            let entry_comps = entry_oid.components();
            let oid_comps = oid.components();

            if oid_comps.len() > entry_comps.len()
                && oid_comps[..entry_comps.len()] == entry_comps[..]
            {
                let suffix = &oid_comps[entry_comps.len()..];
                if !suffix.is_empty() {
                    let col_subid = suffix[0];
                    let row_index_parts = &suffix[1..];
                    let row_index = row_index_parts
                        .iter()
                        .map(|c| c.to_string())
                        .collect::<Vec<_>>()
                        .join(".");

                    col_subids.insert(col_subid);
                    let value_str = value.to_string();
                    row_map
                        .entry(row_index)
                        .or_default()
                        .insert(col_subid, value_str);
                }
            }
        }

        // Use user-selected columns (from column-select modal confirm)
        let columns: Vec<(u32, String)> = if !self.pending_table_columns.is_empty() {
            std::mem::take(&mut self.pending_table_columns)
        } else {
            // Fallback path (no column select shown — no-MIB-metadata path)
            // Synthesize from walk data, cap at 20
            let mut cols = Vec::new();
            // Collect children (columns) from the entry node
            for &child_idx in &entry_node.children {
                if let Some(col_node) = self.oid_tree.get(child_idx)
                    && let Some(oid) = self.oid_tree.resolve_oid(child_idx)
                {
                    let oid_comps = oid.components();
                    if !oid_comps.is_empty() {
                        let subid = oid_comps[oid_comps.len() - 1];
                        cols.push((subid, col_node.name.clone()));
                    }
                }
            }
            // If no columns found in MIB, synthesize from walk data
            if cols.is_empty() {
                cols = col_subids
                    .iter()
                    .enumerate()
                    .map(|(i, &subid)| (subid, format!("col{}", i + 1)))
                    .collect();
            } else {
                // Sort by subid
                cols.sort_by_key(|&(subid, _)| subid);
            }
            // Cap columns at 20 (show last 20 by subid)
            if cols.len() > 20 {
                let start = cols.len() - 20;
                cols = cols.split_off(start);
            }
            cols
        };

        // Check if we'll truncate before consuming row_map
        let total_rows = row_map.len();
        let will_truncate = total_rows > 100;

        // Build row data
        let rows: Vec<(String, Vec<String>)> = row_map
            .into_iter()
            .take(100)
            .map(|(row_idx, value_map)| {
                let values = columns
                    .iter()
                    .map(|&(subid, _)| {
                        value_map
                            .get(&subid)
                            .cloned()
                            .unwrap_or_else(|| String::from("-"))
                    })
                    .collect();
                (row_idx, values)
            })
            .collect();

        // Update title if we truncated
        let title = if will_truncate {
            format!(
                "{} ({} rows, truncated at 100)",
                entry_node.name, total_rows
            )
        } else {
            entry_node.name.clone()
        };

        // Populate modal and persist entry context for refresh
        if let Some(Modal::TableView(modal)) = &mut self.modal {
            modal.title = title;
            modal.entry_idx = Some(entry_idx);
            modal.entry_oid = Some(entry_oid);
            modal.populate(columns, rows);
        }
    }

    /// Append a streaming walk batch to the results panel.
    /// Creates a new ResultEntry on first batch, extends it on subsequent ones.
    fn append_walk_batch(
        &mut self,
        request_oid: &mib_parser::Oid,
        pairs: &[(mib_parser::Oid, snmp_client::SnmpValue)],
    ) {
        let formatted: Vec<(String, String)> = pairs
            .iter()
            .map(|(oid, val)| {
                let name = self.oid_tree.resolve_name(oid);
                let typed_val = format!("{}: {}", val.type_name(), val);
                (name, typed_val)
            })
            .collect();

        if let Some(idx) = self.walk_in_progress_idx
            && let Some(entry) = self.results_state.entries.get_mut(idx)
            && let ResultValue::Multiple(ref mut existing) = entry.result
        {
            existing.extend(formatted);
        } else {
            // Create new entry for this walk
            let entry = ResultEntry {
                operation: OperationType::Walk,
                oid: request_oid.to_string(),
                object_name: String::new(),
                result: ResultValue::Multiple(formatted),
                timestamp: SystemTime::now(),
            };
            self.results_state.entries.push(entry);
            self.walk_in_progress_idx = Some(self.results_state.entries.len() - 1);
        }
    }

    /// Finalize a streaming walk entry: apply truncation and clear in-progress tracking.
    fn finalize_walk_entry(&mut self, total: usize) {
        if let Some(idx) = self.walk_in_progress_idx.take()
            && let Some(entry) = self.results_state.entries.get_mut(idx)
            && let ResultValue::Multiple(ref mut pairs) = entry.result
        {
            let limit = self.max_walk_entries;
            if pairs.len() > limit {
                pairs.truncate(limit);
                pairs.push((
                    String::new(),
                    format!("... ({} more entries truncated)", total - limit),
                ));
            }
        }
    }

    /// Finalize a streaming table walk: synthesize a MultiValue response for the table handler.
    fn finalize_table_walk(&mut self) {
        let results = std::mem::take(&mut self.pending_table_walk_results);
        let request_oid = self
            .pending_table_entry_oid
            .clone()
            .unwrap_or_else(|| mib_parser::Oid::new(vec![]));
        let response = SnmpResponse::multi_value(OperationType::Walk, request_oid, results);
        self.handle_table_walk_response(&response);
    }

    /// Handle an SNMP response from the background worker.
    pub fn handle_snmp_response(&mut self, response: SnmpResponse) {
        // Don't clear inflight for streaming walk batches — walk is still in progress
        let is_walk_batch = matches!(response.result, SnmpResult::WalkBatch(_));
        if !is_walk_batch {
            self.inflight_op = None;
        }

        // Handle streaming walk responses
        if response.operation == OperationType::Walk {
            match &response.result {
                SnmpResult::WalkBatch(pairs) => {
                    if self.pending_table_query {
                        // Accumulate for table modal
                        self.pending_table_walk_results
                            .extend(pairs.iter().cloned());
                    } else {
                        // Append to results panel incrementally
                        self.append_walk_batch(&response.request_oid, pairs);
                    }
                    return;
                }
                SnmpResult::WalkComplete(total) => {
                    if self.pending_table_query {
                        self.pending_table_query = false;
                        self.finalize_table_walk();
                    } else {
                        self.finalize_walk_entry(*total);
                    }
                    return;
                }
                SnmpResult::Error(_) if self.pending_table_query => {
                    self.pending_table_query = false;
                    self.pending_table_walk_results.clear();
                    self.pending_table_columns.clear();
                    // Fall through to handle_table_walk_response for error display
                    self.handle_table_walk_response(&response);
                    return;
                }
                SnmpResult::Error(_) => {
                    // Walk error — finalize any in-progress entry, then show error
                    self.finalize_walk_entry(0);
                    // Fall through to push_result_entry
                }
                _ => {} // MultiValue from old code paths (shouldn't happen)
            }
        }

        // Legacy: intercept table walk MultiValue responses (shouldn't fire with streaming)
        if self.pending_table_query && response.operation == OperationType::Walk {
            self.pending_table_query = false;
            self.handle_table_walk_response(&response);
            return;
        }

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
                        ref alias,
                        ref host,
                        ref version,
                    } = self.connection
                    {
                        let alias = alias.clone();
                        let host = host.clone();
                        let version = version.clone();
                        self.connection = ConnectionState::Connected {
                            alias,
                            host,
                            version,
                        };
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
                            let alias = self
                                .pending_connection_entry
                                .as_ref()
                                .map(|e| e.alias.clone())
                                .unwrap_or_default();
                            self.connection = ConnectionState::Validating {
                                alias,
                                host,
                                version,
                            };
                            // Send validation GET sysDescr.0 to verify device is reachable
                            let sys_descr = mib_parser::Oid::new(vec![1, 3, 6, 1, 2, 1, 1, 1, 0]);
                            if let Some(ref worker) = self.worker
                                && worker.try_send(SnmpRequest::Get(sys_descr)).is_ok()
                            {
                                self.pending_validation = true;
                                self.inflight_op = Some(OperationType::Get);
                            } else {
                                // Worker unavailable — fall back to connected without validation
                                if let ConnectionState::Validating {
                                    ref alias,
                                    ref host,
                                    ref version,
                                } = self.connection
                                {
                                    self.connection = ConnectionState::Connected {
                                        alias: alias.clone(),
                                        host: host.clone(),
                                        version: version.clone(),
                                    };
                                }
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
            // WalkBatch/WalkComplete are handled in handle_snmp_response before reaching here
            SnmpResult::WalkBatch(_) | SnmpResult::WalkComplete(_) => return,
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

    /// Export all result entries to a timestamped file under ~/.snmp-tui/exports/.
    fn export_results(&mut self) {
        if self.results_state.entries.is_empty() {
            self.status_message = Some((
                "No results to export".to_string(),
                std::time::Instant::now(),
            ));
            return;
        }

        let home = match std::env::var_os("HOME") {
            Some(h) => std::path::PathBuf::from(h),
            None => {
                self.status_message = Some((
                    "Export failed: HOME not set".to_string(),
                    std::time::Instant::now(),
                ));
                return;
            }
        };

        let export_dir = home.join(".snmp-tui").join("exports");
        if let Err(e) = std::fs::create_dir_all(&export_dir) {
            self.status_message =
                Some((format!("Export failed: {}", e), std::time::Instant::now()));
            return;
        }

        let ts = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let filename = format!("export_{}.txt", ts);
        let filepath = export_dir.join(&filename);

        let mut content = String::new();
        for entry in &self.results_state.entries {
            let entry_ts = entry
                .timestamp
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            content.push_str(&format!(
                "[{}] {} {}\n",
                entry_ts, entry.operation, entry.oid
            ));
            if !entry.object_name.is_empty() {
                content.push_str(&format!("  Name: {}\n", entry.object_name));
            }
            match &entry.result {
                ResultValue::Single(v) => {
                    content.push_str(&format!("  {}\n", v));
                }
                ResultValue::Multiple(pairs) => {
                    for (name, val) in pairs {
                        if name.is_empty() {
                            content.push_str(&format!("  {}\n", val));
                        } else {
                            content.push_str(&format!("  {} = {}\n", name, val));
                        }
                    }
                }
                ResultValue::Error(e) => {
                    content.push_str(&format!("  ERROR: {}\n", e));
                }
            }
            content.push('\n');
        }

        match std::fs::write(&filepath, &content) {
            Ok(()) => {
                let count = self.results_state.entries.len();
                self.status_message = Some((
                    format!("Exported {} entries to ~/{}", count, filename),
                    std::time::Instant::now(),
                ));
            }
            Err(e) => {
                self.status_message =
                    Some((format!("Export failed: {}", e), std::time::Instant::now()));
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
        // Block SET on branch nodes — only leaf OBJECT-TYPEs make sense
        if !node.children.is_empty() {
            return;
        }
        let oid = self
            .oid_tree
            .resolve_oid(node_idx)
            .map(|o| o.to_string())
            .unwrap_or_default();
        let name = node.name.clone();
        let syntax = node.mib_object.as_ref().and_then(|m| m.syntax.clone());

        let node_kind = if self.is_table_column(node_idx) {
            SetNodeKind::TableColumn
        } else {
            SetNodeKind::Scalar
        };

        let mut modal = SetModal::new(oid.clone(), name, syntax, node_kind);

        // Pre-fill index from last GETNEXT result if it matches this column's base OID
        if node_kind == SetNodeKind::TableColumn
            && let Some((ref base_oid, ref result_oid)) = self.last_getnext_oid
        {
            let base_str = base_oid.to_string();
            if base_str == oid {
                // The result OID is base_oid.index — extract the suffix
                let result_str = result_oid.to_string();
                if let Some(suffix) = result_str.strip_prefix(&base_str) {
                    let suffix = suffix.strip_prefix('.').unwrap_or(suffix);
                    if !suffix.is_empty() {
                        modal.prefill_index(suffix);
                    }
                }
            }
        }

        self.modal = Some(Modal::Set(modal));
    }

    /// Handle SET modal confirmation.
    fn confirm_set(&mut self) {
        let (oid_str, value) = {
            let set_modal = match &self.modal {
                Some(Modal::Set(m)) => m,
                _ => return,
            };
            if !set_modal.is_ready() {
                return;
            }
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
                    // MibManager: Esc navigates back through layers
                    Some(Modal::MibManager(m)) => {
                        match m.view {
                            MibManagerView::ObjectList => {
                                if let Some(ref mut ov) = m.object_view {
                                    if ov.search_active {
                                        ov.deactivate_search();
                                    } else {
                                        m.close_object_view();
                                    }
                                }
                            }
                            MibManagerView::FileList => {
                                if m.search_active {
                                    m.deactivate_search();
                                } else {
                                    self.modal = None;
                                }
                            }
                            MibManagerView::LoadInput => {
                                m.view = MibManagerView::FileList;
                                m.load_input.clear();
                            }
                            MibManagerView::ConfirmUnload => {
                                m.view = MibManagerView::FileList;
                                m.unload_target = None;
                            }
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
                // Extract deferred MibManager actions before the mutable borrow.
                let mib_confirm_action: Option<MibAction> =
                    if let Some(Modal::MibManager(ref m)) = self.modal {
                        match m.view {
                            MibManagerView::LoadInput => {
                                Some(MibAction::LoadPath(m.load_input.clone()))
                            }
                            MibManagerView::ConfirmUnload => m
                                .unload_target
                                .filter(|&i| i < m.files.len())
                                .map(|i| MibAction::UnloadFile(m.files[i].path.clone())),
                            _ => None,
                        }
                    } else {
                        None
                    };

                match &mut self.modal {
                    Some(Modal::ConnectionManager(_)) => {
                        self.confirm_connection_manager();
                    }
                    Some(Modal::Set(_)) => self.confirm_set(),
                    Some(Modal::Search(_)) => self.confirm_search(),
                    Some(Modal::MibManager(m)) => match m.view {
                        MibManagerView::FileList => {
                            if m.search_active {
                                m.search_active = false;
                            } else {
                                let tree = &self.oid_tree;
                                m.open_object_view(tree);
                            }
                        }
                        MibManagerView::ObjectList => {
                            if let Some(ref mut ov) = m.object_view
                                && ov.search_active
                            {
                                ov.search_active = false;
                            }
                        }
                        _ => {} // LoadInput and ConfirmUnload handled by deferred action below
                    },
                    Some(Modal::TableColumnSelect(_)) => self.confirm_table_column_select(),
                    Some(Modal::TableView(_)) => {}
                    None => {}
                }

                // Execute deferred MibManager actions (requires &mut self, done after borrow ends).
                if let Some(action) = mib_confirm_action {
                    match action {
                        MibAction::LoadPath(path) => {
                            self.mib_load_path(&path);
                            let files = self.mib_files.clone();
                            if let Some(Modal::MibManager(ref mut m)) = self.modal {
                                m.view = MibManagerView::FileList;
                                m.load_input.clear();
                                m.refresh_files(files);
                                m.feedback_message =
                                    Some(("MIBs loaded successfully".to_string(), false));
                            }
                        }
                        MibAction::UnloadFile(path) => {
                            self.mib_unload_file(&path);
                            let files = self.mib_files.clone();
                            if let Some(Modal::MibManager(ref mut m)) = self.modal {
                                m.view = MibManagerView::FileList;
                                m.unload_target = None;
                                m.refresh_files(files);
                                m.feedback_message = Some(("MIB unloaded".to_string(), false));
                            }
                        }
                        MibAction::ReloadAll => {
                            self.mib_reload_all();
                            let files = self.mib_files.clone();
                            if let Some(Modal::MibManager(ref mut m)) = self.modal {
                                m.refresh_files(files);
                                m.feedback_message = Some(("MIBs reloaded".to_string(), false));
                            }
                        }
                    }
                }
                return;
            }
            _ => {}
        }

        // Route input to modal if active
        if self.modal.is_some() {
            match msg {
                Message::ModalTabNext => match &mut self.modal {
                    Some(Modal::ConnectionManager(mgr)) => {
                        if let Some(ref mut edit) = mgr.edit_view {
                            edit.focus_next();
                        }
                    }
                    Some(Modal::Set(m)) => m.focus_next(),
                    Some(Modal::TableColumnSelect(_)) => {}
                    Some(Modal::TableView(_)) => {}
                    Some(Modal::Search(_)) | Some(Modal::MibManager(_)) => {}
                    None => {}
                },
                Message::ModalTabPrev => match &mut self.modal {
                    Some(Modal::ConnectionManager(mgr)) => {
                        if let Some(ref mut edit) = mgr.edit_view {
                            edit.focus_prev();
                        }
                    }
                    Some(Modal::Set(m)) => m.focus_prev(),
                    Some(Modal::TableColumnSelect(_)) => {}
                    Some(Modal::TableView(_)) => {}
                    Some(Modal::Search(_)) | Some(Modal::MibManager(_)) => {}
                    None => {}
                },
                Message::ModalChar(c) => {
                    // Extract deferred MibManager actions before the mutable borrow.
                    let mib_char_action: Option<MibAction> =
                        if let Some(Modal::MibManager(ref m)) = self.modal {
                            match m.view {
                                MibManagerView::FileList if !m.search_active => match c {
                                    'r' | 'R' => Some(MibAction::ReloadAll),
                                    _ => None,
                                },
                                MibManagerView::ConfirmUnload if c == 'y' => m
                                    .unload_target
                                    .filter(|&i| i < m.files.len())
                                    .map(|i| MibAction::UnloadFile(m.files[i].path.clone())),
                                _ => None,
                            }
                        } else {
                            None
                        };

                    match &mut self.modal {
                        Some(Modal::ConnectionManager(mgr)) => {
                            if let Some(ref mut edit) = mgr.edit_view {
                                edit.type_char(c);
                            } else {
                                match c {
                                    'd' => mgr.delete_selected(),
                                    _ => {
                                        mgr.cancel_pending_delete();
                                        match c {
                                            'j' => mgr.scroll_down(),
                                            'k' => mgr.scroll_up(),
                                            'n' => mgr.open_new(),
                                            'e' => mgr.open_edit(),
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                        Some(Modal::Set(m)) => m.type_char(c),
                        Some(Modal::Search(m)) => {
                            let tree = &self.oid_tree;
                            m.type_char(c, tree);
                        }
                        Some(Modal::MibManager(m)) => match m.view {
                            MibManagerView::FileList => {
                                if m.search_active {
                                    m.search_char(c);
                                } else {
                                    match c {
                                        'j' => m.scroll_down(),
                                        'k' => m.scroll_up(),
                                        '/' => m.activate_search(),
                                        'u' => {
                                            if !m.selected_file_is_bundled()
                                                && let Some(&idx) = m.filtered.get(m.selected)
                                            {
                                                m.unload_target = Some(idx);
                                                m.view = MibManagerView::ConfirmUnload;
                                            }
                                        }
                                        'a' => {
                                            m.load_input.clear();
                                            m.view = MibManagerView::LoadInput;
                                        }
                                        'r' | 'R' => {} // handled by deferred action
                                        _ => {}
                                    }
                                }
                            }
                            MibManagerView::ObjectList => {
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
                                }
                            }
                            MibManagerView::LoadInput => {
                                m.load_input.push(c);
                            }
                            MibManagerView::ConfirmUnload => {
                                if c == 'n' {
                                    m.view = MibManagerView::FileList;
                                    m.unload_target = None;
                                }
                                // 'y' handled by deferred action
                            }
                        },
                        Some(Modal::TableColumnSelect(m)) => {
                            if c == ' ' {
                                m.toggle();
                            }
                        }
                        Some(Modal::TableView(_)) => {}
                        None => {}
                    }

                    // Execute deferred MibManager actions.
                    if let Some(action) = mib_char_action {
                        match action {
                            MibAction::ReloadAll => {
                                self.mib_reload_all();
                                let files = self.mib_files.clone();
                                if let Some(Modal::MibManager(ref mut m)) = self.modal {
                                    m.refresh_files(files);
                                    m.feedback_message = Some(("MIBs reloaded".to_string(), false));
                                }
                            }
                            MibAction::UnloadFile(path) => {
                                self.mib_unload_file(&path);
                                let files = self.mib_files.clone();
                                if let Some(Modal::MibManager(ref mut m)) = self.modal {
                                    m.view = MibManagerView::FileList;
                                    m.unload_target = None;
                                    m.refresh_files(files);
                                    m.feedback_message = Some(("MIB unloaded".to_string(), false));
                                }
                            }
                            MibAction::LoadPath(path) => {
                                self.mib_load_path(&path);
                                let files = self.mib_files.clone();
                                if let Some(Modal::MibManager(ref mut m)) = self.modal {
                                    m.view = MibManagerView::FileList;
                                    m.load_input.clear();
                                    m.refresh_files(files);
                                    m.feedback_message =
                                        Some(("MIBs loaded successfully".to_string(), false));
                                }
                            }
                        }
                    }
                }
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
                    Some(Modal::MibManager(m)) => match m.view {
                        MibManagerView::FileList => {
                            if m.search_active {
                                m.search_backspace();
                            }
                        }
                        MibManagerView::ObjectList => {
                            if let Some(ref mut ov) = m.object_view
                                && ov.search_active
                            {
                                ov.search_backspace();
                            }
                        }
                        MibManagerView::LoadInput => {
                            m.load_input.pop();
                        }
                        MibManagerView::ConfirmUnload => {}
                    },
                    Some(Modal::TableColumnSelect(_)) => {}
                    Some(Modal::TableView(_)) => {}
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
                    Some(Modal::MibManager(m)) => match m.view {
                        MibManagerView::FileList => m.scroll_down(),
                        MibManagerView::ObjectList => {
                            if let Some(ref mut ov) = m.object_view {
                                ov.scroll_down();
                            }
                        }
                        _ => {}
                    },
                    Some(Modal::TableColumnSelect(m)) => m.scroll_down(),
                    Some(Modal::TableView(m)) => m.scroll_down(),
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
                    Some(Modal::MibManager(m)) => match m.view {
                        MibManagerView::FileList => m.scroll_up(),
                        MibManagerView::ObjectList => {
                            if let Some(ref mut ov) = m.object_view {
                                ov.scroll_up();
                            }
                        }
                        _ => {}
                    },
                    Some(Modal::TableColumnSelect(m)) => m.scroll_up(),
                    Some(Modal::TableView(m)) => m.scroll_up(),
                    _ => {}
                },
                Message::ModalLeft => {
                    if let Some(Modal::TableView(m)) = &mut self.modal {
                        m.scroll_left();
                    }
                }
                Message::ModalRight => {
                    if let Some(Modal::TableView(m)) = &mut self.modal {
                        m.scroll_right();
                    }
                }
                Message::ModalJumpTop => match &mut self.modal {
                    Some(Modal::Search(m)) => m.jump_top(),
                    Some(Modal::MibManager(m)) => m.jump_top(),
                    Some(Modal::TableColumnSelect(m)) => m.jump_top(),
                    Some(Modal::TableView(m)) => m.jump_top(),
                    _ => {}
                },
                Message::ModalJumpBottom => match &mut self.modal {
                    Some(Modal::Search(m)) => m.jump_bottom(),
                    Some(Modal::MibManager(m)) => m.jump_bottom(),
                    Some(Modal::TableColumnSelect(m)) => m.jump_bottom(),
                    Some(Modal::TableView(m)) => m.jump_bottom(),
                    _ => {}
                },
                Message::ModalPageUp => {
                    if let Some(Modal::Search(m)) = &mut self.modal {
                        m.page_up();
                    }
                }
                Message::ModalPageDown => {
                    if let Some(Modal::Search(m)) = &mut self.modal {
                        m.page_down();
                    }
                }
                Message::TableRefresh => {
                    self.refresh_table_view();
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
            Message::SnmpTableQuery => {
                self.open_table_view();
            }
            Message::OpenConnectionManager => {
                self.open_connection_manager(false);
            }
            Message::OpenSetModal => {
                self.open_set_modal();
            }
            Message::OpenSearchModal => {
                self.modal = Some(Modal::Search(SearchModal::new()));
            }
            Message::OpenMibManager => {
                let files = self.mib_files.clone();
                self.modal = Some(Modal::MibManager(MibManagerModal::new(files)));
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
                self.walk_in_progress_idx = None;
            }
            Message::ExportResults => {
                self.export_results();
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
            Message::PanelGrow => match self.focused {
                FocusedPanel::Tree => {
                    self.tree_width_percent = (self.tree_width_percent + 5).min(70);
                }
                FocusedPanel::Detail => {
                    self.detail_height_percent = (self.detail_height_percent + 5).min(85);
                }
                FocusedPanel::Results => {
                    self.detail_height_percent =
                        self.detail_height_percent.saturating_sub(5).max(15);
                }
            },
            Message::PanelShrink => match self.focused {
                FocusedPanel::Tree => {
                    self.tree_width_percent = self.tree_width_percent.saturating_sub(5).max(15);
                }
                FocusedPanel::Detail => {
                    self.detail_height_percent =
                        self.detail_height_percent.saturating_sub(5).max(15);
                }
                FocusedPanel::Results => {
                    self.detail_height_percent = (self.detail_height_percent + 5).min(85);
                }
            },
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
        let mut app = App::new(tree, Vec::new(), &config);

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
                    alias: "test".to_string(),
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
                    alias: "test".to_string(),
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
        let mut app = App::new(tree, Vec::new(), &config);
        app.connection = ConnectionState::Connected {
            alias: "test".to_string(),
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
