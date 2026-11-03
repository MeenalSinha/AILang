# Universal Memory Fabric

AILang v3.0 introduces the **Universal Memory Fabric** (`memory_fabric/` crate), providing unified memory scopes across the lifetime of single agents, teams, and whole organizations.

## 1. Scopes

The fabric defines 8 distinct scoping tiers:

1. **`Session`**: Ephemeral, active only during the current process or interactive session.
2. **`Persistent`**: Key-value or document persistence across process restarts.
3. **`Personal`**: Bound to a specific user identity.
4. **`Team`**: Shared across all agents participating in a team/supervisor tree.
5. **`Organization`**: Accessible across multiple applications within an enterprise namespace.
6. **`Global`**: Cross-cluster, federated read-only or synchronized knowledge.
7. **`Transactional`**: ACID-backed scratchpad supporting rollback and commit semantics.
8. **`Vector`**: Dense embedding space indexed for semantic kNN retrieval.

## 2. Architecture & API

The core abstraction is the `MemoryBackend` trait:

```rust
pub trait MemoryBackend: Send + Sync {
    fn store(&self, scope: MemoryScope, key: &str, value: &str) -> Result<()>;
    fn retrieve(&self, scope: MemoryScope, query: &str) -> Result<Vec<MemoryEntry>>;
    fn delete(&self, scope: MemoryScope, key: &str) -> Result<()>;
    fn snapshot(&self, scope: MemoryScope) -> Result<MemorySnapshot>;
    fn restore(&self, snapshot: &MemorySnapshot) -> Result<()>;
}
```

## 3. Versioning & Conflict Resolution

- Every mutation increments a sequential vector clock.
- Snapshots record immutable points-in-time for checkpointing agent processes.
- Semantic recall leverages cosine similarity over embedded representations with recency weighting.

