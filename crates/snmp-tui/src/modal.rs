use std::path::PathBuf;

use mib_parser::{NodeIndex, OidTree, Syntax};
use ratatui::widgets::TableState;
use snmp_client::SnmpValue;

use crate::config::ConnectionEntry;

/// Active modal dialog.
pub enum Modal {
    ConnectionManager(ConnectionManagerModal),
    Set(SetModal),
    Search(SearchModal),
    MibManager(MibManagerModal),
    TableColumnSelect(TableColumnSelectModal),
    TableView(TableViewModal),
}

// ============================================================
// Table Column Select Modal
// ============================================================

pub struct ColumnItem {
    pub subid: u32,
    pub name: String,
    pub checked: bool,
}

pub struct TableColumnSelectModal {
    pub title: String,
    pub columns: Vec<ColumnItem>,
    pub cursor: usize,
    pub scroll_offset: usize,
    pub error: Option<String>,
    pub max_columns: usize,
}

impl TableColumnSelectModal {
    /// Create a new column selection modal with all columns from the list,
    /// pre-checking the first 10 (sorted by subid) and leaving the rest unchecked.
    pub fn new(title: String, columns: Vec<(u32, String)>) -> Self {
        let items: Vec<ColumnItem> = columns
            .into_iter()
            .enumerate()
            .map(|(idx, (subid, name))| ColumnItem {
                subid,
                name,
                checked: idx < 10,
            })
            .collect();

        Self {
            title,
            columns: items,
            cursor: 0,
            scroll_offset: 0,
            error: None,
            max_columns: 20,
        }
    }

    /// Toggle the checked state of the item at cursor.
    /// If trying to check when 20 already checked, set an error instead.
    pub fn toggle(&mut self) {
        if self.cursor >= self.columns.len() {
            return;
        }

        if self.columns[self.cursor].checked {
            // Unchecking is always allowed
            self.columns[self.cursor].checked = false;
            self.error = None;
        } else {
            // Checking: only if fewer than max_columns already checked
            let count = self.columns.iter().filter(|item| item.checked).count();
            if count < self.max_columns {
                self.columns[self.cursor].checked = true;
                self.error = None;
            } else {
                self.error = Some("Max 20 columns selected".to_string());
            }
        }
    }

    /// Move cursor down, updating scroll_offset to keep cursor visible.
    pub fn scroll_down(&mut self) {
        if !self.columns.is_empty() && self.cursor + 1 < self.columns.len() {
            self.cursor += 1;
            // Keep cursor visible in a reasonable window (5-line buffer from bottom)
            let visible_height = 10; // typical visible height in modal
            if self.cursor >= self.scroll_offset + visible_height {
                self.scroll_offset = self.cursor - visible_height + 1;
            }
            self.error = None;
        }
    }

    /// Move cursor up, updating scroll_offset to keep cursor visible.
    pub fn scroll_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            if self.cursor < self.scroll_offset {
                self.scroll_offset = self.cursor;
            }
            self.error = None;
        }
    }

    /// Jump to the first item.
    pub fn jump_top(&mut self) {
        self.cursor = 0;
        self.scroll_offset = 0;
        self.error = None;
    }

    /// Jump to the last item.
    pub fn jump_bottom(&mut self) {
        if !self.columns.is_empty() {
            self.cursor = self.columns.len() - 1;
            let visible_height = 10;
            self.scroll_offset = if self.columns.len() > visible_height {
                self.columns.len() - visible_height
            } else {
                0
            };
            self.error = None;
        }
    }

    /// Return the number of checked items.
    pub fn checked_count(&self) -> usize {
        self.columns.iter().filter(|item| item.checked).count()
    }

    /// Return the checked columns as (subid, name) sorted by subid.
    pub fn selected_columns(&self) -> Vec<(u32, String)> {
        let mut selected: Vec<(u32, String)> = self
            .columns
            .iter()
            .filter(|item| item.checked)
            .map(|item| (item.subid, item.name.clone()))
            .collect();
        selected.sort_by_key(|&(subid, _)| subid);
        selected
    }
}

// ============================================================
// Table View Modal
// ============================================================

pub struct TableViewModal {
    pub title: String,
    /// Column metadata: (subid, name)
    pub columns: Vec<(u32, String)>,
    /// Row data: (row_index_str, values aligned to columns)
    pub rows: Vec<(String, Vec<String>)>,
    /// Currently selected row index
    pub selected_row: usize,
    /// Horizontal scroll offset (first visible column)
    pub col_scroll: usize,
    /// Ratatui table state for selection
    pub table_state: TableState,
    /// Whether data is still loading
    pub loading: bool,
    /// Error message if load failed
    pub error: Option<String>,
    /// Entry node index for refresh
    pub entry_idx: Option<mib_parser::NodeIndex>,
    /// Entry OID for refresh
    pub entry_oid: Option<mib_parser::Oid>,
}

