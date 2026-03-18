use pest::Parser;
use pest_derive::Parser;
use std::collections::HashMap;

use crate::error::ParseError;
use crate::oid::Oid;
use crate::types::{Access, MibObject, Status, Syntax};

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct MibParser;

/// A parsed MIB module with its definitions and import dependencies.
#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub name: String,
    pub objects: Vec<MibObject>,
    pub imports: Vec<ImportClause>,
    /// OID assignments: name -> parent chain (for resolving symbolic OIDs)
    pub oid_assignments: HashMap<String, Vec<OidComponent>>,
}

/// An import clause: symbols imported FROM a module.
#[derive(Debug, Clone)]
pub struct ImportClause {
    pub symbols: Vec<String>,
    pub from_module: String,
}

/// A component in an OID value notation.
#[derive(Debug, Clone)]
pub enum OidComponent {
    NameAndNumber(String, u32),
    NumberOnly(u32),
    NameOnly(String),
}

/// Parse a MIB file source string into a list of modules.
pub fn parse_mib(source: &str) -> Result<Vec<ParsedModule>, ParseError> {
    let pairs =
        MibParser::parse(Rule::mib_file, source).map_err(|e| ParseError::Grammar(e.to_string()))?;

    let mut modules = Vec::new();

    for pair in pairs {
        if pair.as_rule() == Rule::mib_file {
            for inner in pair.into_inner() {
                if inner.as_rule() == Rule::module_definition {
                    modules.push(parse_module_definition(inner)?);
                }
            }
        }
    }

    Ok(modules)
}

fn parse_module_definition(pair: pest::iterators::Pair<Rule>) -> Result<ParsedModule, ParseError> {
    let mut name = String::new();
    let mut objects = Vec::new();
    let mut imports = Vec::new();
    let mut oid_assignments: HashMap<String, Vec<OidComponent>> = HashMap::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::module_name => {
                name = inner.as_str().to_string();
            }
            Rule::module_body => {
                for body_item in inner.into_inner() {
                    match body_item.as_rule() {
                        Rule::imports_section => {
                            imports = parse_imports(body_item);
                        }
                        Rule::assignment => {
                            parse_assignment(body_item, &name, &mut objects, &mut oid_assignments);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    Ok(ParsedModule {
        name,
        objects,
        imports,
        oid_assignments,
    })
}

fn parse_imports(pair: pest::iterators::Pair<Rule>) -> Vec<ImportClause> {
    let mut clauses = Vec::new();

    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::import_clause {
            let mut symbols = Vec::new();
            let mut from_module = String::new();

            for part in inner.into_inner() {
                match part.as_rule() {
                    Rule::symbol_list => {
                        for sym in part.into_inner() {
                            if sym.as_rule() == Rule::symbol {
                                symbols.push(sym.as_str().to_string());
                            }
                        }
                    }
                    Rule::module_reference => {
                        from_module = part.as_str().to_string();
                    }
                    _ => {}
                }
            }

            clauses.push(ImportClause {
                symbols,
                from_module,
            });
        }
    }

    clauses
}

fn parse_assignment(
    pair: pest::iterators::Pair<Rule>,
    module_name: &str,
    objects: &mut Vec<MibObject>,
    oid_assignments: &mut HashMap<String, Vec<OidComponent>>,
) {
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::module_identity_def => {
                if let Some(obj) = parse_module_identity(inner, module_name) {
                    let name = obj.name.clone();
                    oid_assignments.insert(name, extract_oid_components_from_object(&obj));
                    objects.push(obj);
                }
            }
            Rule::object_type_def => {
                if let Some(obj) = parse_object_type(inner, module_name) {
                    let name = obj.name.clone();
                    oid_assignments.insert(name, extract_oid_components_from_object(&obj));
                    objects.push(obj);
                }
            }
            Rule::object_identity_def => {
                if let Some(obj) = parse_object_identity(inner, module_name) {
                    let name = obj.name.clone();
                    oid_assignments.insert(name, extract_oid_components_from_object(&obj));
                    objects.push(obj);
                }
            }
            Rule::textual_convention_def => {
                if let Some(obj) = parse_textual_convention(inner, module_name) {
                    objects.push(obj);
                }
            }
            Rule::object_identifier_assignment => {
                if let Some((obj_name, components)) =
                    parse_oid_assignment(inner, module_name, objects)
                {
                    oid_assignments.insert(obj_name, components);
                }
            }
            Rule::notification_type_def
            | Rule::object_group_def
            | Rule::module_compliance_def
            | Rule::notification_group_def
            | Rule::agent_capabilities_def => {
                if let Some(obj) = parse_generic_def_with_oid(inner, module_name) {
                    let name = obj.name.clone();
                    oid_assignments.insert(name, extract_oid_components_from_object(&obj));
                    objects.push(obj);
                }
            }
            Rule::trap_type_def => {
                // SMIv1 TRAP-TYPE — skip for now
            }
            Rule::type_assignment => {
                // Sequence definitions — handled separately if needed
            }
            Rule::value_assignment | Rule::macro_definition => {
                // Skip
            }
            _ => {}
        }
    }
}

// ---- Helpers to extract data from parse tree ----

/// Store OID components temporarily during parsing (before resolution).
#[derive(Debug, Clone)]
struct RawOidValue {
    components: Vec<OidComponent>,
}

fn parse_oid_value(pair: pest::iterators::Pair<Rule>) -> RawOidValue {
    let mut components = Vec::new();

    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::oid_component {
            for comp in inner.into_inner() {
                match comp.as_rule() {
                    Rule::named_and_number => {
                        let mut name = String::new();
                        let mut num = 0u32;
                        for part in comp.into_inner() {
                            match part.as_rule() {
                                Rule::identifier => name = part.as_str().to_string(),
                                Rule::integer_value => {
                                    num = match part.as_str().parse() {
                                        Ok(v) => v,
                                        Err(_) => continue, // Skip malformed components
                                    };
                                }
                                _ => {}
                            }
                        }
                        components.push(OidComponent::NameAndNumber(name, num));
                    }
                    Rule::number_only => {
                        let num: u32 = match comp.as_str().parse() {
                            Ok(v) => v,
                            Err(_) => continue, // Skip malformed components
                        };
                        components.push(OidComponent::NumberOnly(num));
                    }
                    Rule::name_only => {
                        components.push(OidComponent::NameOnly(comp.as_str().to_string()));
                    }
                    _ => {}
                }
            }
        }
    }

    RawOidValue { components }
}

