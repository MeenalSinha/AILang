# Security Architecture & Policy Engine

AILang v3.0 implements Zero-Trust by default for all agent actions, tool invocations, and network operations.

## 1. Declarative Policies

Access controls are defined using first-class `policy` blocks or within `application` manifests:

```ail
policy SecurityPolicy {
    allow PM jira;
    allow PM internet;
    allow Developer filesystem.write;
    deny Developer production.database;
    deny * root.exec;
}
```

Rules specify:
- **Subject**: Target agent name or wildcard `*`.
- **Action**: `allow` or `deny`.
- **Resource**: Hierarchical permission string (e.g. `filesystem.read`, `terminal`, `network.outbound`).

## 2. Evaluation Precedence

The Policy Engine evaluates rules with strict precedence:
1. **Explicit Deny**: Any matching `deny` rule immediately blocks access, overriding any allows.
2. **Explicit Allow**: A matching `allow` rule permits execution.
3. **Default Deny**: If no rules match the subject and requested capability, access is rejected.

## 3. Tool Sandboxing & Audit Trail

All tool executions are validated before invocation:
- Arguments are sanitized.
- Permissions are checked against granted agent capabilities and active policy rules.
- The outcome is immutably recorded in the `AuditLog` with timestamps, caller identity, capability requested, and decision status.

