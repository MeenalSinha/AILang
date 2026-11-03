# Evaluation & Quality Assurance

AILang v3.0 builds automated benchmarking and quality evaluation directly into the compilation and test pipeline.

## 1. Goal Verification

When evaluating an autonomous workload, the evaluation framework assesses:
- **Constraint Satisfaction**: Did the execution remain within budget and latency thresholds?
- **Convergence Rate**: How many iterations of the Reason-Verify-Revise loop were needed?
- **Determinism**: Did mock model outputs match canonical golden baselines?

## 2. Golden Master Testing

Every release of AILang runs end-to-end golden tests (`tests/golden/`) comparing the exact standard output and execution trace against approved baselines.
Any variance in token counting, diagnostic codes, or flow control flags an immediate test failure.

