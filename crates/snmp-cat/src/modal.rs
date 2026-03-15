use mib_parser::{NodeIndex, OidTree, Syntax};
use snmp_client::{AuthProtocol, PrivProtocol, SnmpConfig, SnmpValue, SnmpVersion, V3Credentials};

/// Active modal dialog.
pub enum Modal {
    Connect(ConnectModal),
    Set(SetModal),
    Search(SearchModal),
    MibInfo(MibInfoModal),
}

// ============================================================
// Connect Modal
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
    pub fn new(host: &str, port: u16, version: &str, community: &str) -> Self {
        Self {
            fields: vec![
                FormField {
                    label: "Host",
                    value: host.to_string(),
                    kind: FieldKind::Text,
                    editable: true,
                },
                FormField {
                    label: "Port",
                    value: port.to_string(),
                    kind: FieldKind::Text,
                    editable: true,
                },
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
                FormField {
                    label: "Community",
                    value: community.to_string(),
                    kind: FieldKind::Text,
                    editable: true,
                },
                // v3 fields (indices 4-8)
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
        self.fields[2].value == "v3"
    }

    /// Return the visible field indices based on version selection.
    pub fn visible_fields(&self) -> Vec<usize> {
        let mut indices = vec![0, 1, 2]; // Host, Port, Version
        if self.is_v3() {
            // v3: show Username, Auth Protocol, Auth Password, Priv Protocol, Priv Password
            indices.extend([4, 5, 6, 7, 8]);
        } else {
            // v1/v2c: show Community
            indices.push(3);
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

    /// Build an SnmpConfig from the form fields.
    pub fn build_config(&self) -> Option<SnmpConfig> {
        let host = self.fields[0].value.trim().to_string();
        if host.is_empty() {
            return None;
        }
        let port: u16 = self.fields[1].value.trim().parse().unwrap_or(161);
        let version = match self.fields[2].value.as_str() {
            "v1" => SnmpVersion::V1,
            "v3" => SnmpVersion::V3,
            _ => SnmpVersion::V2c,
        };
        let community = self.fields[3].value.clone();

        let v3_credentials = if version == SnmpVersion::V3 {
            let username = self.fields[4].value.clone();
            let auth_protocol = match self.fields[5].value.as_str() {
                "MD5" => Some(AuthProtocol::Md5),
                "SHA" => Some(AuthProtocol::Sha1),
                "SHA-224" => Some(AuthProtocol::Sha224),
                "SHA-256" => Some(AuthProtocol::Sha256),
                "SHA-384" => Some(AuthProtocol::Sha384),
                "SHA-512" => Some(AuthProtocol::Sha512),
                _ => None,
            };
            let auth_password = if auth_protocol.is_some() {
                Some(self.fields[6].value.clone())
            } else {
                None
            };
            let priv_protocol = match self.fields[7].value.as_str() {
                "DES" => Some(PrivProtocol::Des),
                "AES-128" => Some(PrivProtocol::Aes128),
                "AES-192" => Some(PrivProtocol::Aes192),
                "AES-256" => Some(PrivProtocol::Aes256),
                _ => None,
            };
            let priv_password = if priv_protocol.is_some() {
                Some(self.fields[8].value.clone())
            } else {
                None
            };
            Some(V3Credentials {
                username,
                auth_protocol,
                auth_password,
                priv_protocol,
                priv_password,
            })
        } else {
            None
        };

        Some(SnmpConfig {
            host,
            port,
            version,
            community,
            timeout_ms: 5000,
            retries: 1,
            v3_credentials,
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