fn extract_oid_components_from_object(obj: &MibObject) -> Vec<OidComponent> {
    // Convert back from resolved OID — this is a fallback; we store raw during parse
    obj.oid
        .components()
        .iter()
        .map(|&c| OidComponent::NumberOnly(c))
        .collect()
}

fn parse_quoted_string(s: &str) -> String {
    // Strip surrounding quotes
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn parse_syntax_type(pair: pest::iterators::Pair<Rule>) -> Syntax {
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::integer_enum_type => {
                let mut variants = Vec::new();
                for item in inner.into_inner() {
                    if item.as_rule() == Rule::enum_item {
                        let mut label = String::new();
                        let mut value = 0i64;
                        for part in item.into_inner() {
                            match part.as_rule() {
                                Rule::identifier => label = part.as_str().to_string(),
                                Rule::integer_value => {
                                    value = part.as_str().parse().unwrap_or(0);
                                }
                                _ => {}
                            }
                        }
                        variants.push((label, value));
                    }
                }
                return Syntax::IntegerEnum(variants);
            }
            Rule::bits_type => {
                let mut variants = Vec::new();
                for item in inner.into_inner() {
                    if item.as_rule() == Rule::enum_item {
                        let mut label = String::new();
                        let mut value = 0i64;
                        for part in item.into_inner() {
                            match part.as_rule() {
                                Rule::identifier => label = part.as_str().to_string(),
                                Rule::integer_value => {
                                    value = part.as_str().parse().unwrap_or(0);
                                }
                                _ => {}
                            }
                        }
                        variants.push((label, value));
                    }
                }
                return Syntax::Bits;
            }
            Rule::sequence_of_type => {
                let mut seq_name = String::new();
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::identifier {
                        seq_name = part.as_str().to_string();
                    }
                }
                return Syntax::Sequence(seq_name);
            }
            Rule::constrained_type => {
                let mut base = Syntax::Integer;
                let mut constraint = String::new();
                for part in inner.into_inner() {
                    match part.as_rule() {
                        Rule::builtin_type => {
                            base = builtin_type_to_syntax(part.as_str());
                        }
                        Rule::named_type => {
                            base = Syntax::TextualConvention(part.as_str().to_string());
                        }
                        Rule::constraint_body => {
                            constraint = part.as_str().trim().to_string();
                        }
                        _ => {}
                    }
                }
                return Syntax::Constrained {
                    base: Box::new(base),
                    constraint,
                };
            }
            Rule::named_type_with_enum => {
                // Named type with enum restriction, e.g. RowStatus { active(1) }
                let mut _tc_name = String::new();
                let mut variants = Vec::new();
                for part in inner.into_inner() {
                    match part.as_rule() {
                        Rule::named_type => {
                            _tc_name = part.as_str().to_string();
                        }
                        Rule::enum_item => {
                            let mut label = String::new();
                            let mut value = 0i64;
                            for p in part.into_inner() {
                                match p.as_rule() {
                                    Rule::identifier => label = p.as_str().to_string(),
                                    Rule::integer_value => {
                                        value = p.as_str().parse().unwrap_or(0);
                                    }
                                    _ => {}
                                }
                            }
                            variants.push((label, value));
                        }
                        _ => {}
                    }
                }
                return Syntax::IntegerEnum(variants);
            }
            Rule::builtin_type => {
                return builtin_type_to_syntax(inner.as_str());
            }
            Rule::named_type => {
                return Syntax::TextualConvention(inner.as_str().to_string());
            }
            _ => {}
        }
    }
    Syntax::Integer // fallback
}

