// kernel/src/isolation.rs
//! Application isolation: namespace management for AILang applications.
//!
//! Each AILang application gets its own namespace — isolated memory,
//! tool, and network contexts — so applications cannot interfere
//! with one another.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Security policy rule.
#[derive(Debug, Clone)]
pub struct PolicyRule {
    /// Agent name or `"*"` for all agents.
    pub subject: String,
    /// Whether this rule allows or denies the action.
    pub action: PolicyAction,
    /// The resource being controlled (e.g. `"internet"`, `"filesystem.write"`).
    pub resource: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PolicyAction {
    Allow,
    Deny,
}

/// Per-application namespace — isolated context for one application.
#[derive(Debug, Clone)]
pub struct AppNamespace {
    pub app_id: String,
    pub app_name: String,
    /// Registered agents (agent name → process ID).
    pub agents: HashMap<String, String>,
    /// Active memory scopes for this application.
    pub memory_scopes: HashSet<String>,
    /// Allowed network endpoints (if restricted).
    pub network_whitelist: Option<HashSet<String>>,
    /// Security policies for this application.
    pub policies: Vec<PolicyRule>,
    /// Resource limits.
    pub max_agents: Option<usize>,
    pub max_tokens: Option<u64>,
    pub max_cost_usd: Option<f64>,
}

impl AppNamespace {
    pub fn new(app_id: &str, app_name: &str) -> Self {
        Self {
            app_id: app_id.to_string(),
            app_name: app_name.to_string(),
            agents: HashMap::new(),
            memory_scopes: HashSet::new(),
            network_whitelist: None,
            policies: Vec::new(),
            max_agents: None,
            max_tokens: None,
            max_cost_usd: None,
        }
    }

    /// Evaluate policies to determine if `subject` may perform `action` on `resource`.
    ///
    /// Rules are evaluated in order; the last matching rule wins.
    /// Default: allow (open unless explicitly denied).
    pub fn check_policy(&self, subject: &str, resource: &str) -> bool {
        let mut permitted = true; // Default: allow
        for rule in &self.policies {
            let subject_matches = rule.subject == "*" || rule.subject == subject;
            let resource_matches = rule.resource == "*"
                || rule.resource == resource
                || resource.starts_with(&format!("{}.", rule.resource))
                || resource.starts_with(&rule.resource);
            if subject_matches && resource_matches {
                permitted = rule.action == PolicyAction::Allow;
            }
        }
        permitted
    }

    pub fn add_policy(&mut self, rule: PolicyRule) {
        self.policies.push(rule);
    }

    pub fn register_agent(&mut self, name: &str, process_id: &str) -> Result<(), String> {
        if let Some(max) = self.max_agents {
            if self.agents.len() >= max {
                return Err(format!(
                    "Application '{}' agent limit ({}) reached",
                    self.app_name, max
                ));
            }
        }
        self.agents.insert(name.to_string(), process_id.to_string());
        Ok(())
    }
}

/// Registry of all application namespaces.
pub struct ApplicationIsolation {
    namespaces: Arc<Mutex<HashMap<String, AppNamespace>>>,
}

impl ApplicationIsolation {
    pub fn new() -> Self {
        Self {
            namespaces: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn create_namespace(&self, app_id: &str, app_name: &str) -> AppNamespace {
        let ns = AppNamespace::new(app_id, app_name);
        self.namespaces.lock().unwrap().insert(app_id.to_string(), ns.clone());
        ns
    }

    pub fn check_policy(&self, app_id: &str, subject: &str, resource: &str) -> bool {
        self.namespaces
            .lock()
            .unwrap()
            .get(app_id)
            .map(|ns| ns.check_policy(subject, resource))
            .unwrap_or(true) // Unknown app: allow by default
    }

    pub fn get_namespace(&self, app_id: &str) -> Option<AppNamespace> {
        self.namespaces.lock().unwrap().get(app_id).cloned()
    }

    pub fn list_apps(&self) -> Vec<String> {
        self.namespaces.lock().unwrap().keys().cloned().collect()
    }
}

impl Default for ApplicationIsolation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_allow() {
        let ns = AppNamespace::new("app-1", "TestApp");
        // No policies → allow everything
        assert!(ns.check_policy("ResearchAgent", "internet"));
        assert!(ns.check_policy("WriterAgent", "filesystem.read"));
    }

    #[test]
    fn explicit_deny_overrides_default() {
        let mut ns = AppNamespace::new("app-1", "TestApp");
        ns.add_policy(PolicyRule {
            subject: "*".to_string(),
            action: PolicyAction::Deny,
            resource: "filesystem.write".to_string(),
        });
        assert!(!ns.check_policy("AnyAgent", "filesystem.write"));
        assert!(ns.check_policy("AnyAgent", "internet")); // not denied
    }

    #[test]
    fn allow_after_deny_wins_last_match() {
        let mut ns = AppNamespace::new("app-1", "TestApp");
        // Deny all internet
        ns.add_policy(PolicyRule {
            subject: "*".to_string(),
            action: PolicyAction::Deny,
            resource: "internet".to_string(),
        });
        // Then explicitly allow ResearchAgent
        ns.add_policy(PolicyRule {
            subject: "ResearchAgent".to_string(),
            action: PolicyAction::Allow,
            resource: "internet".to_string(),
        });

        assert!(ns.check_policy("ResearchAgent", "internet")); // allowed by last rule
        assert!(!ns.check_policy("OtherAgent", "internet")); // still denied
    }

    #[test]
    fn agent_limit_enforced() {
        let mut ns = AppNamespace::new("app-1", "LimitedApp");
        ns.max_agents = Some(2);
        assert!(ns.register_agent("A", "agent-1").is_ok());
        assert!(ns.register_agent("B", "agent-2").is_ok());
        assert!(ns.register_agent("C", "agent-3").is_err());
    }

    #[test]
    fn isolation_registry_check_policy() {
        let isolation = ApplicationIsolation::new();
        let mut ns = isolation.create_namespace("app-1", "SecureApp");
        ns.add_policy(PolicyRule {
            subject: "SandboxAgent".to_string(),
            action: PolicyAction::Deny,
            resource: "filesystem".to_string(),
        });
        // Re-register with policies
        {
            let mut namespaces = isolation.namespaces.lock().unwrap();
            namespaces.insert("app-1".to_string(), ns);
        }

        assert!(!isolation.check_policy("app-1", "SandboxAgent", "filesystem.write"));
        assert!(isolation.check_policy("app-1", "SandboxAgent", "internet"));
    }
}