impl TableViewModal {
    /// Create a new loading modal with the given title and entry context for refresh.
    pub fn new_loading(
        title: String,
        entry_idx: Option<mib_parser::NodeIndex>,
        entry_oid: Option<mib_parser::Oid>,
    ) -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        Self {
            title,
            columns: Vec::new(),
            rows: Vec::new(),
            selected_row: 0,
            col_scroll: 0,
            table_state,
            loading: true,
            error: None,
            entry_idx,
            entry_oid,
        }
    }

    /// Reset the modal to loading state (for refresh).
    pub fn reset_to_loading(&mut self) {
        self.rows.clear();
        self.selected_row = 0;
        self.col_scroll = 0;
        self.loading = true;
        self.error = None;
        self.table_state.select(Some(0));
    }

    /// Populate the modal with column and row data.
    pub fn populate(&mut self, columns: Vec<(u32, String)>, rows: Vec<(String, Vec<String>)>) {
        self.columns = columns;
        self.rows = rows;
        self.loading = false;
        self.error = None;
        self.selected_row = 0;
        self.col_scroll = 0;
        self.table_state
            .select(if self.rows.is_empty() { None } else { Some(0) });
    }

    /// Set an error message and clear loading state.
    pub fn set_error(&mut self, msg: String) {
        self.loading = false;
        self.error = Some(msg);
        self.table_state.select(None);
    }

    /// Move selection down one row.
    pub fn scroll_down(&mut self) {
        if !self.rows.is_empty() && self.selected_row + 1 < self.rows.len() {
            self.selected_row += 1;
            self.table_state.select(Some(self.selected_row));
        }
    }

    /// Move selection up one row.
    pub fn scroll_up(&mut self) {
        if self.selected_row > 0 {
            self.selected_row -= 1;
            self.table_state.select(Some(self.selected_row));
        }
    }

    /// Scroll view left (columns).
    pub fn scroll_left(&mut self) {
        if self.col_scroll > 0 {
            self.col_scroll -= 1;
        }
    }

    /// Scroll view right (columns).
    pub fn scroll_right(&mut self) {
        if !self.columns.is_empty() && self.col_scroll + 1 < self.columns.len() {
            self.col_scroll += 1;
        }
    }

    /// Jump to first row.
    pub fn jump_top(&mut self) {
        self.selected_row = 0;
        self.table_state
            .select(if self.rows.is_empty() { None } else { Some(0) });
    }

    /// Jump to last row.
    pub fn jump_bottom(&mut self) {
        if !self.rows.is_empty() {
            self.selected_row = self.rows.len() - 1;
            self.table_state.select(Some(self.selected_row));
        }
    }
}

// ============================================================
// Connection Manager Modal
// ============================================================

pub struct ConnectionManagerModal {
    pub connections: Vec<ConnectionEntry>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub viewport_height: usize,
    /// Sub-view: editing/creating a connection (reuses ConnectModal with alias).
    pub edit_view: Option<ConnectModal>,
    /// Index into `connections` being edited, or None for a new connection.
    pub editing_index: Option<usize>,
    /// Original alias of the connection being edited (for in-place update).
    pub editing_original_alias: Option<String>,
    /// Whether this was opened at startup (Esc = quit) vs mid-session (Esc = close modal).
    pub is_startup: bool,
    /// Whether a delete confirmation is pending (press 'd' twice to confirm).
    pub pending_delete: bool,
}

impl ConnectionManagerModal {
    pub fn new(
        connections: Vec<ConnectionEntry>,
        last_connection: Option<String>,
        is_startup: bool,
    ) -> Self {
        let selected = if let Some(ref alias) = last_connection {
            connections
                .iter()
                .position(|c| &c.alias == alias)
                .unwrap_or(0)
        } else {
            0
        };
        Self {
            connections,
            selected,
            scroll_offset: 0,
            viewport_height: 0,
            edit_view: None,
            editing_index: None,
            editing_original_alias: None,
            is_startup,
            pending_delete: false,
        }
    }

    pub fn scroll_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn scroll_down(&mut self) {
        if !self.connections.is_empty() && self.selected + 1 < self.connections.len() {
            self.selected += 1;
            if self.viewport_height > 0
                && self.selected >= self.scroll_offset + self.viewport_height
            {
                self.scroll_offset = self.selected - self.viewport_height + 1;
            }
        }
    }

