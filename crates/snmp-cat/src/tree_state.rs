use std::collections::HashSet;

use mib_parser::{NodeIndex, OidTree};

/// State for the collapsible MIB tree panel.
pub struct TreeState {
    /// Index into the `visible` list (the highlighted row).
    pub selected: usize,
    /// Set of expanded node indices.
    expanded: HashSet<NodeIndex>,
    /// Scroll offset (first visible row in the viewport).
    pub scroll_offset: usize,
    /// Flattened list of currently visible nodes: (NodeIndex, depth).
    visible: Vec<(NodeIndex, usize)>,
    /// Whether `g` was pressed as a prefix key (for `gg` command).
    pub pending_g: bool,
}

impl TreeState {
    /// Build initial tree state. Expands the root so its children are visible.
    pub fn new(tree: &OidTree) -> Self {
        let mut expanded = HashSet::new();
        expanded.insert(tree.root());

        let mut state = Self {
            selected: 0,
            expanded,
            scroll_offset: 0,
            visible: Vec::new(),
            pending_g: false,
        };
        state.rebuild_visible(tree);
        state
    }

    /// Rebuild the flattened visible-node list via DFS.
    pub fn rebuild_visible(&mut self, tree: &OidTree) {
        self.visible.clear();
        self.flatten_dfs(tree, tree.root(), 0);

        if !self.visible.is_empty() && self.selected >= self.visible.len() {
            self.selected = self.visible.len() - 1;
        }
    }

    fn flatten_dfs(&mut self, tree: &OidTree, index: NodeIndex, depth: usize) {
        let node = match tree.get(index) {
            Some(n) => n,
            None => return,
        };

        // Skip the root node itself
        if index != tree.root() {
            self.visible.push((index, depth));
        }

        if self.expanded.contains(&index) {
            for &child_idx in &node.children {
                let child_depth = if index == tree.root() {
                    depth
                } else {
                    depth + 1
                };
                self.flatten_dfs(tree, child_idx, child_depth);
            }
        }
    }

    /// Get the currently visible nodes as (NodeIndex, depth) pairs.
    pub fn visible_nodes(&self) -> &[(NodeIndex, usize)] {
        &self.visible
    }

    /// Get the number of visible nodes.
    #[cfg(test)]
    pub fn visible_count(&self) -> usize {
        self.visible.len()
    }

    /// Get the NodeIndex of the currently selected node (if any).
    pub fn selected_node(&self) -> Option<NodeIndex> {
        self.visible.get(self.selected).map(|&(idx, _)| idx)
    }

    /// Move selection down by one.
    pub fn move_down(&mut self) {
        if self.selected + 1 < self.visible.len() {
            self.selected += 1;
        }
    }

    /// Move selection up by one.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Jump to the first node.
    pub fn jump_top(&mut self) {
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Jump to the last node.
    pub fn jump_bottom(&mut self) {
        if !self.visible.is_empty() {
            self.selected = self.visible.len() - 1;
        }
    }

    /// Expand the selected node (if it has children and is collapsed).
    pub fn expand(&mut self, tree: &OidTree) -> bool {
        if let Some(&(idx, _)) = self.visible.get(self.selected)
            && let Some(node) = tree.get(idx)
            && !node.children.is_empty()
            && !self.expanded.contains(&idx)
        {
            self.expanded.insert(idx);
            self.rebuild_visible(tree);
            return true;
        }
        false
    }

    /// Collapse the selected node. If already collapsed (or a leaf), move to parent.
    pub fn collapse(&mut self, tree: &OidTree) -> bool {
        if let Some(&(idx, _)) = self.visible.get(self.selected)
            && let Some(node) = tree.get(idx)
        {
            if self.expanded.contains(&idx) && !node.children.is_empty() {
                self.expanded.remove(&idx);
                self.rebuild_visible(tree);
                return true;
            }
            // Move to parent
            if let Some(parent_idx) = node.parent
                && parent_idx != tree.root()
                && let Some(pos) = self.visible.iter().position(|&(vi, _)| vi == parent_idx)
            {
                self.selected = pos;
                return true;
            }
        }
        false
    }

    /// Toggle expand/collapse of the selected node.
    #[allow(dead_code)]
    pub fn toggle(&mut self, tree: &OidTree) {
        if let Some(&(idx, _)) = self.visible.get(self.selected)
            && let Some(node) = tree.get(idx)
            && !node.children.is_empty()
        {
            if self.expanded.contains(&idx) {
                self.expanded.remove(&idx);
            } else {
                self.expanded.insert(idx);
            }
            self.rebuild_visible(tree);
        }
    }

    /// Adjust scroll offset so the selected row is visible within the viewport.
    pub fn ensure_visible(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.selected - viewport_height + 1;
        }
    }

