# Upgrading from AILang v2.0 to v3.0

This guide outlines breaking changes, new primitives, and recommended migration patterns for transitioning codebases from AILang v2.0 to v3.0.

## 1. Reserved Keywords

The following words are now reserved keywords in v3.0:
- `goal`, `plan`, `supervisor`, `resource`, `intelligence`, `allocate`
- `autonomous`, `persistent`, `restart_on_failure`, `constraint`, `optimize`, `budget`
- `device`, `camera`, `microphone`, `speaker`, `sensor`
- `ui`, `panel`, `dashboard`
- `event`, `emit`, `when`
- `reason`, `verify`, `revise`
- `application`, `policy`, `allow`, `deny`
- `multimodal`, `image`, `audio`, `video`, `migrate`, `checkpoint`, `recover`

If your v2.0 code used any of these names as variable or function identifiers, rename them or use string literals.

## 2. Agent Supervisions

In v2.0, agent failure was unmanaged. In v3.0, wrap cooperating agents in a `supervisor`:

```ail
// Before (v2.0):
agent Worker { model FastModel; }

// After (v3.0):
agent Worker { model FastModel; }
supervisor AppSupervisor one_for_one {
    children [Worker];
}
```

## 3. Goals Replace Free-Form Loops

Instead of manual `while` loops for reasoning convergence, use `goal` with `reason`:

```ail
// v3.0:
goal RefineOutput "Draft documentation" {
    reason max 3 iterations {
        // iterative generation
    }
}
```
