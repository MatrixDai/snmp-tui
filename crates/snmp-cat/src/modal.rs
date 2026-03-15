use mib_parser::{NodeIndex, OidTree, Syntax};
use snmp_client::SnmpValue;

use crate::config::ConnectionEntry;

/// Active modal dialog.
pub enum Modal {
    ConnectionManager(ConnectionManagerModal),
    Set(SetModal),
    Search(SearchModal),
    MibInfo(MibInfoModal),
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
        self.edit_view = Some(ConnectModal::new(&alias, "localhost", 161, "v2c", "public"));
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
                &entry.community,
            ));
            // Fill v3 fields if applicable
            if let Some(ref mut modal) = self.edit_view
                && entry.version == "v3"
            {
                if let Some(ref u) = entry.username {
                    modal.fields[5].value = u.clone();
                }
                if let Some(ref p) = entry.auth_protocol {
                    modal.fields[6].value = p.clone();
                }
                if let Some(ref p) = entry.auth_password {
                    modal.fields[7].value = p.clone();
                }
                if let Some(ref p) = entry.priv_protocol {
                    modal.fields[8].value = p.clone();
                }
                if let Some(ref p) = entry.priv_password {
                    modal.fields[9].value = p.clone();
                }
            }
        }
    }

    /// Delete the currently selected connection.
    pub fn delete_selected(&mut self) {
        if !self.connections.is_empty() {
            let alias = self.connections[self.selected].alias.clone();
            self.connections.remove(self.selected);
            crate::config::delete_connection(&alias);
            if self.selected >= self.connections.len() && self.selected > 0 {
                self.selected -= 1;
            }
        }
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
    pub fn new(alias: &str, host: &str, port: u16, version: &str, community: &str) -> Self {
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
                // 4: Community
                FormField {
                    label: "Community",
                    value: community.to_string(),
                    kind: FieldKind::Text,
                    editable: true,
                },
                // v3 fields (indices 5-9)
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
            indices.extend([5, 6, 7, 8, 9]);
        } else {
            // v1/v2c: show Community
            indices.push(4);
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

        let port: u16 = self.fields[2].value.trim().parse().unwrap_or(161);
        let version = self.fields[3].value.clone();
        let community = self.fields[4].value.clone();

        let (username, auth_protocol, auth_password, priv_protocol, priv_password) =
            if version == "v3" {
                let u = Some(self.fields[5].value.clone()).filter(|s| !s.is_empty());
                let ap = Some(self.fields[6].value.clone()).filter(|s| s != "None");
                let apw = if ap.is_some() {
                    Some(self.fields[7].value.clone())
                } else {
                    None
                };
                let pp = Some(self.fields[8].value.clone()).filter(|s| s != "None");
                let ppw = if pp.is_some() {
                    Some(self.fields[9].value.clone())
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
            community,
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

pub struct SetModal {
    pub oid: String,
    pub name: String,
    pub syntax_label: String,
    pub value_input: String,
    /// Hint text for what kind of value to enter.
    pub value_hint: String,
    /// The syntax type used to construct the SnmpValue.
    syntax: Option<Syntax>,
    /// Whether this looks like a scalar OID (auto-append .0).
    pub is_scalar: bool,
}

impl SetModal {
    pub fn new(oid: String, name: String, syntax: Option<Syntax>, is_scalar: bool) -> Self {
        let syntax_label = syntax
            .as_ref()
            .map(|s| format!("{:?}", s))
            .unwrap_or_else(|| "Unknown".to_string());
        let value_hint = Self::hint_for_syntax(syntax.as_ref());
        Self {
            oid,
            name,
            syntax_label,
            value_input: String::new(),
            value_hint,
            syntax,
            is_scalar,
        }
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
                if name.contains("String") || name == "DisplayString" {
                    "Enter string value".to_string()
                } else {
                    format!("Enter {} value", name)
                }
            }
            _ => "Enter value".to_string(),
        }
    }

    pub fn type_char(&mut self, c: char) {
        self.value_input.push(c);
    }

    pub fn backspace(&mut self) {
        self.value_input.pop();
    }

    /// Build an SnmpValue from the input, if valid.
    pub fn build_value(&self) -> Option<SnmpValue> {
        let input = self.value_input.trim();
        if input.is_empty() {
            return None;
        }
        Some(self.parse_value(input))
    }

    fn parse_value(&self, input: &str) -> SnmpValue {
        let base_syntax = self.base_syntax();
        match base_syntax {
            Some(Syntax::Integer | Syntax::Integer32) | Some(Syntax::IntegerEnum(_)) => {
                // Try parsing as integer; if it fails, try matching enum label
                if let Ok(v) = input.parse::<i64>() {
                    return SnmpValue::Integer(v);
                }
                if let Some(Syntax::IntegerEnum(variants)) = &self.syntax
                    && let Some((_, val)) = variants.iter().find(|(label, _)| label == input)
                {
                    return SnmpValue::Integer(*val);
                }
                // Fall back to integer parse attempt
                SnmpValue::Integer(input.parse().unwrap_or(0))
            }
            Some(Syntax::Counter32 | Syntax::Gauge32 | Syntax::Unsigned32) => {
                SnmpValue::Gauge32(input.parse().unwrap_or(0))
            }
            Some(Syntax::Counter64) => SnmpValue::Counter64(input.parse().unwrap_or(0)),
            Some(Syntax::TimeTicks) => SnmpValue::TimeTicks(input.parse().unwrap_or(0)),
            Some(Syntax::IpAddress) => {
                let parts: Vec<u8> = input.split('.').filter_map(|p| p.parse().ok()).collect();
                if parts.len() == 4 {
                    SnmpValue::IpAddress([parts[0], parts[1], parts[2], parts[3]])
                } else {
                    SnmpValue::OctetString(input.as_bytes().to_vec())
                }
            }
            Some(Syntax::ObjectIdentifier) => {
                let components: Vec<u32> =
                    input.split('.').filter_map(|p| p.parse().ok()).collect();
                SnmpValue::ObjectIdentifier(mib_parser::Oid::new(components))
            }
            _ => {
                // Default: try integer, then string
                if let Ok(v) = input.parse::<i64>() {
                    SnmpValue::Integer(v)
                } else {
                    SnmpValue::OctetString(input.as_bytes().to_vec())
                }
            }
        }
    }

    fn base_syntax(&self) -> Option<&Syntax> {
        match &self.syntax {
            Some(Syntax::Constrained { base, .. }) => Some(base),
            other => other.as_ref(),
        }
    }

    /// Return the OID to use for the SET request, appending .0 for scalars.
    pub fn effective_oid(&self) -> String {
        if self.is_scalar && !self.oid.is_empty() {
            format!("{}.0", self.oid)
        } else {
            self.oid.clone()
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
// MIB Info Modal
// ============================================================

pub struct MibInfoModal {
    /// Full list of (module_name, object_count, source_file).
    pub modules: Vec<(String, usize, String)>,
    /// Filtered view (indices into `modules`) when searching.
    pub filtered: Vec<usize>,
    /// Total object count across all modules.
    pub total_objects: usize,
    /// Cursor position in filtered list.
    pub selected: usize,
    /// Scroll offset for the list.
    pub scroll_offset: usize,
    /// Viewport height (set during render).
    pub viewport_height: usize,
    /// Search state.
    pub search_active: bool,
    pub search_query: String,
    /// Sub-view: object list for a selected module.
    pub object_view: Option<ObjectListView>,
}

impl MibInfoModal {
    pub fn new(tree: &OidTree) -> Self {
        let mut module_map: std::collections::BTreeMap<String, (usize, String)> =
            std::collections::BTreeMap::new();
        for i in 0..tree.node_count() {
            let idx = NodeIndex::from_raw(i);
            if let Some(node) = tree.get(idx)
                && let Some(ref mib_obj) = node.mib_object
                && !mib_obj.module.is_empty()
            {
                let entry = module_map
                    .entry(mib_obj.module.clone())
                    .or_insert((0, mib_obj.source_file.clone()));
                entry.0 += 1;
            }
        }
        let total_objects = module_map.values().map(|(c, _)| c).sum();
        let modules: Vec<_> = module_map
            .into_iter()
            .map(|(name, (count, file))| (name, count, file))
            .collect();
        let filtered = (0..modules.len()).collect();
        Self {
            modules,
            filtered,
            total_objects,
            selected: 0,
            scroll_offset: 0,
            viewport_height: 0,
            search_active: false,
            search_query: String::new(),
            object_view: None,
        }
    }

    pub fn scroll_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            // Adjust scroll to keep selected visible
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn scroll_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
            // Adjust scroll to keep selected visible
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
        self.filtered = (0..self.modules.len()).collect();
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
            self.filtered = (0..self.modules.len()).collect();
        } else {
            self.filtered = self
                .modules
                .iter()
                .enumerate()
                .filter(|(_, (name, _, _))| name.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn open_object_view(&mut self, tree: &OidTree) {
        let module_idx = match self.filtered.get(self.selected) {
            Some(&idx) => idx,
            None => return,
        };
        let module_name = &self.modules[module_idx].0;
        let mut objects: Vec<(String, String)> = Vec::new();
        for i in 0..tree.node_count() {
            let idx = NodeIndex::from_raw(i);
            if let Some(node) = tree.get(idx)
                && let Some(ref mib_obj) = node.mib_object
                && mib_obj.module == *module_name
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
        self.object_view = Some(ObjectListView {
            module_name: module_name.clone(),
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
