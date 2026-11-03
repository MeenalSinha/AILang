// application/src/lib.rs
//! # ailang-application
//!
//! Formal AILang application model: lifecycle, isolation, and policy enforcement.
//!
//! An AILang v3.0 `application` declaration creates a formal container that owns
//! agents, models, knowledge sources, memory stores, and workflows, and enforces
//! security policies across all of them.
//!
//! ## Lifecycle
//!
//! ```text
//! Installing → Initialized → Starting → Running
//!                                          ↓
//!                              Paused ← Running → Upgrading
//!                                          ↓
//!                                       Stopping → Stopped → Uninstalling
//!                                          ↓ (on crash)
//!                                       Recovering
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Application lifecycle state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AppLifecycle {
    Installing,
    Initialized,
    Starting,
    Running,
    Paused,
    Upgrading { from_version: String, to_version: String },
    Stopping,
    Stopped,
    Recovering { reason: String },
    Uninstalling,
}

impl AppLifecycle {
    pub fn is_active(&self) -> bool {
        matches!(self, AppLifecycle::Running | AppLifecycle::Paused)
    }

    pub fn can_transition_to(&self, next: &AppLifecycle) -> bool {
        use AppLifecycle::*;
        match (self, next) {
            (Installing, Initialized) => true,
            (Initialized, Starting) => true,
            (Starting, Running) => true,
            (Running, Paused) => true,
            (Paused, Running) => true,
            (Running, Upgrading { .. }) => true,
            (Upgrading { .. }, Running) => true,
            (Running, Stopping) | (Paused, Stopping) => true,
            (Stopping, Stopped) => true,
            (Stopped, Uninstalling) => true,
            (Running, Recovering { .. }) | (Starting, Recovering { .. }) => true,
            (Recovering { .. }, Running) | (Recovering { .. }, Stopped) => true,
            _ => false,
        }
    }
}

/// A security policy rule: allow or deny an agent's access to a resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Agent name or `"*"` for all agents in this application.
    pub subject: String,
    pub action: PolicyAction,
    /// Resource path, e.g. `"internet"`, `"filesystem.write"`, `"knowledge.read"`.
    pub resource: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PolicyAction {
    Allow,
    Deny,
}

/// Per-application resource limits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_agents: Option<usize>,
    pub max_memory_mb: Option<usize>,
    pub max_tokens: Option<u64>,
    pub max_cost_usd: Option<f64>,
}

/// A complete AILang application specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationSpec {
    pub name: String,
    pub version: String,
    pub agents: Vec<String>,
    pub models: Vec<String>,
    pub knowledge: Vec<String>,
    pub memory: Vec<String>,
    pub workflows: Vec<String>,
    pub policies: Vec<PolicyRule>,
    pub limits: ResourceLimits,
}

/// A running AILang application instance.
pub struct Application {
    pub id: String,
    pub spec: ApplicationSpec,
    lifecycle: Arc<Mutex<AppLifecycle>>,
    /// Accumulated resource usage.
    tokens_used: Arc<Mutex<u64>>,
    cost_used: Arc<Mutex<f64>>,
    /// Registered agents: name → process_id.
    registered_agents: Arc<Mutex<HashMap<String, String>>>,
}

