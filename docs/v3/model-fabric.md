# Model Fabric & Automated Failover

The **Model Fabric** (`kernel/src/model_fabric.rs`) treats LLMs and SLMs not as static API endpoints, but as dynamically routeable compute nodes.

## 1. Capabilities & Health Checking

Models declare capabilities:
- `TextGeneration`
- `CodeGeneration`
- `StructuredOutput`
- `MultimodalVision`
- `AudioIn`
- `AudioOut`
- `FastInference`
- `DeepReasoning`

Each model spec tracks latency, throughput, error rates, and consecutive failure counts.

## 2. Failover Chains

When an agent requests inference, the Model Fabric executes a prioritized routing algorithm:
1. Select the primary model matching the required capabilities and constraints (e.g. latency, cost).
2. If the primary model returns a 5xx error, rate-limit, or times out, failover triggers immediately.
3. The request is retried against the designated fallback in the chain without interrupting agent execution.
4. Alerts are logged to the Health Monitor.

```text
[ Primary: Claude-3.7-Sonnet ] ──(Rate Limit / Error)──► [ Fallback: Gemini-2.0-Flash ]
                                                                 │
                                                                 ▼ (Offline)
                                                        [ Local: Llama-3.3-8B ]
```