fn builtin_type_to_syntax(s: &str) -> Syntax {
    // Normalize whitespace for multi-word types
    let normalized: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match normalized.as_str() {
        "INTEGER" => Syntax::Integer,
        "Integer32" => Syntax::Integer32,
        "OCTET STRING" => Syntax::OctetString,
        "OBJECT IDENTIFIER" => Syntax::ObjectIdentifier,
        "IpAddress" => Syntax::IpAddress,
        "Counter32" | "Counter" => Syntax::Counter32,
        "Counter64" => Syntax::Counter64,
        "Gauge32" | "Gauge" => Syntax::Gauge32,
        "Unsigned32" => Syntax::Unsigned32,
        "TimeTicks" => Syntax::TimeTicks,
        "Opaque" => Syntax::Opaque,
        "NetworkAddress" => Syntax::IpAddress,
        "BITS" => Syntax::Bits,
        "NULL" => Syntax::Integer, // placeholder
        _ => Syntax::TextualConvention(normalized),
    }
}

fn parse_access_value(s: &str) -> Access {
    match s.trim() {
        "not-accessible" => Access::NotAccessible,
        "accessible-for-notify" => Access::AccessibleForNotify,
        "read-only" => Access::ReadOnly,
        "read-write" => Access::ReadWrite,
        "read-create" => Access::ReadCreate,
        "write-only" => Access::ReadWrite, // map to read-write
        _ => Access::ReadOnly,
    }
}

fn parse_status_value(s: &str) -> Status {
    match s.trim() {
        "current" => Status::Current,
        "deprecated" => Status::Deprecated,
        "obsolete" => Status::Obsolete,
        "mandatory" => Status::Mandatory,
        "optional" => Status::Optional,
        _ => Status::Current,
    }
}

// ---- Module-level definition parsers ----

fn parse_module_identity(
    pair: pest::iterators::Pair<Rule>,
    module_name: &str,
) -> Option<MibObject> {
    let mut obj_name = String::new();
    let mut description = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => {
                if obj_name.is_empty() {
                    obj_name = inner.as_str().to_string();
                }
            }
            Rule::description_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::quoted_string {
                        description = Some(parse_quoted_string(part.as_str()));
                    }
                }
            }
            _ => {}
        }
    }

    Some(MibObject {
        name: obj_name,
        oid: Oid::new(Vec::new()), // resolved later
        module: module_name.to_string(),
        source_file: String::new(),
        syntax: None,
        access: None,
        status: Some(Status::Current),
        description,
        index_clause: None,
        defval: None,
    })
}

