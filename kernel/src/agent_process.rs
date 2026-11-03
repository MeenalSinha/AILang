// kernel/src/agent_process.rs
//! Agent-as-OS-process: lifecycle, checkpointing, restart policies.
//!
//! In AILang v3.0, every `agent` declaration creates an `AgentProcess` entry
//! in the kernel's process table — analogous to how an OS fork() creates a
//! process table entry. The kernel tracks state, enforces resource budgets,
//! takes checkpoints, and decides whether to restart failed agents.
//!
//! ## V3.0 honest limitations
//!
//! Actual agent *execution* still happens synchronously in the interpreter.
//! `AgentProcessTable` is the *metadata* layer: state tracking, checkpointing,
//! and restart decision logic. A future async executor would use this table
//! to schedule real concurrent agent execution.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// The lifecycle states an agent process can be in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentState {
    /// Process entry created, not yet started.
    Created,
    /// Agent is actively executing.
    Running,
    /// Agent has been explicitly suspended (e.g. resource pressure).
    Suspended,
    /// Agent crashed — reason recorded, awaiting supervisor decision.
    Crashed { reason: String },
    /// Supervisor decided to restart; checkpoint restore in progress.
    Recovering,
    /// Agent completed normally.
    Completed,
    /// Agent was explicitly stopped or max restarts exceeded.
    Terminated,
}

/// Resource budget for a single agent process.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceAllocation {
    /// Maximum tokens the agent may consume (across all model calls).
    pub token_budget: Option<u64>,
    /// Maximum USD cost the agent may incur.
    pub cost_budget: Option<f64>,
    /// Maximum wall-clock latency for any single model call.
    pub latency_limit_ms: Option<u64>,
    /// Maximum energy budget (for edge/device deployments).
    pub energy_budget_wh: Option<f64>,
}

/// A point-in-time snapshot of an agent's state for crash recovery.
#[derive(Debug, Clone)]
pub struct AgentCheckpoint {
    /// Wall-clock time the checkpoint was taken.
    pub timestamp: Instant,
    /// Key-value snapshot of the agent's named variables at checkpoint time.
    pub state_snapshot: HashMap<String, String>,
    /// 0.0–1.0 estimate of goal completion at checkpoint time.
    pub goal_progress: f64,
}

/// One entry in the kernel's agent process table.
#[derive(Debug, Clone)]
pub struct AgentProcess {
    /// Unique kernel-assigned process ID (`agent-N`).
    pub id: String,
    /// Human-readable agent name (matches the AILang `agent Name` declaration).
    pub name: String,
    /// Current lifecycle state.
    pub state: AgentState,
    /// Whether the agent runs without explicit invocation.
    pub autonomous: bool,
    /// Whether the agent's memory should persist across process boundaries.
    pub persistent: bool,
    /// Whether the supervisor should restart this agent on failure.
    pub restart_on_failure: bool,
    /// The goal this agent is working toward (if it has one).
    pub goal: Option<String>,
    /// Resource budget enforced by the kernel.
    pub resources: ResourceAllocation,
    /// How many times this agent has been restarted so far.
    pub restart_count: u32,
    /// Maximum allowed restarts before the agent is permanently terminated.
    pub max_restarts: u32,
    /// When this process entry was created.
    pub created_at: Instant,
    /// The most recent checkpoint (if any).
    pub last_checkpoint: Option<AgentCheckpoint>,
    /// Accumulated resource usage.
    pub tokens_used: u64,
    pub cost_used: f64,
}

/// Configuration used to spawn an `AgentProcess`.
#[derive(Debug, Default)]
pub struct AgentProcessConfig {
    pub autonomous: bool,
    pub persistent: bool,
    pub restart_on_failure: bool,
    pub goal: Option<String>,
    pub resources: ResourceAllocation,
    pub max_restarts: Option<u32>,
}