    /// Check if a node is expanded.
    pub fn is_expanded(&self, idx: NodeIndex) -> bool {
        self.expanded.contains(&idx)
    }

    /// Navigate to a specific node: expand all ancestors and select it.
    pub fn navigate_to(&mut self, target: NodeIndex, tree: &OidTree) {
        // Walk up from target to root, collecting ancestors
        let mut ancestors = Vec::new();
        let mut current = target;
        while let Some(node) = tree.get(current) {
            if let Some(parent) = node.parent {
                ancestors.push(parent);
                current = parent;
            } else {
                break;
            }
        }

        // Expand all ancestors
        for ancestor in ancestors {
            self.expanded.insert(ancestor);
        }

        // Rebuild visible list
        self.rebuild_visible(tree);

        // Find target in visible list and select it
        if let Some(pos) = self.visible.iter().position(|&(idx, _)| idx == target) {
            self.selected = pos;
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
        tree.insert(&Oid::new(vec![1, 3, 6, 1]), "internet");
        tree.insert(&Oid::new(vec![1, 3, 6, 1, 2]), "mgmt");
        tree.insert(&Oid::new(vec![1, 3, 6, 1, 4]), "private");
        tree.sort_children();
        tree
    }

    #[test]
    fn initial_state_shows_root_children() {
        let tree = make_test_tree();
        let state = TreeState::new(&tree);
        assert!(state.visible_count() > 0);
        let first = state.selected_node().unwrap();
        assert_eq!(tree.get(first).unwrap().name, "iso");
    }

    #[test]
    fn move_down_and_up() {
        let tree = make_test_tree();
        let mut state = TreeState::new(&tree);
        assert_eq!(state.selected, 0);

        // Only "iso" is visible initially
        state.move_down();
        assert_eq!(state.selected, 0);

        // Expand iso → shows org
        state.expand(&tree);
        assert!(state.visible_count() > 1);

        state.move_down();
        assert_eq!(state.selected, 1);

        state.move_up();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn expand_and_collapse() {
        let tree = make_test_tree();
        let mut state = TreeState::new(&tree);

        let initial_count = state.visible_count();
        assert_eq!(initial_count, 1);

        state.expand(&tree);
        assert!(state.visible_count() > initial_count);

        state.collapse(&tree);
        assert_eq!(state.visible_count(), initial_count);
    }

    #[test]
    fn jump_top_and_bottom() {
        let tree = make_test_tree();
        let mut state = TreeState::new(&tree);
        state.expand(&tree); // iso
        state.move_down();
        state.expand(&tree); // org
        state.move_down();
        state.expand(&tree); // dod
        state.move_down();
        state.expand(&tree); // internet

        let count = state.visible_count();
        assert!(count > 3);

        state.jump_bottom();
        assert_eq!(state.selected, count - 1);

        state.jump_top();
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn ensure_visible_scrolls() {
        let tree = make_test_tree();
        let mut state = TreeState::new(&tree);
        state.expand(&tree);
        state.move_down();
        state.expand(&tree);
        state.move_down();
        state.expand(&tree);
        state.move_down();
        state.expand(&tree);

        state.jump_bottom();
        state.ensure_visible(3);

        assert!(state.scroll_offset > 0);
        assert!(state.selected < state.scroll_offset + 3);
    }

    #[test]
    fn collapse_leaf_moves_to_parent() {
        let tree = make_test_tree();
        let mut state = TreeState::new(&tree);
        // Expand iso → org → dod → internet
        state.expand(&tree);
        state.move_down();
        state.expand(&tree);
        state.move_down();
        state.expand(&tree);
        state.move_down();
        state.expand(&tree);

        // Navigate to mgmt (a leaf under internet)
        state.move_down();
        let selected = state.selected_node().unwrap();
        assert_eq!(tree.get(selected).unwrap().name, "mgmt");

        // Collapse on leaf → should move to parent (internet)
        state.collapse(&tree);
        let selected = state.selected_node().unwrap();
        assert_eq!(tree.get(selected).unwrap().name, "internet");
    }
}
