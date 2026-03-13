use std::collections::HashMap;

use crate::oid::Oid;
use crate::types::MibObject;

/// Index into the OidTree arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeIndex(usize);

/// A node in the OID tree.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub subid: u32,
    pub parent: Option<NodeIndex>,
    pub children: Vec<NodeIndex>,
    pub mib_object: Option<MibObject>,
}

/// Arena-based OID tree with O(1) lookup by OID.
#[derive(Debug, Clone)]
pub struct OidTree {
    nodes: Vec<Node>,
    root: NodeIndex,
    oid_map: HashMap<Oid, NodeIndex>,
}

impl Default for OidTree {
    fn default() -> Self {
        Self::new()
    }
}

impl OidTree {
    /// Create a new tree with a root node.
    pub fn new() -> Self {
        let root = Node {
            name: String::new(),
            subid: 0,
            parent: None,
            children: Vec::new(),
            mib_object: None,
        };
        Self {
            nodes: vec![root],
            root: NodeIndex(0),
            oid_map: HashMap::new(),
        }
    }

    /// Return the root node index.
    pub fn root(&self) -> NodeIndex {
        self.root
    }

    /// Get a reference to a node by index.
    pub fn get(&self, index: NodeIndex) -> Option<&Node> {
        self.nodes.get(index.0)
    }

    /// Get a mutable reference to a node by index.
    pub fn get_mut(&mut self, index: NodeIndex) -> Option<&mut Node> {
        self.nodes.get_mut(index.0)
    }

    /// Insert a named node at the given OID, creating intermediate nodes as needed.
    /// Returns the index of the inserted (or existing) node.
    pub fn insert(&mut self, oid: &Oid, name: &str) -> NodeIndex {
        let mut current = self.root;
        let components = oid.components();

        for (depth, &subid) in components.iter().enumerate() {
            // Check if a child with this subid already exists
            let existing = self.nodes[current.0]
                .children
                .iter()
                .find(|&&child_idx| self.nodes[child_idx.0].subid == subid)
                .copied();

            let is_leaf = depth == components.len() - 1;

            current = match existing {
                Some(child_idx) => {
                    if is_leaf {
                        self.nodes[child_idx.0].name = name.to_string();
                    }
                    child_idx
                }
                None => {
                    let node_name = if is_leaf {
                        name.to_string()
                    } else {
                        String::new()
                    };
                    let new_index = NodeIndex(self.nodes.len());
                    self.nodes.push(Node {
                        name: node_name,
                        subid,
                        parent: Some(current),
                        children: Vec::new(),
                        mib_object: None,
                    });
                    self.nodes[current.0].children.push(new_index);
                    new_index
                }
            };

            // Register every node along the path in the lookup map
            let partial_oid = Oid::new(components[..=depth].to_vec());
            self.oid_map.insert(partial_oid, current);
        }

        current
    }

    /// Look up a node index by OID.
    pub fn lookup(&self, oid: &Oid) -> Option<NodeIndex> {
        self.oid_map.get(oid).copied()
    }

    /// Resolve the full OID for a node by walking up to root.
    pub fn resolve_oid(&self, index: NodeIndex) -> Option<Oid> {
        let mut components = Vec::new();
        let mut current = index;

        // Don't resolve the root node itself
        if current == self.root {
            return Some(Oid::new(Vec::new()));
        }

        loop {
            let node = self.nodes.get(current.0)?;
            components.push(node.subid);
            match node.parent {
                Some(parent) if parent == self.root => break,
                Some(parent) => current = parent,
                None => break,
            }
        }

        components.reverse();
        Some(Oid::new(components))
    }

    /// Return the total number of nodes (including root).
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Return true if the tree contains only the root node.
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_lookup() {
        let mut tree = OidTree::new();
        let oid = Oid::new(vec![1, 3, 6, 1]);
        let idx = tree.insert(&oid, "internet");

        assert_eq!(tree.lookup(&oid), Some(idx));
        assert_eq!(tree.get(idx).unwrap().name, "internet");
    }

    #[test]
    fn intermediate_nodes_created() {
        let mut tree = OidTree::new();
        let oid = Oid::new(vec![1, 3, 6]);
        tree.insert(&oid, "dod");

        // Total nodes: root + 1 + 3 + 6
        assert_eq!(tree.len(), 4);

        // Intermediate node at 1 should exist but be unnamed
        let intermediate = tree.lookup(&Oid::new(vec![1]));
        assert!(intermediate.is_some());
        assert_eq!(tree.get(intermediate.unwrap()).unwrap().name, "");
    }

    #[test]
    fn resolve_oid_round_trip() {
        let mut tree = OidTree::new();
        let oid = Oid::new(vec![1, 3, 6, 1, 2, 1]);
        let idx = tree.insert(&oid, "mib-2");

        let resolved = tree.resolve_oid(idx).unwrap();
        assert_eq!(resolved, oid);
    }

    #[test]
    fn empty_tree() {
        let tree = OidTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 1); // root node
    }

    #[test]
    fn overlapping_inserts() {
        let mut tree = OidTree::new();
        tree.insert(&Oid::new(vec![1, 3, 6]), "dod");
        tree.insert(&Oid::new(vec![1, 3, 6, 1]), "internet");

        // Shared prefix nodes should not be duplicated
        // root + 1 + 3 + 6 + 1 = 5
        assert_eq!(tree.len(), 5);
        assert!(!tree.is_empty());
    }
}
