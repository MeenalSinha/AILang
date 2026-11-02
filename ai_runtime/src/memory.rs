//! Memory: `remember`/`recall`/`forget`, backed by an interface so a real
//! storage backend (vector database, SQL, etc.) can be substituted for
//! [`InMemoryStore`] later without changing anything above this trait.
//!
//! ## v1.0: versioning and agent/team scoping
//!
//! [`InMemoryStore`] now keeps every value ever `remember`ed for a key
//! (with a timestamp), not just the latest — real memory *versioning*,
//! not just overwrite-in-place. `recall`/`all` still surface only the
//! current (most recent) value by default, so existing v0.5 callers see
//! no behavior change; [`InMemoryStore::history`] is the new opt-in way
//! to see everything.
//!
//! Agent- and team-scoped memory don't need new storage machinery: a
//! named `memory Name persistent;` declaration already gets its own
//! `InMemoryStore` instance (see `ailang-interpreter`'s `memory_stores`
//! registry), and multiple agents referencing the *same* named memory in
//! their `memory` field naturally share it — that's "team memory" for
//! free, no separate concept required. What v1.0 adds is a way to
//! `remember`/`recall`/`forget` *against* a specific named store instead
//! of only the single global one v0.5 supported — see
//! `ailang-interpreter`'s `remember_in`/`recall_in`/`forget_in` builtins.

use std::cell::RefCell;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// A memory backend for agent/session memory. `remember`/`recall`/`forget`
/// are the exact three verbs the v0.5 spec calls for; `all` exists so
/// tooling (a future `ail` subcommand, or the REPL's `:history`-style
/// introspection) can list what's stored without needing a query.
pub trait MemoryStore {
    fn remember(&self, key: &str, value: &str);
    /// Look up `query` — first as an exact key match, then (if that
    /// misses) as a case-insensitive substring match against either keys
    /// or values, returning the most recently remembered match. Returns
    /// `None` if nothing matches.
    fn recall(&self, query: &str) -> Option<String>;
    fn forget(&self, key: &str);
    fn all(&self) -> Vec<(String, String)>;
    /// Every value ever remembered under `key`, oldest first, as
    /// `(value, unix_timestamp_seconds)` pairs. Empty if the key was
    /// never remembered (or has been `forget`-ten).
    fn history(&self, key: &str) -> Vec<(String, f64)>;
}

#[derive(Default)]
struct Entry {
    key: String,
    versions: Vec<(String, f64)>, // (value, remembered_at) oldest first
}

/// The v1.0 memory backend: an in-process, `RefCell`-guarded `Vec` of
/// versioned key/value entries. Deliberately simple — no persistence
/// across process runs, no indexing beyond linear scan — because the
/// point of this release is the *interface*, not a production memory
/// system; see the crate-level doc for what's real vs. placeholder.
#[derive(Default)]
pub struct InMemoryStore {
    entries: RefCell<Vec<Entry>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MemoryStore for InMemoryStore {
    fn remember(&self, key: &str, value: &str) {
        let mut entries = self.entries.borrow_mut();
        let now = now_seconds();
        if let Some(existing) = entries.iter_mut().find(|e| e.key == key) {
            existing.versions.push((value.to_string(), now));
        } else {
            entries.push(Entry {
                key: key.to_string(),
                versions: vec![(value.to_string(), now)],
            });
        }
    }

    fn recall(&self, query: &str) -> Option<String> {
        let entries = self.entries.borrow();
        if let Some(entry) = entries.iter().find(|e| e.key == query) {
            return entry.versions.last().map(|(v, _)| v.clone());
        }
        let needle = query.to_lowercase();
        entries
            .iter()
            .rev()
            .find(|e| {
                let current = e.versions.last().map(|(v, _)| v.as_str()).unwrap_or("");
                e.key.to_lowercase().contains(&needle) || current.to_lowercase().contains(&needle)
            })
            .and_then(|e| e.versions.last().map(|(v, _)| v.clone()))
    }

    fn forget(&self, key: &str) {
        self.entries.borrow_mut().retain(|e| e.key != key);
    }

    fn all(&self) -> Vec<(String, String)> {
        self.entries
            .borrow()
            .iter()
            .filter_map(|e| e.versions.last().map(|(v, _)| (e.key.clone(), v.clone())))
            .collect()
    }

    fn history(&self, key: &str) -> Vec<(String, f64)> {
        self.entries
            .borrow()
            .iter()
            .find(|e| e.key == key)
            .map(|e| e.versions.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_and_recall_exact_key() {
        let store = InMemoryStore::new();
        store.remember("favorite_drink", "coffee");
        assert_eq!(store.recall("favorite_drink"), Some("coffee".to_string()));
    }

    #[test]
    fn recall_falls_back_to_substring_search() {
        let store = InMemoryStore::new();
        store.remember("favorite_drink", "User likes coffee in the morning");
        assert_eq!(
            store.recall("coffee"),
            Some("User likes coffee in the morning".to_string())
        );
    }

    #[test]
    fn recall_is_case_insensitive() {
        let store = InMemoryStore::new();
        store.remember("Note", "Loves COFFEE");
        assert_eq!(store.recall("coffee"), Some("Loves COFFEE".to_string()));
    }

    #[test]
    fn remember_overwrites_current_value_but_keeps_history() {
        let store = InMemoryStore::new();
        store.remember("k", "v1");
        store.remember("k", "v2");
        assert_eq!(store.recall("k"), Some("v2".to_string()));
        assert_eq!(store.all().len(), 1);
        let history = store.history("k");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].0, "v1");
        assert_eq!(history[1].0, "v2");
    }

    #[test]
    fn history_is_empty_for_a_never_remembered_key() {
        let store = InMemoryStore::new();
        assert!(store.history("nope").is_empty());
    }

    #[test]
    fn forget_removes_the_key_and_its_history() {
        let store = InMemoryStore::new();
        store.remember("k", "v");
        store.forget("k");
        assert_eq!(store.recall("k"), None);
        assert!(store.history("k").is_empty());
    }

    #[test]
    fn recall_returns_none_when_nothing_matches() {
        let store = InMemoryStore::new();
        store.remember("k", "v");
        assert_eq!(store.recall("nonexistent"), None);
    }

    #[test]
    fn versions_have_increasing_or_equal_timestamps() {
        let store = InMemoryStore::new();
        store.remember("k", "v1");
        store.remember("k", "v2");
        let history = store.history("k");
        assert!(history[1].1 >= history[0].1);
    }
}
