# AILang v3.0 Agent Architecture

In AILang v3.0, agents are elevated from runtime abstractions to **Operating System Processes**. The Intelligence Kernel tracks agent state, schedules tasks, meters resource consumption, enforces policies, and supervises agent trees.

## 1. Process Lifecycle

Each agent process transitions through a deterministic state machine:

```text
[ Uninitialized ]
       │
       ▼
  [ Starting ] ───► [ Running ] ───► [ Paused ]
       │                │                 │
       │                ▼                 ▼
       └──────────► [ Failed ] ◄──────────┘
                        │
                        ▼
                 [ Terminated ]
```

States:
- **Uninitialized**: Process allocated in the `AgentProcessTable` with its spec, but no execution context created.
- **Starting**: Mounting memory volumes, allocating token quotas, acquiring model handles.
- **Running**: Active reasoning or executing tool calls.
- **Paused**: Suspended execution (e.g., waiting for human-in-the-loop approval or rate-limit backoff).
- **Failed**: Hit an unrecoverable exception or exceeded constraint boundaries.
- **Terminated**: Cleanly shut down, state checkpointed to memory fabric.

## 2. Process Control Block (PCB)

In `kernel/src/agent_process.rs`, every agent has an `AgentProcess` entry containing:
- `id`: Unique agent UUID.
- `name`: Symbolic identifier defined in source code.
- `state`: Current `AgentState`.
- `spec`: Configuration flags (`autonomous`, `persistent`, `restart_on_failure`).
- `priority`: Scheduling priority (0 to 255).
- `tokens_used`: Cumulative token expenditure.
- `cost_spent`: Cumulative dollar expenditure.
- `checkpoint_id`: Latest snapshot identifier in Universal Memory Fabric.

## 3. Supervision & Fault Tolerance

AILang v3.0 implements Erlang/OTP-style supervision trees:
- **`OneForOne`**: If a child crashes, only that child process is restarted.
- **`OneForAll`**: If a child crashes, all sibling processes in the supervisor group are terminated and restarted.
- **`RestForOne`**: If a child crashes, any sibling process declared *after* the failed process in source order is restarted.

```ail
supervisor TeamSupervisor one_for_one {
    children [PM, Developer, Tester];
}
```

## 4. Honest Foundation Status

| Feature | v3.0 Foundation | Production Target |
| :--- | :--- | :--- |
| Process Scheduling | Priority-ordered in-memory queue | Preemptive CFS scheduler across threads/nodes |
| Memory Isolation | Thread-safe `Arc<Mutex>` per process | Hardware-enforced WASM/microVM memory isolation |
| Checkpointing | Serialized state snapshot to MemoryFabric | Copy-on-write page streaming to distributed storage |
