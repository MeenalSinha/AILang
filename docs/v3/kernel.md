# AILang v3.0 — Intelligence Kernel Architecture

## Overview

The Intelligence Kernel is the foundational layer introduced in AILang v3.0.
It manages AI resources (agents, models, memory, knowledge, tools, goals,
context, devices) the same way a traditional OS kernel manages CPU, memory,
storage, network, processes, and devices.

## Architecture Diagram

```
                    AILang APPLICATION
                           │
                           ▼
                 ┌───────────────────┐
                 │   AILang LANGUAGE  │
                 │  (v3.0 extended)   │
                 └─────────┬─────────┘
                           │
                           ▼
                 ┌───────────────────┐
                 │    COMPILER        │
                 │ (Lexer→Parser→AST) │
                 └─────────┬─────────┘
                           │
                           ▼
                 ┌───────────────────────────────────────┐
                 │          INTELLIGENCE KERNEL            │
                 │                                        │
                 │  ┌──────────┐  ┌──────────────────┐   │
                 │  │  Agent   │  │  Goal Engine      │   │
                 │  │ Process  │  │  (plan/execute/   │   │
                 │  │  Table   │  │   verify/revise)  │   │
                 │  └──────────┘  └──────────────────┘   │
                 │                                        │
                 │  ┌──────────┐  ┌──────────────────┐   │
                 │  │Supervisor│  │  Model Fabric      │   │
                 │  │ Registry │  │  (failover chain)  │   │
                 │  └──────────┘  └──────────────────┘   │
                 │                                        │
                 │  ┌──────────┐  ┌──────────────────┐   │
                 │  │ Context  │  │  Resource Manager  │   │
                 │  │ Manager  │  │  (budget tracking) │   │
                 │  └──────────┘  └──────────────────┘   │
                 │                                        │
                 │  ┌──────────┐  ┌──────────────────┐   │
                 │  │ Event    │  │  Health Monitor    │   │
                 │  │   Bus    │  │  (system doctor)   │   │
                 │  └──────────┘  └──────────────────┘   │
                 │                                        │
                 │  ┌──────────┐  ┌──────────────────┐   │
                 │  │ Planner  │  │  App Isolation    │   │
                 │  │(task     │  │  (policy engine)  │   │
                 │  │ graphs)  │  │                   │   │
                 │  └──────────┘  └──────────────────┘   │
                 └─────────┬─────────────────────────────┘
                           │
        ┌──────────────────┼──────────────────┐
        ▼                  ▼                  ▼
   Memory Fabric     AI Runtime           Application
   (8 scope types)  (providers/tools)    (lifecycle/policy)
        │                  │                  │
        └──────────────────┼──────────────────┘
                           ▼
                 ┌───────────────────┐
                 │ DISTRIBUTED AI    │
                 │     RUNTIME       │
                 └─────────┬─────────┘
                           │
             ┌─────────────┼─────────────┐
             ▼             ▼             ▼
          LOCAL          EDGE          CLOUD
```

## Kernel Subsystems

### 1. Agent Process Table (`kernel::agent_process`)

Manages agent lifecycle as OS processes:

```
Created → Running → Suspended → Recovering → Running
                    ↓
                  Crashed → (supervisor decides) → Restarting
                    ↓ (max restarts exceeded)
                  Terminated
```

Features:
- Process table with unique kernel IDs
- Resource budget tracking (tokens, cost, energy)
- Checkpoint/restore for crash recovery
- Restart policy enforcement

### 2. Goal Engine (`kernel::goal_engine`)

Converts high-level goals into executable plans:

```
Goal (declared) → Planning → ExecutionPlan (task graph) → Executing
                                    ↓
                              Task allocation
                                    ↓
                              Agent execution
                                    ↓
                              Verification (constraint check)
                                    ↓
                         Completed (success/failure)
```

### 3. Supervisor Registry (`kernel::supervisor`)

Hierarchical fault tolerance inspired by Erlang/OTP:

- **OneForOne**: Only restart the failed child
- **OneForAll**: Restart all children on any failure
- **RestForOne**: Restart failed child + all subsequent children

