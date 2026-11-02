//! AI Scheduler foundation: a real priority queue for deferred task
//! execution. The queue itself (enqueue, dequeue-by-priority, stable
//! ordering for equal priorities) is genuinely implemented and tested;
//! actually *calling* a scheduled function is `ailang-interpreter`'s job
//! (it needs a `FunctionValue`, which this crate deliberately doesn't
//! depend on — see the crate-level doc's dependency-direction rule), so
//! this module only holds names/priorities.
//!
//! ## Scope
//!
//! This is priority scheduling only — not the resource-aware, cost-aware,
//! latency-aware, GPU-aware, or energy-aware scheduling the v2.0 brief
//! describes, all of which need real telemetry (actual resource usage,
//! actual provider cost/latency data) this offline-by-default foundation
//! doesn't have. See `docs/roadmap.md`.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Debug, Clone, Eq, PartialEq)]
struct ScheduledTask {
    priority: i64,
    // Tie-breaker: earlier-enqueued tasks run first among equal
    // priorities (a real, if simple, fairness guarantee) — implemented
    // as a descending sequence number so a max-heap naturally prefers the
    // smaller (earlier) sequence number when priorities tie.
    sequence: std::cmp::Reverse<u64>,
    function_name: String,
}

impl Ord for ScheduledTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.sequence.cmp(&other.sequence))
    }
}

impl PartialOrd for ScheduledTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A real priority queue of function names awaiting execution. Higher
/// `priority` runs first; among equal priorities, earlier-scheduled tasks
/// run first (FIFO tie-breaking).
#[derive(Default)]
pub struct PriorityScheduler {
    heap: BinaryHeap<ScheduledTask>,
    next_sequence: u64,
}

impl PriorityScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn schedule(&mut self, function_name: &str, priority: i64) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.heap.push(ScheduledTask {
            priority,
            sequence: std::cmp::Reverse(sequence),
            function_name: function_name.to_string(),
        });
    }

    /// Removes and returns every scheduled function name, in priority
    /// (then FIFO) order.
    pub fn drain(&mut self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.heap.len());
        while let Some(task) = self.heap.pop() {
            out.push(task.function_name);
        }
        out
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drains_in_priority_order_highest_first() {
        let mut scheduler = PriorityScheduler::new();
        scheduler.schedule("low", 1);
        scheduler.schedule("high", 10);
        scheduler.schedule("medium", 5);
        assert_eq!(scheduler.drain(), vec!["high", "medium", "low"]);
    }

    #[test]
    fn equal_priorities_run_in_fifo_order() {
        let mut scheduler = PriorityScheduler::new();
        scheduler.schedule("first", 5);
        scheduler.schedule("second", 5);
        scheduler.schedule("third", 5);
        assert_eq!(scheduler.drain(), vec!["first", "second", "third"]);
    }

    #[test]
    fn drain_empties_the_queue() {
        let mut scheduler = PriorityScheduler::new();
        scheduler.schedule("a", 1);
        scheduler.drain();
        assert!(scheduler.is_empty());
        assert_eq!(scheduler.drain(), Vec::<String>::new());
    }

    #[test]
    fn len_reflects_pending_tasks() {
        let mut scheduler = PriorityScheduler::new();
        assert_eq!(scheduler.len(), 0);
        scheduler.schedule("a", 1);
        scheduler.schedule("b", 2);
        assert_eq!(scheduler.len(), 2);
    }
}
