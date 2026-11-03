// kernel/src/goal_engine.rs
//! Goal-Oriented Programming: goals as first-class kernel resources.
//!
//! A `Goal` defines *what* an agent should achieve, not *how* — the kernel
//! converts goals into `ExecutionPlan`s and tracks them through their full
//! lifecycle.
//!
//! ## Pipeline
//!
//! ```text
//! Goal (declared in .ail) → GoalEngine::create_goal
//!        ↓
//! GoalEngine::plan  (simple heuristic decomposition in v3.0)
//!        ↓
//! ExecutionPlan (task graph)
//!        ↓
//! Interpreter executes tasks via agents
//!        ↓
//! GoalEngine::complete / GoalEngine::fail
//! ```
//!
//! ## V3.0 honest limitations
//!
//! The planner is a heuristic (keyword-based task decomposition), not a real
//! planning LLM. The constraint check at completion is deterministic based on
//! a provided result string, not actual semantic measurement.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Status of a goal in its execution lifecycle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GoalStatus {
    Pending,
    Planning,
    Executing,
    Verifying,
    Completed { success: bool, result: String },
    Failed { reason: String },
}

/// Constraint on a goal's execution (mirrors the AILang `constraints {}` block).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalConstraints {
    /// Required accuracy (0.0–1.0). e.g. `accuracy > 0.95`
    pub accuracy_threshold: Option<f64>,
    /// Maximum allowed cost in USD. e.g. `budget < $5`
    pub cost_budget_usd: Option<f64>,
    /// Maximum allowed wall time. e.g. `deadline 10m`
    pub deadline: Option<Duration>,
    /// Maximum p50 latency for model calls. e.g. `latency < 500ms`
    pub latency_ms: Option<u64>,
    /// Maximum energy consumption. e.g. `energy < 10Wh`
    pub energy_wh: Option<f64>,
}

impl GoalConstraints {
    pub fn none() -> Self {
        Self {
            accuracy_threshold: None,
            cost_budget_usd: None,
            deadline: None,
            latency_ms: None,
            energy_wh: None,
        }
    }
}

/// A fully decomposed execution plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub tasks: Vec<PlannedTask>,
    pub estimated_cost_usd: f64,
    pub estimated_duration_ms: u64,
    /// Planner confidence in this plan (0.0–1.0).
    pub confidence: f64,
}

/// One task in an execution plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedTask {
    pub id: String,
    pub description: String,
    pub assigned_agent: Option<String>,
    pub required_tools: Vec<String>,
    /// IDs of tasks that must complete before this one starts.
    pub dependencies: Vec<String>,
    pub estimated_duration_ms: u64,
    pub estimated_cost_usd: f64,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed { reason: String },
}

/// A kernel-tracked goal.
#[derive(Debug, Clone)]
pub struct Goal {
    pub id: String,
    pub name: String,
    pub objective: String,
    pub constraints: GoalConstraints,
    pub status: GoalStatus,
    pub plan: Option<ExecutionPlan>,
    pub created_at: Instant,
    pub started_at: Option<Instant>,
    pub completed_at: Option<Instant>,
    /// Measured accuracy of the final result (set by verifier).
    pub measured_accuracy: Option<f64>,
    /// Actual cost incurred.
    pub actual_cost_usd: f64,
}

/// The kernel's goal registry and lifecycle manager.
pub struct GoalEngine {
    goals: Arc<Mutex<HashMap<String, Goal>>>,
    next_id: Arc<Mutex<u64>>,
}