fn parse_object_type(pair: pest::iterators::Pair<Rule>, module_name: &str) -> Option<MibObject> {
    let mut obj_name = String::new();
    let mut syntax = None;
    let mut access = None;
    let mut status = None;
    let mut description = None;
    let mut index_clause = None;
    let mut defval = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => {
                if obj_name.is_empty() {
                    obj_name = inner.as_str().to_string();
                }
            }
            Rule::syntax_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::syntax_type {
                        syntax = Some(parse_syntax_type(part));
                    }
                }
            }
            Rule::access_clause | Rule::max_access_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::access_value {
                        access = Some(parse_access_value(part.as_str()));
                    }
                }
            }
            Rule::status_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::status_value {
                        status = Some(parse_status_value(part.as_str()));
                    }
                }
            }
            Rule::description_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::quoted_string {
                        description = Some(parse_quoted_string(part.as_str()));
                    }
                }
            }
            Rule::index_clause => {
                let mut indices = Vec::new();
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::index_entry {
                        for idx_part in part.into_inner() {
                            if idx_part.as_rule() == Rule::identifier {
                                indices.push(idx_part.as_str().to_string());
                            }
                        }
                    }
                }
                if !indices.is_empty() {
                    index_clause = Some(indices);
                }
            }
            Rule::defval_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::defval_value {
                        defval = Some(part.as_str().trim().to_string());
                    }
                }
            }
            _ => {}
        }
    }

    Some(MibObject {
        name: obj_name,
        oid: Oid::new(Vec::new()), // resolved later
        module: module_name.to_string(),
        source_file: String::new(),
        syntax,
        access,
        status,
        description,
        index_clause,
        defval,
    })
}

fn parse_object_identity(
    pair: pest::iterators::Pair<Rule>,
    module_name: &str,
) -> Option<MibObject> {
    let mut obj_name = String::new();
    let mut status = None;
    let mut description = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => {
                if obj_name.is_empty() {
                    obj_name = inner.as_str().to_string();
                }
            }
            Rule::status_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::status_value {
                        status = Some(parse_status_value(part.as_str()));
                    }
                }
            }
            Rule::description_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::quoted_string {
                        description = Some(parse_quoted_string(part.as_str()));
                    }
                }
            }
            _ => {}
        }
    }

    Some(MibObject {
        name: obj_name,
        oid: Oid::new(Vec::new()),
        module: module_name.to_string(),
        source_file: String::new(),
        syntax: None,
        access: None,
        status,
        description,
        index_clause: None,
        defval: None,
    })
}

fn parse_textual_convention(
    pair: pest::iterators::Pair<Rule>,
    module_name: &str,
) -> Option<MibObject> {
    let mut obj_name = String::new();
    let mut status = None;
    let mut description = None;
    let mut syntax = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => {
                if obj_name.is_empty() {
                    obj_name = inner.as_str().to_string();
                }
            }
            Rule::status_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::status_value {
                        status = Some(parse_status_value(part.as_str()));
                    }
                }
            }
            Rule::description_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::quoted_string {
                        description = Some(parse_quoted_string(part.as_str()));
                    }
                }
            }
            Rule::syntax_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::syntax_type {
                        syntax = Some(parse_syntax_type(part));
                    }
                }
            }
            _ => {}
        }
    }

    Some(MibObject {
        name: obj_name,
        oid: Oid::new(Vec::new()), // TCs don't have OIDs
        module: module_name.to_string(),
        source_file: String::new(),
        syntax,
        access: None,
        status,
        description,
        index_clause: None,
        defval: None,
    })
}

fn parse_oid_assignment(
    pair: pest::iterators::Pair<Rule>,
    module_name: &str,
    objects: &mut Vec<MibObject>,
) -> Option<(String, Vec<OidComponent>)> {
    let mut obj_name = String::new();
    let mut raw_oid = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => {
                if obj_name.is_empty() {
                    obj_name = inner.as_str().to_string();
                }
            }
            Rule::oid_value => {
                raw_oid = Some(parse_oid_value(inner));
            }
            _ => {}
        }
    }

    let raw = raw_oid?;
    let components = raw.components.clone();

    objects.push(MibObject {
        name: obj_name.clone(),
        oid: Oid::new(Vec::new()), // resolved later
        module: module_name.to_string(),
        source_file: String::new(),
        syntax: None,
        access: None,
        status: None,
        description: None,
        index_clause: None,
        defval: None,
    });

    Some((obj_name, components))
}