    /// Open the edit view for a new connection with default values.
    pub fn open_new(&mut self) {
        self.editing_index = None;
        self.editing_original_alias = None;
        // Generate a random alias like "device-1234"
        let rand_num = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_millis()
            % 10000;
        let alias = format!("device-{}", rand_num);
        self.edit_view = Some(ConnectModal::new(
            &alias,
            "localhost",
            161,
            "v2c",
            "public",
            "private",
        ));
    }

    /// Open the edit view for the currently selected connection.
    pub fn open_edit(&mut self) {
        if let Some(entry) = self.connections.get(self.selected) {
            self.editing_index = Some(self.selected);
            self.editing_original_alias = Some(entry.alias.clone());
            self.edit_view = Some(ConnectModal::new(
                &entry.alias,
                &entry.host,
                entry.port,
                &entry.version,
                &entry.read_community,
                &entry.write_community,
            ));
            // Fill v3 fields if applicable
            if let Some(ref mut modal) = self.edit_view
                && entry.version == "v3"
            {
                if let Some(ref u) = entry.username {
                    modal.fields[6].value = u.clone();
                }
                if let Some(ref p) = entry.auth_protocol {
                    modal.fields[7].value = p.clone();
                }
                if let Some(ref p) = entry.auth_password {
                    modal.fields[8].value = p.clone();
                }
                if let Some(ref p) = entry.priv_protocol {
                    modal.fields[9].value = p.clone();
                }
                if let Some(ref p) = entry.priv_password {
                    modal.fields[10].value = p.clone();
                }
            }
        }
    }

    /// Delete the currently selected connection (requires two presses of 'd').
    /// First press sets `pending_delete = true`, second press performs deletion.
    pub fn delete_selected(&mut self) {
        if self.connections.is_empty() {
            return;
        }
        if self.pending_delete {
            let alias = self.connections[self.selected].alias.clone();
            self.connections.remove(self.selected);
            crate::config::delete_connection(&alias);
            if self.selected >= self.connections.len() && self.selected > 0 {
                self.selected -= 1;
            }
            self.pending_delete = false;
        } else {
            self.pending_delete = true;
        }
    }

    /// Cancel pending delete when navigating away.
    pub fn cancel_pending_delete(&mut self) {
        self.pending_delete = false;
    }

    /// Get the currently selected connection entry, if any.
    pub fn selected_entry(&self) -> Option<&ConnectionEntry> {
        self.connections.get(self.selected)
    }
}

// ============================================================
// Connect Modal (with Alias field)
// ============================================================

pub struct ConnectModal {
    pub fields: Vec<FormField>,
    pub focused_field: usize,
}

pub struct FormField {
    pub label: &'static str,
    pub value: String,
    pub kind: FieldKind,
    pub editable: bool,
}

pub enum FieldKind {
    Text,
    /// Cycle through options with Enter/Space
    Cycle(Vec<String>),
}

impl ConnectModal {
    pub fn new(
        alias: &str,
        host: &str,
        port: u16,
        version: &str,
        read_community: &str,
        write_community: &str,
    ) -> Self {
        Self {
            fields: vec![
                // 0: Alias
                FormField {
                    label: "Alias",
                    value: alias.to_string(),
                    kind: FieldKind::Text,
                    editable: true,
                },
                // 1: Host
                FormField {
                    label: "Host",
                    value: host.to_string(),
                    kind: FieldKind::Text,
                    editable: true,
                },
                // 2: Port
                FormField {
                    label: "Port",
                    value: port.to_string(),
                    kind: FieldKind::Text,
                    editable: true,
                },
                // 3: Version
                FormField {
                    label: "Version",
                    value: version.to_string(),
                    kind: FieldKind::Cycle(vec![
                        "v1".to_string(),
                        "v2c".to_string(),
                        "v3".to_string(),
                    ]),
                    editable: true,
                },
                // 4: Read Community
                FormField {
                    label: "Read Community",
                    value: read_community.to_string(),
                    kind: FieldKind::Text,
                    editable: true,
                },
                // 5: Write Community
                FormField {
                    label: "Write Community",
                    value: write_community.to_string(),
                    kind: FieldKind::Text,
                    editable: true,
                },
                // v3 fields (indices 6-10)
                FormField {
                    label: "Username",
                    value: String::new(),
                    kind: FieldKind::Text,
                    editable: true,
                },
                FormField {
                    label: "Auth Protocol",
                    value: "None".to_string(),
                    kind: FieldKind::Cycle(vec![
                        "None".to_string(),
                        "MD5".to_string(),
                        "SHA".to_string(),
                        "SHA-224".to_string(),
                        "SHA-256".to_string(),
                        "SHA-384".to_string(),
                        "SHA-512".to_string(),
                    ]),
                    editable: true,
                },
                FormField {
                    label: "Auth Password",
                    value: String::new(),
                    kind: FieldKind::Text,
                    editable: true,
                },
                FormField {
                    label: "Priv Protocol",
                    value: "None".to_string(),
                    kind: FieldKind::Cycle(vec![
                        "None".to_string(),
                        "DES".to_string(),
                        "AES-128".to_string(),
                        "AES-192".to_string(),
                        "AES-256".to_string(),
                    ]),
                    editable: true,
                },
                FormField {
                    label: "Priv Password",
                    value: String::new(),
                    kind: FieldKind::Text,
                    editable: true,
                },
            ],
            focused_field: 0,
        }
    }