### 4. Model Fabric (`kernel::model_fabric`)

Unified model abstraction with automatic failover:

```
Request for TextGeneration capability
     ↓
Try Primary (GPT5) → Available? → Use it
     ↓ Unavailable
Try Fallback (Claude) → Available? → Use it
     ↓ Unavailable
Try Emergency (LocalModel) → Available? → Use it
     ↓ Unavailable
Use Mock (always available)
```

Supports all model types:
- TextGeneration, ImageUnderstanding, SpeechRecognition
- SpeechSynthesis, Embedding, CodeGeneration, Reasoning
- MultiModal, Classification, ObjectDetection

### 5. Event Bus (`kernel::event_bus`)

Durable OS-level event system:

```ail
// Standard OS events
on agent_failed { ... }
on model_available { ... }
on document_added { ... }
on goal_completed { ... }
on device_connected { ... }
on memory_changed { ... }
on security_violation { ... }
```

Supports:
- Named handlers (per event type)
- Wildcard handlers (`"*"`)
- Full event history (within process lifetime)

### 6. Health Monitor (`kernel::health_monitor`)

Self-diagnostic platform powering `ail system doctor`:

- Per-component health status (Healthy/Degraded/Critical/Unknown)
- Alert history with severity levels and actionable suggestions
- System-wide aggregate health rollup
- Human-readable and machine-readable reports

### 7. Context Manager (`kernel::context_manager`)

Token-budget-aware context window management:

```
Context Entries (priority-ordered):
  [200] Conversation turns    → included first
  [180] Current task          → included second
  [150] Agent context         → included third
  [120] Memory retrievals     → included fourth
  [100] Knowledge chunks      → included fifth
  [ 80] Tool results          → included sixth
  [ 40] Environment context   → included last (if budget remains)
```

Features:
- Automatic expiry (TTL-based)
- Word-count compression for oversized entries
- Per-kind clearing (e.g. clear all tool results)

### 8. Resource Manager (`kernel::resource_manager`)

Intelligence-as-a-resource allocation and tracking:

```ail
resource ResearchIntelligence {
    model GPT5
    budget 1000 tokens
    latency < 500ms
}

allocate ResearchIntelligence to Researcher
```

Enforces:
- Token budgets (per agent, per resource)
- Cost budgets (in USD)
- Latency constraints
- Energy budgets (for edge devices)

### 9. Planner (`kernel::planner`)

Goal → Task graph decomposition:

```
Objective: "Create a report on quantum computing"
     ↓
TaskGraph:
  Task 1: "Create a" (no deps)
  Task 2: "report on" (deps: Task 1)
  Task 3: "quantum computing" (deps: Task 2)
```

> **V3.0 Note**: The planner uses a heuristic word-count decomposition.
> A production planner would invoke a planning LLM.

### 10. Application Isolation (`kernel::isolation`)

Per-application namespaces:

- Agent registry per application
- Security policy evaluation (allow/deny rules)
- Resource limit enforcement
- Memory namespace isolation

## V3.0 Honest Limitations

| Subsystem | Status | Future Path |
|-----------|--------|-------------|
| Agent Process Table | ✓ Real metadata tracking | Real async execution requires Tokio |
| Goal Engine | ✓ Real lifecycle + constraint check | Real planner needs LLM |
| Supervisor | ✓ Real strategy logic | Real restart needs async executor |
| Model Fabric | ✓ Real failover selection | Real inference in ai_runtime |
| Event Bus | ✓ Real sync dispatch + history | Async dispatch needs Tokio |
| Health Monitor | ✓ Real component tracking | Real metrics need OpenTelemetry |
| Context Manager | ✓ Real priority/TTL/compression | Semantic compression needs LLM |
| Resource Manager | ✓ Real budget enforcement | Real cost needs provider billing APIs |
| Planner | ✓ Real task graphs | Smart decomposition needs planning LLM |
| App Isolation | ✓ Real policy evaluation | OS-level isolation needs seccomp/WASM |