fn parse_generic_def_with_oid(
    pair: pest::iterators::Pair<Rule>,
    module_name: &str,
) -> Option<MibObject> {
    let mut obj_name = String::new();
    let mut status = None;
    let mut description = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => {
                if obj_name.is_empty() {
                    obj_name = inner.as_str().to_string();
                }
            }
            Rule::status_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::status_value {
                        status = Some(parse_status_value(part.as_str()));
                    }
                }
            }
            Rule::description_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::quoted_string {
                        description = Some(parse_quoted_string(part.as_str()));
                    }
                }
            }
            _ => {}
        }
    }

    Some(MibObject {
        name: obj_name,
        oid: Oid::new(Vec::new()),
        module: module_name.to_string(),
        source_file: String::new(),
        syntax: None,
        access: None,
        status,
        description,
        index_clause: None,
        defval: None,
    })
}

// ============================================================
// OID Resolution
// ============================================================

/// Well-known root OIDs for bootstrapping resolution.
pub fn well_known_oids() -> HashMap<String, Vec<u32>> {
    let mut m = HashMap::new();
    m.insert("iso".to_string(), vec![1]);
    m.insert("org".to_string(), vec![1, 3]);
    m.insert("dod".to_string(), vec![1, 3, 6]);
    m.insert("internet".to_string(), vec![1, 3, 6, 1]);
    m.insert("directory".to_string(), vec![1, 3, 6, 1, 1]);
    m.insert("mgmt".to_string(), vec![1, 3, 6, 1, 2]);
    m.insert("mib-2".to_string(), vec![1, 3, 6, 1, 2, 1]);
    m.insert("transmission".to_string(), vec![1, 3, 6, 1, 2, 1, 10]);
    m.insert("experimental".to_string(), vec![1, 3, 6, 1, 3]);
    m.insert("private".to_string(), vec![1, 3, 6, 1, 4]);
    m.insert("enterprises".to_string(), vec![1, 3, 6, 1, 4, 1]);
    m.insert("security".to_string(), vec![1, 3, 6, 1, 5]);
    m.insert("snmpV2".to_string(), vec![1, 3, 6, 1, 6]);
    m.insert("snmpDomains".to_string(), vec![1, 3, 6, 1, 6, 1]);
    m.insert("snmpProxys".to_string(), vec![1, 3, 6, 1, 6, 2]);
    m.insert("snmpModules".to_string(), vec![1, 3, 6, 1, 6, 3]);
    m.insert("zeroDotZero".to_string(), vec![0, 0]);
    m
}

