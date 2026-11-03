// memory_fabric/src/lib.rs
//! # ailang-memory-fabric
//!
//! Universal Memory Fabric for AILang v3.0.
//!
//! Traditional AI frameworks give every agent a single flat key-value store.
//! The Memory Fabric provides eight distinct memory scopes — each with
//! different lifetime, sharing, and retrieval semantics — matching how
//! human memory actually works:
//!
//! | Scope        | Lifetime        | Sharing        | Use case                   |
//! |--------------|-----------------|----------------|----------------------------|
//! | Working      | Current call    | Agent-private  | Reasoning scratchpad       |
//! | Session      | Process run     | Agent-private  | Conversation state         |
//! | LongTerm     | Persistent file | Agent-private  | Learned facts              |
//! | Semantic     | Persistent      | Agent-private  | Embedding-indexed concepts |
//! | Episodic     | Persistent      | Agent-private  | Past events timeline       |
//! | Procedural   | Persistent      | Shareable      | Skills & procedures        |
//! | Shared       | Session/Persist | Team/Org       | Collaborative knowledge    |
//! | Global       | Persistent      | All agents     | System-wide facts          |
//!
//! ## V3.0 honest limitations
//!
//! All backends in this release are in-memory (no file persistence across
//! process restarts). The `sync()` method is a no-op on `InMemoryBackend`.
//! File-backed and database-backed backends are the next step; the
//! `MemoryBackend` trait is the stable extension point for them.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub mod backend;

pub use backend::InMemoryBackend;

/// A single memory retrieval result.
#[derive(Debug, Clone)]
pub struct MemoryResult {
    pub key: String,
    pub value: String,
    /// Relevance score 0.0–1.0 (1.0 = exact key match).
    pub score: f64,
    pub metadata: HashMap<String, String>,
}

/// A versioned memory entry for history tracking.
#[derive(Debug, Clone)]
pub struct MemoryVersion {
    pub value: String,
    pub version: u64,
    pub stored_at: Instant,
}

/// A point-in-time snapshot of a memory backend.
#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    pub entries: Vec<(String, String, HashMap<String, String>)>,
    pub taken_at: Instant,
}

/// The stable trait that every memory backend must implement.
///
/// New backends (SQLite, Redis, Postgres, vector DB, etc.) only need
/// to implement this trait; the rest of the language runtime doesn't change.
pub trait MemoryBackend: Send + Sync {
    fn store(&self, key: &str, value: &str, metadata: HashMap<String, String>);
    fn retrieve(&self, query: &str, limit: usize) -> Vec<MemoryResult>;
    fn forget(&self, key: &str) -> bool;
    fn list(&self) -> Vec<(String, String)>;
    fn history(&self, key: &str) -> Vec<MemoryVersion>;
    /// Persist to durable storage (no-op for in-memory backends).
    fn sync(&self) -> Result<(), String>;
    fn snapshot(&self) -> MemorySnapshot;
    fn restore_snapshot(&self, snapshot: MemorySnapshot) -> Result<(), String>;
}

/// Memory scope identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MemoryScope {
    Working,
    Session,
    LongTerm,
    Semantic,
    Episodic,
    Procedural,
    Shared { namespace: String },
    Global,
    Named(String),
}

impl MemoryScope {
    pub fn key(&self) -> String {
        match self {
            MemoryScope::Working => "working".into(),
            MemoryScope::Session => "session".into(),
            MemoryScope::LongTerm => "long_term".into(),
            MemoryScope::Semantic => "semantic".into(),
            MemoryScope::Episodic => "episodic".into(),
            MemoryScope::Procedural => "procedural".into(),
            MemoryScope::Shared { namespace } => format!("shared:{}", namespace),
            MemoryScope::Global => "global".into(),
            MemoryScope::Named(n) => format!("named:{}", n),
        }
    }
}

/// The unified Memory Fabric — the single entry point for all memory operations.
pub struct MemoryFabric {
    backends: Arc<Mutex<HashMap<String, Arc<dyn MemoryBackend>>>>,
}

impl MemoryFabric {
    pub fn new() -> Self {
        let mut backends: HashMap<String, Arc<dyn MemoryBackend>> = HashMap::new();
        // Register default in-memory backend for every standard scope
        for scope in &[
            "working", "session", "long_term", "semantic",
            "episodic", "procedural", "global",
        ] {
            backends.insert(
                scope.to_string(),
                Arc::new(InMemoryBackend::new()) as Arc<dyn MemoryBackend>,
            );
        }
        Self {
            backends: Arc::new(Mutex::new(backends)),
        }
    }

    /// Register a custom backend for a scope (replaces the default in-memory one).
    pub fn register_backend(&self, scope: &MemoryScope, backend: Arc<dyn MemoryBackend>) {
        self.backends.lock().unwrap().insert(scope.key(), backend);
    }

    fn ensure_scope(&self, scope: &MemoryScope) {
        let mut backends = self.backends.lock().unwrap();
        backends
            .entry(scope.key())
            .or_insert_with(|| Arc::new(InMemoryBackend::new()) as Arc<dyn MemoryBackend>);
    }

