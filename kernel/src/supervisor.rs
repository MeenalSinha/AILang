// kernel/src/supervisor.rs
//! Hierarchical agent supervision — fault-tolerant agent orchestration.
//!
//! Inspired by Erlang/OTP's supervision trees, the `Supervisor` manages a
//! group of child agents and automatically restarts them on failure
//! according to a configurable strategy.
//!
//! ## Strategies
//!
//! - `OneForOne`: restart only the failed child.
//! - `OneForAll`: restart all children when one fails (for tightly coupled groups).
//! - `RestForOne`: restart the failed child and all children declared after it.
//!
//! ## V3.0 honest limitations
//!
//! The supervisor tracks state and makes restart *decisions*. The actual
//! re-execution of an agent's body is the interpreter's responsibility:
//! the interpreter checks `should_restart()` after a crash and re-runs
//! the agent block if the supervisor permits it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// How a supervisor responds to child failures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SupervisionStrategy {
    /// Restart only the failed child.
    OneForOne,
    /// Restart all children when any one fails.
    OneForAll,
    /// Restart the failed child and every child declared after it.
    RestForOne,
}

/// The current status of a supervised child agent.
#[derive(Debug, Clone, PartialEq)]
pub enum ChildStatus {
    Running,
    Failed { reason: String, at: Instant },
    Restarting,
    Stopped,
}

/// Specification for one supervised child agent.
#[derive(Debug, Clone)]
pub struct ChildSpec {
    /// The agent's name (matches `agent Name` declaration).
    pub name: String,
    pub status: ChildStatus,
    /// How many times this child has been restarted.
    pub restart_count: u32,
    /// Maximum restarts before the supervisor gives up on this child.
    pub max_restarts: u32,
}

/// A supervisor managing a group of child agents.
///
/// # Example (AILang syntax)
///
/// ```ail
/// supervisor Research {
///     strategy one_for_one
///     children [Researcher, Analyst, Writer]
/// }
/// ```
pub struct Supervisor {
    pub name: String,
    pub strategy: SupervisionStrategy,
    children: Arc<Mutex<Vec<ChildSpec>>>,
}