/// Resolve OID components to a numeric OID using the name->OID map.
pub fn resolve_oid_components(
    components: &[OidComponent],
    name_map: &HashMap<String, Vec<u32>>,
) -> Option<Oid> {
    let mut result = Vec::new();

    for (i, comp) in components.iter().enumerate() {
        match comp {
            OidComponent::NameAndNumber(name, num) => {
                if i == 0 {
                    // First component: try to resolve the name
                    if let Some(parent_oid) = name_map.get(name) {
                        result.extend_from_slice(parent_oid);
                    } else {
                        // Name not found; use just the number
                        result.push(*num);
                    }
                } else {
                    result.push(*num);
                }
            }
            OidComponent::NumberOnly(num) => {
                result.push(*num);
            }
            OidComponent::NameOnly(name) => {
                if let Some(parent_oid) = name_map.get(name) {
                    result.extend_from_slice(parent_oid);
                } else {
                    return None; // Can't resolve
                }
            }
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(Oid::new(result))
    }
}

/// Resolve all OIDs in a set of parsed modules. Returns a map from object name to resolved OID.
pub fn resolve_all_oids(modules: &[ParsedModule]) -> HashMap<String, Oid> {
    let mut name_map: HashMap<String, Vec<u32>> = well_known_oids();
    let mut resolved: HashMap<String, Oid> = HashMap::new();

    // Collect all OID assignments from all modules
    let mut all_assignments: Vec<(String, Vec<OidComponent>)> = Vec::new();
    for module in modules {
        for (name, components) in &module.oid_assignments {
            all_assignments.push((name.clone(), components.clone()));
        }
        // Also add object names from objects that have OID components stored
        // (they are in oid_assignments already via parse_assignment)
    }

    // Multi-pass resolution: keep trying until no more progress
    let mut remaining = all_assignments;
    for _pass in 0..20 {
        let mut still_unresolved = Vec::new();
        let mut made_progress = false;

        for (name, components) in remaining {
            if let Some(oid) = resolve_oid_components(&components, &name_map) {
                name_map.insert(name.clone(), oid.components().to_vec());
                resolved.insert(name, oid);
                made_progress = true;
            } else {
                still_unresolved.push((name, components));
            }
        }

        if !made_progress || still_unresolved.is_empty() {
            break;
        }
        remaining = still_unresolved;
    }

    resolved
}

/// Store raw OID components alongside objects during parsing.
/// This is needed because we parse the OID value but can't resolve it
/// until all modules are loaded.
#[derive(Debug, Clone)]
pub struct RawParsedModule {
    pub name: String,
    pub source_file: String,
    pub objects: Vec<(MibObject, Vec<OidComponent>)>,
    pub imports: Vec<ImportClause>,
    pub oid_assignments: HashMap<String, Vec<OidComponent>>,
}

/// Parse a MIB file into raw modules with unresolved OID components.
pub fn parse_mib_raw(source: &str) -> Result<Vec<RawParsedModule>, ParseError> {
    let pairs =
        MibParser::parse(Rule::mib_file, source).map_err(|e| ParseError::Grammar(e.to_string()))?;

    let mut modules = Vec::new();

    for pair in pairs {
        if pair.as_rule() == Rule::mib_file {
            for inner in pair.into_inner() {
                if inner.as_rule() == Rule::module_definition {
                    modules.push(parse_module_definition_raw(inner)?);
                }
            }
        }
    }

    Ok(modules)
}

fn parse_module_definition_raw(
    pair: pest::iterators::Pair<Rule>,
) -> Result<RawParsedModule, ParseError> {
    let mut name = String::new();
    let mut objects: Vec<(MibObject, Vec<OidComponent>)> = Vec::new();
    let mut imports = Vec::new();
    let mut oid_assignments: HashMap<String, Vec<OidComponent>> = HashMap::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::module_name => {
                name = inner.as_str().to_string();
            }
            Rule::module_body => {
                for body_item in inner.into_inner() {
                    match body_item.as_rule() {
                        Rule::imports_section => {
                            imports = parse_imports(body_item);
                        }
                        Rule::assignment => {
                            parse_assignment_raw(
                                body_item,
                                &name,
                                &mut objects,
                                &mut oid_assignments,
                            );
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    Ok(RawParsedModule {
        name,
        source_file: String::new(),
        objects,
        imports,
        oid_assignments,
    })
}

fn parse_assignment_raw(
    pair: pest::iterators::Pair<Rule>,
    module_name: &str,
    objects: &mut Vec<(MibObject, Vec<OidComponent>)>,
    oid_assignments: &mut HashMap<String, Vec<OidComponent>>,
) {
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::module_identity_def => {
                if let Some((obj, comps)) = parse_def_with_oid_raw(inner, module_name, true) {
                    let name = obj.name.clone();
                    oid_assignments.insert(name, comps.clone());
                    objects.push((obj, comps));
                }
            }
            Rule::object_type_def => {
                if let Some((obj, comps)) = parse_object_type_raw(inner, module_name) {
                    let name = obj.name.clone();
                    oid_assignments.insert(name, comps.clone());
                    objects.push((obj, comps));
                }
            }
            Rule::object_identity_def => {
                if let Some((obj, comps)) = parse_def_with_oid_raw(inner, module_name, false) {
                    let name = obj.name.clone();
                    oid_assignments.insert(name, comps.clone());
                    objects.push((obj, comps));
                }
            }
            Rule::textual_convention_def => {
                if let Some(obj) = parse_textual_convention(inner, module_name) {
                    objects.push((obj, Vec::new()));
                }
            }
            Rule::object_identifier_assignment => {
                let mut obj_name = String::new();
                let mut raw_oid = None;

                for part in inner.into_inner() {
                    match part.as_rule() {
                        Rule::identifier => {
                            if obj_name.is_empty() {
                                obj_name = part.as_str().to_string();
                            }
                        }
                        Rule::oid_value => {
                            raw_oid = Some(parse_oid_value(part));
                        }
                        _ => {}
                    }
                }

                if let Some(raw) = raw_oid {
                    let comps = raw.components;
                    oid_assignments.insert(obj_name.clone(), comps.clone());
                    objects.push((
                        MibObject {
                            name: obj_name,
                            oid: Oid::new(Vec::new()),
                            module: module_name.to_string(),
                            source_file: String::new(),
                            syntax: None,
                            access: None,
                            status: None,
                            description: None,
                            index_clause: None,
                            defval: None,
                        },
                        comps,
                    ));
                }
            }
            Rule::notification_type_def
            | Rule::object_group_def
            | Rule::module_compliance_def
            | Rule::notification_group_def
            | Rule::agent_capabilities_def => {
                if let Some((obj, comps)) = parse_def_with_oid_raw(inner, module_name, false) {
                    let name = obj.name.clone();
                    oid_assignments.insert(name, comps.clone());
                    objects.push((obj, comps));
                }
            }
            _ => {}
        }
    }
}

fn parse_def_with_oid_raw(
    pair: pest::iterators::Pair<Rule>,
    module_name: &str,
    is_module_identity: bool,
) -> Option<(MibObject, Vec<OidComponent>)> {
    let mut obj_name = String::new();
    let mut status = None;
    let mut description = None;
    let mut raw_oid = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => {
                if obj_name.is_empty() {
                    obj_name = inner.as_str().to_string();
                }
            }
            Rule::status_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::status_value {
                        status = Some(parse_status_value(part.as_str()));
                    }
                }
            }
            Rule::description_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::quoted_string {
                        description = Some(parse_quoted_string(part.as_str()));
                    }
                }
            }
            Rule::oid_value => {
                raw_oid = Some(parse_oid_value(inner));
            }
            _ => {}
        }
    }

    let raw = raw_oid?;
    let comps = raw.components;

    Some((
        MibObject {
            name: obj_name,
            oid: Oid::new(Vec::new()),
            module: module_name.to_string(),
            source_file: String::new(),
            syntax: None,
            access: None,
            status: if is_module_identity {
                Some(Status::Current)
            } else {
                status
            },
            description,
            index_clause: None,
            defval: None,
        },
        comps,
    ))
}

