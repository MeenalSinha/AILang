// kernel/src/event_bus.rs
//! Durable event bus for the AILang OS event system.
//!
//! In AILang v3.0, *everything* can emit events and *anything* can subscribe
//! to them. This is the runtime backing for:
//!
//! ```ail
//! emit "goal_completed" payload result
//!
//! on agent_failed { ... }
//! on model_available { ... }
//! on document_added { ... }
//! ```
//!
//! ## V3.0 honest limitations
//!
//! Events are dispatched synchronously in the calling thread.
//! A future async executor would dispatch events concurrently across tokio tasks.
//! The history is in-memory; "durable" here means events survive within the
//! process lifetime, not across crashes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// A single emitted event.
#[derive(Debug, Clone)]
pub struct Event {
    pub id: String,
    pub name: String,
    pub payload: Option<String>,
    pub emitted_at: Instant,
    pub source: Option<String>,
}

/// Type alias for event handler closures.
pub type EventHandler = Box<dyn Fn(&Event) + Send + Sync>;

/// The AILang OS event bus: pub/sub + history.
///
/// Handlers registered for event name `"*"` receive all events (wildcard).
pub struct EventBus {
    handlers: Arc<Mutex<HashMap<String, Vec<EventHandler>>>>,
    history: Arc<Mutex<Vec<Event>>>,
    next_id: Arc<Mutex<u64>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(Mutex::new(HashMap::new())),
            history: Arc::new(Mutex::new(Vec::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Subscribe a handler to a named event (or `"*"` for all events).
    pub fn subscribe(&self, event_name: &str, handler: EventHandler) {
        self.handlers
            .lock()
            .unwrap()
            .entry(event_name.to_string())
            .or_default()
            .push(handler);
    }

    /// Emit an event, dispatching it to all subscribers synchronously.
    pub fn emit(&self, name: &str, payload: Option<String>, source: Option<&str>) {
        let id = {
            let mut next = self.next_id.lock().unwrap();
            let id = format!("event-{}", *next);
            *next += 1;
            id
        };

        let event = Event {
            id,
            name: name.to_string(),
            payload,
            emitted_at: Instant::now(),
            source: source.map(|s| s.to_string()),
        };

        // Append to persistent history
        self.history.lock().unwrap().push(event.clone());

        // Dispatch to named handlers and wildcard handlers
        let handlers = self.handlers.lock().unwrap();
        for key in &[name, "*"] {
            if let Some(event_handlers) = handlers.get(*key) {
                for handler in event_handlers {
                    handler(&event);
                }
            }
        }
    }

    /// Full event history (all events ever emitted).
    pub fn history(&self) -> Vec<Event> {
        self.history.lock().unwrap().clone()
    }

    /// Events with a specific name.
    pub fn events_for(&self, name: &str) -> Vec<Event> {
        self.history
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.name == name)
            .cloned()
            .collect()
    }

    /// Total count of events emitted with a specific name.
    pub fn count(&self, name: &str) -> usize {
        self.events_for(name).len()
    }

    /// Whether any event with the given name has been emitted.
    pub fn has_event(&self, name: &str) -> bool {
        self.history.lock().unwrap().iter().any(|e| e.name == name)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn emit_and_history() {
        let bus = EventBus::new();
        bus.emit("goal_completed", Some("research done".into()), Some("ResearchAgent"));
        bus.emit("agent_started", None, None);

        assert_eq!(bus.history().len(), 2);
        assert!(bus.has_event("goal_completed"));
        assert!(bus.has_event("agent_started"));
        assert!(!bus.has_event("model_failed"));
    }

    #[test]
    fn named_handler_invoked() {
        let bus = EventBus::new();
        let received = Arc::new(Mutex::new(Vec::new()));
        let r = received.clone();

        bus.subscribe("goal_completed", Box::new(move |e| {
            r.lock().unwrap().push(e.name.clone());
        }));

        bus.emit("goal_completed", None, None);
        bus.emit("other_event", None, None);

        let got = received.lock().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0], "goal_completed");
    }

    #[test]
    fn wildcard_handler_receives_all_events() {
        let bus = EventBus::new();
        let count = Arc::new(Mutex::new(0usize));
        let c = count.clone();

        bus.subscribe("*", Box::new(move |_| {
            *c.lock().unwrap() += 1;
        }));

        bus.emit("agent_failed", None, None);
        bus.emit("model_available", None, None);
        bus.emit("document_added", None, None);

        assert_eq!(*count.lock().unwrap(), 3);
    }

    #[test]
    fn payload_and_source_preserved() {
        let bus = EventBus::new();
        bus.emit("data_event", Some("hello world".into()), Some("DataAgent"));

        let events = bus.events_for("data_event");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload.as_deref(), Some("hello world"));
        assert_eq!(events[0].source.as_deref(), Some("DataAgent"));
    }

    #[test]
    fn multiple_subscribers_on_same_event() {
        let bus = EventBus::new();
        let count = Arc::new(Mutex::new(0usize));
        for _ in 0..3 {
            let c = count.clone();
            bus.subscribe("ping", Box::new(move |_| { *c.lock().unwrap() += 1; }));
        }
        bus.emit("ping", None, None);
        assert_eq!(*count.lock().unwrap(), 3);
    }

    #[test]
    fn event_ids_are_unique() {
        let bus = EventBus::new();
        for _ in 0..5 {
            bus.emit("test", None, None);
        }
        let ids: Vec<String> = bus.history().into_iter().map(|e| e.id).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len());
    }
}

