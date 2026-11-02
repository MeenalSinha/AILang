//! Capability-based permissions: an `agent`'s `permissions [...]` field
//! becomes a [`PermissionSet`] the tool registry checks before invoking
//! anything capability-gated.

use std::collections::HashSet;

/// The set of capability names an agent has been explicitly granted.
/// Deliberately just a name-set (not a richer scoped-permission model —
/// e.g. `filesystem:read` vs `filesystem:write`) for v0.5; see
/// `docs/roadmap.md` for scoped permissions as a v1.0 candidate.
#[derive(Debug, Clone, Default)]
pub struct PermissionSet(HashSet<String>);

impl PermissionSet {
    pub fn new(names: impl IntoIterator<Item = String>) -> Self {
        Self(names.into_iter().collect())
    }

    pub fn grants(&self, capability: &str) -> bool {
        self.0.contains(capability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_only_listed_capabilities() {
        let perms = PermissionSet::new(vec!["internet".to_string()]);
        assert!(perms.grants("internet"));
        assert!(!perms.grants("filesystem"));
    }

    #[test]
    fn empty_permission_set_grants_nothing() {
        let perms = PermissionSet::default();
        assert!(!perms.grants("internet"));
    }
}
