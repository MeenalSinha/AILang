//! Observability foundation: a real, in-process execution trace recording
//! workflow stages, team fan-outs, and tool invocations as they happen,
//! with real wall-clock timestamps — enough to answer "what ran, in what
//! order, and when" for a single program run.
//!
//! ## Scope
//!
//! This is a trace **recorder**, not the full "Observability" subsystem
//! the v1.0 brief describes (no distributed tracing across processes, no
//! metrics aggregation, no cost/token-usage tracking — there's no real
//! model billing to track when `MockProvider` is the default provider, see
//! `providers.rs`). What's real: every event has an accurate timestamp
//! (`std::time::Instant`-relative, so ordering and durations are
//! trustworthy even though wall-clock accuracy depends on the host clock),
//! and `ail run --trace` (see `cli/src/main.rs`) prints it as a readable
//! timeline. See `docs/roadmap.md` for the rest of "Observability."

use std::cell::RefCell;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub struct TraceEvent {
    pub kind: String,
    pub label: String,
    /// Seconds since this `ExecutionTrace` was created (i.e. since the
    /// interpreter that owns it started running the program) — a
    /// monotonic, always-increasing measure of "when in this run,"
    /// which is what a timeline needs; not a wall-clock/Unix timestamp
    /// (see `security::AuditEntry`/`memory`'s versioning for that instead).
    pub elapsed_seconds: f64,
}

pub struct ExecutionTrace {
    started_at: Instant,
    events: RefCell<Vec<TraceEvent>>,
}

impl Default for ExecutionTrace {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            events: RefCell::new(Vec::new()),
        }
    }
}

impl ExecutionTrace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, kind: &str, label: &str) {
        self.events.borrow_mut().push(TraceEvent {
            kind: kind.to_string(),
            label: label.to_string(),
            elapsed_seconds: self.started_at.elapsed().as_secs_f64(),
        });
    }

    pub fn events(&self) -> Vec<TraceEvent> {
        self.events.borrow().clone()
    }

    /// Renders the trace as a simple, readable timeline —
    /// `[+0.000123s] workflow_stage: Researcher`, one line per event, in
    /// recorded order. Used by `ail run --trace`.
    pub fn render_timeline(&self) -> String {
        self.events
            .borrow()
            .iter()
            .map(|e| format!("[+{:.6}s] {}: {}", e.elapsed_seconds, e.kind, e.label))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_events_in_order() {
        let trace = ExecutionTrace::new();
        trace.record("workflow_stage", "Researcher");
        trace.record("workflow_stage", "Writer");
        let events = trace.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].label, "Researcher");
        assert_eq!(events[1].label, "Writer");
    }

    #[test]
    fn timestamps_are_monotonically_non_decreasing() {
        let trace = ExecutionTrace::new();
        trace.record("a", "first");
        trace.record("a", "second");
        let events = trace.events();
        assert!(events[1].elapsed_seconds >= events[0].elapsed_seconds);
    }

    #[test]
    fn render_timeline_produces_one_line_per_event() {
        let trace = ExecutionTrace::new();
        trace.record("tool_invocation", "Calculator");
        trace.record("workflow_stage", "Reviewer");
        let rendered = trace.render_timeline();
        assert_eq!(rendered.lines().count(), 2);
        assert!(rendered.contains("tool_invocation: Calculator"));
        assert!(rendered.contains("workflow_stage: Reviewer"));
    }

    #[test]
    fn empty_trace_renders_as_empty_string() {
        let trace = ExecutionTrace::new();
        assert_eq!(trace.render_timeline(), "");
    }
}
