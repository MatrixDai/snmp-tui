use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::ParseError;
use crate::oid::Oid;
use crate::parser::{OidComponent, RawParsedModule, parse_mib_raw, resolve_oid_components};
use crate::tree::OidTree;

/// Load MIB files from the given paths and return a unified OID tree.
///
/// This is the main public API for the parser. It:
/// 1. Reads and parses each file
/// 2. Resolves IMPORTS across modules
/// 3. Resolves symbolic OID references to numeric OIDs
/// 4. Inserts all objects into the OID tree
pub fn load_mibs(paths: &[PathBuf]) -> Result<OidTree, ParseError> {
    let mut all_modules = Vec::new();

    for path in paths {
        let source = std::fs::read_to_string(path)?;
        let modules = parse_mib_raw(&source)?;
        all_modules.extend(modules);
    }

    build_tree_from_modules(&all_modules)
}

/// Load MIBs from source strings (useful for embedded/bundled MIBs).
pub fn load_mibs_from_sources(sources: &[(&str, &str)]) -> Result<OidTree, ParseError> {
    let mut all_modules = Vec::new();

    for (name, source) in sources {
        let modules = parse_mib_raw(source)
            .map_err(|e| ParseError::Grammar(format!("Error parsing {}: {}", name, e)))?;
        all_modules.extend(modules);
    }

    build_tree_from_modules(&all_modules)
}

/// Build an OidTree from a set of parsed raw modules.
fn build_tree_from_modules(all_modules: &[RawParsedModule]) -> Result<OidTree, ParseError> {
    // Step 1: Build the name->OID resolution map
    let resolved_oids = resolve_all_module_oids(all_modules);

    // Step 2: Build the OID tree
    let mut tree = OidTree::new();

    // Insert well-known root nodes
    tree.insert(&Oid::new(vec![1]), "iso");
    tree.insert(&Oid::new(vec![1, 3]), "org");
    tree.insert(&Oid::new(vec![1, 3, 6]), "dod");
    tree.insert(&Oid::new(vec![1, 3, 6, 1]), "internet");
    tree.insert(&Oid::new(vec![1, 3, 6, 1, 1]), "directory");
    tree.insert(&Oid::new(vec![1, 3, 6, 1, 2]), "mgmt");
    tree.insert(&Oid::new(vec![1, 3, 6, 1, 2, 1]), "mib-2");
    tree.insert(&Oid::new(vec![1, 3, 6, 1, 3]), "experimental");
    tree.insert(&Oid::new(vec![1, 3, 6, 1, 4]), "private");
    tree.insert(&Oid::new(vec![1, 3, 6, 1, 4, 1]), "enterprises");
    tree.insert(&Oid::new(vec![1, 3, 6, 1, 5]), "security");
    tree.insert(&Oid::new(vec![1, 3, 6, 1, 6]), "snmpV2");
    tree.insert(&Oid::new(vec![1, 3, 6, 1, 6, 1]), "snmpDomains");
    tree.insert(&Oid::new(vec![1, 3, 6, 1, 6, 2]), "snmpProxys");
    tree.insert(&Oid::new(vec![1, 3, 6, 1, 6, 3]), "snmpModules");

    // Step 3: Insert all objects with resolved OIDs
    for module in all_modules {
        for (obj, _comps) in &module.objects {
            if let Some(oid) = resolved_oids.get(&obj.name)
                && !oid.is_empty()
            {
                let idx = tree.insert(oid, &obj.name);
                let mut resolved_obj = obj.clone();
                resolved_obj.oid = oid.clone();
                if let Some(node) = tree.get_mut(idx) {
                    node.mib_object = Some(resolved_obj);
                }
            }
        }
    }

    tree.sort_children();

    Ok(tree)
}

/// Resolve all OIDs across all modules using multi-pass resolution.
fn resolve_all_module_oids(modules: &[RawParsedModule]) -> HashMap<String, Oid> {
    let mut name_map: HashMap<String, Vec<u32>> = well_known_oids();
    let mut resolved: HashMap<String, Oid> = HashMap::new();

    // Collect all OID assignments
    let mut all_assignments: Vec<(String, Vec<OidComponent>)> = Vec::new();
    for module in modules {
        for (name, components) in &module.oid_assignments {
            all_assignments.push((name.clone(), components.clone()));
        }
        // Also add from objects that might not be in oid_assignments
        for (obj, comps) in &module.objects {
            if !comps.is_empty() && !all_assignments.iter().any(|(n, _)| n == &obj.name) {
                all_assignments.push((obj.name.clone(), comps.clone()));
            }
        }
    }

    // Multi-pass resolution
    let mut remaining = all_assignments;
    for _pass in 0..30 {
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

/// Well-known root OIDs for bootstrapping resolution.
fn well_known_oids() -> HashMap<String, Vec<u32>> {
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
