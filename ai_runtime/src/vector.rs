//! A native vector engine: real embedding-backed indexing, similarity
//! search, and metadata filtering, built on the same real feature-hashing
//! [`crate::embed`] used everywhere else in this crate.
//!
//! ## Scope
//!
//! In-memory only (no persistence across process runs) and single
//! embedding backend (the hashing-trick `embed`, not a pluggable "provide
//! your own embedding model" abstraction yet — see `docs/roadmap.md`).
//! What's real: insertion, cosine-similarity ranking, and exact-match
//! metadata filtering all genuinely work and are tested against real
//! data, not simulated.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::embedding::{cosine_similarity, embed};

#[derive(Debug, Clone, PartialEq)]
pub struct VectorEntry {
    pub id: String,
    pub vector: Vec<f64>,
    pub text: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorSearchResult {
    pub id: String,
    pub text: String,
    pub score: f64,
    pub metadata: HashMap<String, String>,
}

/// A named collection of vector entries. Multiple indexes are supported
/// by keeping one `VectorIndex` per name in a registry (see
/// `ailang-interpreter`'s `vector_indexes` field), the same pattern used
/// for named `memory` stores.
#[derive(Default)]
pub struct VectorIndex {
    entries: RefCell<Vec<VectorEntry>>,
}

impl VectorIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Embeds `text` and inserts (or, if `id` already exists, replaces)
    /// an entry.
    pub fn add(&self, id: &str, text: &str, metadata: HashMap<String, String>) {
        let vector = embed(text);
        let mut entries = self.entries.borrow_mut();
        if let Some(existing) = entries.iter_mut().find(|e| e.id == id) {
            existing.vector = vector;
            existing.text = text.to_string();
            existing.metadata = metadata;
        } else {
            entries.push(VectorEntry {
                id: id.to_string(),
                vector,
                text: text.to_string(),
                metadata,
            });
        }
    }

    pub fn remove(&self, id: &str) {
        self.entries.borrow_mut().retain(|e| e.id != id);
    }

    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    /// Embeds `query` and returns the top `k` entries by cosine
    /// similarity, highest first, restricted to entries whose metadata
    /// contains every key/value pair in `filter` (exact string match; an
    /// empty filter matches everything).
    pub fn search(
        &self,
        query: &str,
        k: usize,
        filter: &HashMap<String, String>,
    ) -> Vec<VectorSearchResult> {
        let query_vec = embed(query);
        let entries = self.entries.borrow();

        let mut results: Vec<VectorSearchResult> = entries
            .iter()
            .filter(|e| filter.iter().all(|(k, v)| e.metadata.get(k) == Some(v)))
            .map(|e| VectorSearchResult {
                id: e.id.clone(),
                text: e.text.clone(),
                score: cosine_similarity(&query_vec, &e.vector),
                metadata: e.metadata.clone(),
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(k);
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn adds_and_searches_by_similarity() {
        let index = VectorIndex::new();
        index.add("1", "rust systems programming memory safety", meta(&[]));
        index.add("2", "baking bread flour yeast water", meta(&[]));

        let results = index.search("rust programming memory safety", 1, &HashMap::new());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "1");
    }

    #[test]
    fn search_respects_k_limit() {
        let index = VectorIndex::new();
        for i in 0..5 {
            index.add(&i.to_string(), &format!("document number {i}"), meta(&[]));
        }
        let results = index.search("document", 3, &HashMap::new());
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn search_filters_by_metadata() {
        let index = VectorIndex::new();
        index.add("1", "quarterly report", meta(&[("dept", "finance")]));
        index.add("2", "quarterly report", meta(&[("dept", "engineering")]));

        let results = index.search("quarterly report", 10, &meta(&[("dept", "finance")]));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "1");
    }

    #[test]
    fn add_with_existing_id_replaces_the_entry() {
        let index = VectorIndex::new();
        index.add("1", "old text", meta(&[]));
        index.add("1", "new text", meta(&[]));
        assert_eq!(index.len(), 1);
        let results = index.search("new text", 1, &HashMap::new());
        assert_eq!(results[0].text, "new text");
    }

    #[test]
    fn remove_deletes_an_entry() {
        let index = VectorIndex::new();
        index.add("1", "text", meta(&[]));
        index.remove("1");
        assert!(index.is_empty());
    }
}