    pub fn is_v3(&self) -> bool {
        self.fields[3].value == "v3"
    }

    /// Return the visible field indices based on version selection.
    pub fn visible_fields(&self) -> Vec<usize> {
        let mut indices = vec![0, 1, 2, 3]; // Alias, Host, Port, Version
        if self.is_v3() {
            // v3: show Username, Auth Protocol, Auth Password, Priv Protocol, Priv Password
            indices.extend([6, 7, 8, 9, 10]);
        } else {
            // v1/v2c: show Read Community, Write Community
            indices.extend([4, 5]);
        }
        indices
    }

    pub fn focus_next(&mut self) {
        let visible = self.visible_fields();
        if let Some(pos) = visible.iter().position(|&i| i == self.focused_field) {
            let next = (pos + 1) % visible.len();
            self.focused_field = visible[next];
        }
    }

    pub fn focus_prev(&mut self) {
        let visible = self.visible_fields();
        if let Some(pos) = visible.iter().position(|&i| i == self.focused_field) {
            let prev = if pos == 0 { visible.len() - 1 } else { pos - 1 };
            self.focused_field = visible[prev];
        }
    }

    /// Down arrow: cycle forward on Cycle fields, or move to next field on Text fields.
    pub fn arrow_down(&mut self) {
        if matches!(self.fields[self.focused_field].kind, FieldKind::Cycle(_)) {
            self.cycle_field();
        } else {
            self.focus_next();
        }
    }

    /// Up arrow: cycle backward on Cycle fields, or move to prev field on Text fields.
    pub fn arrow_up(&mut self) {
        if let FieldKind::Cycle(ref options) = self.fields[self.focused_field].kind {
            if let Some(pos) = options
                .iter()
                .position(|o| o == &self.fields[self.focused_field].value)
            {
                let prev = if pos == 0 { options.len() - 1 } else { pos - 1 };
                self.fields[self.focused_field].value = options[prev].clone();
            }
            // If version changed, ensure focused_field is still visible
            let visible = self.visible_fields();
            if !visible.contains(&self.focused_field) {
                self.focused_field = visible[0];
            }
        } else {
            self.focus_prev();
        }
    }

    pub fn type_char(&mut self, c: char) {
        let field = &mut self.fields[self.focused_field];
        if !field.editable {
            return;
        }
        match &field.kind {
            FieldKind::Text => field.value.push(c),
            FieldKind::Cycle(options) => {
                // Cycle to next option
                if let Some(pos) = options.iter().position(|o| o == &field.value) {
                    field.value = options[(pos + 1) % options.len()].clone();
                }
            }
        }
    }

    pub fn backspace(&mut self) {
        let field = &mut self.fields[self.focused_field];
        if matches!(field.kind, FieldKind::Text) {
            field.value.pop();
        }
    }

    pub fn cycle_field(&mut self) {
        let field = &mut self.fields[self.focused_field];
        if let FieldKind::Cycle(ref options) = field.kind
            && let Some(pos) = options.iter().position(|o| o == &field.value)
        {
            field.value = options[(pos + 1) % options.len()].clone();
        }
        // If version changed, ensure focused_field is still visible
        let visible = self.visible_fields();
        if !visible.contains(&self.focused_field) {
            self.focused_field = visible[0];
        }
    }

    /// Build a `ConnectionEntry` from the form fields.
    pub fn build_connection_entry(&self) -> Option<ConnectionEntry> {
        let host = self.fields[1].value.trim().to_string();
        if host.is_empty() {
            return None;
        }
        let alias = self.fields[0].value.trim().to_string();
        // Auto-generate alias if empty
        let alias = if alias.is_empty() {
            format!("{}:{}", host, self.fields[2].value.trim())
        } else {
            alias
        };

        let port: u16 = match self.fields[2].value.trim().parse() {
            Ok(p) if p > 0 => p,
            _ => return None, // Invalid port — refuse to build entry
        };
        let version = self.fields[3].value.clone();
        let read_community = self.fields[4].value.clone();
        let write_community = self.fields[5].value.clone();

        let (username, auth_protocol, auth_password, priv_protocol, priv_password) =
            if version == "v3" {
                let u = Some(self.fields[6].value.clone()).filter(|s| !s.is_empty());
                let ap = Some(self.fields[7].value.clone()).filter(|s| s != "None");
                let apw = if ap.is_some() {
                    Some(self.fields[8].value.clone())
                } else {
                    None
                };
                let pp = Some(self.fields[9].value.clone()).filter(|s| s != "None");
                let ppw = if pp.is_some() {
                    Some(self.fields[10].value.clone())
                } else {
                    None
                };
                (u, ap, apw, pp, ppw)
            } else {
                (None, None, None, None, None)
            };

        Some(ConnectionEntry {
            alias,
            host,
            port,
            version,
            read_community,
            write_community,
            username,
            auth_protocol,
            auth_password,
            priv_protocol,
            priv_password,
        })
    }
}

