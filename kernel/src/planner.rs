// kernel/src/planner.rs
//! Goal → Task Graph decomposition.
//!
//! The planner converts a goal's objective string into a task graph that
//! agents can execute. In v3.0, this is a deterministic heuristic planner.
//! A production planner would call a planning LLM.

/// A directed task graph where edges represent dependencies.
#[derive(Debug, Clone)]
pub struct TaskGraph {
    pub tasks: Vec<Task>,
    pub edges: Vec<(usize, usize)>, // (from_index, to_index) — to depends on from
}

/// One node in the task graph.
#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub assigned_agent: Option<String>,
    pub required_tools: Vec<String>,
    pub estimated_tokens: u64,
    pub estimated_cost_usd: f64,
    pub estimated_duration_ms: u64,
}

impl TaskGraph {
    /// Return tasks in topological order (dependencies before dependents).
    pub fn topological_order(&self) -> Vec<&Task> {
        // Build adjacency from edges
        let n = self.tasks.len();
        let mut in_degree = vec![0usize; n];
        let mut adj: Vec<Vec<usize>> = vec![vec![]; n];

        for &(from, to) in &self.edges {
            adj[from].push(to);
            in_degree[to] += 1;
        }

        let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut result = Vec::new();

        while !queue.is_empty() {
            let node = queue.remove(0);
            result.push(&self.tasks[node]);
            for &next in &adj[node] {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    queue.push(next);
                }
            }
        }
        result
    }

    pub fn estimated_total_cost(&self) -> f64 {
        self.tasks.iter().map(|t| t.estimated_cost_usd).sum()
    }

    pub fn estimated_total_duration_ms(&self) -> u64 {
        // Critical path: sum of sequential tasks (simplified — no parallelism in v3.0)
        self.tasks.iter().map(|t| t.estimated_duration_ms).sum()
    }
}

/// The heuristic planner.
pub struct Planner;

impl Planner {
    pub fn new() -> Self {
        Self
    }

    /// Decompose an objective into a task graph.
    ///
    /// Heuristic: split objective into N subtasks by word-count chunks,
    /// linked in a linear chain (each depends on the previous).
    ///
    /// A production planner would use an LLM here.
    pub fn decompose(&self, goal_id: &str, objective: &str) -> TaskGraph {
        let words: Vec<&str> = objective.split_whitespace().collect();
        let num_tasks = (words.len() / 5 + 1).clamp(1, 8);

        let mut tasks = Vec::new();
        let mut edges = Vec::new();

        for i in 0..num_tasks {
            let start = i * words.len() / num_tasks;
            let end = ((i + 1) * words.len() / num_tasks).min(words.len());
            let chunk: Vec<&str> = words[start..end].to_vec();

            tasks.push(Task {
                id: format!("{}-task-{}", goal_id, i + 1),
                description: format!("Step {}: {}", i + 1, chunk.join(" ")),
                assigned_agent: None,
                required_tools: vec![],
                estimated_tokens: 200 * (i + 1) as u64,
                estimated_cost_usd: 0.002 * (i + 1) as f64,
                estimated_duration_ms: 500 * (i + 1) as u64,
            });

            if i > 0 {
                edges.push((i - 1, i)); // linear chain
            }
        }

        TaskGraph { tasks, edges }
    }

    /// Assign agents to tasks based on keyword matching.
    ///
    /// Simple heuristic: assigns tasks mentioning "search", "find" to search agents;
    /// tasks mentioning "write", "report" to writing agents; otherwise round-robins.
    pub fn assign_agents(
        &self,
        graph: &mut TaskGraph,
        available_agents: &[String],
    ) {
        if available_agents.is_empty() {
            return;
        }
        for (i, task) in graph.tasks.iter_mut().enumerate() {
            let desc_lower = task.description.to_lowercase();
            let agent = if desc_lower.contains("search") || desc_lower.contains("find") {
                available_agents
                    .iter()
                    .find(|a| a.to_lowercase().contains("search") || a.to_lowercase().contains("research"))
                    .unwrap_or(&available_agents[i % available_agents.len()])
            } else if desc_lower.contains("write") || desc_lower.contains("report") {
                available_agents
                    .iter()
                    .find(|a| a.to_lowercase().contains("write") || a.to_lowercase().contains("publish"))
                    .unwrap_or(&available_agents[i % available_agents.len()])
            } else {
                &available_agents[i % available_agents.len()]
            };
            task.assigned_agent = Some(agent.clone());
        }
    }
}

impl Default for Planner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompose_single_word_objective() {
        let p = Planner::new();
        let g = p.decompose("goal-1", "Summarize");
        assert!(!g.tasks.is_empty());
    }

    #[test]
    fn decompose_creates_sequential_dependencies() {
        let p = Planner::new();
        let g = p.decompose("goal-1", "step one step two step three step four step five step six");
        assert!(g.tasks.len() > 1);
        // Linear chain: edge i → i+1
        for (from, to) in &g.edges {
            assert_eq!(to - from, 1);
        }
    }

    #[test]
    fn topological_order_respects_dependencies() {
        let p = Planner::new();
        let g = p.decompose("goal-1", "a b c d e f g h i j k l m n o p q r s t");
        let ordered = g.topological_order();
        assert_eq!(ordered.len(), g.tasks.len());
        // First task should have no dependencies (index 0 in chain)
        assert!(ordered[0].id.ends_with("-task-1"));
    }

    #[test]
    fn agent_assignment_round_robins() {
        let p = Planner::new();
        let mut g = p.decompose("goal-1", "a b c d e f g h i j k l m n o");
        let agents = vec!["AgentA".to_string(), "AgentB".to_string()];
        p.assign_agents(&mut g, &agents);
        // All tasks should have an assigned agent
        assert!(g.tasks.iter().all(|t| t.assigned_agent.is_some()));
    }

    #[test]
    fn estimated_totals_are_nonzero() {
        let p = Planner::new();
        let g = p.decompose("goal-1", "analyze research write publish");
        assert!(g.estimated_total_cost() > 0.0);
        assert!(g.estimated_total_duration_ms() > 0);
    }
}

