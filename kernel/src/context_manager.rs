// kernel/src/context_manager.rs
//! Context Operating System: manages all context types for AI executions.
//!
//! The context manager is the kernel's answer to the "context window problem":
//! it tracks all relevant context (conversation, task, memory, knowledge, etc.)
//! and produces an optimized context window for each model call.
//!
//! ## Operations
//!
//! - `add`: Store a context entry with a priority
//! - `build_context_window`: Produce a token-budget-respecting ordered context
//! - `compress`: Word-count compression of oversized entries
//! - `clear_kind`: Remove all entries of a specific type
//!
//! ## V3.0 honest limitations
//!
//! Compression and prioritization are word-count based, not semantic.
//! A production implementation would use an LLM or embedding-based compressor.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Category of a context entry — affects prioritization and eviction policy.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextKind {
    /// Active conversation turns (very high priority).
    Conversation,
    /// Current task description (high priority).
    Task,
    /// Agent-specific context (medium priority).
    Agent,
    /// Retrieved memory entries (medium priority).
    Memory,
    /// Retrieved knowledge chunks (medium priority).
    Knowledge,
    /// Tool call results (medium-low priority).
    Tool,
    /// Environment/system context (low priority).
    Environment,
}

impl ContextKind {
    /// Default priority for each kind. Callers can override.
    pub fn default_priority(&self) -> u8 {
        match self {
            ContextKind::Conversation => 200,
            ContextKind::Task => 180,
            ContextKind::Agent => 150,
            ContextKind::Memory => 120,
            ContextKind::Knowledge => 100,
            ContextKind::Tool => 80,
            ContextKind::Environment => 40,
        }
    }
}

/// One entry in the context manager.
#[derive(Debug, Clone)]
pub struct ContextEntry {
    pub key: String,
    pub value: String,
    pub kind: ContextKind,
    /// Priority 0 (lowest) – 255 (highest). Higher entries make the window first.
    pub priority: u8,
    /// Rough word-count estimate of tokens.
    pub token_estimate: usize,
    /// If set, the entry expires at this time.
    pub expires_at: Option<Instant>,
    pub created_at: Instant,
}

/// The kernel's context manager.
pub struct ContextManager {
    entries: Arc<Mutex<Vec<ContextEntry>>>,
    /// Maximum number of tokens in the built context window.
    max_tokens: usize,
}

impl ContextManager {
    pub fn new(max_tokens: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
            max_tokens,
        }
    }

    /// Add or replace a context entry.
    pub fn add(&self, key: &str, value: &str, kind: ContextKind, priority: u8) {
        let token_estimate = estimate_tokens(value);
        let mut entries = self.entries.lock().unwrap();
        // Evict expired entries first
        let now = Instant::now();
        entries.retain(|e| e.expires_at.map(|exp| exp > now).unwrap_or(true));
        // Replace existing entry with the same key
        entries.retain(|e| e.key != key);
        entries.push(ContextEntry {
            key: key.to_string(),
            value: value.to_string(),
            kind,
            priority,
            token_estimate,
            expires_at: None,
            created_at: Instant::now(),
        });
    }

    /// Add a context entry that automatically expires after `ttl`.
    pub fn add_with_ttl(
        &self,
        key: &str,
        value: &str,
        kind: ContextKind,
        priority: u8,
        ttl: Duration,
    ) {
        let token_estimate = estimate_tokens(value);
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|e| e.key != key);
        entries.push(ContextEntry {
            key: key.to_string(),
            value: value.to_string(),
            kind,
            priority,
            token_estimate,
            expires_at: Some(Instant::now() + ttl),
            created_at: Instant::now(),
        });
    }

    /// Retrieve a single context entry by key.
    pub fn get(&self, key: &str) -> Option<String> {
        let entries = self.entries.lock().unwrap();
        entries.iter().find(|e| e.key == key).map(|e| e.value.clone())
    }

    /// Remove a specific entry.
    pub fn remove(&self, key: &str) {
        self.entries.lock().unwrap().retain(|e| e.key != key);
    }

    /// Build a context window respecting `max_tokens`.
    ///
    /// Entries are ordered by priority (descending), then by recency.
    /// Entries that don't fit in the remaining token budget are skipped.
    pub fn build_context_window(&self) -> Vec<ContextEntry> {
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap().clone();

        // Drop expired entries
        entries.retain(|e| e.expires_at.map(|exp| exp > now).unwrap_or(true));

        // Sort: priority descending, then most-recent first
        entries.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(b.created_at.cmp(&a.created_at))
        });

        // Greedy fit into token budget
        let mut result = Vec::new();
        let mut total_tokens = 0usize;
        for entry in entries {
            if total_tokens + entry.token_estimate <= self.max_tokens {
                total_tokens += entry.token_estimate;
                result.push(entry);
            }
        }
        result
    }

    /// Compress oversized entries to fit within word limits.
    ///
    /// Entries with more than `max_words` words are truncated and tagged `[compressed]`.
    pub fn compress(&self, max_words: usize) {
        let mut entries = self.entries.lock().unwrap();
        for entry in entries.iter_mut() {
            if entry.token_estimate > max_words {
                let words: Vec<&str> = entry.value.split_whitespace().collect();
                let compressed: Vec<&str> = words.iter().take(max_words).copied().collect();
                entry.value = format!("{} [compressed]", compressed.join(" "));
                entry.token_estimate = max_words;
            }
        }
    }

    /// Remove all entries of the specified kind.
    pub fn clear_kind(&self, kind: &ContextKind) {
        self.entries.lock().unwrap().retain(|e| &e.kind != kind);
    }

    /// Total estimated tokens of all non-expired entries.
    pub fn total_tokens(&self) -> usize {
        let now = Instant::now();
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.expires_at.map(|exp| exp > now).unwrap_or(true))
            .map(|e| e.token_estimate)
            .sum()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

