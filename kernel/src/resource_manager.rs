// kernel/src/resource_manager.rs
//! Intelligence as a Resource: allocation, tracking, and budget enforcement.
//!
//! In AILang v3.0, intelligence resources are managed by the kernel like
//! any other OS resource (CPU, memory). A `resource` declaration in .ail
//! registers a named resource with the kernel; `allocate` binds it to an agent.
//!
//! ## AILang syntax
//!
//! ```ail
//! resource intelligence {
//!     model GPT5
//!     budget 1000 tokens
//!     latency < 500ms
//! }
//!
//! allocate intelligence to Researcher
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The kind of resource being managed.
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceKind {
    /// AI inference capacity (model + token budget).
    Intelligence,
    /// Memory storage capacity.
    Memory,
    /// Compute capacity (CPU/GPU).
    Compute,
    /// Named model resource.
    Model,
    /// Knowledge retrieval resource.
    Knowledge,
}

/// Declaration of a named resource.
#[derive(Debug, Clone)]
pub struct ResourceSpec {
    pub name: String,
    pub kind: ResourceKind,
    /// The model backing this intelligence resource (if any).
    pub model: Option<String>,
    /// Maximum tokens this resource may allocate.
    pub token_budget: Option<u64>,
    /// Maximum cost budget (USD).
    pub cost_budget: Option<f64>,
    /// Maximum acceptable latency per call (ms).
    pub latency_limit_ms: Option<u64>,
    /// Maximum energy (watt-hours) for edge deployments.
    pub energy_budget_wh: Option<f64>,
}

/// Tracks usage of one resource by one agent.
#[derive(Debug, Clone)]
pub struct AllocationRecord {
    pub resource_name: String,
    pub agent_id: String,
    pub tokens_used: u64,
    pub cost_used: f64,
    pub energy_used_wh: f64,
    pub call_count: u64,
}

/// The kernel's resource registry and usage tracker.
pub struct ResourceManager {
    resources: Arc<Mutex<HashMap<String, ResourceSpec>>>,
    allocations: Arc<Mutex<Vec<AllocationRecord>>>,
}