/// The kernel's process table: a thread-safe registry of all agent processes.
pub struct AgentProcessTable {
    processes: Arc<Mutex<HashMap<String, AgentProcess>>>,
    next_id: Arc<Mutex<u64>>,
}

impl AgentProcessTable {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Create a new process table entry for the named agent.
    /// Returns the kernel-assigned process ID.
    pub fn spawn(&self, name: &str, config: AgentProcessConfig) -> String {
        let mut next = self.next_id.lock().unwrap();
        let id = format!("agent-{}", *next);
        *next += 1;

        let process = AgentProcess {
            id: id.clone(),
            name: name.to_string(),
            state: AgentState::Created,
            autonomous: config.autonomous,
            persistent: config.persistent,
            restart_on_failure: config.restart_on_failure,
            goal: config.goal,
            resources: config.resources,
            restart_count: 0,
            max_restarts: config.max_restarts.unwrap_or(3),
            created_at: Instant::now(),
            last_checkpoint: None,
            tokens_used: 0,
            cost_used: 0.0,
        };

        self.processes.lock().unwrap().insert(id.clone(), process);
        id
    }

    /// Transition an agent process to a new state.
    pub fn set_state(&self, id: &str, state: AgentState) {
        if let Some(p) = self.processes.lock().unwrap().get_mut(id) {
            p.state = state;
        }
    }

    /// Record a checkpoint for an agent (call before each significant decision point).
    pub fn checkpoint(&self, id: &str, snapshot: HashMap<String, String>, progress: f64) {
        if let Some(p) = self.processes.lock().unwrap().get_mut(id) {
            p.last_checkpoint = Some(AgentCheckpoint {
                timestamp: Instant::now(),
                state_snapshot: snapshot,
                goal_progress: progress,
            });
        }
    }

    /// Track resource consumption for an agent.
    pub fn track_usage(&self, id: &str, tokens: u64, cost_usd: f64) {
        if let Some(p) = self.processes.lock().unwrap().get_mut(id) {
            p.tokens_used += tokens;
            p.cost_used += cost_usd;
        }
    }

    /// Check whether an agent's resource budget has been exceeded.
    pub fn check_budget(&self, id: &str) -> Result<(), String> {
        let processes = self.processes.lock().unwrap();
        if let Some(p) = processes.get(id) {
            if let Some(token_budget) = p.resources.token_budget {
                if p.tokens_used >= token_budget {
                    return Err(format!(
                        "Agent '{}' exceeded token budget ({}/{})",
                        p.name, p.tokens_used, token_budget
                    ));
                }
            }
            if let Some(cost_budget) = p.resources.cost_budget {
                if p.cost_used >= cost_budget {
                    return Err(format!(
                        "Agent '{}' exceeded cost budget (${:.4}/${:.4})",
                        p.name, p.cost_used, cost_budget
                    ));
                }
            }
        }
        Ok(())
    }

    /// Determine if a crashed agent should be restarted.
    pub fn should_restart(&self, id: &str) -> bool {
        if let Some(p) = self.processes.lock().unwrap().get(id) {
            p.restart_on_failure
                && p.restart_count < p.max_restarts
                && matches!(p.state, AgentState::Crashed { .. })
        } else {
            false
        }
    }

    /// Increment restart count and transition to Recovering.
    /// Returns true if the restart is permitted, false if max restarts exceeded.
    pub fn restart(&self, id: &str) -> bool {
        if let Some(p) = self.processes.lock().unwrap().get_mut(id) {
            if p.restart_on_failure && p.restart_count < p.max_restarts {
                p.restart_count += 1;
                p.state = AgentState::Recovering;
                return true;
            }
        }
        false
    }

    /// Permanently terminate an agent.
    pub fn terminate(&self, id: &str) {
        if let Some(p) = self.processes.lock().unwrap().get_mut(id) {
            p.state = AgentState::Terminated;
        }
    }

