# AILang v3.0 — New Language Constructs

This file documents all v3.0 language constructs with full examples.

---

## Goal-Oriented Programming

```ail
// Declare a goal with constraints
goal BuildResearchReport {
    objective "Create a comprehensive report on quantum computing"

    constraints {
        accuracy > 0.95
        budget < $5
        deadline 10m
        latency < 500ms
    }
}
```

---

## Agent Supervisors

```ail
// Hierarchical supervision — one_for_one strategy
supervisor ResearchSupervisor {
    strategy one_for_one

    children [Researcher, Analyst, Writer]
}

// Autonomous persistent agent
agent ResearchDaemon {
    autonomous
    persistent
    restart_on_failure

    model mock
    goal "Monitor scientific discoveries"
}
```

---

## Intelligence as a Resource

```ail
// Declare intelligence resource with budget
resource ResearchIntelligence {
    model mock
    budget 1000 tokens
    latency < 500ms
    constraint energy < 10Wh
}

// Allocate resource to agent
allocate ResearchIntelligence to Researcher
```

---

## Model Fabric with Failover

```ail
// Primary, fallback, emergency models
model primary GPT5 { endpoint "http://api.gpt5.example" }
model fallback Claude { endpoint "http://api.claude.example" }
model emergency LocalModel { endpoint "http://localhost:11434" }

// The runtime automatically fails over: GPT5 → Claude → LocalModel → mock
agent Researcher {
    models [GPT5, Claude, LocalModel]
}
```

---

## Controlled Reasoning Loop

```ail
agent SmartResearcher {
    model mock

    fn research(topic: string) -> string {
        let result = ""

        reason {
            max_iterations 5
            budget 500 tokens

            plan
            execute
            verify
            revise
        }

        return result
    }
}
```

---

## Device Abstraction

```ail
// Declare devices with explicit permissions
device VisionCamera {
    kind camera
    permission camera
}

device AudioInput {
    kind microphone
    permission audio
}

// Use in a pipeline
pipeline VisionAssistant {
    VisionCamera
    -> vision_model
    -> reasoning
    -> AudioInput
}
```

---

## Declarative UI

```ail
ui AgentDashboard {
    title "AI Control Center"

    panel Agents
    panel Memory
    panel Goals
    panel Tasks
    panel Observability
}
```

---

## Event-Driven OS

```ail
// Emit events
emit "goal_completed" payload result
emit "agent_failed" payload error_message
emit "model_unavailable" payload "GPT5"

// Subscribe to events
on goal_completed {
    remember "last_goal_result" result
}

on agent_failed {
    supervisor.handle_failure(failed_agent, reason)
}

on model_unavailable {
    model_fabric.failover()
}
```

---

## Application Model

```ail
application ResearchPlatform {
    agents [ResearchDaemon, Analyst, Writer, Publisher]

    models [GPT5, Claude, LocalModel]

    knowledge [papers_kb, web_sources]

    memory [ResearchMemory]

    workflows [ResearchWorkflow]

    policies {
        allow ResearchDaemon internet
        allow ResearchDaemon knowledge.read
        deny ResearchDaemon filesystem.write
        allow Analyst knowledge.read
        allow Writer filesystem.write
        deny * secrets.export
    }
}
```

---

## Security Policies

```ail
policy StrictResearchPolicy {
    allow ResearchAgent internet
    allow ResearchAgent knowledge.read
    deny ResearchAgent filesystem.write
    deny ResearchAgent secrets.read
    allow Writer filesystem.write
    deny * external.api
}
```

---

## Memory Fabric Scopes

```ail
// Session memory (process-lifetime)
memory SessionStore {
    scope session
}

// Long-term persistent memory
memory ResearchMemory {
    scope long_term
    persistent
    encrypted
}

// Shared team memory
memory TeamKnowledge {
    scope shared
    namespace "ResearchTeam"
}

// Organizational memory
memory OrgFacts {
    scope organization
    persistent
    encrypted
}
```

---

## Multimodal Pipelines

```ail
// Declare multimodal models
model vision {
    capability image_understanding
}

model speech {
    capability speech_recognition
    capability speech_synthesis
}

// Multimodal pipeline
pipeline PersonalAssistant {
    input image camera_frame
    input audio microphone
    input text command

    camera_frame -> vision
    microphone -> speech
    -> reasoning
    -> speech_synthesis
}
```

---

## Migration from v2.0

```bash
# Automated migration
ail migrate --from 2.0 --to 3.0

# What it does:
# 1. Detects deprecated v2.0 patterns
# 2. Explains the changes needed
# 3. Applies automated transformations where possible
# 4. Lists manual changes required
```

---

## V3.0 CLI Reference

```bash
# Core
ail run <file>
ail build <file>
ail check <file>
ail fmt [file]
ail test [path]
ail repl

# Agent management
ail agent list
ail agent inspect <name>
ail agent start <name>
ail agent stop <name>
ail agent restart <name>

# Model management
ail model list
ail model inspect <name>
ail model failover <from> <to>

# Memory inspection
ail memory inspect [scope]
ail memory export [scope]

# Goal tracking
ail goal list
ail goal inspect <id>
ail goal create <name> <objective>

# Observability
ail trace [run_id]
ail replay <run_id>
ail profile [file]
ail eval [file]

# Security
ail security scan
ail package verify

# System health
ail system status
ail system doctor

# Migration
ail migrate --from 2.0 --to 3.0
```

