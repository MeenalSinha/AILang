# Observability & Distributed Tracing

In an AI Operating System, runtime observability is paramount.

## 1. Tracing Infrastructure

AILang records structured execution traces across:
- **Process Transitions**: Agent state changes (Starting, Running, Failed, Checkpointed).
- **Tool Invocations**: Arguments, execution latency, and return status.
- **Model Inferences**: Token consumption, prompt templating, response latency, and failovers.
- **Memory Operations**: Store, retrieve, similarity scores, and eviction events.

## 2. CLI Inspection

```bash
# View live status of all agent processes
ail system status

# Run comprehensive system diagnostics
ail system doctor

# Inspect model health and latency
ail model list

# Dump audit log
ail security audit
```
