// memory_fabric/src/backend.rs
//! Default in-memory backend implementation for all memory scopes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::{MemoryBackend, MemoryResult, MemorySnapshot, MemoryVersion};

struct Entry {
    key: String,
    versions: Vec<(String, HashMap<String, String>, u64, Instant)>, // (value, metadata, version, time)
}

/// The default backend: an in-memory versioned key-value store.
///
/// - Stores every version of every key (history tracking).
/// - Retrieval: exact key match → score 1.0; substring match → score 0.5.
/// - Thread-safe via `Arc<Mutex<>>`.
pub struct InMemoryBackend {
    entries: Arc<Mutex<Vec<Entry>>>,
    next_version: Arc<Mutex<u64>>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
            next_version: Arc::new(Mutex::new(1)),
        }
    }

    fn next_ver(&self) -> u64 {
        let mut v = self.next_version.lock().unwrap();
        let ver = *v;
        *v += 1;
        ver
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBackend for InMemoryBackend {
    fn store(&self, key: &str, value: &str, metadata: HashMap<String, String>) {
        let ver = self.next_ver();
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.iter_mut().find(|e| e.key == key) {
            entry.versions.push((value.to_string(), metadata, ver, Instant::now()));
        } else {
            entries.push(Entry {
                key: key.to_string(),
                versions: vec![(value.to_string(), metadata, ver, Instant::now())],
            });
        }
    }

    fn retrieve(&self, query: &str, limit: usize) -> Vec<MemoryResult> {
        let entries = self.entries.lock().unwrap();
        let needle = query.to_lowercase();

        // Collect one result per unique key (latest version)
        let mut results: Vec<MemoryResult> = entries
            .iter()
            .filter_map(|entry| {
                let latest = entry.versions.last()?;
                let key_lower = entry.key.to_lowercase();
                let val_lower = latest.0.to_lowercase();

                let score = if key_lower == needle {
                    1.0
                } else if key_lower.contains(&needle) || val_lower.contains(&needle) {
                    0.5
                } else {
                    return None;
                };

                Some(MemoryResult {
                    key: entry.key.clone(),
                    value: latest.0.clone(),
                    score,
                    metadata: latest.1.clone(),
                })
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }

    fn forget(&self, key: &str) -> bool {
        let mut entries = self.entries.lock().unwrap();
        let before = entries.len();
        entries.retain(|e| e.key != key);
        entries.len() < before
    }

    fn list(&self) -> Vec<(String, String)> {
        let entries = self.entries.lock().unwrap();
        entries
            .iter()
            .filter_map(|e| {
                e.versions.last().map(|(v, _, _, _)| (e.key.clone(), v.clone()))
            })
            .collect()
    }

    fn history(&self, key: &str) -> Vec<MemoryVersion> {
        let entries = self.entries.lock().unwrap();
        entries
            .iter()
            .find(|e| e.key == key)
            .map(|e| {
                e.versions
                    .iter()
                    .map(|(v, _, ver, t)| MemoryVersion {
                        value: v.clone(),
                        version: *ver,
                        stored_at: *t,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn sync(&self) -> Result<(), String> {
        // In-memory: no durable storage, sync is a no-op.
        Ok(())
    }

    fn snapshot(&self) -> MemorySnapshot {
        let entries = self.entries.lock().unwrap();
        let snapshot_entries: Vec<_> = entries
            .iter()
            .filter_map(|e| {
                e.versions.last().map(|(v, m, _, _)| {
                    (e.key.clone(), v.clone(), m.clone())
                })
            })
            .collect();
        MemorySnapshot {
            entries: snapshot_entries,
            taken_at: Instant::now(),
        }
    }

    fn restore_snapshot(&self, snapshot: MemorySnapshot) -> Result<(), String> {
        let mut entries = self.entries.lock().unwrap();
        *entries = snapshot
            .entries
            .into_iter()
            .map(|(key, value, metadata)| Entry {
                key,
                versions: vec![(value, metadata, 0, Instant::now())],
            })
            .collect();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_exact_retrieve() {
        let backend = InMemoryBackend::new();
        backend.store("color", "blue", HashMap::new());
        let results = backend.retrieve("color", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].value, "blue");
        assert_eq!(results[0].score, 1.0);
    }

    #[test]
    fn substring_retrieve() {
        let backend = InMemoryBackend::new();
        backend.store("sky_color", "blue horizon", HashMap::new());
        let results = backend.retrieve("sky", 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].score, 0.5);
    }

    #[test]
    fn versioning_preserves_history() {
        let backend = InMemoryBackend::new();
        backend.store("key", "v1", HashMap::new());
        backend.store("key", "v2", HashMap::new());
        backend.store("key", "v3", HashMap::new());

        let history = backend.history("key");
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].value, "v1");
        assert_eq!(history[2].value, "v3");

        // retrieve returns latest
        let results = backend.retrieve("key", 1);
        assert_eq!(results[0].value, "v3");
    }

    #[test]
    fn forget_removes_all_versions() {
        let backend = InMemoryBackend::new();
        backend.store("temp", "a", HashMap::new());
        backend.store("temp", "b", HashMap::new());
        assert!(backend.forget("temp"));
        assert!(backend.retrieve("temp", 10).is_empty());
        assert!(backend.history("temp").is_empty());
    }

    #[test]
    fn snapshot_and_restore_roundtrip() {
        let backend = InMemoryBackend::new();
        backend.store("x", "10", HashMap::new());
        backend.store("y", "20", HashMap::new());

        let snap = backend.snapshot();
        backend.forget("x");
        backend.forget("y");
        assert!(backend.list().is_empty());

        backend.restore_snapshot(snap).unwrap();
        let list = backend.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn metadata_preserved() {
        let backend = InMemoryBackend::new();
        let mut meta = HashMap::new();
        meta.insert("source".to_string(), "user_input".to_string());
        backend.store("preference", "dark mode", meta.clone());

        let results = backend.retrieve("preference", 1);
        assert_eq!(results[0].metadata["source"], "user_input");
    }

    #[test]
    fn limit_respected() {
        let backend = InMemoryBackend::new();
        for i in 0..10 {
            backend.store(&format!("key{}", i), "value", HashMap::new());
        }
        // All keys contain "key" as substring
        let results = backend.retrieve("key", 3);
        assert_eq!(results.len(), 3);
    }
}