    pub fn store(
        &self,
        scope: &MemoryScope,
        key: &str,
        value: &str,
        metadata: HashMap<String, String>,
    ) {
        self.ensure_scope(scope);
        let backends = self.backends.lock().unwrap();
        if let Some(backend) = backends.get(&scope.key()) {
            backend.store(key, value, metadata);
        }
    }

    pub fn retrieve(
        &self,
        scope: &MemoryScope,
        query: &str,
        limit: usize,
    ) -> Vec<MemoryResult> {
        self.ensure_scope(scope);
        let backends = self.backends.lock().unwrap();
        backends
            .get(&scope.key())
            .map(|b| b.retrieve(query, limit))
            .unwrap_or_default()
    }

    pub fn forget(&self, scope: &MemoryScope, key: &str) -> bool {
        let backends = self.backends.lock().unwrap();
        backends
            .get(&scope.key())
            .map(|b| b.forget(key))
            .unwrap_or(false)
    }

    pub fn list(&self, scope: &MemoryScope) -> Vec<(String, String)> {
        let backends = self.backends.lock().unwrap();
        backends
            .get(&scope.key())
            .map(|b| b.list())
            .unwrap_or_default()
    }

    pub fn snapshot(&self, scope: &MemoryScope) -> Option<MemorySnapshot> {
        let backends = self.backends.lock().unwrap();
        backends.get(&scope.key()).map(|b| b.snapshot())
    }

    pub fn restore_snapshot(
        &self,
        scope: &MemoryScope,
        snapshot: MemorySnapshot,
    ) -> Result<(), String> {
        let backends = self.backends.lock().unwrap();
        backends
            .get(&scope.key())
            .ok_or_else(|| format!("No backend for scope {:?}", scope.key()))
            .and_then(|b| b.restore_snapshot(snapshot))
    }

    /// Synchronize all persistent backends to durable storage.
    pub fn sync_all(&self) -> Vec<(String, String)> {
        let backends = self.backends.lock().unwrap();
        backends
            .iter()
            .filter_map(|(scope, backend)| {
                backend.sync().err().map(|e| (scope.clone(), e))
            })
            .collect()
    }
}

impl Default for MemoryFabric {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_retrieve_across_scopes() {
        let fabric = MemoryFabric::new();

        fabric.store(&MemoryScope::Working, "scratch", "temporary data", HashMap::new());
        fabric.store(&MemoryScope::LongTerm, "fact", "the sky is blue", HashMap::new());

        let working = fabric.retrieve(&MemoryScope::Working, "scratch", 5);
        let lt = fabric.retrieve(&MemoryScope::LongTerm, "fact", 5);

        assert!(!working.is_empty());
        assert!(!lt.is_empty());
        assert_eq!(working[0].value, "temporary data");
        assert_eq!(lt[0].value, "the sky is blue");
    }

    #[test]
    fn scopes_are_isolated() {
        let fabric = MemoryFabric::new();
        fabric.store(&MemoryScope::Session, "key", "session value", HashMap::new());

        // Same key in a different scope returns nothing
        let result = fabric.retrieve(&MemoryScope::Working, "key", 5);
        assert!(result.is_empty());
    }

    #[test]
    fn shared_scope_with_namespace() {
        let fabric = MemoryFabric::new();
        let team_scope = MemoryScope::Shared { namespace: "TeamAlpha".into() };

        fabric.store(&team_scope, "decision", "use approach A", HashMap::new());
        let results = fabric.retrieve(&team_scope, "decision", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].value, "use approach A");
    }

    #[test]
    fn forget_removes_from_scope() {
        let fabric = MemoryFabric::new();
        fabric.store(&MemoryScope::Session, "temp", "delete me", HashMap::new());
        assert!(!fabric.retrieve(&MemoryScope::Session, "temp", 5).is_empty());

        let removed = fabric.forget(&MemoryScope::Session, "temp");
        assert!(removed);
        assert!(fabric.retrieve(&MemoryScope::Session, "temp", 5).is_empty());
    }

    #[test]
    fn snapshot_and_restore() {
        let fabric = MemoryFabric::new();
        fabric.store(&MemoryScope::LongTerm, "a", "alpha", HashMap::new());
        fabric.store(&MemoryScope::LongTerm, "b", "beta", HashMap::new());

        let snap = fabric.snapshot(&MemoryScope::LongTerm).unwrap();
        assert_eq!(snap.entries.len(), 2);

        fabric.forget(&MemoryScope::LongTerm, "a");
        fabric.forget(&MemoryScope::LongTerm, "b");
        assert!(fabric.list(&MemoryScope::LongTerm).is_empty());

        fabric.restore_snapshot(&MemoryScope::LongTerm, snap).unwrap();
        let restored = fabric.list(&MemoryScope::LongTerm);
        assert_eq!(restored.len(), 2);
    }

    #[test]
    fn named_scope_auto_created() {
        let fabric = MemoryFabric::new();
        let scope = MemoryScope::Named("custom_store".into());
        fabric.store(&scope, "x", "42", HashMap::new());
        let results = fabric.retrieve(&scope, "x", 1);
        assert_eq!(results[0].value, "42");
    }
}