// ============================================================
// SET Modal
// ============================================================

/// What kind of OID node we're setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SetNodeKind {
    /// Scalar OBJECT-TYPE — auto-append `.0`.
    Scalar,
    /// Table column — user must supply a row index.
    TableColumn,
    /// Anything else — use OID as-is.
    Other,
}

/// Which field has focus in the SET modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetFieldFocus {
    Index,
    Value,
}

pub struct SetModal {
    pub oid: String,
    pub name: String,
    pub syntax_label: String,
    pub value_input: String,
    /// Hint text for what kind of value to enter.
    pub value_hint: String,
    /// The syntax type used to construct the SnmpValue.
    syntax: Option<Syntax>,
    /// What kind of node this is.
    pub node_kind: SetNodeKind,
    /// Row index input for table columns.
    pub index_input: String,
    /// Which field is focused (only meaningful for TableColumn).
    pub focus: SetFieldFocus,
}

impl SetModal {
    pub fn new(oid: String, name: String, syntax: Option<Syntax>, node_kind: SetNodeKind) -> Self {
        let syntax_label = syntax
            .as_ref()
            .map(|s| format!("{:?}", s))
            .unwrap_or_else(|| "Unknown".to_string());
        let value_hint = Self::hint_for_syntax(syntax.as_ref());
        let focus = SetFieldFocus::Value;
        Self {
            oid,
            name,
            syntax_label,
            value_input: String::new(),
            value_hint,
            syntax,
            node_kind,
            index_input: if matches!(node_kind, SetNodeKind::TableColumn) {
                "1".to_string()
            } else {
                String::new()
            },
            focus,
        }
    }

    /// Pre-fill the index field (e.g. from a previous GETNEXT result).
    pub fn prefill_index(&mut self, index: &str) {
        self.index_input = index.to_string();
    }

    fn hint_for_syntax(syntax: Option<&Syntax>) -> String {
        match syntax {
            Some(Syntax::Integer | Syntax::Integer32) => "Enter integer value".to_string(),
            Some(Syntax::Counter32 | Syntax::Gauge32 | Syntax::Unsigned32) => {
                "Enter unsigned integer".to_string()
            }
            Some(Syntax::Counter64) => "Enter 64-bit integer".to_string(),
            Some(Syntax::TimeTicks) => "Enter timeticks value".to_string(),
            Some(Syntax::OctetString) => "Enter string value".to_string(),
            Some(Syntax::IpAddress) => "Enter IP (e.g. 192.168.1.1)".to_string(),
            Some(Syntax::ObjectIdentifier) => "Enter OID (e.g. 1.3.6.1.2.1)".to_string(),
            Some(Syntax::IntegerEnum(variants)) => {
                let opts: Vec<String> = variants
                    .iter()
                    .take(5)
                    .map(|(label, val)| format!("{}({})", label, val))
                    .collect();
                format!("Values: {}", opts.join(", "))
            }
            Some(Syntax::Constrained { base, .. }) => Self::hint_for_syntax(Some(base)),
            Some(Syntax::TextualConvention(name)) => {
                if name == "Boolean" || name == "TruthValue" {
                    "Enter 0 (false) or 1 (true)".to_string()
                } else if name.contains("String") || name == "DisplayString" {
                    "Enter string value".to_string()
                } else {
                    format!("Enter {} value", name)
                }
            }
            _ => "Enter value".to_string(),
        }
    }

    pub fn type_char(&mut self, c: char) {
        match self.focus {
            SetFieldFocus::Index => self.index_input.push(c),
            SetFieldFocus::Value => self.value_input.push(c),
        }
    }

    pub fn backspace(&mut self) {
        match self.focus {
            SetFieldFocus::Index => {
                self.index_input.pop();
            }
            SetFieldFocus::Value => {
                self.value_input.pop();
            }
        }
    }

    pub fn focus_next(&mut self) {
        if self.node_kind == SetNodeKind::TableColumn {
            self.focus = match self.focus {
                SetFieldFocus::Index => SetFieldFocus::Value,
                SetFieldFocus::Value => SetFieldFocus::Index,
            };
        }
    }

