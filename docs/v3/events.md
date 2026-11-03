# Event-Driven Architecture

AILang v3.0 features an asynchronous, decoupled event bus that routes signals between agents, devices, user interactions, and external webhooks.

## 1. Syntax

Events are emitted with `emit` and received with `on`:

```ail
// Emitting with optional payload
emit "document_uploaded" "financial_report.pdf";

// Emitting standalone signal
emit "system_health_check";

// Event handler
on document_uploaded {
    println("Processing document: " + payload);
    // Payload is implicitly in scope inside event handlers
}
```

## 2. Topic Matching & Wildcards

In the Kernel Event Bus (`kernel/src/event_bus.rs`):
- Topics support hierarchy separated by dots: `sensor.temperature.update`.
- Wildcard subscriptions: `sensor.*` or `*`.
- Priority-ordered event queues ensure safety-critical alerts preempt standard telemetry.