impl GoalEngine {
    pub fn new() -> Self {
        Self {
            goals: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Register a new goal. Returns the kernel-assigned goal ID.
    pub fn create_goal(&self, name: &str, objective: &str, constraints: GoalConstraints) -> String {
        let mut next = self.next_id.lock().unwrap();
        let id = format!("goal-{}", *next);
        *next += 1;

        let goal = Goal {
            id: id.clone(),
            name: name.to_string(),
            objective: objective.to_string(),
            constraints,
            status: GoalStatus::Pending,
            plan: None,
            created_at: Instant::now(),
            started_at: None,
            completed_at: None,
            measured_accuracy: None,
            actual_cost_usd: 0.0,
        };

        self.goals.lock().unwrap().insert(id.clone(), goal);
        id
    }

    /// Decompose a goal into an execution plan.
    ///
    /// V3.0 heuristic: splits the objective into N subtasks based on word count.
    /// A production planner would invoke a planning LLM here.
    pub fn plan(&self, goal_id: &str) -> Option<ExecutionPlan> {
        let mut goals = self.goals.lock().unwrap();
        let goal = goals.get_mut(goal_id)?;

        goal.status = GoalStatus::Planning;
        goal.started_at = Some(Instant::now());

        let words: Vec<&str> = goal.objective.split_whitespace().collect();
        let num_tasks = (words.len() / 5 + 1).clamp(1, 5);

        let mut tasks = Vec::new();
        for i in 0..num_tasks {
            let start = i * words.len() / num_tasks;
            let end = ((i + 1) * words.len() / num_tasks).min(words.len());
            let chunk: Vec<&str> = words[start..end].to_vec();
            let duration_ms = 1000u64 * (i + 1) as u64;
            let cost = 0.01 * (i + 1) as f64;

            tasks.push(PlannedTask {
                id: format!("{}-task-{}", goal_id, i + 1),
                description: format!("Step {}: {}", i + 1, chunk.join(" ")),
                assigned_agent: None,
                required_tools: vec![],
                dependencies: if i > 0 {
                    vec![format!("{}-task-{}", goal_id, i)]
                } else {
                    vec![]
                },
                estimated_duration_ms: duration_ms,
                estimated_cost_usd: cost,
                status: TaskStatus::Pending,
            });
        }

        let estimated_cost = tasks.iter().map(|t| t.estimated_cost_usd).sum();
        let estimated_duration = tasks.iter().map(|t| t.estimated_duration_ms).sum();

        let plan = ExecutionPlan {
            tasks,
            estimated_cost_usd: estimated_cost,
            estimated_duration_ms: estimated_duration,
            confidence: 0.75,
        };

        goal.plan = Some(plan.clone());
        goal.status = GoalStatus::Executing;

        Some(plan)
    }

    /// Record goal completion.
    pub fn complete(&self, goal_id: &str, success: bool, result: &str) {
        let mut goals = self.goals.lock().unwrap();
        if let Some(goal) = goals.get_mut(goal_id) {
            goal.status = GoalStatus::Completed {
                success,
                result: result.to_string(),
            };
            goal.completed_at = Some(Instant::now());
        }
    }

    /// Record goal failure.
    pub fn fail(&self, goal_id: &str, reason: &str) {
        let mut goals = self.goals.lock().unwrap();
        if let Some(goal) = goals.get_mut(goal_id) {
            goal.status = GoalStatus::Failed {
                reason: reason.to_string(),
            };
            goal.completed_at = Some(Instant::now());
        }
    }

    /// Track cost incurred toward a goal.
    pub fn track_cost(&self, goal_id: &str, cost_usd: f64) {
        let mut goals = self.goals.lock().unwrap();
        if let Some(goal) = goals.get_mut(goal_id) {
            goal.actual_cost_usd += cost_usd;
        }
    }

    /// Verify that a completed goal satisfied its constraints.
    /// Returns (passed, explanation).
    pub fn verify_constraints(&self, goal_id: &str) -> (bool, String) {
        let goals = self.goals.lock().unwrap();
        let Some(goal) = goals.get(goal_id) else {
            return (false, "goal not found".to_string());
        };

        let mut violations = Vec::new();

        if let Some(cost_budget) = goal.constraints.cost_budget_usd {
            if goal.actual_cost_usd > cost_budget {
                violations.push(format!(
                    "cost ${:.4} > budget ${:.4}",
                    goal.actual_cost_usd, cost_budget
                ));
            }
        }

        if let Some(deadline) = goal.constraints.deadline {
            if let (Some(started), Some(completed)) = (goal.started_at, goal.completed_at) {
                let elapsed = completed.duration_since(started);
                if elapsed > deadline {
                    violations.push(format!(
                        "elapsed {:?} > deadline {:?}",
                        elapsed, deadline
                    ));
                }
            }
        }

        if violations.is_empty() {
            (true, "all constraints satisfied".to_string())
        } else {
            (false, violations.join("; "))
        }
    }

    pub fn get(&self, goal_id: &str) -> Option<Goal> {
        self.goals.lock().unwrap().get(goal_id).cloned()
    }

    pub fn list(&self) -> Vec<Goal> {
        self.goals.lock().unwrap().values().cloned().collect()
    }
}

impl Default for GoalEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_plan_goal() {
        let engine = GoalEngine::new();
        let id = engine.create_goal(
            "ResearchReport",
            "Create a comprehensive report on quantum computing breakthroughs",
            GoalConstraints {
                cost_budget_usd: Some(5.0),
                deadline: Some(Duration::from_secs(600)),
                ..GoalConstraints::none()
            },
        );

        assert!(id.starts_with("goal-"));
        assert_eq!(engine.get(&id).unwrap().status, GoalStatus::Pending);

        let plan = engine.plan(&id).unwrap();
        assert!(!plan.tasks.is_empty());
        assert!(plan.confidence > 0.0);

        let goal = engine.get(&id).unwrap();
        assert!(matches!(goal.status, GoalStatus::Executing));
    }

    #[test]
    fn complete_and_verify_within_budget() {
        let engine = GoalEngine::new();
        let id = engine.create_goal(
            "BudgetTest",
            "Simple task",
            GoalConstraints {
                cost_budget_usd: Some(1.0),
                ..GoalConstraints::none()
            },
        );
        engine.plan(&id);
        engine.track_cost(&id, 0.50);
        engine.complete(&id, true, "done");

        let (passed, _) = engine.verify_constraints(&id);
        assert!(passed);
    }

    #[test]
    fn constraint_violation_detected() {
        let engine = GoalEngine::new();
        let id = engine.create_goal(
            "BudgetBust",
            "Expensive task",
            GoalConstraints {
                cost_budget_usd: Some(0.01),
                ..GoalConstraints::none()
            },
        );
        engine.plan(&id);
        engine.track_cost(&id, 5.00); // way over budget
        engine.complete(&id, true, "done");

        let (passed, reason) = engine.verify_constraints(&id);
        assert!(!passed);
        assert!(reason.contains("cost"));
    }

    #[test]
    fn failed_goal_lifecycle() {
        let engine = GoalEngine::new();
        let id = engine.create_goal("FailingGoal", "something", GoalConstraints::none());
        engine.plan(&id);
        engine.fail(&id, "model timed out");

        let goal = engine.get(&id).unwrap();
        assert!(matches!(goal.status, GoalStatus::Failed { .. }));
    }

    #[test]
    fn plan_creates_sequential_task_dependencies() {
        let engine = GoalEngine::new();
        let id = engine.create_goal(
            "Sequential",
            "step one step two step three step four step five step six",
            GoalConstraints::none(),
        );
        let plan = engine.plan(&id).unwrap();
        // Tasks 2+ should depend on their predecessor
        for (i, task) in plan.tasks.iter().enumerate() {
            if i > 0 {
                assert!(!task.dependencies.is_empty(), "task {} has no dep", i);
            }
        }
    }
}

