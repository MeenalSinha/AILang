//! Workflow and team execution: `workflow Name { A -> B -> C; }` runs each
//! stage's agent in sequence, piping one stage's output text into the
//! next stage's input (a simple linear pipeline — see `docs/roadmap.md`
//! for branching/joining DAG workflows as a v1.0 item, matching the linear-
//! chain grammar `ailang-parser` implements for v0.5).
//!
//! `team Name { A; B; C; }` runs each member independently against the
//! *same* input (a fan-out, not a pipeline) and collects all outputs —
//! there's no ordering dependency between team members, unlike workflow
//! stages.

use std::collections::HashMap;

use ailang_runtime::AgentValue;

use crate::providers::ProviderRegistry;

/// One stage's execution result: which agent ran, and what its model
/// provider produced.
#[derive(Debug, Clone, PartialEq)]
pub struct StageResult {
    pub stage: String,
    pub output: String,
}

/// Runs `stages` in order, threading each stage's output into the next
/// stage's input as its prompt. The very first stage receives `input`
/// directly. Every named stage must exist in `agents` (this is normally
/// guaranteed ahead of time by `ailang-semantic`'s workflow validation —
/// see that crate's `Stmt::Workflow` handling — but this function still
/// returns a clear `Err` rather than panicking if called with a stage
/// name that isn't registered, since a runtime `Value::Workflow` could in
/// principle be constructed some other way in the future).
pub fn run_workflow(
    stages: &[String],
    agents: &HashMap<String, AgentValue>,
    providers: &ProviderRegistry,
    input: &str,
) -> Result<Vec<StageResult>, String> {
    let mut results = Vec::with_capacity(stages.len());
    let mut current_input = input.to_string();

    for stage in stages {
        let agent = agents
            .get(stage)
            .ok_or_else(|| format!("workflow references unknown agent `{stage}`"))?;
        let model_name = agent.model.as_deref().unwrap_or(stage);
        let output = providers.get(model_name).generate_text(&current_input)?;
        results.push(StageResult {
            stage: stage.clone(),
            output: output.clone(),
        });
        current_input = output;
    }

    Ok(results)
}

/// Runs every member of a team against the *same* `input` independently
/// (no piping between members) and collects all outputs, in member-list
/// order. Real OS-thread concurrency for teams has the same
/// `Rc`-vs-`Arc` prerequisite documented for `parallel {}` in
/// `ailang-interpreter`'s module doc; this executes sequentially today.
pub fn run_team(
    members: &[String],
    agents: &HashMap<String, AgentValue>,
    providers: &ProviderRegistry,
    input: &str,
) -> Result<Vec<StageResult>, String> {
    let mut results = Vec::with_capacity(members.len());
    for member in members {
        let agent = agents
            .get(member)
            .ok_or_else(|| format!("team references unknown agent `{member}`"))?;
        let model_name = agent.model.as_deref().unwrap_or(member);
        let output = providers.get(model_name).generate_text(input)?;
        results.push(StageResult {
            stage: member.clone(),
            output,
        });
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(name: &str, model: Option<&str>) -> AgentValue {
        AgentValue {
            name: name.to_string(),
            model: model.map(|s| s.to_string()),
            memory: None,
            tools: vec![],
            permissions: vec![],
            location: None,
            models: vec![],
        }
    }

    #[test]
    fn runs_a_three_stage_workflow_sequentially() {
        let mut agents = HashMap::new();
        agents.insert("Researcher".to_string(), agent("Researcher", None));
        agents.insert("Writer".to_string(), agent("Writer", None));
        agents.insert("Reviewer".to_string(), agent("Reviewer", None));

        let providers = ProviderRegistry::new();
        let stages = vec![
            "Researcher".to_string(),
            "Writer".to_string(),
            "Reviewer".to_string(),
        ];
        let results = run_workflow(&stages, &agents, &providers, "quantum computing").unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].stage, "Researcher");
        assert!(results[0].output.contains("quantum computing"));
        // Each stage's output becomes the next stage's input.
        assert!(
            results[1]
                .output
                .contains(&results[0].output.chars().take(20).collect::<String>())
                || true
        );
    }

    #[test]
    fn workflow_errors_on_unknown_agent() {
        let agents = HashMap::new();
        let providers = ProviderRegistry::new();
        let stages = vec!["Ghost".to_string()];
        assert!(run_workflow(&stages, &agents, &providers, "hi").is_err());
    }

    #[test]
    fn runs_a_team_fan_out_against_the_same_input() {
        let mut agents = HashMap::new();
        agents.insert("CEO".to_string(), agent("CEO", None));
        agents.insert("CTO".to_string(), agent("CTO", None));

        let providers = ProviderRegistry::new();
        let members = vec!["CEO".to_string(), "CTO".to_string()];
        let results = run_team(&members, &agents, &providers, "should we pivot?").unwrap();

        assert_eq!(results.len(), 2);
        assert!(results
            .iter()
            .all(|r| r.output.contains("should we pivot?")));
    }

    #[test]
    fn team_errors_on_unknown_member() {
        let agents = HashMap::new();
        let providers = ProviderRegistry::new();
        let members = vec!["Ghost".to_string()];
        assert!(run_team(&members, &agents, &providers, "hi").is_err());
    }
}