fn estimate_tokens(text: &str) -> usize {
    // Heuristic: ~1 token per word (real tokenizer would be more precise)
    text.split_whitespace().count().max(1)
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new(4096)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_retrieve() {
        let cm = ContextManager::new(1000);
        cm.add("task", "Research quantum computing", ContextKind::Task, 180);
        assert_eq!(cm.get("task"), Some("Research quantum computing".to_string()));
    }

    #[test]
    fn replace_existing_key() {
        let cm = ContextManager::new(1000);
        cm.add("key", "v1", ContextKind::Conversation, 200);
        cm.add("key", "v2", ContextKind::Conversation, 200);
        assert_eq!(cm.entry_count(), 1);
        assert_eq!(cm.get("key"), Some("v2".to_string()));
    }

    #[test]
    fn build_context_window_respects_token_budget() {
        let cm = ContextManager::new(10); // very small budget
        // Add lots of content
        for i in 0..20 {
            let val = format!("word{} word{} word{}", i, i, i); // ~3 tokens each
            cm.add(&format!("key{}", i), &val, ContextKind::Knowledge, 100);
        }
        let window = cm.build_context_window();
        let total: usize = window.iter().map(|e| e.token_estimate).sum();
        assert!(total <= 10, "total {} exceeds budget", total);
    }

    #[test]
    fn priority_determines_window_order() {
        let cm = ContextManager::new(1000);
        cm.add("low", "low priority content", ContextKind::Environment, 40);
        cm.add("high", "high priority content", ContextKind::Conversation, 200);
        cm.add("med", "medium priority content", ContextKind::Task, 150);

        let window = cm.build_context_window();
        assert_eq!(window[0].key, "high");
        assert_eq!(window[1].key, "med");
        assert_eq!(window[2].key, "low");
    }

    #[test]
    fn compress_truncates_large_entries() {
        let cm = ContextManager::new(10000);
        let long_text = (0..100).map(|i| format!("word{}", i)).collect::<Vec<_>>().join(" ");
        cm.add("big", &long_text, ContextKind::Knowledge, 100);
        
        cm.compress(20);
        
        let val = cm.get("big").unwrap();
        assert!(val.contains("[compressed]"), "should be tagged as compressed");
        assert!(val.split_whitespace().count() <= 21, "should be truncated");
    }

    #[test]
    fn clear_kind_removes_matching_entries() {
        let cm = ContextManager::new(1000);
        cm.add("conv1", "hi", ContextKind::Conversation, 200);
        cm.add("task1", "do stuff", ContextKind::Task, 180);
        cm.add("mem1", "remember", ContextKind::Memory, 120);

        cm.clear_kind(&ContextKind::Memory);
        assert_eq!(cm.entry_count(), 2);
        assert!(cm.get("mem1").is_none());
    }

    #[test]
    fn ttl_expiry() {
        let cm = ContextManager::new(1000);
        cm.add_with_ttl("temp", "expires soon", ContextKind::Tool, 80, Duration::from_millis(1));
        // Wait for expiry
        std::thread::sleep(Duration::from_millis(5));
        // Expired entries are cleaned up during the next add or build
        cm.add("trigger", "cleanup", ContextKind::Environment, 40);
        assert!(cm.get("temp").is_none());
    }
}