    pub fn focus_prev(&mut self) {
        self.focus_next(); // only two fields, same as next
    }

    /// Whether the modal is ready to submit.
    pub fn is_ready(&self) -> bool {
        if self.node_kind == SetNodeKind::TableColumn && self.index_input.trim().is_empty() {
            return false;
        }
        !self.value_input.trim().is_empty()
    }

    /// Build an SnmpValue from the input, if valid.
    /// Returns None if input is empty or cannot be parsed for the expected type.
    pub fn build_value(&self) -> Option<SnmpValue> {
        let input = self.value_input.trim();
        if input.is_empty() {
            return None;
        }
        self.parse_value(input)
    }

    fn parse_value(&self, input: &str) -> Option<SnmpValue> {
        let base_syntax = self.base_syntax();
        match base_syntax {
            Some(Syntax::Integer | Syntax::Integer32) | Some(Syntax::IntegerEnum(_)) => {
                // Try parsing as integer
                if let Ok(v) = input.parse::<i64>() {
                    return Some(SnmpValue::Integer(v));
                }
                // Try matching enum label
                if let Some(Syntax::IntegerEnum(variants)) = &self.syntax
                    && let Some((_, val)) = variants.iter().find(|(label, _)| label == input)
                {
                    return Some(SnmpValue::Integer(*val));
                }
                // Invalid integer input — return None to signal error
                None
            }
            Some(Syntax::Counter32 | Syntax::Gauge32 | Syntax::Unsigned32) => {
                input.parse().ok().map(SnmpValue::Gauge32)
            }
            Some(Syntax::Counter64) => input.parse().ok().map(SnmpValue::Counter64),
            Some(Syntax::TimeTicks) => input.parse().ok().map(SnmpValue::TimeTicks),
            Some(Syntax::IpAddress) => {
                let parts: Vec<u8> = input.split('.').filter_map(|p| p.parse().ok()).collect();
                if parts.len() == 4 {
                    Some(SnmpValue::IpAddress([
                        parts[0], parts[1], parts[2], parts[3],
                    ]))
                } else {
                    None // Invalid IP address format
                }
            }
            Some(Syntax::ObjectIdentifier) => {
                let components: Vec<u32> =
                    input.split('.').filter_map(|p| p.parse().ok()).collect();
                if components.is_empty() {
                    None
                } else {
                    Some(SnmpValue::ObjectIdentifier(mib_parser::Oid::new(
                        components,
                    )))
                }
            }
            Some(Syntax::OctetString | Syntax::Opaque | Syntax::Bits) => {
                Some(SnmpValue::OctetString(input.as_bytes().to_vec()))
            }
            Some(Syntax::TextualConvention(name)) => {
                if name == "Boolean" || name == "TruthValue" {
                    match input.to_lowercase().as_str() {
                        "true" | "1" => Some(SnmpValue::Integer(1)),
                        "false" | "0" => Some(SnmpValue::Integer(0)),
                        _ => input.parse::<i64>().ok().map(SnmpValue::Integer),
                    }
                } else if Self::is_string_tc(name) {
                    Some(SnmpValue::OctetString(input.as_bytes().to_vec()))
                } else {
                    // Unknown TC base type — try integer, then fall back to string
                    if let Ok(v) = input.parse::<i64>() {
                        Some(SnmpValue::Integer(v))
                    } else {
                        Some(SnmpValue::OctetString(input.as_bytes().to_vec()))
                    }
                }
            }
            _ => {
                // Default: try integer, then string
                if let Ok(v) = input.parse::<i64>() {
                    Some(SnmpValue::Integer(v))
                } else {
                    Some(SnmpValue::OctetString(input.as_bytes().to_vec()))
                }
            }
        }
    }

    /// Check if a textual convention name is known to be string-based.
    fn is_string_tc(name: &str) -> bool {
        matches!(
            name,
            "DisplayString"
                | "SnmpAdminString"
                | "Utf8String"
                | "NameString"
                | "PhysAddress"
                | "MacAddress"
                | "TAddress"
                | "DateAndTime"
                | "InternationalDisplayString"
                | "OwnerString"
        ) || name.contains("String")
            || name.contains("Address")
    }

    fn base_syntax(&self) -> Option<&Syntax> {
        match &self.syntax {
            Some(Syntax::Constrained { base, .. }) => Some(base),
            other => other.as_ref(),
        }
    }

