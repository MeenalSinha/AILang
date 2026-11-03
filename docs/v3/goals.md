# Goal-Oriented Programming in AILang v3.0

Rather than specifying imperative steps, AILang v3.0 allows developers to define declarative objectives with strict physical and financial constraints.

## 1. Syntax & Declaration

```ail
goal DeepResearch "Produce a comprehensive research report on quantum algorithms" {
    let topic = "quantum computing";
    println("Researching: " + topic);
    
    reason max 3 iterations {
        println("Synthesizing literature...");
    }
    
    emit "research_complete" topic;
}
```

## 2. Goal Constraints

Goals can enforce constraints across multiple dimensions:
- **`accuracy`**: Minimum evaluation metric (e.g., F1 > 0.90).
- **`budget`**: Maximum financial cost in USD (e.g., budget < 5.00).
- **`deadline`**: Time limit for completion.
- **`latency`**: Per-turn or total wall-clock limit.
- **`energy`**: Estimated compute energy envelope.

```text
constraints {
    accuracy > 0.95;
    budget < 2.50;
    latency < 5000;
}
```

## 3. The Reason-Verify-Revise (RVR) Loop

Inside a goal, cognitive execution is structured around the `reason` construct:
1. **Reason**: Formulate a hypothesis or generation.
2. **Verify**: Check output against constraints, schemas, or unit tests.
3. **Revise**: If verification fails, supply error feedback to the model context and iterate until satisfied or iterations/budget exhausted.

```ail
reason max 5 iterations budget 1.0 {
    // Attempt generation and iterative refinement
}
```

## 4. Kernel Goal Engine

The Goal Engine (`kernel/src/goal_engine.rs`) manages:
- **Goal Decomposition**: Breaks high-level objectives into a directed acyclic task graph.
- **Constraint Monitoring**: Evaluates resource consumption at each stage.
- **Plan Adjustment**: Recalibrates subtasks if intermediate results do not meet quality thresholds.