impl Supervisor {
    pub fn new(name: &str, strategy: SupervisionStrategy) -> Self {
        Self {
            name: name.to_string(),
            strategy,
            children: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register a child agent under this supervisor.
    pub fn add_child(&self, name: &str, max_restarts: u32) {
        self.children.lock().unwrap().push(ChildSpec {
            name: name.to_string(),
            status: ChildStatus::Running,
            restart_count: 0,
            max_restarts,
        });
    }

    /// Called when a child agent crashes.
    ///
    /// Returns the list of agent names that the supervisor has decided
    /// should be restarted. The interpreter must re-execute their blocks.
    pub fn handle_failure(&self, failed_agent: &str, reason: &str) -> Vec<String> {
        let mut children = self.children.lock().unwrap();

        // Mark the failed child
        if let Some(child) = children.iter_mut().find(|c| c.name == failed_agent) {
            child.status = ChildStatus::Failed {
                reason: reason.to_string(),
                at: Instant::now(),
            };
            child.restart_count += 1;
        }

        let to_restart: Vec<String> = match &self.strategy {
            SupervisionStrategy::OneForOne => {
                // Only restart the failed child if it hasn't exhausted its restarts
                children
                    .iter()
                    .filter(|c| c.name == failed_agent && c.restart_count <= c.max_restarts)
                    .map(|c| c.name.clone())
                    .collect()
            }
            SupervisionStrategy::OneForAll => {
                // Restart every child
                children.iter().map(|c| c.name.clone()).collect()
            }
            SupervisionStrategy::RestForOne => {
                // Restart the failed child and all children after it (in declaration order)
                let mut found = false;
                children
                    .iter()
                    .filter(|c| {
                        if c.name == failed_agent {
                            found = true;
                        }
                        found
                    })
                    .map(|c| c.name.clone())
                    .collect()
            }
        };

        // Mark selected children as restarting
        for name in &to_restart {
            if let Some(child) = children.iter_mut().find(|c| &c.name == name) {
                child.status = ChildStatus::Restarting;
            }
        }

        to_restart
    }

    /// Called when a child has successfully restarted.
    pub fn mark_running(&self, agent: &str) {
        let mut children = self.children.lock().unwrap();
        if let Some(child) = children.iter_mut().find(|c| c.name == agent) {
            child.status = ChildStatus::Running;
        }
    }

    /// Explicitly stop a child.
    pub fn stop_child(&self, agent: &str) {
        let mut children = self.children.lock().unwrap();
        if let Some(child) = children.iter_mut().find(|c| c.name == agent) {
            child.status = ChildStatus::Stopped;
        }
    }

    pub fn status(&self) -> Vec<ChildSpec> {
        self.children.lock().unwrap().clone()
    }

    /// Whether all children are currently running.
    pub fn all_healthy(&self) -> bool {
        self.children
            .lock()
            .unwrap()
            .iter()
            .all(|c| c.status == ChildStatus::Running)
    }
}

/// A registry of all supervisors declared in the program.
pub struct SupervisorRegistry {
    supervisors: Arc<Mutex<HashMap<String, Supervisor>>>,
}

impl SupervisorRegistry {
    pub fn new() -> Self {
        Self {
            supervisors: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register(&self, supervisor: Supervisor) {
        self.supervisors
            .lock()
            .unwrap()
            .insert(supervisor.name.clone(), supervisor);
    }

    pub fn get_mut_and_handle_failure(
        &self,
        supervisor_name: &str,
        failed_agent: &str,
        reason: &str,
    ) -> Option<Vec<String>> {
        let supervisors = self.supervisors.lock().unwrap();
        supervisors
            .get(supervisor_name)
            .map(|s| s.handle_failure(failed_agent, reason))
    }

    pub fn list_names(&self) -> Vec<String> {
        self.supervisors.lock().unwrap().keys().cloned().collect()
    }
}

impl Default for SupervisorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_supervisor(strategy: SupervisionStrategy) -> Supervisor {
        let s = Supervisor::new("TestSupervisor", strategy);
        s.add_child("Alpha", 3);
        s.add_child("Beta", 3);
        s.add_child("Gamma", 3);
        s
    }

    #[test]
    fn one_for_one_restarts_only_failed_child() {
        let s = make_supervisor(SupervisionStrategy::OneForOne);
        let to_restart = s.handle_failure("Beta", "crashed");
        assert_eq!(to_restart, vec!["Beta"]);
    }

    #[test]
    fn one_for_all_restarts_every_child() {
        let s = make_supervisor(SupervisionStrategy::OneForAll);
        let to_restart = s.handle_failure("Alpha", "crashed");
        assert_eq!(to_restart.len(), 3);
        assert!(to_restart.contains(&"Alpha".to_string()));
        assert!(to_restart.contains(&"Beta".to_string()));
        assert!(to_restart.contains(&"Gamma".to_string()));
    }

    #[test]
    fn rest_for_one_restarts_failed_and_subsequent() {
        let s = make_supervisor(SupervisionStrategy::RestForOne);
        // Beta is index 1, so Beta + Gamma should restart
        let to_restart = s.handle_failure("Beta", "crashed");
        assert!(to_restart.contains(&"Beta".to_string()));
        assert!(to_restart.contains(&"Gamma".to_string()));
        assert!(!to_restart.contains(&"Alpha".to_string()));
    }

    #[test]
    fn one_for_one_respects_max_restarts() {
        let s = Supervisor::new("S", SupervisionStrategy::OneForOne);
        s.add_child("OnlyChild", 1);

        // First failure: should restart
        let r1 = s.handle_failure("OnlyChild", "first crash");
        assert_eq!(r1, vec!["OnlyChild"]);

        s.mark_running("OnlyChild");

        // Second failure: max restarts (1) exceeded
        let r2 = s.handle_failure("OnlyChild", "second crash");
        assert!(r2.is_empty(), "should not restart after exceeding max");
    }

    #[test]
    fn mark_running_after_restart() {
        let s = make_supervisor(SupervisionStrategy::OneForOne);
        s.handle_failure("Alpha", "crash");
        assert_eq!(s.status()[0].status, ChildStatus::Restarting);
        s.mark_running("Alpha");
        assert_eq!(s.status()[0].status, ChildStatus::Running);
    }

    #[test]
    fn all_healthy_false_when_any_failed() {
        let s = make_supervisor(SupervisionStrategy::OneForOne);
        assert!(s.all_healthy());
        s.handle_failure("Gamma", "boom");
        assert!(!s.all_healthy());
    }
}