fn parse_object_type_raw(
    pair: pest::iterators::Pair<Rule>,
    module_name: &str,
) -> Option<(MibObject, Vec<OidComponent>)> {
    let mut obj_name = String::new();
    let mut syntax = None;
    let mut access = None;
    let mut status = None;
    let mut description = None;
    let mut index_clause = None;
    let mut defval = None;
    let mut raw_oid = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => {
                if obj_name.is_empty() {
                    obj_name = inner.as_str().to_string();
                }
            }
            Rule::syntax_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::syntax_type {
                        syntax = Some(parse_syntax_type(part));
                    }
                }
            }
            Rule::access_clause | Rule::max_access_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::access_value {
                        access = Some(parse_access_value(part.as_str()));
                    }
                }
            }
            Rule::status_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::status_value {
                        status = Some(parse_status_value(part.as_str()));
                    }
                }
            }
            Rule::description_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::quoted_string {
                        description = Some(parse_quoted_string(part.as_str()));
                    }
                }
            }
            Rule::index_clause => {
                let mut indices = Vec::new();
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::index_entry {
                        for idx_part in part.into_inner() {
                            if idx_part.as_rule() == Rule::identifier {
                                indices.push(idx_part.as_str().to_string());
                            }
                        }
                    }
                }
                if !indices.is_empty() {
                    index_clause = Some(indices);
                }
            }
            Rule::defval_clause => {
                for part in inner.into_inner() {
                    if part.as_rule() == Rule::defval_value {
                        defval = Some(part.as_str().trim().to_string());
                    }
                }
            }
            Rule::oid_value => {
                raw_oid = Some(parse_oid_value(inner));
            }
            _ => {}
        }
    }

    let raw = raw_oid?;
    let comps = raw.components;

    Some((
        MibObject {
            name: obj_name,
            oid: Oid::new(Vec::new()),
            module: module_name.to_string(),
            source_file: String::new(),
            syntax,
            access,
            status,
            description,
            index_clause,
            defval,
        },
        comps,
    ))
}