impl Application {
    pub fn new(spec: ApplicationSpec) -> Self {
        let id = app_id(&spec.name, &spec.version);
        Self {
            id,
            spec,
            lifecycle: Arc::new(Mutex::new(AppLifecycle::Installing)),
            tokens_used: Arc::new(Mutex::new(0)),
            cost_used: Arc::new(Mutex::new(0.0)),
            registered_agents: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Attempt a lifecycle transition. Returns `Err` if the transition is invalid.
    pub fn transition(&self, next: AppLifecycle) -> Result<(), String> {
        let mut lifecycle = self.lifecycle.lock().unwrap();
        if lifecycle.can_transition_to(&next) {
            *lifecycle = next;
            Ok(())
        } else {
            Err(format!(
                "Invalid lifecycle transition: {:?} → {:?}",
                *lifecycle, next
            ))
        }
    }

    pub fn lifecycle(&self) -> AppLifecycle {
        self.lifecycle.lock().unwrap().clone()
    }

    /// Check whether `subject` (agent name) may access `resource`.
    ///
    /// Rules evaluated in declaration order; last matching rule wins.
    /// Default: allow.
    pub fn check_policy(&self, subject: &str, resource: &str) -> bool {
        let mut permitted = true;
        for rule in &self.spec.policies {
            let subject_matches = rule.subject == "*" || rule.subject == subject;
            let resource_matches = rule.resource == "*"
                || rule.resource == resource
                || resource.starts_with(&format!("{}.", rule.resource))
                || rule.resource.ends_with(".*") && {
                    let prefix = rule.resource.trim_end_matches(".*");
                    resource.starts_with(prefix)
                };
            if subject_matches && resource_matches {
                permitted = rule.action == PolicyAction::Allow;
            }
        }
        permitted
    }

    /// Register an agent within this application.
    pub fn register_agent(&self, name: &str, process_id: &str) -> Result<(), String> {
        let mut agents = self.registered_agents.lock().unwrap();
        if let Some(max) = self.spec.limits.max_agents {
            if agents.len() >= max {
                return Err(format!(
                    "Application '{}' agent limit ({}) exceeded",
                    self.spec.name, max
                ));
            }
        }
        agents.insert(name.to_string(), process_id.to_string());
        Ok(())
    }

    pub fn registered_agents(&self) -> HashMap<String, String> {
        self.registered_agents.lock().unwrap().clone()
    }

    /// Track resource usage for this application.
    pub fn track_usage(&self, tokens: u64, cost_usd: f64) -> Result<(), String> {
        let mut t = self.tokens_used.lock().unwrap();
        let mut c = self.cost_used.lock().unwrap();

        *t += tokens;
        *c += cost_usd;

        if let Some(max_tokens) = self.spec.limits.max_tokens {
            if *t > max_tokens {
                return Err(format!(
                    "Application '{}' token limit ({}) exceeded",
                    self.spec.name, max_tokens
                ));
            }
        }
        if let Some(max_cost) = self.spec.limits.max_cost_usd {
            if *c > max_cost {
                return Err(format!(
                    "Application '{}' cost limit (${:.2}) exceeded",
                    self.spec.name, max_cost
                ));
            }
        }
        Ok(())
    }

    pub fn usage(&self) -> (u64, f64) {
        (
            *self.tokens_used.lock().unwrap(),
            *self.cost_used.lock().unwrap(),
        )
    }
}

fn app_id(name: &str, version: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    version.hash(&mut h);
    format!("app-{:016x}", h.finish())
}

/// Global application registry.
pub struct ApplicationRegistry {
    apps: Arc<Mutex<HashMap<String, Arc<Application>>>>,
}

impl ApplicationRegistry {
    pub fn new() -> Self {
        Self {
            apps: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register(&self, app: Application) -> Arc<Application> {
        let app = Arc::new(app);
        self.apps.lock().unwrap().insert(app.id.clone(), app.clone());
        app
    }

    pub fn get(&self, id: &str) -> Option<Arc<Application>> {
        self.apps.lock().unwrap().get(id).cloned()
    }

    pub fn find_by_name(&self, name: &str) -> Option<Arc<Application>> {
        self.apps
            .lock()
            .unwrap()
            .values()
            .find(|a| a.spec.name == name)
            .cloned()
    }

    pub fn list(&self) -> Vec<Arc<Application>> {
        self.apps.lock().unwrap().values().cloned().collect()
    }
}

impl Default for ApplicationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_spec(name: &str) -> ApplicationSpec {
        ApplicationSpec {
            name: name.to_string(),
            version: "3.0.0".to_string(),
            agents: vec!["Researcher".to_string()],
            models: vec!["mock".to_string()],
            knowledge: vec![],
            memory: vec!["session".to_string()],
            workflows: vec![],
            policies: vec![],
            limits: ResourceLimits::default(),
        }
    }

    #[test]
    fn lifecycle_transitions_valid() {
        let app = Application::new(test_spec("TestApp"));
        assert_eq!(app.lifecycle(), AppLifecycle::Installing);
        app.transition(AppLifecycle::Initialized).unwrap();
        app.transition(AppLifecycle::Starting).unwrap();
        app.transition(AppLifecycle::Running).unwrap();
        assert!(app.lifecycle().is_active());
    }

    #[test]
    fn invalid_lifecycle_transition_fails() {
        let app = Application::new(test_spec("TestApp"));
        // Cannot go from Installing directly to Running
        assert!(app.transition(AppLifecycle::Running).is_err());
    }

    #[test]
    fn default_policy_allows_all() {
        let app = Application::new(test_spec("TestApp"));
        assert!(app.check_policy("ResearchAgent", "internet"));
        assert!(app.check_policy("WriterAgent", "filesystem.write"));
    }

    #[test]
    fn explicit_deny_policy() {
        let mut spec = test_spec("SecureApp");
        spec.policies.push(PolicyRule {
            subject: "*".to_string(),
            action: PolicyAction::Deny,
            resource: "filesystem.write".to_string(),
        });
        let app = Application::new(spec);
        assert!(!app.check_policy("AnyAgent", "filesystem.write"));
        assert!(app.check_policy("AnyAgent", "internet"));
    }

    #[test]
    fn specific_agent_allow_overrides_global_deny() {
        let mut spec = test_spec("PolicyTest");
        spec.policies.push(PolicyRule {
            subject: "*".to_string(),
            action: PolicyAction::Deny,
            resource: "internet".to_string(),
        });
        spec.policies.push(PolicyRule {
            subject: "Researcher".to_string(),
            action: PolicyAction::Allow,
            resource: "internet".to_string(),
        });
        let app = Application::new(spec);
        assert!(app.check_policy("Researcher", "internet"));
        assert!(!app.check_policy("Writer", "internet"));
    }

    #[test]
    fn agent_registration_with_limit() {
        let mut spec = test_spec("LimitedApp");
        spec.limits.max_agents = Some(2);
        let app = Application::new(spec);
        app.register_agent("A", "pid-1").unwrap();
        app.register_agent("B", "pid-2").unwrap();
        assert!(app.register_agent("C", "pid-3").is_err());
    }

    #[test]
    fn token_budget_enforcement() {
        let mut spec = test_spec("TokenLimited");
        spec.limits.max_tokens = Some(100);
        let app = Application::new(spec);
        app.track_usage(50, 0.05).unwrap();
        assert!(app.track_usage(51, 0.05).is_err());
    }

    #[test]
    fn usage_tracking() {
        let app = Application::new(test_spec("UsageTest"));
        app.track_usage(100, 0.10).unwrap();
        app.track_usage(200, 0.20).unwrap();
        let (tokens, cost) = app.usage();
        assert_eq!(tokens, 300);
        assert!((cost - 0.30).abs() < 1e-6);
    }

    #[test]
    fn registry_find_by_name() {
        let registry = ApplicationRegistry::new();
        let app = Application::new(test_spec("MyApp"));
        registry.register(app);
        assert!(registry.find_by_name("MyApp").is_some());
        assert!(registry.find_by_name("Other").is_none());
    }
}

