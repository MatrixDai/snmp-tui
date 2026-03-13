use std::fmt;
use std::str::FromStr;

/// A wrapper around a sequence of OID components in dotted notation (e.g. 1.3.6.1.2.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Oid(Vec<u32>);

impl Oid {
    /// Create a new OID from a vector of components.
    pub fn new(components: Vec<u32>) -> Self {
        Self(components)
    }

    /// Return the OID components as a slice.
    pub fn components(&self) -> &[u32] {
        &self.0
    }

    /// Return the number of components.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Return true if the OID has no components.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Return true if `self` is a subtree of (starts with) `other`.
    pub fn is_subtree_of(&self, other: &Oid) -> bool {
        self.0.starts_with(&other.0) && self.0.len() > other.0.len()
    }

    /// Return a new OID with `subid` appended.
    pub fn child(&self, subid: u32) -> Oid {
        let mut components = self.0.clone();
        components.push(subid);
        Oid(components)
    }

    /// Return the parent OID (all components except the last), or None if empty.
    pub fn parent(&self) -> Option<Oid> {
        if self.0.len() <= 1 {
            None
        } else {
            Some(Oid(self.0[..self.0.len() - 1].to_vec()))
        }
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(|c| c.to_string()).collect();
        write!(f, "{}", parts.join("."))
    }
}

impl FromStr for Oid {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix('.').unwrap_or(s);
        if s.is_empty() {
            return Ok(Oid(Vec::new()));
        }
        let components: Result<Vec<u32>, _> = s
            .split('.')
            .map(|part| {
                part.parse::<u32>()
                    .map_err(|e| format!("invalid OID component '{}': {}", part, e))
            })
            .collect();
        Ok(Oid(components?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dotted_notation() {
        let oid: Oid = "1.3.6.1.2.1".parse().unwrap();
        assert_eq!(oid.components(), &[1, 3, 6, 1, 2, 1]);
    }

    #[test]
    fn parse_leading_dot() {
        let oid: Oid = ".1.3.6.1".parse().unwrap();
        assert_eq!(oid.components(), &[1, 3, 6, 1]);
    }

    #[test]
    fn display_dotted() {
        let oid = Oid::new(vec![1, 3, 6, 1, 2, 1]);
        assert_eq!(oid.to_string(), "1.3.6.1.2.1");
    }

    #[test]
    fn subtree_check() {
        let parent = Oid::new(vec![1, 3, 6]);
        let child = Oid::new(vec![1, 3, 6, 1]);
        let sibling = Oid::new(vec![1, 3, 7]);

        assert!(child.is_subtree_of(&parent));
        assert!(!parent.is_subtree_of(&child));
        assert!(!sibling.is_subtree_of(&parent));
        // Not a subtree of itself
        assert!(!parent.is_subtree_of(&parent));
    }

    #[test]
    fn parent_and_child() {
        let oid = Oid::new(vec![1, 3, 6]);
        let child = oid.child(1);
        assert_eq!(child, Oid::new(vec![1, 3, 6, 1]));
        assert_eq!(child.parent(), Some(oid));
    }

    #[test]
    fn parent_of_single_component_is_none() {
        let oid = Oid::new(vec![1]);
        assert_eq!(oid.parent(), None);
    }

    #[test]
    fn empty_oid() {
        let oid = Oid::new(vec![]);
        assert!(oid.is_empty());
        assert_eq!(oid.len(), 0);
        assert_eq!(oid.parent(), None);
    }

    #[test]
    fn parse_invalid() {
        assert!("1.3.abc".parse::<Oid>().is_err());
    }
}