    /// Return the OID to use for the SET request.
    /// Scalar → `.0`, TableColumn → `.{index}`, Other → raw OID.
    pub fn effective_oid(&self) -> String {
        match self.node_kind {
            SetNodeKind::Scalar => format!("{}.0", self.oid),
            SetNodeKind::TableColumn => {
                let idx = self.index_input.trim();
                if idx.is_empty() {
                    self.oid.clone()
                } else {
                    format!("{}.{}", self.oid, idx)
                }
            }
            SetNodeKind::Other => self.oid.clone(),
        }
    }
}

// ============================================================
// Search Modal
// ============================================================

pub struct SearchModal {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub selected: usize,
    /// Max results to show.
    pub max_results: usize,
}

pub struct SearchResult {
    pub node_idx: NodeIndex,
    pub name: String,
    pub oid: String,
}

impl SearchModal {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            max_results: 100,
        }
    }

    pub fn type_char(&mut self, c: char, tree: &OidTree) {
        self.query.push(c);
        self.update_results(tree);
    }

    pub fn backspace(&mut self, tree: &OidTree) {
        self.query.pop();
        self.update_results(tree);
    }

    pub fn select_next(&mut self) {
        if !self.results.is_empty() && self.selected + 1 < self.results.len() {
            self.selected += 1;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn selected_node(&self) -> Option<NodeIndex> {
        self.results.get(self.selected).map(|r| r.node_idx)
    }

    fn update_results(&mut self, tree: &OidTree) {
        self.results.clear();
        self.selected = 0;

        let query = self.query.to_lowercase();
        if query.is_empty() {
            return;
        }

        // Iterate all nodes in the tree, matching by name
        for i in 0..tree.node_count() {
            let idx = NodeIndex::from_raw(i);
            if let Some(node) = tree.get(idx)
                && !node.name.is_empty()
                && node.name.to_lowercase().contains(&query)
            {
                let oid = tree
                    .resolve_oid(idx)
                    .map(|o| o.to_string())
                    .unwrap_or_default();
                self.results.push(SearchResult {
                    node_idx: idx,
                    name: node.name.clone(),
                    oid,
                });
                if self.results.len() >= self.max_results {
                    break;
                }
            }
        }
    }
}

// ============================================================
// MIB Manager Modal
// ============================================================

/// Status of a MIB file entry.
#[derive(Debug, Clone, PartialEq)]
pub enum MibFileStatus {
    Loaded,
    ParseError(String),
    ReadError(String),
}

/// Metadata for a single MIB file tracked by the application.
#[derive(Debug, Clone)]
pub struct MibFileEntry {
    pub path: PathBuf,
    pub modules: Vec<String>,
    pub object_count: usize,
    pub status: MibFileStatus,
    pub is_bundled: bool,
}

/// Which sub-view the MIB Manager is showing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MibManagerView {
    FileList,
    ObjectList,
    LoadInput,
    ConfirmUnload,
}

pub struct MibManagerModal {
    /// Snapshot of MIB files at modal open / after last operation.
    pub files: Vec<MibFileEntry>,
    /// Filtered indices into `files`.
    pub filtered: Vec<usize>,
    /// Cursor position in filtered list.
    pub selected: usize,
    pub scroll_offset: usize,
    pub viewport_height: usize,
    pub search_active: bool,
    pub search_query: String,
    pub view: MibManagerView,
    /// Sub-view: object list for a selected file.
    pub object_view: Option<ObjectListView>,
    /// Text being typed for load-new path.
    pub load_input: String,
    /// Index into `files` of the entry pending unload confirmation.
    pub unload_target: Option<usize>,
    /// Transient feedback message after an operation (message, is_error).
    pub feedback_message: Option<(String, bool)>,
}

impl MibManagerModal {
    pub fn new(files: Vec<MibFileEntry>) -> Self {
        let n = files.len();
        Self {
            files,
            filtered: (0..n).collect(),
            selected: 0,
            scroll_offset: 0,
            viewport_height: 0,
            search_active: false,
            search_query: String::new(),
            view: MibManagerView::FileList,
            object_view: None,
            load_input: String::new(),
            unload_target: None,
            feedback_message: None,
        }
    }

    /// Update the files snapshot after a rebuild, preserving cursor position.
    pub fn refresh_files(&mut self, files: Vec<MibFileEntry>) {
        let selected_path = self
            .filtered
            .get(self.selected)
            .and_then(|&i| self.files.get(i))
            .map(|e| e.path.clone());

        let n = files.len();
        self.files = files;
        self.filtered = (0..n).collect();

        if !self.search_query.is_empty() {
            self.refilter();
        }

        // Restore selection by path if possible
        if let Some(ref path) = selected_path
            && let Some(pos) = self
                .filtered
                .iter()
                .position(|&i| &self.files[i].path == path)
        {
            self.selected = pos;
            return;
        }
        // Clamp
        if !self.filtered.is_empty() && self.selected >= self.filtered.len() {
            self.selected = self.filtered.len() - 1;
        }
    }