    /// List all processes (all states).
    pub fn list(&self) -> Vec<AgentProcess> {
        self.processes.lock().unwrap().values().cloned().collect()
    }

    /// Get a single process by ID.
    pub fn get(&self, id: &str) -> Option<AgentProcess> {
        self.processes.lock().unwrap().get(id).cloned()
    }

    /// Find a process by agent name (returns first match).
    pub fn find_by_name(&self, name: &str) -> Option<AgentProcess> {
        self.processes
            .lock()
            .unwrap()
            .values()
            .find(|p| p.name == name)
            .cloned()
    }

    /// All running processes.
    pub fn running(&self) -> Vec<AgentProcess> {
        self.processes
            .lock()
            .unwrap()
            .values()
            .filter(|p| p.state == AgentState::Running)
            .cloned()
            .collect()
    }
}

impl Default for AgentProcessTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_creates_process_in_created_state() {
        let table = AgentProcessTable::new();
        let id = table.spawn("TestAgent", AgentProcessConfig::default());
        let p = table.get(&id).unwrap();
        assert_eq!(p.name, "TestAgent");
        assert_eq!(p.state, AgentState::Created);
        assert_eq!(p.restart_count, 0);
    }

    #[test]
    fn lifecycle_transitions() {
        let table = AgentProcessTable::new();
        let id = table.spawn("LifecycleAgent", AgentProcessConfig {
            restart_on_failure: true,
            ..Default::default()
        });
        
        table.set_state(&id, AgentState::Running);
        assert_eq!(table.get(&id).unwrap().state, AgentState::Running);
        
        table.set_state(&id, AgentState::Crashed { reason: "test".into() });
        assert!(table.should_restart(&id));
        
        assert!(table.restart(&id));
        assert_eq!(table.get(&id).unwrap().restart_count, 1);
        assert_eq!(table.get(&id).unwrap().state, AgentState::Recovering);
    }

    #[test]
    fn max_restarts_enforced() {
        let table = AgentProcessTable::new();
        let id = table.spawn("MaxRestartAgent", AgentProcessConfig {
            restart_on_failure: true,
            max_restarts: Some(2),
            ..Default::default()
        });

        for _ in 0..2 {
            table.set_state(&id, AgentState::Crashed { reason: "boom".into() });
            assert!(table.restart(&id));
        }

        table.set_state(&id, AgentState::Crashed { reason: "boom again".into() });
        assert!(!table.restart(&id)); // max restarts reached
        assert!(!table.should_restart(&id));
    }

    #[test]
    fn token_budget_enforcement() {
        let table = AgentProcessTable::new();
        let id = table.spawn("BudgetAgent", AgentProcessConfig {
            resources: ResourceAllocation {
                token_budget: Some(100),
                ..Default::default()
            },
            ..Default::default()
        });

        table.track_usage(&id, 50, 0.01);
        assert!(table.check_budget(&id).is_ok());

        table.track_usage(&id, 51, 0.01);
        assert!(table.check_budget(&id).is_err());
    }

    #[test]
    fn checkpoint_and_recovery_metadata() {
        let table = AgentProcessTable::new();
        let id = table.spawn("CheckpointAgent", AgentProcessConfig::default());

        let mut snapshot = HashMap::new();
        snapshot.insert("research_progress".to_string(), "50%".to_string());
        table.checkpoint(&id, snapshot, 0.5);

        let p = table.get(&id).unwrap();
        let cp = p.last_checkpoint.unwrap();
        assert_eq!(cp.goal_progress, 0.5);
        assert_eq!(cp.state_snapshot["research_progress"], "50%");
    }

    #[test]
    fn find_by_name() {
        let table = AgentProcessTable::new();
        table.spawn("Alpha", AgentProcessConfig::default());
        table.spawn("Beta", AgentProcessConfig::default());

        assert!(table.find_by_name("Alpha").is_some());
        assert!(table.find_by_name("Gamma").is_none());
    }
}