impl ResourceManager {
    pub fn new() -> Self {
        Self {
            resources: Arc::new(Mutex::new(HashMap::new())),
            allocations: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register a resource declaration.
    pub fn declare(&self, spec: ResourceSpec) {
        self.resources.lock().unwrap().insert(spec.name.clone(), spec);
    }

    /// Allocate a resource to an agent. Returns an error if the resource
    /// doesn't exist or is already fully allocated (budget = 0).
    pub fn allocate(&self, resource_name: &str, agent_id: &str) -> Result<(), String> {
        let resources = self.resources.lock().unwrap();
        if !resources.contains_key(resource_name) {
            return Err(format!("Resource '{}' not declared", resource_name));
        }

        // Check if already allocated to this agent (idempotent)
        let mut allocations = self.allocations.lock().unwrap();
        let already = allocations
            .iter()
            .any(|a| a.resource_name == resource_name && a.agent_id == agent_id);
        if !already {
            allocations.push(AllocationRecord {
                resource_name: resource_name.to_string(),
                agent_id: agent_id.to_string(),
                tokens_used: 0,
                cost_used: 0.0,
                energy_used_wh: 0.0,
                call_count: 0,
            });
        }
        Ok(())
    }

    /// Record resource usage for an agent.
    pub fn track_usage(
        &self,
        resource_name: &str,
        agent_id: &str,
        tokens: u64,
        cost_usd: f64,
    ) {
        let mut allocations = self.allocations.lock().unwrap();
        if let Some(alloc) = allocations
            .iter_mut()
            .find(|a| a.resource_name == resource_name && a.agent_id == agent_id)
        {
            alloc.tokens_used += tokens;
            alloc.cost_used += cost_usd;
            alloc.call_count += 1;
        }
    }

    /// Check whether an agent has exceeded their resource budget.
    pub fn check_budget(&self, resource_name: &str, agent_id: &str) -> Result<(), String> {
        let resources = self.resources.lock().unwrap();
        let allocations = self.allocations.lock().unwrap();

        if let Some(spec) = resources.get(resource_name) {
            if let Some(alloc) = allocations
                .iter()
                .find(|a| a.resource_name == resource_name && a.agent_id == agent_id)
            {
                if let Some(token_budget) = spec.token_budget {
                    if alloc.tokens_used >= token_budget {
                        return Err(format!(
                            "Token budget exceeded: used {}/{} for resource '{}'",
                            alloc.tokens_used, token_budget, resource_name
                        ));
                    }
                }
                if let Some(cost_budget) = spec.cost_budget {
                    if alloc.cost_used >= cost_budget {
                        return Err(format!(
                            "Cost budget exceeded: ${:.4}/${:.4} for resource '{}'",
                            alloc.cost_used, cost_budget, resource_name
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Get all allocation records for a specific agent.
    pub fn agent_usage(&self, agent_id: &str) -> Vec<AllocationRecord> {
        self.allocations
            .lock()
            .unwrap()
            .iter()
            .filter(|a| a.agent_id == agent_id)
            .cloned()
            .collect()
    }

    /// Total tokens and cost consumed across all allocations.
    pub fn system_totals(&self) -> (u64, f64) {
        let allocations = self.allocations.lock().unwrap();
        let total_tokens: u64 = allocations.iter().map(|a| a.tokens_used).sum();
        let total_cost: f64 = allocations.iter().map(|a| a.cost_used).sum();
        (total_tokens, total_cost)
    }

    pub fn list_resources(&self) -> Vec<ResourceSpec> {
        self.resources.lock().unwrap().values().cloned().collect()
    }

    pub fn get_resource(&self, name: &str) -> Option<ResourceSpec> {
        self.resources.lock().unwrap().get(name).cloned()
    }
}

impl Default for ResourceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intel_resource(name: &str, token_budget: u64) -> ResourceSpec {
        ResourceSpec {
            name: name.to_string(),
            kind: ResourceKind::Intelligence,
            model: Some("mock".to_string()),
            token_budget: Some(token_budget),
            cost_budget: Some(1.0),
            latency_limit_ms: Some(500),
            energy_budget_wh: None,
        }
    }

    #[test]
    fn declare_and_allocate() {
        let rm = ResourceManager::new();
        rm.declare(intel_resource("intel", 1000));
        assert!(rm.allocate("intel", "agent-1").is_ok());
        assert_eq!(rm.agent_usage("agent-1").len(), 1);
    }

    #[test]
    fn allocate_unknown_resource_fails() {
        let rm = ResourceManager::new();
        let result = rm.allocate("nonexistent", "agent-1");
        assert!(result.is_err());
    }

    #[test]
    fn token_budget_enforcement() {
        let rm = ResourceManager::new();
        rm.declare(intel_resource("intel", 100));
        rm.allocate("intel", "agent-1").unwrap();
        rm.track_usage("intel", "agent-1", 50, 0.05);
        assert!(rm.check_budget("intel", "agent-1").is_ok());

        rm.track_usage("intel", "agent-1", 51, 0.05);
        assert!(rm.check_budget("intel", "agent-1").is_err());
    }

    #[test]
    fn cost_budget_enforcement() {
        let rm = ResourceManager::new();
        rm.declare(intel_resource("intel", 100_000));
        rm.allocate("intel", "agent-1").unwrap();
        rm.track_usage("intel", "agent-1", 10, 0.80);
        assert!(rm.check_budget("intel", "agent-1").is_ok());
        rm.track_usage("intel", "agent-1", 10, 0.21); // total 1.01 > 1.0
        assert!(rm.check_budget("intel", "agent-1").is_err());
    }

    #[test]
    fn idempotent_allocation() {
        let rm = ResourceManager::new();
        rm.declare(intel_resource("intel", 1000));
        rm.allocate("intel", "agent-1").unwrap();
        rm.allocate("intel", "agent-1").unwrap(); // second call should not create duplicate
        assert_eq!(rm.agent_usage("agent-1").len(), 1);
    }

    #[test]
    fn system_totals_across_agents() {
        let rm = ResourceManager::new();
        rm.declare(intel_resource("intel", 10000));
        rm.allocate("intel", "agent-1").unwrap();
        rm.allocate("intel", "agent-2").unwrap();
        rm.track_usage("intel", "agent-1", 100, 0.10);
        rm.track_usage("intel", "agent-2", 200, 0.20);
        let (tokens, cost) = rm.system_totals();
        assert_eq!(tokens, 300);
        assert!((cost - 0.30).abs() < 1e-6);
    }
}