    /// Returns true when printable characters should be routed to text input.
    pub fn is_text_input_mode(&self) -> bool {
        match self.view {
            MibManagerView::LoadInput => true,
            MibManagerView::FileList => self.search_active,
            MibManagerView::ObjectList => self
                .object_view
                .as_ref()
                .map(|ov| ov.search_active)
                .unwrap_or(false),
            MibManagerView::ConfirmUnload => false,
        }
    }

    pub fn scroll_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn scroll_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
            if self.viewport_height > 0
                && self.selected >= self.scroll_offset + self.viewport_height
            {
                self.scroll_offset = self.selected - self.viewport_height + 1;
            }
        }
    }

    pub fn jump_top(&mut self) {
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn jump_bottom(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = self.filtered.len() - 1;
            if self.viewport_height > 0 && self.selected >= self.viewport_height {
                self.scroll_offset = self.selected - self.viewport_height + 1;
            }
        }
    }

    pub fn activate_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
    }

    pub fn deactivate_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
        self.filtered = (0..self.files.len()).collect();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn search_char(&mut self, c: char) {
        self.search_query.push(c);
        self.refilter();
    }

    pub fn search_backspace(&mut self) {
        self.search_query.pop();
        self.refilter();
    }

    fn refilter(&mut self) {
        let query = self.search_query.to_lowercase();
        if query.is_empty() {
            self.filtered = (0..self.files.len()).collect();
        } else {
            self.filtered = self
                .files
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    let filename = entry
                        .path
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or("");
                    filename.to_lowercase().contains(&query)
                        || entry
                            .modules
                            .iter()
                            .any(|m| m.to_lowercase().contains(&query))
                })
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Drill into the selected file's objects.
    pub fn open_object_view(&mut self, tree: &OidTree) {
        let file_idx = match self.filtered.get(self.selected) {
            Some(&idx) => idx,
            None => return,
        };
        let file_entry = &self.files[file_idx];
        let mut objects: Vec<(String, String)> = Vec::new();
        for i in 0..tree.node_count() {
            let idx = NodeIndex::from_raw(i);
            if let Some(node) = tree.get(idx)
                && let Some(ref mib_obj) = node.mib_object
                && file_entry.modules.iter().any(|m| m == &mib_obj.module)
            {
                let oid_str = if mib_obj.oid.is_empty() {
                    String::new()
                } else {
                    mib_obj.oid.to_string()
                };
                objects.push((mib_obj.name.clone(), oid_str));
            }
        }
        objects.sort_by(|a, b| a.0.cmp(&b.0));
        let filtered = (0..objects.len()).collect();
        let module_title = match file_entry.modules.len() {
            0 => file_entry
                .path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("Unknown")
                .to_string(),
            1 => file_entry.modules[0].clone(),
            _ => format!("{}..", file_entry.modules[0]),
        };
        self.view = MibManagerView::ObjectList;
        self.object_view = Some(ObjectListView {
            module_name: module_title,
            objects,
            selected: 0,
            scroll_offset: 0,
            viewport_height: 0,
            search_active: false,
            search_query: String::new(),
            filtered,
        });
    }

    pub fn close_object_view(&mut self) {
        self.object_view = None;
        self.view = MibManagerView::FileList;
    }

    /// Return whether the currently selected file is a bundled (core) MIB.
    pub fn selected_file_is_bundled(&self) -> bool {
        let Some(&file_idx) = self.filtered.get(self.selected) else {
            return false;
        };
        self.files[file_idx].is_bundled
    }
}

pub struct ObjectListView {
    pub module_name: String,
    pub objects: Vec<(String, String)>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub viewport_height: usize,
    pub search_active: bool,
    pub search_query: String,
    pub filtered: Vec<usize>,
}

impl ObjectListView {
    pub fn scroll_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn scroll_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
            if self.viewport_height > 0
                && self.selected >= self.scroll_offset + self.viewport_height
            {
                self.scroll_offset = self.selected - self.viewport_height + 1;
            }
        }
    }

    pub fn activate_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
    }

    pub fn deactivate_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
        self.filtered = (0..self.objects.len()).collect();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn search_char(&mut self, c: char) {
        self.search_query.push(c);
        self.refilter();
    }

    pub fn search_backspace(&mut self) {
        self.search_query.pop();
        self.refilter();
    }

    fn refilter(&mut self) {
        let query = self.search_query.to_lowercase();
        if query.is_empty() {
            self.filtered = (0..self.objects.len()).collect();
        } else {
            self.filtered = self
                .objects
                .iter()
                .enumerate()
                .filter(|(_, (name, oid))| {
                    name.to_lowercase().contains(&query) || oid.contains(&query)
                })
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }
}
